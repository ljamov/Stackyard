//! Checks a connection one step at a time (find the server, reach its
//! port, handshake, log in, open the files, run commands) and says in plain
//! words which step stopped, whose side the problem is on, and what to try.
//!
//! Logging in for real uses the same steps, so its errors read the same.

use std::io::ErrorKind;
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::time::Duration;

use serde::Serialize;

use super::{Connection, Protocol};
use crate::Error;

/// How long reaching the server's port may take.
pub(crate) const REACH_TIMEOUT: Duration = Duration::from_secs(15);

/// The steps of connecting, in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Stage {
    /// Turning the server address into an IP address.
    Find,
    /// Opening a connection to the port.
    Reach,
    /// SSH's secure handshake (SFTP only).
    Handshake,
    Login,
    /// Opening a session channel after login (SFTP only).
    Session,
    /// Starting SFTP, or listing the start folder over FTP.
    Files,
    /// Running a shell command (SFTP only; optional).
    Commands,
}

impl Stage {
    fn all(protocol: Protocol) -> &'static [Stage] {
        match protocol {
            Protocol::Sftp => &[
                Stage::Find,
                Stage::Reach,
                Stage::Handshake,
                Stage::Login,
                Stage::Session,
                Stage::Files,
                Stage::Commands,
            ],
            Protocol::Ftps | Protocol::Ftp => {
                &[Stage::Find, Stage::Reach, Stage::Login, Stage::Files]
            }
            Protocol::Plugin => &[Stage::Login, Stage::Files],
        }
    }
}

/// Whose side a problem is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Whose {
    /// This computer's connection, or something between it and the server.
    Internet,
    /// The server or the hosting account's settings.
    Server,
    /// The user name, password, key, or what the account may do.
    Access,
    Stackyard,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Ok,
    Failed,
    /// Did not work, but nothing depends on it.
    Warning,
    /// Not tried, because an earlier step failed.
    Skipped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Step {
    pub stage: Stage,
    pub status: Status,
    /// What happened, in plain words.
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub whose: Option<Whose>,
    /// What to try.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fix: Option<String>,
}

/// The result of [`super::Sessions::check`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Report {
    pub steps: Vec<Step>,
    /// The server key (SFTP) seen, for a first connect to trust.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_key: Option<String>,
    /// The FTPS certificate to trust, when it is not trusted yet.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub certificate: Option<String>,
    pub key_changed: bool,
}

/// Where connecting stopped, why, and what to try.
#[derive(Debug)]
pub(crate) struct Stopped {
    pub stage: Stage,
    pub whose: Whose,
    pub detail: String,
    pub fix: String,
    /// An error the app acts on itself (a changed key, a certificate to trust).
    pub typed: Option<Box<Error>>,
}

impl Stopped {
    pub fn new(
        stage: Stage,
        whose: Whose,
        detail: impl Into<String>,
        fix: impl Into<String>,
    ) -> Self {
        Self {
            stage,
            whose,
            detail: detail.into(),
            fix: fix.into(),
            typed: None,
        }
    }

    /// The error a normal login reports: what failed, whose side, what to try.
    pub fn into_error(self) -> Error {
        match self.typed {
            Some(e) => *e,
            None => Error::Remote(format!(
                "{} ({}) {}",
                self.detail,
                whose_label(self.whose),
                self.fix
            )),
        }
    }
}

fn whose_label(whose: Whose) -> &'static str {
    match whose {
        Whose::Internet => "internet or firewall problem",
        Whose::Server => "server problem",
        Whose::Access => "login or settings problem",
        Whose::Stackyard => "Stackyard problem",
    }
}

/// Turns the server address into IP addresses.
pub(crate) fn find(host: &str, port: u16) -> Result<Vec<SocketAddr>, Stopped> {
    let typed = host.trim();
    if typed.is_empty() || typed.contains("://") || typed.contains('/') || typed.contains(' ') {
        return Err(Stopped::new(
            Stage::Find,
            Whose::Access,
            format!("\"{host}\" is not a server address."),
            "Type only the server's name or IP address, e.g. example.com or 203.0.113.10, without https:// or a folder.",
        ));
    }
    match (typed, port).to_socket_addrs() {
        Ok(addrs) => {
            let addrs: Vec<SocketAddr> = addrs.collect();
            if addrs.is_empty() {
                Err(not_found(host, "no address"))
            } else {
                Ok(addrs)
            }
        }
        Err(e) => Err(not_found(host, &e.to_string())),
    }
}

fn not_found(host: &str, why: &str) -> Stopped {
    Stopped::new(
        Stage::Find,
        Whose::Internet,
        format!("Could not find the server {host} ({why})."),
        "Check the server address for typos. If it is right, check that this computer is online; a new domain can also take a few hours before it works everywhere, so try the server's IP address from the hosting panel.",
    )
}

/// Opens a connection to the server's port.
pub(crate) fn reach(connection: &Connection, addrs: &[SocketAddr]) -> Result<TcpStream, Stopped> {
    let mut last = None;
    for addr in addrs {
        match TcpStream::connect_timeout(addr, REACH_TIMEOUT) {
            Ok(tcp) => return Ok(tcp),
            Err(e) => last = Some(e),
        }
    }
    let (host, port) = (&connection.host, connection.port);
    let usual = match connection.protocol {
        Protocol::Sftp => "SFTP is usually port 22, but some hosts use their own (SiteGround 18765, Hostinger 65002, others 2222)",
        Protocol::Ftps | Protocol::Ftp => "FTP and FTPS are usually port 21",
        Protocol::Plugin => "HTTPS is port 443",
    };
    let e = last.expect("find gives at least one address");
    Err(match e.kind() {
        ErrorKind::ConnectionRefused => Stopped::new(
            Stage::Reach,
            Whose::Server,
            format!("{host} is there, but nothing accepts connections on port {port}."),
            format!("The port or the way to connect is probably wrong: {usual}. Check both in your hosting panel."),
        ),
        ErrorKind::TimedOut | ErrorKind::WouldBlock => Stopped::new(
            Stage::Reach,
            Whose::Internet,
            format!("{host} did not answer on port {port}."),
            format!("Something is blocking the way. Most often the host only lets in allowed IP addresses (add yours in the hosting panel, e.g. Cloudways or Kinsta \"IP allowlist\"), or it blocked your IP for a while after failed logins (wait 15 to 60 minutes). Also check the port: {usual}. A VPN or office network can block it too."),
        ),
        _ => Stopped::new(
            Stage::Reach,
            Whose::Internet,
            format!("Could not connect to {host} port {port}: {e}."),
            "Check that this computer is online, and try again without a VPN.",
        ),
    })
}

/// What went wrong in SSH's handshake, from the error and the server's own
/// goodbye message when it sent one.
pub(crate) fn handshake_failed(
    connection: &Connection,
    e: &russh::Error,
    goodbye: Option<String>,
) -> Stopped {
    let host = &connection.host;
    let said = said(goodbye);
    match e {
        russh::Error::Version => Stopped::new(
            Stage::Handshake,
            Whose::Access,
            format!("{host} port {} answered, but it is not an SSH (SFTP) server.", connection.port),
            "This port is for something else, often FTP. Check the SFTP port in the hosting panel, or choose FTP or FTPS if the host only offers those.",
        ),
        russh::Error::NoCommonAlgo { .. } | russh::Error::UnknownAlgo => Stopped::new(
            Stage::Handshake,
            Whose::Stackyard,
            format!("{host} and Stackyard could not agree on how to encrypt the connection ({e})."),
            "This is something Stackyard should support. Please report it with the name of the host; meanwhile FileZilla or WinSCP may connect.",
        ),
        russh::Error::Disconnect | russh::Error::HUP | russh::Error::IO(_) => Stopped::new(
            Stage::Handshake,
            Whose::Server,
            format!("{host} closed the connection before the login{said}."),
            "Servers do this to IP addresses they have blocked, often for a while after several failed logins, or that are not on their allowed list. Wait 15 to 60 minutes and try once, or add your IP address to the allowed list in the hosting panel. Also check that the port is the SFTP port, not FTP or the website.",
        ),
        _ => Stopped::new(
            Stage::Handshake,
            Whose::Server,
            format!("The secure handshake with {host} failed: {e}{said}."),
            "Check that the port is the SFTP (SSH) port. If it is, try again in a few minutes.",
        ),
    }
}

/// The server hung up when the session after login was opened.
pub(crate) fn session_failed(
    connection: &Connection,
    e: &russh::Error,
    goodbye: Option<String>,
) -> Stopped {
    let (host, user) = (&connection.host, &connection.user);
    let said = said(goodbye);
    let detail = match e {
        russh::Error::ChannelOpenFailure(why) => {
            let why = match why {
                russh::ChannelOpenFailure::AdministrativelyProhibited => {
                    "not allowed for this user"
                }
                russh::ChannelOpenFailure::ResourceShortage => "too many sessions open",
                _ => "it would not",
            };
            format!(
                "{host} accepted the login for {user} but refused to open a session ({why}){said}."
            )
        }
        russh::Error::Disconnect | russh::Error::HUP | russh::Error::IO(_) => {
            format!("{host} accepted the login for {user}, then closed the connection{said}.")
        }
        other => format!(
            "{host} accepted the login for {user}, but opening a session failed: {other}{said}."
        ),
    };
    Stopped::new(
        Stage::Session,
        Whose::Access,
        detail,
        format!("The login is right, but the account may not use it this way. In your hosting panel, check that SSH/SFTP access is turned on for {user} and the account is not suspended. Some hosts allow only one or two connections at once: close FileZilla or WinSCP, stop other clones from this server, wait a minute and try again."),
    )
}

/// The login was refused.
pub(crate) fn login_refused(
    connection: &Connection,
    with_key: bool,
    detail: Option<String>,
) -> Stopped {
    let user = &connection.user;
    let what = if with_key { "key" } else { "password" };
    Stopped::new(
        Stage::Login,
        Whose::Access,
        match detail {
            Some(d) => format!("{} refused the login for {user}: {d}.", connection.host),
            None => format!("{} refused the login for {user}.", connection.host),
        },
        format!("Check the user name and {what}. Use the SFTP/SSH or FTP user from the hosting panel, not your hosting account or WordPress login, and paste the password to avoid typos. Many wrong tries can get your IP blocked for a while."),
    )
}

fn said(goodbye: Option<String>) -> String {
    match goodbye
        .map(|g| g.trim().to_string())
        .filter(|g| !g.is_empty())
    {
        Some(g) => format!("; {g}"),
        None => String::new(),
    }
}

/// A finished check: `done` steps, then the one that stopped (if any),
/// then the rest marked skipped.
pub(crate) fn report(
    protocol: Protocol,
    done: Vec<Step>,
    stopped: Option<Stopped>,
    server_key: Option<String>,
) -> Report {
    let mut steps = done;
    let mut certificate = None;
    let mut key_changed = false;
    if let Some(stop) = stopped {
        match stop.typed.as_deref() {
            Some(Error::RemoteCertificate { fingerprint, .. }) => {
                certificate = Some(fingerprint.clone())
            }
            Some(Error::ServerKeyChanged { .. }) => key_changed = true,
            _ => {}
        }
        steps.push(Step {
            stage: stop.stage,
            status: Status::Failed,
            detail: stop.detail,
            whose: Some(stop.whose),
            fix: Some(stop.fix),
        });
    }
    for stage in Stage::all(protocol) {
        if !steps.iter().any(|s| s.stage == *stage) {
            steps.push(Step {
                stage: *stage,
                status: Status::Skipped,
                detail: String::new(),
                whose: None,
                fix: None,
            });
        }
    }
    Report {
        steps,
        server_key,
        certificate,
        key_changed,
    }
}

pub(crate) fn ok(stage: Stage, detail: impl Into<String>) -> Step {
    Step {
        stage,
        status: Status::Ok,
        detail: detail.into(),
        whose: None,
        fix: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn connection(host: &str, port: u16) -> Connection {
        Connection {
            id: "test0001".into(),
            site: "a.stack".into(),
            name: "Live".into(),
            protocol: Protocol::Sftp,
            host: host.into(),
            port,
            user: "me".into(),
            key_file: None,
            remote_dir: String::new(),
            local_dir: String::new(),
            skip: Vec::new(),
            server_key: None,
        }
    }

    #[test]
    fn a_web_address_is_not_a_server_address() {
        let stop = find("https://example.com/", 22).unwrap_err();
        assert_eq!(stop.stage, Stage::Find);
        assert!(stop.fix.contains("without https://"));
    }

    #[test]
    fn a_closed_port_is_the_servers_side() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let c = connection("127.0.0.1", port);
        let stop = reach(&c, &find(&c.host, port).unwrap()).unwrap_err();
        assert_eq!((stop.stage, stop.whose), (Stage::Reach, Whose::Server));
        assert!(stop.fix.contains("port 22"));
    }

    #[test]
    fn hanging_up_after_login_says_so_with_the_servers_words() {
        let c = connection("example.com", 22);
        let stop = session_failed(
            &c,
            &russh::Error::Disconnect,
            Some("the server said: \"Too many sessions\"".into()),
        );
        assert_eq!(stop.stage, Stage::Session);
        assert!(stop
            .detail
            .contains("accepted the login for me, then closed the connection"));
        assert!(stop.detail.contains("Too many sessions"));
        let text = stop.into_error().to_string();
        assert!(text.contains("(login or settings problem)"), "{text}");
    }

    #[test]
    fn skipped_steps_follow_the_one_that_failed() {
        let stop = Stopped::new(Stage::Login, Whose::Access, "no", "fix");
        let report = report(
            Protocol::Sftp,
            vec![
                ok(Stage::Find, "a"),
                ok(Stage::Reach, "b"),
                ok(Stage::Handshake, "c"),
            ],
            Some(stop),
            None,
        );
        let states: Vec<Status> = report.steps.iter().map(|s| s.status).collect();
        assert_eq!(
            states,
            [
                Status::Ok,
                Status::Ok,
                Status::Ok,
                Status::Failed,
                Status::Skipped,
                Status::Skipped,
                Status::Skipped
            ]
        );
    }
}
