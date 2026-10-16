//! Remote servers for a site: browse, upload and download its files over
//! SFTP (SSH), FTPS or plain FTP.
//!
//! Connections are kept in `remote.toml`. Passwords and key passphrases are
//! never written there: they go to the system's own password store (see
//! [`secrets`]). Hosting panels such as Cloudways, cPanel or Plesk, and any
//! VPS, all offer SFTP or FTP, so these cover them without a separate
//! integration per host.

mod ftp;
pub mod secrets;
mod sftp;
pub mod sync;

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::settings::{read_toml, write_toml};
use crate::{paths, Error, Result};

pub use sync::{Action, Compare, Direction, Outcome, Plan, PlannedFile, Progress};

const FILE: &str = "remote.toml";
const MAX_NAME: usize = 80;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    /// Files over SSH; what most hosts and every VPS offer.
    Sftp,
    /// FTP with TLS (explicit, `AUTH TLS`).
    Ftps,
    /// Plain FTP: the password and files travel unencrypted.
    Ftp,
}

impl Protocol {
    pub fn default_port(self) -> u16 {
        match self {
            Protocol::Sftp => 22,
            Protocol::Ftps | Protocol::Ftp => 21,
        }
    }
}

/// One server a site's files go to, e.g. "Production".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Connection {
    /// Eight letters and digits, made when the connection is added.
    pub id: String,
    /// The site's domain, e.g. `myapp.stack`.
    pub site: String,
    pub name: String,
    pub protocol: Protocol,
    pub host: String,
    pub port: u16,
    pub user: String,
    /// SFTP only: log in with this private key instead of a password. The
    /// stored secret is then the key's passphrase, if it has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_file: Option<PathBuf>,
    /// The folder on the server that matches the site's folder, e.g.
    /// `/home/master/applications/abc/public_html`. Empty means the folder
    /// the server starts in.
    #[serde(default)]
    pub remote_dir: String,
    /// Part of the site's folder to transfer, e.g. `public`; empty means all of it.
    #[serde(default)]
    pub local_dir: String,
    /// Names or paths never transferred, e.g. `.git` or `storage/logs`.
    #[serde(default = "default_skip")]
    pub skip: Vec<String>,
    /// The server key (SFTP) or certificate (FTPS) fingerprint this
    /// connection trusts. SFTP records the key on first connect and refuses
    /// a different one later; FTPS sets it only when the user chooses to
    /// trust a certificate the system does not.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_key: Option<String>,
}

pub fn default_skip() -> Vec<String> {
    [".git", "node_modules", ".DS_Store", "Thumbs.db", ".env"]
        .map(String::from)
        .to_vec()
}

/// What a person fills in to add or change a connection.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct ConnectionInput {
    pub site: String,
    pub name: String,
    pub protocol: Option<Protocol>,
    pub host: String,
    pub port: Option<u16>,
    pub user: String,
    #[serde(default)]
    pub key_file: Option<PathBuf>,
    #[serde(default)]
    pub remote_dir: String,
    #[serde(default)]
    pub local_dir: String,
    pub skip: Option<Vec<String>>,
    /// The password, or the key's passphrase. `None` keeps the stored one;
    /// an empty string removes it.
    #[serde(default)]
    pub password: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct ConnectionFile {
    #[serde(default, rename = "connection")]
    connections: Vec<Connection>,
}

/// The connections in `remote.toml`.
#[derive(Debug, Default)]
pub struct RemoteStore {
    connections: Vec<Connection>,
    file: PathBuf,
}

impl RemoteStore {
    pub fn load() -> Result<Self> {
        Self::load_from(&paths::home().join(FILE))
    }

    pub fn load_from(path: &Path) -> Result<Self> {
        let file: ConnectionFile = read_toml(path)?.unwrap_or_default();
        Ok(Self {
            connections: file.connections,
            file: path.to_owned(),
        })
    }

    pub fn save(&self) -> Result<()> {
        write_toml(
            &self.file,
            &ConnectionFile {
                connections: self.connections.clone(),
            },
        )
    }

    pub fn list(&self) -> &[Connection] {
        &self.connections
    }

    pub fn for_site<'a>(&'a self, site: &'a str) -> impl Iterator<Item = &'a Connection> {
        self.connections.iter().filter(move |c| c.site == site)
    }

    pub fn get(&self, id: &str) -> Result<&Connection> {
        self.connections
            .iter()
            .find(|c| c.id == id)
            .ok_or_else(|| Error::Remote(format!("no connection with id {id:?}")))
    }

    fn get_mut(&mut self, id: &str) -> Result<&mut Connection> {
        self.connections
            .iter_mut()
            .find(|c| c.id == id)
            .ok_or_else(|| Error::Remote(format!("no connection with id {id:?}")))
    }

    /// Adds a connection and stores its password, if one was given.
    pub fn add(&mut self, input: ConnectionInput) -> Result<Connection> {
        let connection = validate(new_id()?, input.clone(), None)?;
        store_password(&connection.id, input.password.as_deref())?;
        self.connections.push(connection.clone());
        Ok(connection)
    }

    /// Changes a connection. A new host, port, user or protocol forgets the
    /// trusted server key, since it belongs to the old server.
    pub fn update(&mut self, id: &str, input: ConnectionInput) -> Result<Connection> {
        let old = self.get(id)?.clone();
        let connection = validate(old.id.clone(), input.clone(), Some(&old))?;
        store_password(id, input.password.as_deref())?;
        *self.get_mut(id)? = connection.clone();
        Ok(connection)
    }

    /// Removes a connection and its stored password.
    pub fn remove(&mut self, id: &str) -> Result<Connection> {
        let index = self
            .connections
            .iter()
            .position(|c| c.id == id)
            .ok_or_else(|| Error::Remote(format!("no connection with id {id:?}")))?;
        let _ = secrets::delete(&secret_account(id));
        Ok(self.connections.remove(index))
    }

    /// Trusts this server key or certificate fingerprint from now on; `None` forgets it.
    pub fn set_server_key(&mut self, id: &str, key: Option<String>) -> Result<()> {
        self.get_mut(id)?.server_key = key;
        Ok(())
    }

    /// Removes every connection of a site, e.g. when the site is removed.
    pub fn forget_site(&mut self, site: &str) {
        let ids: Vec<String> = self.for_site(site).map(|c| c.id.clone()).collect();
        for id in ids {
            let _ = self.remove(&id);
        }
    }
}

fn secret_account(id: &str) -> String {
    format!("remote-{id}")
}

fn store_password(id: &str, password: Option<&str>) -> Result<()> {
    match password {
        None => Ok(()),
        Some("") => secrets::delete(&secret_account(id)),
        Some(secret) => secrets::set(&secret_account(id), secret),
    }
}

/// Whether a password or passphrase is stored for this connection.
pub fn has_password(id: &str) -> bool {
    matches!(secrets::get(&secret_account(id)), Ok(Some(_)))
}

fn validate(id: String, input: ConnectionInput, old: Option<&Connection>) -> Result<Connection> {
    let bad = |m: &str| Err(Error::Remote(m.to_string()));
    let name = input.name.trim().to_string();
    if name.is_empty() || name.chars().count() > MAX_NAME {
        return bad("give the connection a name of up to 80 characters, e.g. Production");
    }
    let host = input.host.trim().trim_end_matches('/').to_string();
    let host = host
        .strip_prefix("sftp://")
        .or_else(|| host.strip_prefix("ftp://"))
        .or_else(|| host.strip_prefix("ftps://"))
        .unwrap_or(&host)
        .to_string();
    if host.is_empty() || host.contains(char::is_whitespace) || host.contains('/') {
        return bad("enter the server's address, e.g. 203.0.113.10 or ftp.example.com");
    }
    let user = input.user.trim().to_string();
    if user.is_empty() {
        return bad("enter the user name to log in with");
    }
    if input.site.trim().is_empty() {
        return bad("pick the site this connection belongs to");
    }
    let protocol = input.protocol.unwrap_or(Protocol::Sftp);
    let port = input.port.unwrap_or(protocol.default_port());
    if port == 0 {
        return bad("the port must be between 1 and 65535");
    }
    let key_file = match input.key_file {
        Some(file) if !file.as_os_str().is_empty() => {
            if protocol != Protocol::Sftp {
                return bad("key files work with SFTP only");
            }
            Some(paths::expand_home(&file))
        }
        _ => None,
    };
    let local_dir = input
        .local_dir
        .trim()
        .trim_matches(['/', '\\'])
        .replace('\\', "/");
    if !is_relative_inside(&local_dir) {
        return bad("the local folder must be a folder inside the site, e.g. public");
    }
    let remote_dir = input.remote_dir.trim().to_string();
    let remote_dir = if remote_dir.len() > 1 {
        remote_dir.trim_end_matches('/').to_string()
    } else {
        remote_dir
    };
    let skip = input
        .skip
        .unwrap_or_else(|| old.map_or_else(default_skip, |o| o.skip.clone()))
        .into_iter()
        .map(|s| s.trim().trim_matches('/').to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let server_key = old.and_then(|o| {
        let same = o.host == host && o.port == port && o.protocol == protocol;
        same.then(|| o.server_key.clone()).flatten()
    });
    Ok(Connection {
        id,
        site: input.site.trim().to_string(),
        name,
        protocol,
        host,
        port,
        user,
        key_file,
        remote_dir,
        local_dir,
        skip,
        server_key,
    })
}

/// A relative path that stays inside its folder: no `..`, no drive or root.
pub(crate) fn is_relative_inside(path: &str) -> bool {
    Path::new(path)
        .components()
        .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
        && !path.split('/').any(|part| part == "..")
}

fn new_id() -> Result<String> {
    const ALPHABET: &[u8] = b"abcdefghijkmnpqrstuvwxyz23456789";
    let mut bytes = [0u8; 8];
    getrandom::fill(&mut bytes).map_err(|e| Error::Remote(format!("no randomness: {e}")))?;
    Ok(bytes
        .iter()
        .map(|b| ALPHABET[usize::from(*b) % ALPHABET.len()] as char)
        .collect())
}

/// A file or folder on the server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Entry {
    pub name: String,
    pub dir: bool,
    pub size: u64,
    /// Last changed, in seconds since 1970; unknown on some FTP servers.
    pub modified: Option<i64>,
}

/// Reports bytes moved so far for the current file; returning `false` stops the transfer.
pub type OnBytes<'a> = &'a mut dyn FnMut(u64) -> bool;

/// What every protocol offers. Paths are absolute, `/`-separated server paths.
pub(crate) trait Remote: Send {
    fn list(&mut self, dir: &str) -> Result<Vec<Entry>>;
    /// Writes the file to `local`, replacing it only once complete, and
    /// gives it the server's modification time when known.
    fn download(&mut self, remote: &str, local: &Path, on_bytes: OnBytes) -> Result<()>;
    /// Uploads the file and, where the server allows, keeps its local
    /// modification time, so later comparisons see the two as the same.
    fn upload(&mut self, local: &Path, remote: &str, on_bytes: OnBytes) -> Result<()>;
    /// Makes a folder; succeeds when it already exists.
    fn make_dir(&mut self, dir: &str) -> Result<()>;
    fn remove_file(&mut self, path: &str) -> Result<()>;
    /// The folder the server starts in, which also checks the connection is alive.
    fn start_dir(&mut self) -> Result<String>;
}

/// Logs in. For SFTP, the server key it presented is checked against the
/// trusted one, and returned so a first connect can remember it.
fn open(connection: &Connection) -> Result<(Box<dyn Remote>, Option<String>)> {
    let secret = secrets::get(&secret_account(&connection.id))?;
    match connection.protocol {
        Protocol::Sftp => {
            let (session, key) = sftp::Sftp::connect(connection, secret)?;
            Ok((Box::new(session), Some(key)))
        }
        Protocol::Ftps | Protocol::Ftp => {
            let session = ftp::Ftp::connect(connection, secret.as_deref())?;
            Ok((Box::new(session), None))
        }
    }
}

/// Open logins, reused between steps so browsing does not log in on every
/// click. A login that went stale is replaced on next use.
#[derive(Default)]
pub struct Sessions {
    open: Mutex<HashMap<String, Box<dyn Remote>>>,
}

impl Sessions {
    /// Runs `work` with a logged-in session for this connection.
    fn with<T>(
        &self,
        connection: &Connection,
        work: impl FnOnce(&mut dyn Remote) -> Result<T>,
    ) -> Result<T> {
        let cached = self.open.lock().unwrap().remove(&connection.id);
        let alive = cached.and_then(|mut s| s.start_dir().is_ok().then_some(s));
        let mut session = match alive {
            Some(s) => s,
            None => {
                let (session, key) = open(connection)?;
                if connection.server_key.is_none() {
                    if let Some(key) = key {
                        remember_key(&connection.id, key)?;
                    }
                }
                session
            }
        };
        let result = work(session.as_mut());
        if result.is_ok() {
            self.open
                .lock()
                .unwrap()
                .insert(connection.id.clone(), session);
        }
        result
    }

    /// Logs out of this connection, e.g. after its settings changed.
    pub fn close(&self, id: &str) {
        let session = self.open.lock().unwrap().remove(id);
        if let Some(session) = session {
            // Logging out waits on the server; never hold up the caller.
            std::thread::spawn(move || drop(session));
        }
    }

    /// Logs in and reports where the server starts, to check the settings work.
    pub fn test(&self, connection: &Connection) -> Result<String> {
        self.close(&connection.id);
        self.with(connection, |r| r.start_dir())
    }

    /// The contents of a server folder; `None` lists the connection's own folder.
    pub fn browse(&self, connection: &Connection, dir: Option<&str>) -> Result<Listing> {
        self.with(connection, |r| {
            let root = remote_root(connection, r)?;
            let dir = dir.map_or_else(|| root.clone(), str::to_string);
            let mut entries = r.list(&dir)?;
            entries.sort_by(|a, b| b.dir.cmp(&a.dir).then_with(|| a.name.cmp(&b.name)));
            Ok(Listing { dir, root, entries })
        })
    }

    /// Works out what to transfer; nothing is changed yet. `only` limits it
    /// to these paths, relative to the connection's folders, which are then
    /// transferred even if unchanged.
    #[allow(clippy::too_many_arguments)]
    pub fn plan(
        &self,
        connection: &Connection,
        local_root: &Path,
        direction: Direction,
        compare: Compare,
        delete: bool,
        only: Option<&[String]>,
        progress: &mut dyn FnMut(Progress) -> bool,
    ) -> Result<Plan> {
        self.with(connection, |r| {
            let root = remote_root(connection, r)?;
            sync::plan(
                r,
                &sync::Sides {
                    local: local_root,
                    remote: &root,
                    skip: &connection.skip,
                },
                direction,
                compare,
                delete,
                only,
                progress,
            )
        })
    }

    /// Carries out a plan made by [`Sessions::plan`].
    pub fn run(
        &self,
        connection: &Connection,
        local_root: &Path,
        files: &[PlannedFile],
        progress: &mut dyn FnMut(Progress) -> bool,
    ) -> Result<Outcome> {
        self.with(connection, |r| {
            let root = remote_root(connection, r)?;
            sync::run(r, local_root, &root, files, progress)
        })
    }
}

/// The folder on the server matching the site's folder.
fn remote_root(connection: &Connection, remote: &mut dyn Remote) -> Result<String> {
    let dir = connection.remote_dir.trim();
    if dir.starts_with('/') {
        return Ok(dir.to_string());
    }
    let start = remote.start_dir()?;
    Ok(if dir.is_empty() || dir == "." {
        start
    } else {
        join(&start, dir)
    })
}

fn remember_key(id: &str, key: String) -> Result<()> {
    let mut store = RemoteStore::load()?;
    store.set_server_key(id, Some(key))?;
    store.save()
}

/// A server folder's contents.
#[derive(Debug, Clone, Serialize)]
pub struct Listing {
    pub dir: String,
    /// The connection's own folder, which matches the site's folder.
    pub root: String,
    pub entries: Vec<Entry>,
}

/// `/a/b` + `c/d` → `/a/b/c/d`.
pub(crate) fn join(base: &str, rel: &str) -> String {
    let rel = rel.trim_start_matches('/');
    if rel.is_empty() {
        base.to_string()
    } else if base.ends_with('/') {
        format!("{base}{rel}")
    } else {
        format!("{base}/{rel}")
    }
}

/// The local folder a connection transfers: the site's folder, or the part
/// of it set in the connection.
pub fn local_root(connection: &Connection, site_folder: &Path) -> PathBuf {
    if connection.local_dir.is_empty() {
        site_folder.to_owned()
    } else {
        site_folder.join(&connection.local_dir)
    }
}

/// Sets a file's modification time, in seconds since 1970.
pub(crate) fn set_mtime(file: &std::fs::File, at: i64) {
    let time = std::time::UNIX_EPOCH + std::time::Duration::from_secs(at.max(0) as u64);
    let _ = file.set_modified(time);
}

/// A file's modification time in seconds since 1970.
pub(crate) fn mtime_of(meta: &std::fs::Metadata) -> Option<i64> {
    meta.modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs() as i64)
}

/// `file.part`, the name a download is written under until it is complete.
pub(crate) fn partial_name(local: &Path) -> PathBuf {
    let mut name = local.as_os_str().to_owned();
    name.push(".part");
    PathBuf::from(name)
}

pub(crate) fn io_error(path: &Path) -> impl FnOnce(std::io::Error) -> Error + '_ {
    move |source| Error::Io {
        path: path.to_owned(),
        source,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input() -> ConnectionInput {
        ConnectionInput {
            site: "myapp.stack".into(),
            name: "Production".into(),
            protocol: Some(Protocol::Sftp),
            host: "sftp://203.0.113.10/".into(),
            user: "master".into(),
            remote_dir: "/home/master/applications/abc/public_html/".into(),
            ..Default::default()
        }
    }

    #[test]
    fn validates_and_cleans_input() {
        let c = validate("id".into(), input(), None).unwrap();
        assert_eq!(c.host, "203.0.113.10");
        assert_eq!(c.port, 22);
        assert_eq!(c.remote_dir, "/home/master/applications/abc/public_html");
        assert_eq!(c.skip, default_skip());

        let ftp = validate(
            "id".into(),
            ConnectionInput {
                protocol: Some(Protocol::Ftps),
                port: None,
                local_dir: "\\public\\".into(),
                ..input()
            },
            None,
        )
        .unwrap();
        assert_eq!(ftp.port, 21);
        assert_eq!(ftp.local_dir, "public");

        for bad in [
            ConnectionInput {
                host: "".into(),
                ..input()
            },
            ConnectionInput {
                user: " ".into(),
                ..input()
            },
            ConnectionInput {
                local_dir: "../other".into(),
                ..input()
            },
            ConnectionInput {
                protocol: Some(Protocol::Ftp),
                key_file: Some("~/.ssh/id_ed25519".into()),
                ..input()
            },
        ] {
            assert!(validate("id".into(), bad, None).is_err());
        }
    }

    #[test]
    fn a_new_server_forgets_the_trusted_key() {
        let mut old = validate("id".into(), input(), None).unwrap();
        old.server_key = Some("SHA256:abc".into());
        let same = validate("id".into(), input(), Some(&old)).unwrap();
        assert_eq!(same.server_key.as_deref(), Some("SHA256:abc"));
        let moved = validate(
            "id".into(),
            ConnectionInput {
                host: "198.51.100.7".into(),
                ..input()
            },
            Some(&old),
        )
        .unwrap();
        assert_eq!(moved.server_key, None);
    }

    #[test]
    fn stores_connections() {
        let path = crate::testutil::tempdir().join(FILE);
        let mut store = RemoteStore::load_from(&path).unwrap();
        let c = validate("abcd2345".into(), input(), None).unwrap();
        store.connections.push(c.clone());
        store.save().unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.contains("password"));
        let reloaded = RemoteStore::load_from(&path).unwrap();
        assert_eq!(reloaded.get("abcd2345").unwrap(), &c);
        assert_eq!(reloaded.for_site("myapp.stack").count(), 1);
        assert_eq!(reloaded.for_site("other.stack").count(), 0);
    }

    #[test]
    fn joins_server_paths() {
        assert_eq!(join("/", "a"), "/a");
        assert_eq!(join("/srv/www", "a/b.txt"), "/srv/www/a/b.txt");
        assert_eq!(join("/srv/www", ""), "/srv/www");
        assert!(is_relative_inside("public/assets"));
        assert!(is_relative_inside(""));
        assert!(!is_relative_inside("../x"));
        assert!(!is_relative_inside("/etc"));
    }
}

/// Runs against a real server, given as
/// `STACKYARD_TEST_REMOTE=protocol|host|port|user|password|key file|folder`:
/// `cargo test -p stackyard-core -- --ignored remote_server_roundtrip`.
#[cfg(test)]
mod server_tests {
    use super::*;

    fn write(root: &Path, rel: &str, data: &str) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, data).unwrap();
    }

    #[test]
    #[ignore = "needs a server in STACKYARD_TEST_REMOTE"]
    fn remote_server_roundtrip() {
        let Ok(spec) = std::env::var("STACKYARD_TEST_REMOTE") else {
            return;
        };
        let parts: Vec<&str> = spec.split('|').collect();
        let protocol = match parts[0] {
            "sftp" => Protocol::Sftp,
            "ftps" => Protocol::Ftps,
            _ => Protocol::Ftp,
        };
        let home = crate::testutil::tempdir();
        std::env::set_var("STACKYARD_HOME", &home);
        let mut store = RemoteStore::load().unwrap();
        let connection = store
            .add(ConnectionInput {
                site: "t.stack".into(),
                name: "Test".into(),
                protocol: Some(protocol),
                host: parts[1].into(),
                port: parts[2].parse().ok(),
                user: parts[3].into(),
                password: Some(parts[4].into()),
                key_file: (!parts[5].is_empty()).then(|| parts[5].into()),
                remote_dir: parts[6].into(),
                ..Default::default()
            })
            .unwrap();
        store.save().unwrap();
        let sessions = Sessions::default();
        let mut connection = connection;
        let start = match sessions.test(&connection) {
            Err(Error::RemoteCertificate { fingerprint, .. }) => {
                // A self-signed test server: trust it as a user would.
                connection.server_key = Some(fingerprint);
                sessions.test(&connection).unwrap()
            }
            other => other.unwrap(),
        };
        assert!(start.starts_with('/'), "{start}");
        let connection = RemoteStore::load()
            .unwrap()
            .get(&connection.id)
            .map(|stored| Connection {
                server_key: connection.server_key.clone().or(stored.server_key.clone()),
                ..stored.clone()
            })
            .unwrap();
        if protocol == Protocol::Sftp {
            assert!(connection
                .server_key
                .as_deref()
                .unwrap()
                .starts_with("SHA256:"));
        }

        let local = home.join("site");
        write(&local, "index.php", "<?php echo 'hi';");
        write(&local, "assets/css/app.css", "body { color: red }");
        write(&local, "a folder/with space.txt", "spaces");
        write(&local, ".git/HEAD", "skipped");
        let mut go = |_: Progress| true;
        let plan = sessions
            .plan(
                &connection,
                &local,
                Direction::Upload,
                Compare::Changed,
                true,
                None,
                &mut go,
            )
            .unwrap();
        assert_eq!(plan.files.len(), 3, "{plan:?}");
        let outcome = sessions
            .run(&connection, &local, &plan.files, &mut go)
            .unwrap();
        assert!(outcome.failed.is_empty(), "{outcome:?}");
        assert_eq!(outcome.done, 3);

        let again = sessions
            .plan(
                &connection,
                &local,
                Direction::Upload,
                Compare::Changed,
                true,
                None,
                &mut go,
            )
            .unwrap();
        assert!(again.files.is_empty(), "{again:?}");

        let listing = sessions.browse(&connection, None).unwrap();
        assert!(listing.entries.iter().any(|e| e.name == "assets" && e.dir));
        assert!(listing
            .entries
            .iter()
            .any(|e| e.name == "index.php" && e.size == 16));

        let copy = home.join("copy");
        let down = sessions
            .plan(
                &connection,
                &copy,
                Direction::Download,
                Compare::Changed,
                false,
                None,
                &mut go,
            )
            .unwrap();
        assert_eq!(down.files.len(), 3);
        sessions
            .run(&connection, &copy, &down.files, &mut go)
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(copy.join("a folder/with space.txt")).unwrap(),
            "spaces"
        );
        let same = sessions
            .plan(
                &connection,
                &copy,
                Direction::Download,
                Compare::Changed,
                false,
                None,
                &mut go,
            )
            .unwrap();
        assert!(same.files.is_empty(), "{same:?}");

        std::fs::remove_file(local.join("index.php")).unwrap();
        let cleanup = sessions
            .plan(
                &connection,
                &local,
                Direction::Upload,
                Compare::Changed,
                true,
                None,
                &mut go,
            )
            .unwrap();
        assert_eq!(cleanup.files.len(), 1);
        assert_eq!(cleanup.files[0].action, Action::DeleteRemote);
        sessions
            .run(&connection, &local, &cleanup.files, &mut go)
            .unwrap();
        let listing = sessions.browse(&connection, None).unwrap();
        assert!(!listing.entries.iter().any(|e| e.name == "index.php"));
    }
}
