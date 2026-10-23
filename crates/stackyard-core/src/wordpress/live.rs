//! Cloning a live WordPress site to this computer.
//!
//! The live site is reached the best way its host allows:
//!
//! - **SSH** (SFTP with a shell): files over SFTP, the database exported
//!   on the server with `mysqldump`, table by table, compressed;
//! - **SFTP or FTP only**: files the same way, the database through the
//!   one-time PHP helper ([`super::helper`]) over HTTPS;
//! - **backup files**: a backup made in the hosting panel (cPanel,
//!   Plesk, DirectAdmin and others) or a zip of the files plus a `.sql`
//!   file, picked on this computer.
//!
//! Large sites can take hours, so every step can be picked up again: files
//! already here with the same size and date are skipped, a file that broke
//! off is continued, and the database comes in chunks whose progress is kept
//! in `tmp/clone-<name>/state.json`. Cloning again with the same name after
//! a failure or Stop continues where it was.
//!
//! The copy gets its own database, every live address rewritten to the
//! local one (serialized data included), a `wp-config.php` for this
//! computer (the live one is kept as `wp-config.live.php`), and the
//! must-use plugin with the safety settings switched on.

use std::collections::BTreeSet;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::config::{self, WpConfig};
use super::env::{self, Environment, Live, Media, Route};
use super::helper::{self, DbSettings, Helper, Info};
use super::replace::{self, Rules};
use super::snapshot;
use super::WpSite;
use crate::catalog::RuntimeKind;
use crate::databases::Client;
use crate::remote::{self, Compare, Connection, Direction, Sessions};
use crate::runtimes::RuntimeManager;
use crate::settings::Settings;
use crate::sites::{Site, SiteDatabase, SiteRuntime, SiteSettings, SiteStore};
use crate::{domain, Error, Result};

fn fail(message: impl Into<String>) -> Error {
    Error::WordPress(message.into())
}

fn stopped() -> Error {
    fail("stopped; clone again with the same name to continue where it stopped")
}

fn io(path: &Path) -> impl FnOnce(std::io::Error) -> Error + '_ {
    move |source| Error::Io {
        path: path.to_owned(),
        source,
    }
}

/// Where the live site comes from.
#[derive(Debug, Clone)]
pub enum Source {
    /// A server, through a saved connection or a draft one already logged
    /// in with [`Sessions::login`].
    Server(Box<Connection>),
    /// Backup files on this computer: archives (`.zip`, `.tar.gz`, `.tar`)
    /// and `.sql` or `.sql.gz` files.
    Files(Vec<PathBuf>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Stage {
    Connecting,
    Files,
    Database,
    Loading,
    Finishing,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Progress {
    pub stage: Stage,
    pub message: String,
    /// Bytes (or files) done and in all; `total` 0 when unknown.
    pub done: u64,
    pub total: u64,
}

/// Reports progress; returning `false` stops the clone.
pub type OnProgress<'a> = &'a mut dyn FnMut(Progress) -> bool;

fn say(progress: OnProgress, stage: Stage, message: impl Into<String>) -> Result<()> {
    if progress(Progress {
        stage,
        message: message.into(),
        done: 0,
        total: 0,
    }) {
        Ok(())
    } else {
        Err(stopped())
    }
}

/// What was found on the live side, before anything is copied.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Found {
    pub route: Option<Route>,
    /// WordPress folders found on the server; the first is used unless
    /// another is picked.
    pub roots: Vec<String>,
    pub root: String,
    /// The live site's address (`home`).
    pub url: String,
    pub wordpress: String,
    pub php: String,
    pub mysql: String,
    pub prefix: String,
    pub tables: usize,
    pub database_bytes: u64,
    /// Notes on how it will be copied, e.g. that the database comes
    /// through the helper because the server has no shell.
    pub notes: Vec<String>,
}

/// What the user chose for the copy.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct CloneOptions {
    /// The new site's name, e.g. `shop` for `shop.stack`.
    pub name: String,
    /// The project folder; a new folder in the sites folder when `None`.
    #[serde(default)]
    pub folder: Option<PathBuf>,
    /// The WordPress folder on the server, when more than one was found.
    #[serde(default)]
    pub root: Option<String>,
    /// The live site's address, needed when the server has no shell and
    /// `wp-config.php` does not name it.
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub media: Media,
    #[serde(default = "yes")]
    pub https: bool,
    /// Makes the copy a staging of this main site (e.g. `shop.stack`): its
    /// address, folder and database are named after it.
    #[serde(default)]
    pub staging_of: Option<String>,
    /// When a site of this name already exists, back it up and put the
    /// copy in its place (its folder and database are replaced).
    #[serde(default)]
    pub replace: bool,
}

fn yes() -> bool {
    true
}

/// The finished copy.
#[derive(Debug, Clone, Serialize)]
pub struct Cloned {
    pub site: Site,
    pub url: String,
    pub live: String,
    /// Places where the live address was changed to the local one.
    pub replaced: u64,
    /// Things worth knowing, e.g. a PHP version that differs from live.
    pub notes: Vec<String>,
}

// ---------------------------------------------------------------------------
// The live server

/// Folders not worth copying: caches and the backups backup plugins keep
/// inside the site, which are often bigger than the site itself.
pub(super) const SKIP: &[&str] = &[
    "wp-content/cache",
    "wp-content/et-cache",
    "wp-content/updraft",
    "wp-content/ai1wm-backups",
    "wp-content/backups-dup-lite",
    "wp-content/backups-dup-pro",
    "wp-content/wpvividbackups",
    "wp-content/backup-db",
    "wp-content/backups",
    "wp-content/uploads/backupbuddy_backups",
    "wp-content/uploads/wp-migrate-db",
    "wp-snapshots",
    "error_log",
];

/// Folder names never searched for WordPress.
const NOT_SEARCHED: &[&str] = &[
    "node_modules",
    "wp-content",
    "wp-includes",
    "wp-admin",
    "cache",
    "logs",
    "log",
    "tmp",
    "mail",
    "etc",
    "ssl",
    "backups",
    "access-logs",
    "perl5",
];

/// A logged-in live server, with what is known about its WordPress.
pub(super) struct Server<'a> {
    pub(super) sessions: &'a Sessions,
    pub(super) connection: Connection,
    pub(super) root: String,
    pub(super) config_text: String,
    pub(super) db: DbSettings,
    pub(super) shell: bool,
    pub(super) php_cli: bool,
    /// The server's command-line PHP, when `php_cli`.
    pub(super) php_bin: String,
    pub(super) dump_tool: Option<String>,
    /// WordPress's own address on the server, for the helper over HTTPS.
    pub(super) siteurl: Option<String>,
    pub(super) helper: Option<Helper>,
    pub(super) http: ureq::Agent,
}

/// The end of the message that asks for the live site's address; the
/// clone screen looks for it to point at the address box.
pub const NEED_ADDRESS: &str =
    "Type it in the Live site's address box, e.g. https://example.com, and look again.";

/// Prints `SYPHP=<path>` for the first command-line PHP that runs.
const FIND_PHP: &str = r#"for p in php php-cli /usr/local/bin/php /usr/bin/php /opt/cpanel/ea-php*/root/usr/bin/php /opt/alt/php*/usr/bin/php /opt/plesk/php/*/bin/php; do v=$("$p" -q -r 'echo "SYPHP:".PHP_VERSION;' 2>/dev/null) || continue; case "$v" in *SYPHP:[0-9]*) echo "SYPHP=$(command -v "$p" || echo "$p")"; exit 0;; esac; done; exit 1"#;

/// The first line that is an absolute path, past any greeting.
fn first_path(out: &str) -> Option<String> {
    out.lines()
        .map(str::trim)
        .find(|l| l.starts_with('/'))
        .map(str::to_string)
}

/// Whether `text` reads like a public domain, e.g. `example.com`.
fn looks_like_domain(text: &str) -> bool {
    let labels: Vec<&str> = text.split('.').collect();
    labels.len() >= 2
        && labels.iter().all(|l| {
            !l.is_empty()
                && !l.starts_with('-')
                && !l.ends_with('-')
                && l.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
        && labels
            .last()
            .is_some_and(|tld| tld.len() >= 2 && tld.bytes().all(|b| b.is_ascii_alphabetic()))
        && !matches!(
            labels.last().copied(),
            Some("php" | "html" | "local" | "stack" | "test" | "localhost" | "internal" | "lan")
        )
}

/// Domains named in a WordPress folder's path (e.g. `/home/u/domains/example.com/public_html/blog`),
/// last first, and the part of the path under the web folder (`/blog`).
fn path_domains(root: &str) -> (Vec<String>, String) {
    let parts: Vec<&str> = root.split('/').filter(|p| !p.is_empty()).collect();
    let web = parts.iter().rposition(|p| {
        matches!(
            *p,
            "public_html" | "httpdocs" | "htdocs" | "www" | "html" | "web" | "public"
        )
    });
    let sub: String = match web {
        Some(at) => parts[at + 1..].iter().map(|p| format!("/{p}")).collect(),
        None => String::new(),
    };
    let mut domains: Vec<String> = parts
        .iter()
        .map(|p| p.to_ascii_lowercase())
        .filter(|p| looks_like_domain(p))
        .collect();
    domains.reverse();
    // A domain-named folder under the web folder is usually its own site.
    let sub = match web {
        Some(at)
            if parts.len() > at + 1 && looks_like_domain(&parts[at + 1].to_ascii_lowercase()) =>
        {
            parts[at + 2..].iter().map(|p| format!("/{p}")).collect()
        }
        _ => sub,
    };
    (domains, sub)
}

/// Quotes text for a POSIX shell.
pub(super) fn sh(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

fn exec_text(
    sessions: &Sessions,
    connection: &Connection,
    command: &str,
) -> Result<(bool, String)> {
    let mut out = Vec::new();
    let exit = sessions.exec(connection, command, &mut out)?;
    Ok((exit.ok(), String::from_utf8_lossy(&out).trim().to_string()))
}

/// Folders under `start` (absolute) that hold WordPress.
fn find_roots(sessions: &Sessions, connection: &Connection, start: &str) -> Result<Vec<String>> {
    let mut found = Vec::new();
    let mut queue = vec![(start.to_string(), 0)];
    let mut listed = 0;
    while let Some((dir, depth)) = (!queue.is_empty()).then(|| queue.remove(0)) {
        listed += 1;
        if listed > 150 {
            break;
        }
        let Ok(entries) = sessions.list(connection, &dir) else {
            continue;
        };
        let has =
            |name: &str, folder: bool| entries.iter().any(|e| e.name == name && e.dir == folder);
        if has("wp-includes", true) && has("wp-load.php", false) {
            found.push(dir);
            continue;
        }
        if depth >= 3 {
            continue;
        }
        let mut subfolders: Vec<&remote::Entry> = entries
            .iter()
            .filter(|e| {
                e.dir && !e.name.starts_with('.') && !NOT_SEARCHED.contains(&e.name.as_str())
            })
            .collect();
        // The usual web folders first.
        subfolders.sort_by_key(|e| {
            !matches!(
                e.name.as_str(),
                "public_html" | "www" | "htdocs" | "httpdocs" | "html" | "public" | "web"
            )
        });
        queue.extend(
            subfolders
                .into_iter()
                .map(|e| (remote::join(&dir, &e.name), depth + 1)),
        );
    }
    Ok(found)
}

fn parent(dir: &str) -> Option<String> {
    let trimmed = dir.trim_end_matches('/');
    let at = trimmed.rfind('/')?;
    Some(if at == 0 {
        "/".into()
    } else {
        trimmed[..at].to_string()
    })
}

/// The database settings in a live `wp-config.php`.
fn db_settings(config: &WpConfig) -> Result<DbSettings> {
    let get = |name: &str| config.get(name).map(str::to_string);
    let missing = |name: &str| {
        fail(format!(
            "the live wp-config.php sets {name} in a way Stackyard can't read (for example from the environment); clone from a backup made in the hosting panel instead"
        ))
    };
    Ok(DbSettings {
        name: get("DB_NAME").ok_or_else(|| missing("DB_NAME"))?,
        user: get("DB_USER").ok_or_else(|| missing("DB_USER"))?,
        password: get("DB_PASSWORD").unwrap_or_default(),
        host: get("DB_HOST").unwrap_or_else(|| "localhost".into()),
        charset: get("DB_CHARSET").unwrap_or_else(|| "utf8mb4".into()),
        prefix: config.prefix().to_string(),
    })
}

/// `mysqldump` options for a `DB_HOST` value: `host`, `host:port` or
/// `host:/path/to/socket`.
fn host_options(host: &str) -> String {
    let host = host.strip_prefix("p:").unwrap_or(host);
    if let Some((name, socket)) = host.split_once(":/") {
        let name = if name.is_empty() { "localhost" } else { name };
        return format!("-h {} -S {}", sh(name), sh(&format!("/{socket}")));
    }
    if let Some((name, port)) = host.rsplit_once(':') {
        if !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) {
            return format!("-h {} -P {port}", sh(name));
        }
    }
    format!(
        "-h {}",
        sh(if host.is_empty() { "localhost" } else { host })
    )
}

pub(super) fn agent() -> ureq::Agent {
    use ureq::tls::{RootCerts, TlsConfig};
    ureq::Agent::config_builder()
        .tls_config(
            TlsConfig::builder()
                .root_certs(RootCerts::PlatformVerifier)
                .build(),
        )
        .timeout_connect(Some(Duration::from_secs(20)))
        .timeout_recv_response(Some(Duration::from_secs(300)))
        .timeout_recv_body(Some(Duration::from_secs(300)))
        .http_status_as_error(false)
        .user_agent(concat!("Stackyard/", env!("CARGO_PKG_VERSION")))
        .build()
        .new_agent()
}

impl<'a> Server<'a> {
    /// Finds WordPress on the server and reads its `wp-config.php`.
    pub(super) fn open(
        sessions: &'a Sessions,
        connection: &Connection,
        root: Option<&str>,
        url: Option<&str>,
        temp: &Path,
        roots: &mut Vec<String>,
    ) -> Result<Self> {
        let start = sessions.root(connection)?;
        *roots = find_roots(sessions, connection, &start)?;
        let root = match root.filter(|r| !r.trim().is_empty()) {
            Some(picked) => picked.trim_end_matches('/').to_string(),
            None => roots.first().cloned().ok_or_else(|| {
                fail(format!(
                    "no WordPress found in {start} or the folders below it; set the connection's server folder to the site's folder"
                ))
            })?,
        };
        let local = temp.join("wp-config.live.php");
        let mut config_text = None;
        for dir in std::iter::once(root.clone()).chain(parent(&root)) {
            let remote = remote::join(&dir, "wp-config.php");
            if sessions
                .download(connection, &remote, &local, &mut |_| true)
                .is_ok()
            {
                config_text = Some(std::fs::read_to_string(&local).map_err(io(&local))?);
                break;
            }
        }
        let config_text = config_text.ok_or_else(|| {
            fail(format!(
                "could not read wp-config.php in {root} or the folder above it"
            ))
        })?;
        let config = WpConfig::read(&config_text);
        let db = db_settings(&config)?;
        // Some servers print a greeting before every command's output.
        let shell = connection.protocol == remote::Protocol::Sftp
            && exec_text(sessions, connection, "echo stackyard")
                .is_ok_and(|(ok, out)| ok && out.lines().any(|l| l.trim() == "stackyard"));
        let mut server = Server {
            sessions,
            connection: connection.clone(),
            root,
            db,
            shell,
            php_cli: false,
            php_bin: "php".into(),
            dump_tool: None,
            siteurl: url
                .map(str::to_string)
                .or_else(|| config.get("WP_SITEURL").map(str::to_string))
                .or_else(|| config.get("WP_HOME").map(str::to_string))
                .map(|u| u.trim_end_matches('/').to_string()),
            config_text,
            helper: None,
            http: agent(),
        };
        if shell {
            server.dump_tool = exec_text(
                sessions,
                connection,
                "command -v mariadb-dump || command -v mysqldump",
            )
            .ok()
            .filter(|(ok, _)| *ok)
            .and_then(|(_, out)| first_path(&out));
            // `php` is often missing from the PATH a non-interactive login
            // gets, or is the CGI build that prints headers first (`-q`).
            if let Some(php) = exec_text(sessions, connection, &format!("sh -c {}", sh(FIND_PHP)))
                .ok()
                .filter(|(ok, _)| *ok)
                .and_then(|(_, out)| {
                    out.lines()
                        .find_map(|l| l.strip_prefix("SYPHP=").map(str::to_string))
                })
            {
                server.php_cli = true;
                server.php_bin = php;
            }
        }
        Ok(server)
    }

    pub(super) fn route(&self) -> Route {
        if self.dump_tool.is_some() {
            Route::Ssh
        } else {
            Route::Helper
        }
    }

    /// Uploads a fresh helper, working for [`helper::LIFETIME_SECS`].
    pub(super) fn upload_helper(&mut self, temp: &Path) -> Result<()> {
        self.upload_helper_with(temp, false)
    }

    /// Uploads a fresh helper; `write` lets it load SQL files. When the
    /// helper must be reached over the site's address and none was given,
    /// the addresses the server suggests are tried.
    pub(super) fn upload_helper_with(&mut self, temp: &Path, write: bool) -> Result<()> {
        let guesses = if !self.php_cli && self.siteurl.is_none() {
            let guesses = self.address_guesses();
            if guesses.is_empty() {
                return Err(self.address_needed(&[]));
            }
            guesses
        } else {
            Vec::new()
        };
        let until = env::now() + helper::LIFETIME_SECS;
        let made = if write {
            helper::make_writing(&self.db, until)?
        } else {
            helper::make(&self.db, until)?
        };
        let local = temp.join(&made.file);
        std::fs::write(&local, &made.source).map_err(io(&local))?;
        let remote = remote::join(&self.root, &made.file);
        let uploaded = self.sessions.upload(&self.connection, &local, &remote);
        let _ = std::fs::remove_file(&local);
        uploaded.map_err(|e| {
            fail(format!(
                "could not put Stackyard's helper in {}: {e}",
                self.root
            ))
        })?;
        self.helper = Some(made);
        if !guesses.is_empty() && !self.reach_at(&guesses) {
            self.remove_helper();
            return Err(self.address_needed(&guesses));
        }
        Ok(())
    }

    /// Keeps the first of `guesses` where the uploaded helper answers.
    fn reach_at(&mut self, guesses: &[String]) -> bool {
        for guess in guesses {
            self.siteurl = Some(guess.clone());
            if self.read_info().is_ok() {
                return true;
            }
        }
        self.siteurl = None;
        false
    }

    /// Why the live site's address must be typed in, in words for the
    /// person cloning.
    fn address_needed(&self, tried: &[String]) -> Error {
        let why = if self.shell {
            "This server runs commands but has no command-line PHP Stackyard can use"
        } else if self.connection.protocol == remote::Protocol::Sftp {
            "This server allows file transfer only, not commands"
        } else {
            "This is an FTP connection, which can't run commands"
        };
        let tried = if tried.is_empty() {
            String::new()
        } else {
            format!(" Stackyard tried {} without finding it.", tried.join(", "))
        };
        fail(format!(
            "{why}, so the database is read through the live site's own address.{tried} {NEED_ADDRESS}"
        ))
    }

    /// Addresses WordPress may live at, best first: the live database's own
    /// `siteurl` when the server can run the `mysql` client, then domains in
    /// the WordPress folder's path and the connection's host and name.
    fn address_guesses(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let mut push = |url: String| {
            if !out.contains(&url) {
                out.push(url);
            }
        };
        if self.shell {
            if let Some(url) = self.database_siteurl() {
                push(url);
            }
        }
        let (domains, sub) = path_domains(&self.root);
        let host = self.connection.host.trim().to_ascii_lowercase();
        let host = ["ftp.", "ftps.", "sftp.", "ssh."]
            .iter()
            .find_map(|p| host.strip_prefix(p))
            .unwrap_or(&host)
            .to_string();
        let name = self.connection.name.trim().to_ascii_lowercase();
        let mut names: Vec<String> = domains;
        for d in [host, name] {
            if looks_like_domain(&d) && !names.contains(&d) {
                names.push(d);
            }
        }
        for d in names.iter().take(3) {
            let bare = d.strip_prefix("www.").unwrap_or(d);
            push(format!("https://{bare}{sub}"));
            push(format!("https://www.{bare}{sub}"));
            push(format!("http://{bare}{sub}"));
        }
        out.truncate(8);
        out
    }

    /// The live database's `siteurl`, read with the server's `mysql` client.
    fn database_siteurl(&self) -> Option<String> {
        let prefix = &self.db.prefix;
        if !prefix
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_')
        {
            return None;
        }
        let client = exec_text(
            self.sessions,
            &self.connection,
            "command -v mariadb || command -v mysql",
        )
        .ok()
        .filter(|(ok, _)| *ok)
        .and_then(|(_, out)| first_path(&out))?;
        let query = format!(
            "SELECT option_value FROM `{prefix}options` WHERE option_name IN ('siteurl', 'home') ORDER BY option_name = 'home'"
        );
        let command = format!(
            "MYSQL_PWD={} {} -N -B {} -u {} {} -e {}",
            sh(&self.db.password),
            sh(&client),
            host_options(&self.db.host),
            sh(&self.db.user),
            sh(&self.db.name),
            sh(&query),
        );
        let (ok, out) = exec_text(self.sessions, &self.connection, &command).ok()?;
        ok.then(|| {
            out.lines()
                .map(str::trim)
                .find(|l| l.starts_with("http://") || l.starts_with("https://"))
                .map(|l| l.trim_end_matches('/').to_string())
        })
        .flatten()
    }

    /// Runs the helper with these arguments, writing its answer to `out`.
    pub(super) fn call(
        &self,
        args: &[(&str, &str)],
        out: &mut dyn Write,
    ) -> Result<Option<helper::Chunk>> {
        let helper = self
            .helper
            .as_ref()
            .ok_or_else(|| fail("the helper is not on the server"))?;
        if self.php_cli {
            let mut command = format!(
                "cd {} && {} -q -d display_errors=0 {}",
                sh(&self.root),
                sh(&self.php_bin),
                sh(&helper.file)
            );
            for (key, value) in args {
                command.push(' ');
                command.push_str(&sh(&format!("{key}={value}")));
            }
            let exit = self.sessions.exec(&self.connection, &command, out)?;
            if !exit.ok() {
                let text: Vec<&str> = exit
                    .errors
                    .lines()
                    .filter(|l| !l.starts_with(helper::META_MARK) && !l.trim().is_empty())
                    .collect();
                return Err(fail(format!(
                    "the helper on the server failed: {}",
                    text.join(" ")
                )));
            }
            return Ok(helper::meta_from_errors(&exit.errors));
        }
        let base = self.siteurl.as_deref().unwrap_or_default();
        let url = format!("{base}/{}", helper.file);
        let mut form: Vec<(&str, &str)> = args.to_vec();
        form.push(("key", &helper.key));
        let mut response = self
            .http
            .post(&url)
            .header("X-Stackyard-Key", &helper.key)
            .send_form(form)
            .map_err(|e| fail(format!("could not reach {url}: {e}")))?;
        let status = response.status().as_u16();
        let meta = response
            .headers()
            .get("x-stackyard-meta")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| serde_json::from_str(v).ok());
        if status != 200 {
            let text = response
                .body_mut()
                .with_config()
                .limit(64 * 1024)
                .read_to_string()
                .unwrap_or_default();
            let said = serde_json::from_str::<serde_json::Value>(&text)
                .ok()
                .and_then(|v| v["error"].as_str().map(str::to_string));
            return Err(fail(match (status, said) {
                (_, Some(said)) => format!("the helper on the server said: {said}"),
                (404, None) => format!(
                    "{url} was not found; check the live site's address (it must be the address of WordPress's own folder)"
                ),
                (code, None) => format!("{url} answered {code}"),
            }));
        }
        let mut reader = response.body_mut().with_config().limit(u64::MAX).reader();
        std::io::copy(&mut reader, out).map_err(|e| fail(format!("reading {url}: {e}")))?;
        Ok(meta)
    }

    /// Asks the helper about the site. When the server's command-line PHP
    /// can't run it (say it lacks mysqli), the site's address is tried.
    pub(super) fn info(&mut self) -> Result<Info> {
        match self.read_info() {
            Err(e) if self.php_cli && self.siteurl.is_some() => {
                self.php_cli = false;
                self.read_info().map_err(|_| e)
            }
            Err(e) if self.php_cli => {
                self.php_cli = false;
                let guesses = self.address_guesses();
                if self.reach_at(&guesses) {
                    self.read_info()
                } else {
                    self.php_cli = true;
                    Err(e)
                }
            }
            other => other,
        }
    }

    fn read_info(&self) -> Result<Info> {
        let mut out = Vec::new();
        self.call(&[("do", "info")], &mut out)?;
        serde_json::from_slice(&out).map_err(|_| {
            fail(format!(
                "the helper's answer could not be read: {}",
                String::from_utf8_lossy(&out[..out.len().min(300)])
            ))
        })
    }

    /// Takes the helper off the server; if deleting it fails, the helper
    /// is asked to delete itself.
    pub(super) fn remove_helper(&mut self) {
        let Some(helper) = self.helper.clone() else {
            return;
        };
        let remote = remote::join(&self.root, &helper.file);
        if self
            .sessions
            .remove_file(&self.connection, &remote)
            .is_err()
        {
            let _ = self.call(&[("do", "finish")], &mut std::io::sink());
        }
        self.helper = None;
    }

    /// Exports one table with `mysqldump` on the server, compressed, into `dest`.
    pub(super) fn dump_table(&self, table: &str, dest: &Path) -> Result<()> {
        let tool = self.dump_tool.as_deref().unwrap_or("mysqldump");
        let command = format!(
            "MYSQL_PWD={} {} --single-transaction --quick --skip-lock-tables --no-tablespaces --add-drop-table --default-character-set={} {} -u {} {} {} | gzip -c",
            sh(&self.db.password),
            sh(tool),
            sh(&self.db.charset),
            host_options(&self.db.host),
            sh(&self.db.user),
            sh(&self.db.name),
            sh(table),
        );
        let part = dest.with_extension("partial");
        let mut file = std::fs::File::create(&part).map_err(io(&part))?;
        let exit = self.sessions.exec(&self.connection, &command, &mut file)?;
        drop(file);
        // The pipe's status is gzip's; mysqldump's own end mark says it finished.
        if !exit.ok() || !dump_completed(&part) {
            let _ = std::fs::remove_file(&part);
            return Err(fail(format!(
                "mysqldump on the server failed for {table}: {}",
                exit.errors.lines().last().unwrap_or("no output")
            )));
        }
        std::fs::rename(&part, dest).map_err(io(dest))
    }
}

/// Whether a gzipped `mysqldump` file ends with its "Dump completed" line.
fn dump_completed(file: &Path) -> bool {
    let Ok(f) = std::fs::File::open(file) else {
        return false;
    };
    let mut reader = flate2::read::GzDecoder::new(std::io::BufReader::new(f));
    let mut tail: Vec<u8> = Vec::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                tail.extend_from_slice(&buf[..n]);
                if tail.len() > 4096 {
                    tail.drain(..tail.len() - 4096);
                }
            }
            Err(_) => return false,
        }
    }
    String::from_utf8_lossy(&tail).contains("-- Dump completed")
}

// ---------------------------------------------------------------------------
// Inspect

/// Looks at the live site without copying anything: where WordPress is,
/// its address, versions and database size, and how it would be copied.
pub fn inspect(
    home: &Path,
    sessions: &Sessions,
    source: &Source,
    root: Option<&str>,
    url: Option<&str>,
) -> Result<Found> {
    match source {
        Source::Server(connection) => {
            let temp = super::temp_dir(home, "inspect")?;
            let result = inspect_server(sessions, connection, root, url, &temp);
            let _ = std::fs::remove_dir_all(&temp);
            result
        }
        Source::Files(files) => {
            let temp = super::temp_dir(home, "inspect")?;
            let result = (|| {
                let unpacked = Unpacked::open(files, &temp)?;
                let config = WpConfig::read(
                    &std::fs::read_to_string(&unpacked.config).map_err(io(&unpacked.config))?,
                );
                let sql = unpacked.sql(&config)?;
                let size = std::fs::metadata(&sql).map(|m| m.len()).unwrap_or(0);
                Ok(Found {
                    route: Some(Route::Files),
                    roots: vec![unpacked.root.display().to_string()],
                    root: unpacked.root.display().to_string(),
                    url: String::new(),
                    wordpress: super::php_variable(
                        &std::fs::read_to_string(unpacked.root.join("wp-includes/version.php"))
                            .unwrap_or_default(),
                        "$wp_version",
                    )
                    .unwrap_or_default(),
                    prefix: config.prefix().to_string(),
                    database_bytes: size,
                    notes: vec![format!(
                        "The database comes from {}.",
                        sql.file_name().unwrap_or_default().to_string_lossy()
                    )],
                    ..Found::default()
                })
            })();
            let _ = std::fs::remove_dir_all(&temp);
            result
        }
    }
}

/// Backs `old` up, then takes it away so a clone can take its place: its
/// database (when it is the one the clone makes) is dropped, its folder
/// emptied when the clone goes into it, and it leaves the sites list.
/// Answers a note naming the backup.
fn replace_site(
    home: &Path,
    store: &mut SiteStore,
    old: &Site,
    folder: &Path,
    database: &str,
    progress: OnProgress,
) -> Result<String> {
    let domain = old.domain();
    say(
        progress,
        Stage::Connecting,
        format!("Backing up {domain} before replacing it…"),
    )?;
    let backup = crate::sitepack::backup(home, old, &mut |_| {})?;
    if let Some(warning) = &backup.warning {
        return Err(fail(format!(
            "{domain} was not replaced: its backup could not include the database ({warning}). The files' backup is at {}.",
            backup.path.display()
        )));
    }
    let backed_up_db = crate::sitepack::site_database(old).map(|d| d.name);
    if backed_up_db.as_deref() == Some(database) {
        let client = Client::open(home)?;
        if client.databases()?.iter().any(|d| d.name == database) {
            client.drop(database)?;
        }
    }
    if same_folder(&old.path, folder) && folder.is_dir() {
        let entries = std::fs::read_dir(folder).map_err(io(folder))?;
        for entry in entries {
            let path = entry.map_err(io(folder))?.path();
            let removed = if path.is_dir() && !path.is_symlink() {
                std::fs::remove_dir_all(&path)
            } else {
                std::fs::remove_file(&path)
            };
            removed.map_err(|e| {
                fail(format!(
                    "could not empty {} to replace {domain} ({e}); close programs using it and clone again. The backup is at {}.",
                    folder.display(),
                    backup.path.display()
                ))
            })?;
        }
    }
    store.remove(&domain)?;
    store.save()?;
    env::forget(home, &domain);
    crate::thumbnails::forget(home, &domain);
    Ok(format!(
        "The earlier {domain} was backed up to {} before it was replaced; restore it under Manage › Backups.",
        backup.path.display()
    ))
}

fn same_folder(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

fn inspect_server(
    sessions: &Sessions,
    connection: &Connection,
    root: Option<&str>,
    url: Option<&str>,
    temp: &Path,
) -> Result<Found> {
    let mut roots = Vec::new();
    let mut server = Server::open(sessions, connection, root, url, temp, &mut roots)?;
    server.upload_helper(temp)?;
    let info = server.info();
    server.remove_helper();
    let info = info?;
    let mut notes = Vec::new();
    match (server.shell, server.dump_tool.is_some(), server.php_cli) {
        (true, true, _) => notes.push(
            "Files come over SFTP and the database is exported on the server with mysqldump."
                .to_string(),
        ),
        (true, false, true) => notes.push(
            "The server has no mysqldump, so the database comes through Stackyard's helper, run with the server's PHP."
                .to_string(),
        ),
        _ => notes.push(
            "The server has no shell Stackyard can use, so the database comes through Stackyard's helper over the site's address. The helper only reads, needs a key, and deletes itself when done or after 30 minutes."
                .to_string(),
        ),
    }
    if roots.len() > 1 {
        notes.push(format!(
            "{} WordPress folders were found; pick the one to clone.",
            roots.len()
        ));
    }
    Ok(Found {
        route: Some(server.route()),
        roots,
        root: server.root.clone(),
        url: if info.home.is_empty() {
            info.siteurl.clone()
        } else {
            info.home.clone()
        },
        wordpress: info.wordpress,
        php: info.php,
        mysql: info.mysql,
        prefix: server.db.prefix.clone(),
        tables: info.tables.len(),
        database_bytes: info.tables.iter().map(|t| t.bytes).sum(),
        notes,
    })
}

// ---------------------------------------------------------------------------
// Backup files

/// Backup files unpacked into a work folder.
struct Unpacked {
    /// WordPress's folder inside the unpacked files.
    root: PathBuf,
    config: PathBuf,
    /// `.sql` and `.sql.gz` files found, with their sizes.
    sql: Vec<(PathBuf, u64)>,
}

fn is_sql(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name.ends_with(".sql") || name.ends_with(".sql.gz")
}

fn is_backup_archive(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    crate::archive::is_archive(&lower) || lower.ends_with(".tar")
}

impl Unpacked {
    fn open(files: &[PathBuf], work: &Path) -> Result<Self> {
        if files.is_empty() {
            return Err(fail("pick the site's backup files"));
        }
        let mut sql = Vec::new();
        for (i, file) in files.iter().enumerate() {
            let name = file
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            if is_sql(&name) {
                let size = std::fs::metadata(file).map_err(io(file))?.len();
                sql.push((file.clone(), size));
            } else if is_backup_archive(&name) {
                crate::archive::extract(file, &work.join(format!("{i}")))?;
            } else if name.to_ascii_lowercase().ends_with(".zst") {
                return Err(fail(format!(
                    "{name} is compressed with zstd, which Stackyard can't open yet; make the backup as .tar.gz or .zip"
                )));
            } else {
                return Err(fail(format!(
                    "{name} is not a backup Stackyard can read; pick .zip, .tar.gz or .tar archives and .sql files"
                )));
            }
        }
        // Panel backups keep archives inside archives (Plesk, DirectAdmin);
        // open those that could hold the site, two levels deep.
        for _ in 0..2 {
            let mut inner = Vec::new();
            walk(work, &mut |path, _| {
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                if is_backup_archive(&name) {
                    inner.push(path.to_owned());
                }
            });
            if inner.is_empty() {
                break;
            }
            for archive in inner {
                let dest = archive.with_extension("unpacked");
                crate::archive::extract(&archive, &dest)?;
                let _ = std::fs::remove_file(&archive);
            }
        }
        let mut roots = Vec::new();
        walk(work, &mut |path, _| {
            if path.file_name().is_some_and(|n| n == "wp-load.php") {
                if let Some(dir) = path.parent() {
                    if dir.join("wp-includes").is_dir() {
                        roots.push(dir.to_owned());
                    }
                }
            }
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            if is_sql(&name) {
                let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
                sql.push((path.to_owned(), size));
            }
        });
        roots.sort_by_key(|r| r.components().count());
        let root = roots.into_iter().next().ok_or_else(|| {
            fail("no WordPress in these files (no wp-load.php with wp-includes next to it)")
        })?;
        let config = [
            root.join("wp-config.php"),
            root.parent()
                .map(|p| p.join("wp-config.php"))
                .unwrap_or_default(),
        ]
        .into_iter()
        .find(|p| p.is_file())
        .ok_or_else(|| fail("the WordPress in these files has no wp-config.php"))?;
        Ok(Unpacked { root, config, sql })
    }

    /// The database file: the one named after the site's database (cPanel
    /// names them so), else the biggest that holds the site's options table.
    fn sql(&self, config: &WpConfig) -> Result<PathBuf> {
        if self.sql.is_empty() {
            return Err(fail(
                "no .sql file in these backups; add the database export (.sql or .sql.gz) to the files",
            ));
        }
        if let Some(name) = config.get("DB_NAME") {
            let wanted = [format!("{name}.sql"), format!("{name}.sql.gz")];
            if let Some((path, _)) = self.sql.iter().find(|(p, _)| {
                let file = p.file_name().unwrap_or_default().to_string_lossy();
                wanted.iter().any(|w| *w == file)
            }) {
                return Ok(path.clone());
            }
        }
        let needle = format!("`{}options`", config.prefix());
        let mut candidates = self.sql.clone();
        candidates.sort_by_key(|(_, size)| std::cmp::Reverse(*size));
        candidates
            .iter()
            .find(|(path, _)| file_contains(path, needle.as_bytes()))
            .or(candidates.first())
            .map(|(path, _)| path.clone())
            .ok_or_else(|| fail("no database file found"))
    }
}

/// Calls `visit` for every file under `dir`.
fn walk(dir: &Path, visit: &mut dyn FnMut(&Path, &std::fs::Metadata)) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.is_dir() {
            walk(&path, visit);
        } else if meta.is_file() {
            visit(&path, &meta);
        }
    }
}

/// Opens a `.sql` or `.sql.gz` file for reading.
fn open_sql(path: &Path) -> Result<Box<dyn Read>> {
    let mut file = std::io::BufReader::new(std::fs::File::open(path).map_err(io(path))?);
    let gz = {
        use std::io::BufRead;
        file.fill_buf()
            .map_err(io(path))?
            .starts_with(&[0x1f, 0x8b])
    };
    Ok(if gz {
        Box::new(flate2::read::MultiGzDecoder::new(file))
    } else {
        Box::new(file)
    })
}

fn file_contains(path: &Path, needle: &[u8]) -> bool {
    let Ok(mut reader) = open_sql(path) else {
        return false;
    };
    let mut window: Vec<u8> = Vec::new();
    let mut buf = vec![0u8; 256 * 1024];
    loop {
        match reader.read(&mut buf) {
            Ok(0) | Err(_) => return false,
            Ok(n) => {
                window.extend_from_slice(&buf[..n]);
                if replace::contains(&window, needle) {
                    return true;
                }
                let keep = needle.len().min(window.len());
                window.drain(..window.len() - keep);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Resumable state

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct TableState {
    name: String,
    /// Where the next chunk starts (helper), empty at first.
    #[serde(default)]
    next: String,
    #[serde(default)]
    done: bool,
    /// Chunk files written so far.
    #[serde(default)]
    parts: u32,
    #[serde(default)]
    bytes: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct State {
    /// What is being cloned, so a different source starts afresh.
    source: String,
    folder: PathBuf,
    #[serde(default)]
    files_done: bool,
    #[serde(default)]
    database_made: bool,
    /// The tables, in order; empty until listed.
    #[serde(default)]
    tables: Vec<TableState>,
    /// The tables come from `mysqldump`, else from the helper.
    #[serde(default)]
    mysqldump: bool,
}

struct Work {
    dir: PathBuf,
    state: State,
}

impl Work {
    fn dir(home: &Path, name: &str) -> PathBuf {
        home.join("tmp").join(format!("clone-{name}"))
    }

    fn open(home: &Path, name: &str, source: &str, folder: &Path) -> Result<Self> {
        let dir = Self::dir(home, name);
        let file = dir.join("state.json");
        let saved: Option<State> = std::fs::read(&file)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok());
        let state = match saved {
            Some(state) if state.source == source && state.folder == folder => state,
            _ => {
                let _ = std::fs::remove_dir_all(&dir);
                State {
                    source: source.to_string(),
                    folder: folder.to_owned(),
                    ..State::default()
                }
            }
        };
        std::fs::create_dir_all(dir.join("db")).map_err(io(&dir))?;
        let work = Work { dir, state };
        work.save()?;
        Ok(work)
    }

    /// Whether an earlier clone of this source into this folder was cut short.
    fn resuming(&self) -> bool {
        self.state.files_done || !self.state.tables.is_empty()
    }

    fn save(&self) -> Result<()> {
        let file = self.dir.join("state.json");
        let partial = self.dir.join("state.json.partial");
        let text = serde_json::to_vec_pretty(&self.state)
            .map_err(|e| fail(format!("could not save the clone's progress: {e}")))?;
        std::fs::write(&partial, text).map_err(io(&partial))?;
        std::fs::rename(&partial, &file).map_err(io(&file))
    }

    fn part(&self, index: usize, table: &str, part: u32) -> PathBuf {
        let safe: String = table
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '_' {
                    c
                } else {
                    '-'
                }
            })
            .collect();
        self.dir
            .join("db")
            .join(format!("{index:05}-{safe}-{part:05}.sql.gz"))
    }

    /// Every chunk file, in order.
    fn parts(&self) -> Vec<PathBuf> {
        let mut files: Vec<PathBuf> = std::fs::read_dir(self.dir.join("db"))
            .map(|entries| {
                entries
                    .flatten()
                    .map(|e| e.path())
                    .filter(|p| p.to_string_lossy().ends_with(".sql.gz"))
                    .collect()
            })
            .unwrap_or_default();
        files.sort();
        files
    }

    fn finish(self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Reads several `.sql.gz` files one after another, as one dump.
pub(super) struct Concat {
    files: std::vec::IntoIter<PathBuf>,
    current: Option<Box<dyn Read>>,
    head: std::io::Cursor<Vec<u8>>,
}

impl Concat {
    pub(super) fn new(files: Vec<PathBuf>) -> Self {
        Concat {
            files: files.into_iter(),
            current: None,
            head: std::io::Cursor::new(
                b"SET NAMES utf8mb4;\nSET FOREIGN_KEY_CHECKS=0;\nSET SQL_MODE='NO_AUTO_VALUE_ON_ZERO';\n"
                    .to_vec(),
            ),
        }
    }
}

impl Read for Concat {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.head.read(buf)?;
        if n > 0 {
            return Ok(n);
        }
        loop {
            if let Some(current) = &mut self.current {
                let n = current.read(buf)?;
                if n > 0 {
                    return Ok(n);
                }
                self.current = None;
                // Each file ends its statements; a new line keeps them apart.
                buf[0] = b'\n';
                return Ok(1);
            }
            match self.files.next() {
                Some(path) => {
                    self.current =
                        Some(open_sql(&path).map_err(|e| std::io::Error::other(e.to_string()))?)
                }
                None => return Ok(0),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Clone

/// Clones the live site into a new local site, as [`CloneOptions`] says.
/// The database server (MySQL or MariaDB) must be running.
pub fn clone(
    home: &Path,
    sessions: &Sessions,
    source: &Source,
    options: &CloneOptions,
    progress: OnProgress,
) -> Result<Cloned> {
    let settings = Settings::load_from(&home.join("settings.toml"))?;
    let name = options.name.trim().to_string();
    domain::validate_site_name(&name)?;
    let mut store = SiteStore::load_from(&home.join("sites.toml"))?;
    let parent = match &options.staging_of {
        Some(domain) => Some(
            store
                .list()
                .iter()
                .find(|s| s.domain() == *domain)
                .cloned()
                .ok_or_else(|| Error::SiteNotFound(domain.clone()))?,
        ),
        None => None,
    };
    let tld = match &parent {
        Some(parent) => domain::normalize_tld(&parent.domain())?,
        None => domain::normalize_tld(&settings.tld)?,
    };
    let domain_name = domain::site_domain(&name, &tld);
    let folder = options
        .folder
        .clone()
        .filter(|p| !p.as_os_str().is_empty())
        .map(|p| crate::paths::expand_home(&p))
        .unwrap_or_else(|| settings.sites_folder(home).join(&name));
    let existing = store
        .list()
        .iter()
        .find(|s| s.domain() == domain_name)
        .cloned();
    let replaced_note = match existing {
        Some(old) if options.replace && parent.is_none() => Some(replace_site(
            home,
            &mut store,
            &old,
            &folder,
            &crate::starters::database_name(&name),
            progress,
        )?),
        Some(_) => return Err(Error::SiteExists(domain_name)),
        None => None,
    };
    let source_key = match source {
        Source::Server(c) => format!(
            "{}://{}@{}:{}{}",
            serde_json::to_string(&c.protocol)
                .unwrap_or_default()
                .trim_matches('"'),
            c.user,
            c.host,
            c.port,
            options.root.clone().unwrap_or_default()
        ),
        Source::Files(files) => files
            .iter()
            .map(|f| f.display().to_string())
            .collect::<Vec<_>>()
            .join("|"),
    };
    // A staging's work folder is named after its whole address, so it
    // can't be taken for a clone of the same name.
    let work_name = match &parent {
        Some(_) => domain_name.replace('.', "-"),
        None => name.clone(),
    };
    let mut work = Work::open(home, &work_name, &source_key, &folder)?;
    let has_files = folder
        .read_dir()
        .map(|mut entries| entries.next().is_some())
        .unwrap_or(false);
    if has_files && !work.resuming() {
        return Err(fail(format!(
            "{} already has files in it; pick a new or empty folder",
            folder.display()
        )));
    }
    std::fs::create_dir_all(&folder).map_err(io(&folder))?;
    let database = match &parent {
        Some(parent) => super::staging::database_name(parent, &name)?,
        None => crate::starters::database_name(&name),
    };
    let client = Client::open(home)?;
    if !work.state.database_made && client.databases()?.iter().any(|d| d.name == database) {
        return Err(fail(format!(
            "a database named {database} already exists; pick another name for the site"
        )));
    }

    let local_url = Site {
        name: name.clone(),
        tld: tld.clone(),
        path: folder.clone(),
        runtime: SiteRuntime::Static,
        https: options.https,
        dev_server: false,
        settings: SiteSettings::default(),
    }
    .url();
    let mut notes: Vec<String> = replaced_note.into_iter().collect();
    let mut replaced = 0;
    let live = match source {
        Source::Server(connection) => {
            let mut server = None;
            let result = clone_from_server(
                sessions,
                connection,
                options,
                &mut work,
                &folder,
                &local_url,
                &database,
                &client,
                &mut server,
                &mut notes,
                &mut replaced,
                progress,
            );
            if let Some(mut server) = server {
                server.remove_helper();
            }
            result?
        }
        Source::Files(files) => {
            clone_from_files(files, &mut work, &folder, &database, &client, progress)?
        }
    };

    say(progress, Stage::Finishing, "Setting up the copy…")?;
    let live_php = live.php.clone();
    let version = php_version(home, &settings, &live_php, &mut notes, progress)?;
    let config_file = folder.join("wp-config.php");
    let text = std::fs::read_to_string(&config_file).map_err(io(&config_file))?;
    let kept = folder.join("wp-config.live.php");
    if !kept.exists() {
        std::fs::write(&kept, &text).map_err(io(&kept))?;
    }
    let port = crate::catalog::service(&client.server().id)
        .map_or(3306, |def| crate::services::port(home, def));
    let (text, config_notes) = local_config(&text, &database, port, &local_url);
    notes.extend(config_notes);
    std::fs::write(&config_file, text).map_err(io(&config_file))?;
    let object_cache = folder.join("wp-content/object-cache.php");
    if object_cache.is_file() {
        let off = folder.join("wp-content/object-cache.php.stackyard-off");
        if std::fs::rename(&object_cache, &off).is_ok() {
            notes.push(
                "The live site's object cache (wp-content/object-cache.php) is switched off on this copy, since its cache server is not here; it is kept as object-cache.php.stackyard-off."
                    .into(),
            );
        }
    }
    if options.media == Media::FromLive {
        let _ = std::fs::create_dir_all(folder.join("wp-content/uploads"));
    }

    let site = Site {
        name: name.clone(),
        tld,
        path: folder.clone(),
        runtime: SiteRuntime::Php { version },
        https: options.https,
        dev_server: false,
        settings: SiteSettings {
            database: Some(SiteDatabase {
                server: "mysql".into(),
                name: database.clone(),
            }),
            ..Default::default()
        },
    };
    let site = store.add(site)?.clone();
    store.save()?;

    let connection = match source {
        // A staging uses its main site's saved connection.
        Source::Server(connection) if parent.is_some() => Some(connection.id.clone()),
        Source::Server(connection) => {
            let saved = sessions.save_copy(connection, &site.domain(), "Live").ok();
            if saved.is_none() {
                notes.push("The live server's connection could not be saved with the site; add it under Manage › Servers.".into());
            }
            saved.map(|c| c.id)
        }
        Source::Files(_) => None,
    };
    let live_url = live.url.trim_end_matches('/').to_string();
    env::save(
        home,
        &site.domain(),
        &Environment {
            kind: if parent.is_some() {
                env::Kind::Staging
            } else {
                env::Kind::Local
            },
            live: Some(Live {
                url: live_url.clone(),
                connection,
                path: live.root.clone(),
                route: live.route,
                media: options.media,
                synced_at: env::now(),
            }),
            safe: true,
            parent: parent.as_ref().map(Site::domain),
            made_from: parent.as_ref().map(|_| "live".to_string()),
            created_at: if parent.is_some() { env::now() } else { 0 },
            shared_media: None,
        },
    )?;
    let wp = WpSite::open(&site)?;
    super::muplugin::install(&wp, &super::muplugin::Options::for_site(home, &wp))?;
    if replaced == 0 {
        if let Some(from) = Some(live_url.as_str()).filter(|u| !u.is_empty()) {
            say(
                progress,
                Stage::Finishing,
                "Changing the live address to the local one…",
            )?;
            match super::search_replace(home, &wp, from, &local_url, true, false) {
                Ok(done) => replaced = done.report.replaced,
                Err(e) => notes.push(format!(
                    "The live address could not be changed everywhere: {e}"
                )),
            }
        }
    }
    if wp.wp_content().join("plugins/woocommerce").is_dir() {
        say(
            progress,
            Stage::Finishing,
            "Switching payments to test mode…",
        )?;
        notes.extend(payments_to_test_mode(home, &wp));
    }
    if live.route != Route::Files {
        say(
            progress,
            Stage::Finishing,
            "Noting where the copy and the live site stand…",
        )?;
        if let Err(e) = super::sync::record_after_clone(home, &site, live.facts.clone()) {
            notes.push(format!("Stackyard could not note how the copy matches the live site ({e}); the first push or pull lists every difference."));
        }
    }
    work.finish();
    Ok(Cloned {
        url: site.url(),
        site,
        live: live_url,
        replaced,
        notes,
    })
}

/// What the clone learned about the live side.
struct LiveSide {
    url: String,
    root: String,
    php: String,
    route: Route,
    /// What the live database looked like, read on the server, for the
    /// copy's sync base.
    facts: Option<snapshot::Facts>,
}

#[allow(clippy::too_many_arguments)]
fn clone_from_server<'a>(
    sessions: &'a Sessions,
    connection: &Connection,
    options: &CloneOptions,
    work: &mut Work,
    folder: &Path,
    local_url: &str,
    database: &str,
    client: &Client,
    server: &mut Option<Server<'a>>,
    notes: &mut Vec<String>,
    replaced: &mut u64,
    progress: OnProgress,
) -> Result<LiveSide> {
    say(
        progress,
        Stage::Connecting,
        "Finding WordPress on the server…",
    )?;
    let mut roots = Vec::new();
    let opened = Server::open(
        sessions,
        connection,
        options.root.as_deref(),
        options.url.as_deref(),
        &work.dir,
        &mut roots,
    )?;
    let s = server.insert(opened);
    s.upload_helper(&work.dir)?;
    let info = s.info()?;
    if s.siteurl.is_none() {
        s.siteurl = Some(info.siteurl.trim_end_matches('/').to_string());
    }
    let live_home = if info.home.is_empty() {
        info.siteurl.clone()
    } else {
        info.home.clone()
    };
    if live_home.is_empty() {
        return Err(fail(
            "the live database has no site address (home) in its options table",
        ));
    }
    if work.state.tables.is_empty() {
        work.state.tables = info
            .tables
            .iter()
            .map(|t| TableState {
                name: t.name.clone(),
                bytes: t.bytes,
                ..TableState::default()
            })
            .collect();
        work.state.mysqldump = s.dump_tool.is_some();
        work.save()?;
    }

    // Files
    if !work.state.files_done {
        let mut skip = remote::default_skip();
        skip.extend(SKIP.iter().map(|s| s.to_string()));
        if let Some(h) = &s.helper {
            skip.push(h.file.clone());
        }
        if options.media == Media::FromLive {
            skip.push("wp-content/uploads".into());
        }
        let files = Connection {
            remote_dir: s.root.clone(),
            skip,
            ..connection.clone()
        };
        let mut last = None;
        for attempt in 0..3 {
            say(progress, Stage::Files, "Comparing files with the server…")?;
            let mut halt = false;
            let plan = sessions.plan(
                &files,
                folder,
                Direction::Download,
                Compare::Changed,
                false,
                None,
                &mut |p| {
                    let message = match &p {
                        remote::Progress::Scanning { found, .. } => {
                            format!("Listing files on the server… {found} found")
                        }
                        remote::Progress::Transfer { path, .. } => path.clone(),
                    };
                    let go = progress(Progress {
                        stage: Stage::Files,
                        message,
                        done: 0,
                        total: 0,
                    });
                    halt |= !go;
                    go
                },
            );
            if halt {
                return Err(stopped());
            }
            let plan = plan?;
            if plan.files.is_empty() {
                last = None;
                break;
            }
            let total = plan.bytes;
            let count = plan.files.len();
            let mut before = 0u64;
            let mut index_seen = usize::MAX;
            let mut current = 0u64;
            let outcome = sessions.run(&files, folder, &plan.files, &mut |p| {
                if let remote::Progress::Transfer {
                    index,
                    path,
                    done,
                    size,
                    ..
                } = &p
                {
                    if *index != index_seen {
                        before += current;
                        index_seen = *index;
                    }
                    current = (*done).min(*size);
                    let go = progress(Progress {
                        stage: Stage::Files,
                        message: format!("Downloading {path} ({} of {count})", index + 1),
                        done: before + current,
                        total,
                    });
                    halt |= !go;
                    return go;
                }
                true
            });
            if halt {
                return Err(stopped());
            }
            let outcome = outcome?;
            if outcome.failed.is_empty() && !outcome.stopped {
                last = None;
                break;
            }
            last = Some(outcome);
            if attempt < 2 {
                std::thread::sleep(Duration::from_secs(2));
            }
        }
        if let Some(outcome) = last {
            let first: Vec<String> = outcome
                .failed
                .iter()
                .take(3)
                .map(|f| format!("{} ({})", f.path, f.error))
                .collect();
            return Err(fail(format!(
                "{} files could not be downloaded, e.g. {}; clone again to continue",
                outcome.failed.len(),
                first.join(", ")
            )));
        }
        // A wp-config.php kept above the WordPress folder comes in too.
        let config = folder.join("wp-config.php");
        if !config.exists() {
            std::fs::write(&config, &s.config_text).map_err(io(&config))?;
        }
        work.state.files_done = true;
        work.save()?;
    }

    // Database
    let total: u64 = work.state.tables.iter().map(|t| t.bytes).sum();
    let count = work.state.tables.len();
    for index in 0..count {
        if work.state.tables[index].done {
            continue;
        }
        let table = work.state.tables[index].name.clone();
        let done_before: u64 = work.state.tables[..index].iter().map(|t| t.bytes).sum();
        let report = |p: &mut dyn FnMut(Progress) -> bool, extra: u64| {
            p(Progress {
                stage: Stage::Database,
                message: format!("Copying the table {table} ({} of {count})", index + 1),
                done: (done_before + extra).min(total),
                total,
            })
        };
        if !report(progress, 0) {
            return Err(stopped());
        }
        if work.state.mysqldump {
            let dest = work.part(index, &table, 0);
            match s.dump_table(&table, &dest) {
                Ok(()) => {
                    let t = &mut work.state.tables[index];
                    t.done = true;
                    t.parts = 1;
                    work.save()?;
                    continue;
                }
                Err(e) if index == 0 || work.state.tables[..index].iter().all(|t| t.done) => {
                    // mysqldump can't read this database (often a client and
                    // server of different makes); the helper takes over.
                    notes.push(format!(
                        "mysqldump on the server did not work ({e}); the database came through Stackyard's helper instead."
                    ));
                    work.state.mysqldump = false;
                    for t in &mut work.state.tables {
                        *t = TableState {
                            name: t.name.clone(),
                            bytes: t.bytes,
                            ..TableState::default()
                        };
                    }
                    for part in work.parts() {
                        let _ = std::fs::remove_file(part);
                    }
                    work.save()?;
                    return clone_from_server_database(
                        s, work, local_url, &live_home, database, client, notes, replaced,
                        progress, &info,
                    );
                }
                Err(e) => return Err(e),
            }
        }
        let mut fetched = 0u64;
        loop {
            let (next, part) = {
                let t = &work.state.tables[index];
                (t.next.clone(), t.parts)
            };
            let dest = work.part(index, &table, part);
            let chunk = fetch_chunk(s, &table, &next, &dest)?;
            fetched += std::fs::metadata(&dest).map(|m| m.len() * 4).unwrap_or(0);
            let t = &mut work.state.tables[index];
            t.next = chunk.next;
            t.parts += 1;
            t.done = chunk.done;
            work.save()?;
            if work.state.tables[index].done {
                break;
            }
            if !report(progress, fetched) {
                return Err(stopped());
            }
        }
    }
    clone_from_server_database(
        s, work, local_url, &live_home, database, client, notes, replaced, progress, &info,
    )
}

/// Loads the copied tables, with the live addresses changed, into the new database.
#[allow(clippy::too_many_arguments)]
fn clone_from_server_database(
    s: &mut Server,
    work: &mut Work,
    local_url: &str,
    live_home: &str,
    database: &str,
    client: &Client,
    notes: &mut Vec<String>,
    replaced: &mut u64,
    progress: OnProgress,
    info: &Info,
) -> Result<LiveSide> {
    if work.state.tables.iter().any(|t| !t.done) {
        // The helper took over from mysqldump: copy everything again with it.
        let count = work.state.tables.len();
        for index in 0..count {
            let table = work.state.tables[index].name.clone();
            if !progress(Progress {
                stage: Stage::Database,
                message: format!("Copying the table {table} ({} of {count})", index + 1),
                done: index as u64,
                total: count as u64,
            }) {
                return Err(stopped());
            }
            while !work.state.tables[index].done {
                let (next, part) = {
                    let t = &work.state.tables[index];
                    (t.next.clone(), t.parts)
                };
                let dest = work.part(index, &table, part);
                let chunk = fetch_chunk(s, &table, &next, &dest)?;
                let t = &mut work.state.tables[index];
                t.next = chunk.next;
                t.parts += 1;
                t.done = chunk.done;
                work.save()?;
            }
        }
    }
    let facts = s.facts().ok();
    s.remove_helper();

    say(
        progress,
        Stage::Loading,
        "Changing addresses and loading the database…",
    )?;
    let mut rules = Rules::site_move(live_home, local_url);
    let siteurl = info.siteurl.trim_end_matches('/');
    if !siteurl.is_empty() && !siteurl.starts_with(live_home.trim_end_matches('/')) {
        let local_siteurl = local_url.to_string();
        rules = rules.and_site_move(siteurl, &local_siteurl);
    }
    let local_root = work.state.folder.to_string_lossy().replace('\\', "/");
    let live_root = if info.abspath.is_empty() {
        s.root.clone()
    } else {
        info.abspath.clone()
    };
    rules = rules.and_path(&live_root, &local_root);
    let load = work.dir.join("load.sql.gz");
    let out = std::fs::File::create(&load).map_err(io(&load))?;
    let mut gz = flate2::write::GzEncoder::new(out, flate2::Compression::fast());
    let report = replace::rewrite(Concat::new(work.parts()), &mut gz, &rules)
        .map_err(|e| fail(format!("could not prepare the database: {e}")))?;
    gz.finish().map_err(io(&load))?;
    if work.state.database_made {
        let _ = client.drop(database);
    }
    work.state.database_made = true;
    work.save()?;
    client.load(database, &load)?;
    let _ = std::fs::remove_file(&load);
    *replaced = report.replaced;
    if !report.damaged.is_empty() {
        notes.push(format!(
            "Some saved settings already had wrong lengths on the live site; the address was changed in them as plain text (tables: {}).",
            report.damaged.keys().cloned().collect::<Vec<_>>().join(", ")
        ));
    }
    if !report.encoded.is_empty() {
        notes.push(format!(
            "Some plugins keep the live address encoded (base64), so it still points at the live site there (tables: {}).",
            report.encoded.keys().cloned().collect::<Vec<_>>().join(", ")
        ));
    }
    Ok(LiveSide {
        url: live_home.to_string(),
        root: s.root.clone(),
        php: info.php.clone(),
        route: s.route(),
        facts,
    })
}

/// Fetches one chunk of a table through the helper into `dest`, trying a
/// few times when the connection drops.
pub(super) fn fetch_chunk(
    s: &Server,
    table: &str,
    from: &str,
    dest: &Path,
) -> Result<helper::Chunk> {
    let mut last = None;
    for attempt in 0..5 {
        if attempt > 0 {
            std::thread::sleep(Duration::from_secs(2u64.pow(attempt)));
        }
        let mut body = Vec::new();
        match s.call(
            &[("do", "table"), ("table", table), ("from", from)],
            &mut body,
        ) {
            Ok(Some(chunk)) => {
                let partial = dest.with_extension("partial");
                let mut file = std::fs::File::create(&partial).map_err(io(&partial))?;
                if chunk.gz {
                    file.write_all(&body).map_err(io(&partial))?;
                } else {
                    let mut gz = flate2::write::GzEncoder::new(file, flate2::Compression::fast());
                    gz.write_all(&body).map_err(io(&partial))?;
                    file = gz.finish().map_err(io(&partial))?;
                }
                drop(file);
                std::fs::rename(&partial, dest).map_err(io(dest))?;
                return Ok(chunk);
            }
            Ok(None) => last = Some(fail("the helper's answer had no chunk information")),
            Err(e) => last = Some(e),
        }
    }
    Err(last.unwrap_or_else(|| fail("the helper did not answer")))
}

fn clone_from_files(
    files: &[PathBuf],
    work: &mut Work,
    folder: &Path,
    database: &str,
    client: &Client,
    progress: OnProgress,
) -> Result<LiveSide> {
    say(progress, Stage::Files, "Unpacking the backup…")?;
    let unpacked_dir = work.dir.join("unpacked");
    let _ = std::fs::remove_dir_all(&unpacked_dir);
    let unpacked = Unpacked::open(files, &unpacked_dir)?;
    let config_text = std::fs::read_to_string(&unpacked.config).map_err(io(&unpacked.config))?;
    let config = WpConfig::read(&config_text);
    let sql = unpacked.sql(&config)?;
    say(progress, Stage::Files, "Moving the files into place…")?;
    move_contents(&unpacked.root, folder)?;
    let config_file = folder.join("wp-config.php");
    if !config_file.exists() {
        std::fs::write(&config_file, &config_text).map_err(io(&config_file))?;
    }
    work.state.files_done = true;
    say(progress, Stage::Loading, "Loading the database…")?;
    if work.state.database_made {
        let _ = client.drop(database);
    }
    work.state.database_made = true;
    work.save()?;
    let sql_in_work = sql.starts_with(&unpacked_dir);
    client.load(database, &sql)?;
    if sql_in_work {
        let _ = std::fs::remove_file(&sql);
    }
    let _ = std::fs::remove_dir_all(&unpacked_dir);
    // The address is read from the loaded database; it is changed after.
    let prefix = config.prefix();
    let rows = client.query(
        Some(database),
        &format!(
            "SELECT option_value FROM `{}` WHERE option_name IN ('home', 'siteurl') ORDER BY option_name = 'siteurl'",
            format!("{prefix}options").replace('`', "``")
        ),
    )?;
    let url = rows
        .first()
        .and_then(|set| set.rows.first())
        .and_then(|row| row.first().cloned().flatten())
        .unwrap_or_default();
    Ok(LiveSide {
        url,
        root: String::new(),
        php: String::new(),
        route: Route::Files,
        facts: None,
    })
}

/// Moves what is inside `from` into `to`, copying across drives.
fn move_contents(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to).map_err(io(to))?;
    for entry in std::fs::read_dir(from).map_err(io(from))? {
        let entry = entry.map_err(io(from))?;
        let target = to.join(entry.file_name());
        if std::fs::rename(entry.path(), &target).is_err() {
            copy_tree(&entry.path(), &target)?;
        }
    }
    Ok(())
}

fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    if from.is_dir() {
        std::fs::create_dir_all(to).map_err(io(to))?;
        for entry in std::fs::read_dir(from).map_err(io(from))? {
            let entry = entry.map_err(io(from))?;
            copy_tree(&entry.path(), &to.join(entry.file_name()))?;
        }
        Ok(())
    } else {
        std::fs::copy(from, to).map(|_| ()).map_err(io(to))
    }
}

/// The live `wp-config.php` changed for this computer: the local database,
/// the local address where the live one was written in, local environment,
/// and no page cache. Returns the text and notes on what changed.
pub fn local_config(
    text: &str,
    database: &str,
    port: u16,
    local_url: &str,
) -> (String, Vec<String>) {
    let mut notes = Vec::new();
    let config = WpConfig::read(text);
    let mut text = config::set_define(text, "DB_NAME", &config::quote(database));
    text = config::set_define(&text, "DB_USER", "'root'");
    text = config::set_define(&text, "DB_PASSWORD", "''");
    text = config::set_define(
        &text,
        "DB_HOST",
        &config::quote(&format!("127.0.0.1:{port}")),
    );
    text = config::set_define(&text, "WP_ENVIRONMENT_TYPE", "'local'");
    for name in ["WP_HOME", "WP_SITEURL"] {
        if config.defines.contains_key(name) {
            text = config::set_define(&text, name, &config::quote(local_url));
        }
    }
    for name in ["COOKIE_DOMAIN", "WP_REDIS_HOST", "WP_REDIS_PASSWORD"] {
        text = config::remove_define(&text, name);
    }
    if config.flag("WP_CACHE") {
        text = config::set_define(&text, "WP_CACHE", "false");
        notes.push("The page cache (WP_CACHE) is switched off on this copy.".into());
    }
    if config.flag("MULTISITE") {
        if let Some(host) = local_url.split("://").nth(1) {
            text = config::set_define(&text, "DOMAIN_CURRENT_SITE", &config::quote(host));
        }
        notes.push("This is a multisite network; the main site works, but subsites on their own domains may need their addresses changed under Network Admin › Sites.".into());
    }
    (text, notes)
}

/// The PHP version for the copy: the live one's when Stackyard offers it
/// (installed now if needed), else the default.
fn php_version(
    home: &Path,
    settings: &Settings,
    live: &str,
    notes: &mut Vec<String>,
    progress: OnProgress,
) -> Result<String> {
    let runtimes = RuntimeManager::new(home);
    let wanted: String = live.split('.').take(2).collect::<Vec<_>>().join(".");
    let offered = crate::catalog::runtime(RuntimeKind::Php)
        .versions
        .contains(&wanted.as_str());
    if offered {
        if runtimes.get(RuntimeKind::Php, &wanted).is_none() {
            say(
                progress,
                Stage::Finishing,
                format!("Installing PHP {wanted}, as on the live site…"),
            )?;
            if let Err(e) = runtimes.install(RuntimeKind::Php, &wanted, &mut |_| {}) {
                notes.push(format!("PHP {wanted}, which the live site runs, could not be installed ({e}); the copy uses the default PHP."));
                return Ok(crate::web::default_version(
                    settings,
                    &runtimes,
                    RuntimeKind::Php,
                ));
            }
        }
        return Ok(wanted);
    }
    let version = crate::web::default_version(settings, &runtimes, RuntimeKind::Php);
    if !wanted.is_empty() {
        notes.push(format!(
            "The live site runs PHP {live}; Stackyard doesn't offer {wanted}, so the copy uses PHP {version}."
        ));
    }
    if runtimes.get(RuntimeKind::Php, &version).is_none() {
        say(
            progress,
            Stage::Finishing,
            format!("Installing PHP {version}…"),
        )?;
        runtimes.install(RuntimeKind::Php, &version, &mut |_| {})?;
    }
    Ok(version)
}

/// Puts WooCommerce payment methods that have a test mode into it; returns notes.
pub(super) fn payments_to_test_mode(home: &Path, wp: &WpSite) -> Vec<String> {
    let mut notes = Vec::new();
    let gateways = match super::woo::gateways(home, wp) {
        Ok(g) => g,
        Err(e) => {
            notes.push(format!("Payment methods could not be checked ({e}); check them in WooCommerce › Settings › Payments before testing checkout."));
            return notes;
        }
    };
    let mut switched = Vec::new();
    let mut unknown = BTreeSet::new();
    for gateway in gateways.iter().filter(|g| g.enabled) {
        match gateway.test_mode {
            Some(true) => {}
            Some(false) => match super::woo::set_test_mode(home, wp, &gateway.id, true) {
                Ok(()) => switched.push(gateway.title.clone()),
                Err(_) => {
                    unknown.insert(gateway.title.clone());
                }
            },
            None => {
                unknown.insert(gateway.title.clone());
            }
        }
    }
    if !switched.is_empty() {
        notes.push(format!("Switched to test mode: {}.", switched.join(", ")));
    }
    if !unknown.is_empty() {
        notes.push(format!(
            "Check these payment methods yourself before testing checkout, as Stackyard can't switch them to test mode: {}.",
            unknown.into_iter().collect::<Vec<_>>().join(", ")
        ));
    }
    notes
}

/// Whether an unfinished clone with this name can be continued.
pub fn unfinished(home: &Path, name: &str) -> bool {
    Work::dir(home, name).join("state.json").is_file()
}

/// Throws away an unfinished clone's downloaded database chunks.
pub fn discard(home: &Path, name: &str) {
    let _ = std::fs::remove_dir_all(Work::dir(home, name));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_domains_in_server_paths() {
        assert_eq!(
            path_domains("/home/u/domains/example.com/public_html"),
            (vec!["example.com".to_string()], String::new())
        );
        assert_eq!(
            path_domains("/home/u/public_html/blog"),
            (vec![], "/blog".to_string())
        );
        assert_eq!(
            path_domains("/home/u/public_html/Shop.example.org/wp"),
            (vec!["shop.example.org".to_string()], "/wp".to_string())
        );
        assert_eq!(
            path_domains("/var/www/example.mk"),
            (vec!["example.mk".to_string()], String::new())
        );
        assert!(looks_like_domain("tranzit.mk"));
        assert!(!looks_like_domain("192.168.1.10"));
        assert!(!looks_like_domain("wp-config.php"));
        assert!(!looks_like_domain("tranzit.stack"));
        assert!(!looks_like_domain("Live"));
        assert_eq!(
            first_path("Welcome!\n/usr/bin/mysqldump\n"),
            Some("/usr/bin/mysqldump".into())
        );
    }

    #[test]
    fn quotes_for_the_shell() {
        assert_eq!(sh("plain"), "'plain'");
        assert_eq!(sh("it's"), r"'it'\''s'");
        assert_eq!(host_options("localhost"), "-h 'localhost'");
        assert_eq!(host_options("db.example:3307"), "-h 'db.example' -P 3307");
        assert_eq!(
            host_options("localhost:/var/run/mysqld/mysqld.sock"),
            "-h 'localhost' -S '/var/run/mysqld/mysqld.sock'"
        );
        assert_eq!(parent("/home/u/public_html"), Some("/home/u".into()));
        assert_eq!(parent("/srv"), Some("/".into()));
    }

    #[test]
    fn reads_database_settings_from_the_live_config() {
        let config = WpConfig::read(
            "<?php define('DB_NAME','shop'); define('DB_USER','u'); define('DB_PASSWORD','p\\'w'); $table_prefix = 'xy_';",
        );
        let db = db_settings(&config).unwrap();
        assert_eq!(
            (db.name.as_str(), db.user.as_str(), db.password.as_str()),
            ("shop", "u", "p'w")
        );
        assert_eq!(db.host, "localhost");
        assert_eq!(db.prefix, "xy_");
        let computed =
            WpConfig::read("<?php define('DB_NAME', getenv('DB_NAME')); define('DB_USER','u');");
        assert!(db_settings(&computed).is_err());
    }

    #[test]
    fn rewrites_the_config_for_this_computer() {
        let live = "<?php\ndefine( 'DB_NAME', 'live_db' );\ndefine( 'DB_USER', 'live' );\ndefine( 'DB_PASSWORD', 'secret' );\ndefine( 'DB_HOST', 'localhost' );\ndefine( 'WP_HOME', 'https://shop.example' );\ndefine( 'WP_CACHE', true );\ndefine( 'COOKIE_DOMAIN', '.shop.example' );\n$table_prefix = 'wp_';\n/* That's all, stop editing! Happy publishing. */\nrequire_once ABSPATH . 'wp-settings.php';\n";
        let (text, notes) = local_config(live, "shop", 3307, "https://shop.stack");
        let config = WpConfig::read(&text);
        assert_eq!(config.get("DB_NAME"), Some("shop"));
        assert_eq!(config.get("DB_USER"), Some("root"));
        assert_eq!(config.get("DB_PASSWORD"), Some(""));
        assert_eq!(config.get("DB_HOST"), Some("127.0.0.1:3307"));
        assert_eq!(config.get("WP_HOME"), Some("https://shop.stack"));
        assert_eq!(config.get("WP_SITEURL"), None);
        assert_eq!(config.get("WP_ENVIRONMENT_TYPE"), Some("local"));
        assert!(!config.flag("WP_CACHE"));
        assert!(!text.contains("COOKIE_DOMAIN"));
        assert!(!text.contains("secret"));
        assert_eq!(notes.len(), 1);
    }

    #[test]
    fn finds_wordpress_and_its_database_in_backups() {
        let dir = crate::testutil::tempdir();
        let zip = dir.join("files.zip");
        crate::archive::fixtures::zip(
            &zip,
            &[
                ("public_html/wp-load.php", b"<?php"),
                ("public_html/wp-includes/version.php", b"<?php $wp_version = '6.8.1';"),
                (
                    "public_html/wp-config.php",
                    b"<?php define('DB_NAME','acc_shop'); define('DB_USER','u'); $table_prefix='wp_';",
                ),
                ("public_html/wp-content/themes/x/style.css", b"/* x */"),
                ("mysql/acc_other.sql", b"CREATE TABLE `zz_options` (x int);"),
                ("mysql/acc_shop.sql", b"CREATE TABLE `wp_options` (x int);"),
            ],
        );
        let work = dir.join("work");
        let unpacked = Unpacked::open(&[zip], &work).unwrap();
        assert!(unpacked.root.ends_with("public_html"));
        let config = WpConfig::read(&std::fs::read_to_string(&unpacked.config).unwrap());
        assert!(unpacked.sql(&config).unwrap().ends_with("acc_shop.sql"));

        // A separate .sql named differently: the one with the site's tables wins.
        let renamed = WpConfig::read("<?php define('DB_NAME','other'); $table_prefix='wp_';");
        assert!(unpacked.sql(&renamed).unwrap().ends_with("acc_shop.sql"));

        let none = dir.join("none.zip");
        crate::archive::fixtures::zip(&none, &[("readme.txt", b"hi")]);
        assert!(Unpacked::open(&[none], &dir.join("w2")).is_err());
    }

    #[test]
    fn keeps_progress_between_tries() {
        let home = crate::testutil::tempdir();
        let folder = home.join("www/shop");
        let mut work = Work::open(&home, "shop", "sftp://a@b:22", &folder).unwrap();
        assert!(!work.resuming());
        work.state.tables.push(TableState {
            name: "wp_posts".into(),
            next: "k:500".into(),
            parts: 2,
            ..TableState::default()
        });
        work.save().unwrap();
        std::fs::write(work.part(0, "wp_posts", 0), b"x").unwrap();
        let again = Work::open(&home, "shop", "sftp://a@b:22", &folder).unwrap();
        assert!(again.resuming());
        assert_eq!(again.state.tables[0].next, "k:500");
        assert_eq!(again.parts().len(), 1);
        assert!(unfinished(&home, "shop"));
        let other = Work::open(&home, "shop", "sftp://a@other:22", &folder).unwrap();
        assert!(!other.resuming());
        assert!(other.parts().is_empty());
        other.finish();
        assert!(!unfinished(&home, "shop"));
    }

    #[test]
    fn reads_chunks_as_one_dump() {
        let dir = crate::testutil::tempdir();
        let a = dir.join("00000-a-00000.sql.gz");
        let b = dir.join("00001-b-00000.sql.gz");
        for (path, text) in [
            (&a, "INSERT INTO a VALUES (1);"),
            (&b, "INSERT INTO b VALUES (2);"),
        ] {
            let mut gz = flate2::write::GzEncoder::new(
                std::fs::File::create(path).unwrap(),
                flate2::Compression::fast(),
            );
            gz.write_all(text.as_bytes()).unwrap();
            gz.finish().unwrap();
        }
        let mut all = String::new();
        Concat::new(vec![a, b]).read_to_string(&mut all).unwrap();
        assert!(all.starts_with("SET NAMES utf8mb4;"));
        assert!(all.contains("INSERT INTO a VALUES (1);\nINSERT INTO b VALUES (2);\n"));
    }
}

/// Clones a real live site, given as
/// `STACKYARD_TEST_LIVE=sftp|host|ssh port|user|password|address|ftp port`
/// (the FTP port is optional), into
/// the Stackyard home in `STACKYARD_HOME` (with MySQL or MariaDB running):
/// `cargo test -p stackyard-core -- --ignored clone_from_a_live_server`.
#[cfg(test)]
mod server_tests {
    use super::*;

    fn clone_once(
        home: &Path,
        source: &Source,
        name: &str,
        url: Option<&str>,
        media: Media,
    ) -> Cloned {
        let sessions = Sessions::default();
        if let Source::Server(connection) = source {
            let password = std::env::var("STACKYARD_TEST_LIVE").unwrap();
            let password = password.split('|').nth(4).map(str::to_string);
            sessions.login(connection, password).unwrap();
        }
        let found = inspect(home, &sessions, source, None, url).unwrap();
        assert!(
            !found.wordpress.is_empty() || found.route == Some(Route::Files),
            "{found:?}"
        );
        let mut seen = Vec::new();
        let options = CloneOptions {
            name: name.into(),
            url: url.map(str::to_string),
            media,
            https: false,
            ..CloneOptions::default()
        };
        // Stopped halfway through the database, then continued.
        let mut copied = 0;
        let first = clone(home, &sessions, source, &options, &mut |p| {
            copied += usize::from(p.stage == Stage::Database);
            copied < 5
        });
        if source_is_server(source) {
            assert!(first.is_err());
            assert!(unfinished(home, name));
        }
        let cloned = match first {
            Ok(cloned) => cloned,
            Err(_) => clone(home, &sessions, source, &options, &mut |p| {
                seen.push(p.stage);
                true
            })
            .unwrap(),
        };
        assert!(!unfinished(home, name));
        cloned
    }

    fn source_is_server(source: &Source) -> bool {
        matches!(source, Source::Server(_))
    }

    fn check(home: &Path, cloned: &Cloned, live: &str) {
        let wp = WpSite::open(&cloned.site).unwrap();
        assert_eq!(
            wp.option(home, "home").unwrap().as_deref(),
            Some(cloned.url.as_str())
        );
        let content = wp
            .sql(
                home,
                &format!(
                    "SELECT post_content FROM {} WHERE ID = 1",
                    wp.table("posts")
                ),
            )
            .unwrap();
        let content = content[0][0].clone().unwrap_or_default();
        assert!(!content.contains(live), "{content}");
        assert!(super::super::muplugin::path(&wp).is_file());
        assert!(cloned.site.path.join("wp-config.live.php").is_file());
        assert_eq!(wp.config.get("WP_ENVIRONMENT_TYPE"), Some("local"));
        let env = env::load(home, &cloned.site.domain()).unwrap().unwrap();
        assert!(env.safe);
        assert_eq!(env.live.unwrap().url, live);
    }

    fn remove(home: &Path, cloned: &Cloned) {
        let mut store = SiteStore::load_from(&home.join("sites.toml")).unwrap();
        let _ = store.remove(&cloned.site.domain());
        store.save().unwrap();
        if let Some(db) = &cloned.site.settings.database {
            let _ = Client::open(home).unwrap().drop(&db.name);
        }
        let _ = std::fs::remove_dir_all(&cloned.site.path);
        env::forget(home, &cloned.site.domain());
        let mut remotes = remote::RemoteStore::load().unwrap();
        remotes.forget_site(&cloned.site.domain());
        remotes.save().unwrap();
    }

    #[test]
    #[ignore = "needs a live server in STACKYARD_TEST_LIVE"]
    fn clone_from_a_live_server() {
        let Ok(spec) = std::env::var("STACKYARD_TEST_LIVE") else {
            return;
        };
        let home = crate::paths::home();
        let parts: Vec<&str> = spec.split('|').collect();
        let live = parts[5].trim_end_matches('/');
        let input = |protocol| remote::ConnectionInput {
            protocol: Some(protocol),
            host: parts[1].into(),
            port: if protocol == remote::Protocol::Sftp {
                parts[2].parse().ok()
            } else {
                parts.get(6).and_then(|p| p.parse().ok())
            },
            user: parts[3].into(),
            ..Default::default()
        };

        // SSH: files over SFTP, the database with mysqldump on the server.
        let sftp = remote::draft(input(remote::Protocol::Sftp)).unwrap();
        let ssh = clone_once(
            &home,
            &Source::Server(Box::new(sftp)),
            "e2e-ssh",
            None,
            Media::Download,
        );
        check(&home, &ssh, live);
        assert!(ssh
            .site
            .path
            .join("wp-content/uploads")
            .read_dir()
            .unwrap()
            .next()
            .is_some());
        remove(&home, &ssh);

        // FTP: the database through the helper over the site's address,
        // media left on the live site.
        if parts.len() > 6 {
            let ftp = remote::draft(input(remote::Protocol::Ftp)).unwrap();
            let copy = clone_once(
                &home,
                &Source::Server(Box::new(ftp)),
                "e2e-ftp",
                Some(live),
                Media::FromLive,
            );
            check(&home, &copy, live);
            let uploads = copy.site.path.join("wp-content/uploads");
            assert!(uploads.read_dir().unwrap().next().is_none());
            remove(&home, &copy);
        }

        // Backup files: the site's files in one archive and its database
        // in a .sql.gz, given as `STACKYARD_TEST_BACKUP=archive|sql`.
        if let Ok(backup) = std::env::var("STACKYARD_TEST_BACKUP") {
            let files = backup.split('|').map(PathBuf::from).collect();
            let copy = clone_once(
                &home,
                &Source::Files(files),
                "e2e-files",
                None,
                Media::Download,
            );
            check(&home, &copy, live);
            remove(&home, &copy);
        }
    }
}
