//! SFTP over SSH, through `russh` (pure Rust, so nothing to install).
//!
//! The server's key is trusted on first connect and remembered with the
//! connection; a different key later is refused, as OpenSSH does.

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use russh::client::{self, Handle, KeyboardInteractiveAuthResponse};
use russh::keys::{HashAlg, PrivateKeyWithHashAlg, PublicKeyOrCertificate};
use russh_sftp::client::SftpSession;
use russh_sftp::protocol::FileAttributes;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};

use super::check::{self, Stage, Status, Stopped, Whose};
use super::{io_error, partial_name, set_mtime, Connection, Entry, OnBytes, Remote};
use crate::{paths, Error, Result};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
/// Longest a single step (one listing, one chunk of a file) may take.
const STEP_TIMEOUT: Duration = Duration::from_secs(60);
const CHUNK: usize = 64 * 1024;

fn fail(message: impl Into<String>) -> Error {
    Error::Remote(message.into())
}

/// The server's goodbye, or why the connection broke, as a phrase.
type Goodbye = Arc<Mutex<Option<String>>>;

struct Client {
    trusted: Option<String>,
    seen: Arc<Mutex<Option<String>>>,
    goodbye: Goodbye,
}

impl client::Handler for Client {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        key: &PublicKeyOrCertificate,
    ) -> std::result::Result<bool, Self::Error> {
        let data = match key {
            PublicKeyOrCertificate::PublicKey { key, .. } => key.key_data(),
            PublicKeyOrCertificate::Certificate(cert) => cert.public_key(),
        };
        let seen = data.fingerprint(HashAlg::Sha256).to_string();
        let ok = self.trusted.as_deref().is_none_or(|t| t == seen);
        *self.seen.lock().unwrap() = Some(seen);
        Ok(ok)
    }

    async fn disconnected(
        &mut self,
        reason: client::DisconnectReason<Self::Error>,
    ) -> std::result::Result<(), Self::Error> {
        match reason {
            client::DisconnectReason::ReceivedDisconnect(info) => {
                let message = info.message.trim();
                *self.goodbye.lock().unwrap() = Some(if message.is_empty() {
                    format!("the server gave the reason {:?}", info.reason_code)
                } else {
                    format!("the server said: \"{message}\"")
                });
                Ok(())
            }
            client::DisconnectReason::Error(e) => {
                *self.goodbye.lock().unwrap() = Some(format!("the connection broke: {e}"));
                Err(e)
            }
        }
    }
}

pub(crate) struct Sftp {
    runtime: tokio::runtime::Runtime,
    ssh: Handle<Client>,
    sftp: SftpSession,
    connection: Connection,
    goodbye: Goodbye,
}

/// The server's goodbye, once the connection task has had a moment to read it.
async fn goodbye_of(goodbye: &Goodbye) -> Option<String> {
    tokio::time::sleep(Duration::from_millis(200)).await;
    goodbye.lock().unwrap().clone()
}

impl Sftp {
    /// Logs in; returns the session and the server key's fingerprint.
    pub(crate) fn connect(
        connection: &Connection,
        secret: Option<String>,
    ) -> Result<(Self, String)> {
        Self::open(connection, secret, &mut Vec::new()).map_err(Stopped::into_error)
    }

    /// Logs in step by step, adding each step that worked to `done`.
    fn open(
        connection: &Connection,
        secret: Option<String>,
        done: &mut Vec<check::Step>,
    ) -> std::result::Result<(Self, String), Stopped> {
        let (host, port) = (connection.host.as_str(), connection.port);
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .thread_name("stackyard-sftp")
            .enable_all()
            .build()
            .map_err(|e| {
                Stopped::new(
                    Stage::Handshake,
                    Whose::Stackyard,
                    format!("Stackyard could not start SFTP: {e}."),
                    "Restart Stackyard and try again.",
                )
            })?;
        let addrs = check::find(host, port)?;
        done.push(check::ok(
            Stage::Find,
            format!("{host} is at {}.", addrs[0].ip()),
        ));
        let tcp = check::reach(connection, &addrs)?;
        done.push(check::ok(Stage::Reach, format!("Port {port} answered.")));
        // An FTP server greets first with "220"; SSH would say "SSH-".
        let _ = tcp.set_read_timeout(Some(Duration::from_secs(5)));
        let mut greeting = [0u8; 4];
        if tcp.peek(&mut greeting).is_ok_and(|n| n >= 3) && greeting.starts_with(b"220") {
            return Err(Stopped::new(
                Stage::Handshake,
                Whose::Access,
                format!("{host} port {port} is an FTP server, not SFTP."),
                "Choose FTP or FTPS under Connect with, or use the SFTP port from the hosting panel (usually 22).",
            ));
        }
        let _ = tcp.set_read_timeout(None);
        let seen = Arc::new(Mutex::new(None));
        let goodbye: Goodbye = Arc::new(Mutex::new(None));
        let handler = Client {
            trusted: connection.server_key.clone(),
            seen: seen.clone(),
            goodbye: goodbye.clone(),
        };
        let (ssh, sftp) = runtime.block_on(async {
            let config = Arc::new(client::Config {
                keepalive_interval: Some(Duration::from_secs(20)),
                ..Default::default()
            });
            let stream = tcp
                .set_nonblocking(true)
                .and_then(|()| tokio::net::TcpStream::from_std(tcp))
                .map_err(|e| {
                    Stopped::new(
                        Stage::Reach,
                        Whose::Stackyard,
                        format!("Stackyard could not use the connection: {e}."),
                        "Restart Stackyard and try again.",
                    )
                })?;
            let _ = stream.set_nodelay(true);
            let mut ssh = match tokio::time::timeout(
                CONNECT_TIMEOUT,
                client::connect_stream(config, stream, handler),
            )
            .await
            {
                Err(_) => {
                    return Err(Stopped::new(
                        Stage::Handshake,
                        Whose::Server,
                        format!("{host} port {port} answered, but did not start SSH."),
                        "This port is probably for something else, such as FTP or the website. Check the SFTP (SSH) port in the hosting panel.",
                    ))
                }
                Ok(Err(e)) => {
                    let found = seen.lock().unwrap().clone();
                    return Err(match (found, &connection.server_key) {
                        (Some(found), Some(expected)) if &found != expected => {
                            let mut stop = Stopped::new(
                                Stage::Handshake,
                                Whose::Server,
                                format!("{host} has a different key than when you first connected."),
                                "If the host moved or rebuilt the server, forget the old key in the connection's settings. If not, don't connect: someone may be listening in.",
                            );
                            stop.typed = Some(Box::new(Error::ServerKeyChanged {
                                expected: expected.clone(),
                                found,
                            }));
                            stop
                        }
                        _ => check::handshake_failed(connection, &e, goodbye_of(&goodbye).await),
                    });
                }
                Ok(Ok(ssh)) => ssh,
            };
            done.push(check::ok(
                Stage::Handshake,
                format!(
                    "Secure connection made; the server's key is {}.",
                    seen.lock().unwrap().clone().unwrap_or_default()
                ),
            ));
            authenticate(&mut ssh, connection, secret, &goodbye).await?;
            done.push(check::ok(
                Stage::Login,
                format!("Logged in as {}.", connection.user),
            ));
            let channel = match tokio::time::timeout(STEP_TIMEOUT, ssh.channel_open_session()).await {
                Ok(Ok(channel)) => channel,
                Ok(Err(e)) => {
                    return Err(check::session_failed(connection, &e, goodbye_of(&goodbye).await))
                }
                Err(_) => {
                    return Err(check::session_failed(
                        connection,
                        &russh::Error::ConnectionTimeout,
                        None,
                    ))
                }
            };
            done.push(check::ok(Stage::Session, "Session opened."));
            let no_sftp = |e: String| {
                Stopped::new(
                    Stage::Files,
                    Whose::Access,
                    format!(
                        "Logged in, but {host} does not offer SFTP to {} ({e}).",
                        connection.user
                    ),
                    "Turn on SFTP for this user in the hosting panel, or connect with FTPS or FTP instead.",
                )
            };
            channel
                .request_subsystem(true, "sftp")
                .await
                .map_err(|e| no_sftp(e.to_string()))?;
            let sftp = SftpSession::new(channel.into_stream())
                .await
                .map_err(|e| no_sftp(e.to_string()))?;
            sftp.set_timeout(STEP_TIMEOUT.as_secs());
            Ok((ssh, sftp))
        })?;
        let key = seen.lock().unwrap().clone().unwrap_or_default();
        Ok((
            Self {
                runtime,
                ssh,
                sftp,
                connection: connection.clone(),
                goodbye,
            },
            key,
        ))
    }

    fn step<T>(
        &self,
        what: &str,
        work: impl std::future::Future<
            Output = std::result::Result<T, russh_sftp::client::error::Error>,
        >,
    ) -> Result<T> {
        self.runtime.block_on(async {
            match tokio::time::timeout(STEP_TIMEOUT, work).await {
                Ok(Ok(value)) => Ok(value),
                Ok(Err(e)) => Err(fail(format!("{what}: {}", sftp_error(e)))),
                Err(_) => Err(fail(format!("{what}: the server stopped answering"))),
            }
        })
    }
}

fn sftp_error(e: russh_sftp::client::error::Error) -> String {
    use russh_sftp::client::error::Error as E;
    use russh_sftp::protocol::StatusCode;
    match e {
        E::Status(status) => match status.status_code {
            StatusCode::NoSuchFile => "no such file or folder".into(),
            StatusCode::PermissionDenied => "permission denied".into(),
            _ if !status.error_message.is_empty() => status.error_message,
            code => format!("{code:?}"),
        },
        E::Timeout => "the server stopped answering".into(),
        other => other.to_string(),
    }
}

async fn authenticate(
    ssh: &mut Handle<Client>,
    connection: &Connection,
    secret: Option<String>,
    goodbye: &Goodbye,
) -> std::result::Result<(), Stopped> {
    let user = connection.user.clone();
    let with_key = connection.key_file.is_some();
    let refused = || check::login_refused(connection, with_key, None);
    // The server hung up in the middle of logging in.
    let broke = |e: russh::Error, said: Option<String>| {
        let said = said.map(|s| format!("; {s}")).unwrap_or_default();
        Stopped::new(
            Stage::Login,
            Whose::Server,
            format!("{} closed the connection while logging in ({e}{said}).", connection.host),
            "Servers do this after too many wrong tries, or to IP addresses they don't allow. Check the user name and password, wait 15 minutes, and try once.",
        )
    };
    if let Some(file) = &connection.key_file {
        let file = paths::expand_home(file);
        let key = russh::keys::load_secret_key(&file, secret.as_deref()).map_err(|e| {
            let text = e.to_string();
            let detail = if secret.is_none() && text.to_lowercase().contains("encrypt") {
                format!("{} is protected by a passphrase.", file.display())
            } else {
                format!("Could not read the key {}: {text}.", file.display())
            };
            Stopped::new(
                Stage::Login,
                Whose::Access,
                detail,
                "Enter the key's passphrase as the password, or pick the private key file (not the .pub one).",
            )
        })?;
        let hash = match ssh.best_supported_rsa_hash().await {
            Ok(hash) => hash.flatten(),
            Err(e) => return Err(broke(e, goodbye_of(goodbye).await)),
        };
        let result = match ssh
            .authenticate_publickey(user, PrivateKeyWithHashAlg::new(Arc::new(key), hash))
            .await
        {
            Ok(result) => result,
            Err(e) => return Err(broke(e, goodbye_of(goodbye).await)),
        };
        return if result.success() {
            Ok(())
        } else {
            Err(refused())
        };
    }
    let password = secret.unwrap_or_default();
    match ssh
        .authenticate_password(user.clone(), password.clone())
        .await
    {
        Ok(result) if result.success() => return Ok(()),
        Ok(_) => {}
        Err(e) => return Err(broke(e, goodbye_of(goodbye).await)),
    }
    // Some servers only ask for the password through "keyboard-interactive".
    let mut reply = ssh
        .authenticate_keyboard_interactive_start(user, None)
        .await
        .map_err(|_| refused())?;
    for _ in 0..3 {
        match reply {
            KeyboardInteractiveAuthResponse::Success => return Ok(()),
            KeyboardInteractiveAuthResponse::Failure { .. } => return Err(refused()),
            KeyboardInteractiveAuthResponse::InfoRequest { prompts, .. } => {
                let answers = prompts.iter().map(|_| password.clone()).collect();
                reply = ssh
                    .authenticate_keyboard_interactive_respond(answers)
                    .await
                    .map_err(|_| refused())?;
            }
        }
    }
    Err(refused())
}

/// Logs in step by step, then tries the start folder and a command.
pub(crate) fn check(connection: &Connection, secret: Option<String>) -> check::Report {
    let mut done = Vec::new();
    let (mut sftp, key) = match Sftp::open(connection, secret, &mut done) {
        Ok(opened) => opened,
        Err(stop) => return check::report(connection.protocol, done, Some(stop), None),
    };
    match sftp.start_dir() {
        Ok(dir) => done.push(check::ok(
            Stage::Files,
            format!("SFTP works; the server starts in {dir}."),
        )),
        Err(e) => {
            let stop = Stopped::new(
                Stage::Files,
                Whose::Server,
                format!("SFTP started, but the start folder could not be read: {e}."),
                "Check in the hosting panel that the user has a home folder it may read.",
            );
            return check::report(connection.protocol, done, Some(stop), Some(key));
        }
    }
    let mut out = Vec::new();
    let works = sftp.exec("echo stackyard", &mut out).is_ok_and(|exit| {
        exit.ok()
            && String::from_utf8_lossy(&out)
                .lines()
                .any(|l| l.trim() == "stackyard")
    });
    done.push(if works {
        check::ok(
            Stage::Commands,
            "Commands work, so databases are copied with the server's own tools (fastest).",
        )
    } else {
        check::Step {
            stage: Stage::Commands,
            status: Status::Warning,
            detail: "This user may only move files, not run commands.".into(),
            whose: Some(Whose::Access),
            fix: Some("Cloning still works: Stackyard puts a small temporary helper on the website to copy the database. For big databases it is faster with SSH (shell) access turned on for this user in the hosting panel.".into()),
        }
    });
    check::report(connection.protocol, done, None, Some(key))
}

impl Drop for Sftp {
    fn drop(&mut self) {
        let ssh = &self.ssh;
        let sftp = &self.sftp;
        self.runtime.block_on(async {
            let _ = tokio::time::timeout(Duration::from_secs(2), async {
                let _ = sftp.close().await;
                let _ = ssh
                    .disconnect(russh::Disconnect::ByApplication, "", "en")
                    .await;
            })
            .await;
        });
    }
}

impl Remote for Sftp {
    fn list(&mut self, dir: &str) -> Result<Vec<Entry>> {
        let entries = self.step(&format!("Listing {dir}"), self.sftp.read_dir(dir))?;
        let mut out = Vec::new();
        for entry in entries {
            let name = entry.file_name();
            if name == "." || name == ".." {
                continue;
            }
            let mut meta = entry.metadata();
            if entry.file_type().is_symlink() {
                // Where the link points decides whether it is a file or folder.
                match self.step(
                    "Following a link",
                    self.sftp.metadata(super::join(dir, &name)),
                ) {
                    Ok(target) => meta = target,
                    Err(_) => continue,
                }
            }
            let dir = meta.file_type().is_dir();
            if !dir && !meta.file_type().is_file() {
                continue;
            }
            out.push(Entry {
                name,
                dir,
                size: if dir { 0 } else { meta.size.unwrap_or(0) },
                modified: meta.mtime.map(i64::from),
            });
        }
        Ok(out)
    }

    fn download(&mut self, remote: &str, local: &Path, on_bytes: OnBytes) -> Result<()> {
        let what = format!("Downloading {remote}");
        let mut source = self.step(&what, self.sftp.open(remote))?;
        let meta = self.step(&what, source.metadata()).ok();
        let modified = meta.as_ref().and_then(|m| m.mtime).map(i64::from);
        let size = meta.as_ref().and_then(|m| m.size);
        let part = partial_name(local);
        let (mut file, mut done) = super::open_partial(&part, size, modified)?;
        if done > 0 {
            let seek = self.runtime.block_on(async {
                tokio::time::timeout(STEP_TIMEOUT, source.seek(std::io::SeekFrom::Start(done)))
                    .await
            });
            if !matches!(seek, Ok(Ok(_))) {
                (file, done) = super::open_partial(&part, None, None)?;
            }
        }
        let mut buf = vec![0u8; CHUNK];
        let copied = loop {
            let read = self.runtime.block_on(async {
                tokio::time::timeout(STEP_TIMEOUT, source.read(&mut buf)).await
            });
            match read {
                Ok(Ok(0)) => break Ok(()),
                Ok(Ok(n)) => {
                    if let Err(e) = std::io::Write::write_all(&mut file, &buf[..n]) {
                        break Err(Error::Io {
                            path: part.clone(),
                            source: e,
                        });
                    }
                    done += n as u64;
                    if !on_bytes(done) {
                        break Err(fail("stopped"));
                    }
                }
                Ok(Err(e)) => break Err(fail(format!("{what}: {e}"))),
                Err(_) => break Err(fail(format!("{what}: the server stopped answering"))),
            }
        };
        self.runtime.block_on(async {
            let _ = source.shutdown().await;
        });
        if let Err(e) = copied {
            super::keep_partial(file, &part, modified);
            return Err(e);
        }
        if let Some(at) = modified {
            set_mtime(&file, at);
        }
        drop(file);
        std::fs::rename(&part, local).map_err(io_error(local))
    }

    fn upload(&mut self, local: &Path, remote: &str, on_bytes: OnBytes) -> Result<()> {
        let what = format!("Uploading {remote}");
        let mut file = std::fs::File::open(local).map_err(io_error(local))?;
        let modified = file.metadata().ok().as_ref().and_then(super::mtime_of);
        let mut target = self.step(&what, self.sftp.create(remote))?;
        let mut buf = vec![0u8; CHUNK];
        let mut done = 0u64;
        let copied = loop {
            let n = match std::io::Read::read(&mut file, &mut buf) {
                Ok(n) => n,
                Err(e) => {
                    break Err(Error::Io {
                        path: local.to_owned(),
                        source: e,
                    })
                }
            };
            if n == 0 {
                break Ok(());
            }
            let wrote = self.runtime.block_on(async {
                tokio::time::timeout(STEP_TIMEOUT, target.write_all(&buf[..n])).await
            });
            match wrote {
                Ok(Ok(())) => {}
                Ok(Err(e)) => break Err(fail(format!("{what}: {e}"))),
                Err(_) => break Err(fail(format!("{what}: the server stopped answering"))),
            }
            done += n as u64;
            if !on_bytes(done) {
                break Err(fail("stopped"));
            }
        };
        let closed = self
            .runtime
            .block_on(async { tokio::time::timeout(STEP_TIMEOUT, target.shutdown()).await });
        copied?;
        match closed {
            Ok(Ok(())) => {}
            Ok(Err(e)) => return Err(fail(format!("{what}: {e}"))),
            Err(_) => return Err(fail(format!("{what}: the server stopped answering"))),
        }
        if let Some(at) = modified {
            let times = FileAttributes {
                atime: Some(at as u32),
                mtime: Some(at as u32),
                ..FileAttributes::empty()
            };
            // Not every server allows it; the upload itself still counts.
            let _ = self.step(&what, self.sftp.set_metadata(remote, times));
        }
        Ok(())
    }

    fn make_dir(&mut self, dir: &str) -> Result<()> {
        let what = format!("Making {dir}");
        match self.step(&what, self.sftp.create_dir(dir)) {
            Ok(()) => Ok(()),
            Err(e) => match self.step(&what, self.sftp.metadata(dir)) {
                Ok(meta) if meta.file_type().is_dir() => Ok(()),
                _ => Err(e),
            },
        }
    }

    fn remove_file(&mut self, path: &str) -> Result<()> {
        self.step(&format!("Deleting {path}"), self.sftp.remove_file(path))
    }

    fn start_dir(&mut self) -> Result<String> {
        self.step("Finding the start folder", self.sftp.canonicalize("."))
    }

    fn exec(&mut self, command: &str, out: &mut dyn std::io::Write) -> Result<super::Exit> {
        use russh::ChannelMsg;
        const KEEP: usize = 8 * 1024;
        let idle = Duration::from_secs(super::EXEC_IDLE);
        self.runtime.block_on(async {
            let mut channel =
                match tokio::time::timeout(STEP_TIMEOUT, self.ssh.channel_open_session()).await {
                    Ok(Ok(channel)) => channel,
                    Ok(Err(e)) => {
                        let said = goodbye_of(&self.goodbye).await;
                        return Err(check::session_failed(&self.connection, &e, said).into_error());
                    }
                    Err(_) => return Err(fail("the server stopped answering")),
                };
            channel
                .exec(true, command)
                .await
                .map_err(|e| fail(format!("the server would not run the command: {e}")))?;
            let mut code = None;
            let mut errors: Vec<u8> = Vec::new();
            loop {
                let message = tokio::time::timeout(idle, channel.wait())
                    .await
                    .map_err(|_| fail("the command stopped answering"))?;
                match message {
                    Some(ChannelMsg::Data { data }) => {
                        out.write_all(&data).map_err(|e| fail(e.to_string()))?
                    }
                    Some(ChannelMsg::ExtendedData { data, .. }) => {
                        errors.extend_from_slice(&data);
                        if errors.len() > KEEP {
                            errors.drain(..errors.len() - KEEP);
                        }
                    }
                    Some(ChannelMsg::ExitStatus { exit_status }) => code = Some(exit_status),
                    Some(ChannelMsg::Failure) => {
                        return Err(fail("the server refused to run commands (no shell access)"))
                    }
                    Some(_) => {}
                    None => break,
                }
            }
            Ok(super::Exit {
                // No status at all: the server dropped the command (killed, or no shell).
                code: code.unwrap_or(255),
                errors: String::from_utf8_lossy(&errors).trim().to_string(),
            })
        })
    }
}
