//! Stackyard Shell: `php`, `node`, `npm`, `composer`, `python`, `go`,
//! `java`, `mysql` and other commands that run the version each project asks
//! for.
//!
//! The commands are small "shims" in `<home>/bin` that all lead back to the
//! `stackyard` program. It sees which name it was started as, works out the
//! version for the current folder and runs the real program from there:
//!
//! 1. the nearest `.stackyard` file, or version file of another tool such as
//!    `.nvmrc`, that pins one ([`pins`]),
//! 2. else a site added for this folder with its own version,
//! 3. else the default version from Settings,
//! 4. else the newest installed version.

pub mod path;
pub mod pins;
pub mod shims;

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::catalog::{self, RuntimeKind};
use crate::download::Http;
use crate::install::Step;
use crate::platform::{Os, Platform};
use crate::runtimes::RuntimeManager;
use crate::services::ServiceManager;
use crate::settings::Settings;
use crate::sites::{Site, SiteStore};
use crate::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Tool {
    Php,
    Composer,
    Node,
    Npm,
    Npx,
    Mysql,
    Mysqldump,
    Psql,
    PgDump,
    RedisCli,
    Python,
    Python3,
    Pip,
    Pip3,
    Go,
    Gofmt,
    Java,
    Javac,
    Jar,
}

impl Tool {
    pub const ALL: [Tool; 19] = [
        Tool::Php,
        Tool::Composer,
        Tool::Node,
        Tool::Npm,
        Tool::Npx,
        Tool::Mysql,
        Tool::Mysqldump,
        Tool::Psql,
        Tool::PgDump,
        Tool::RedisCli,
        Tool::Python,
        Tool::Python3,
        Tool::Pip,
        Tool::Pip3,
        Tool::Go,
        Tool::Gofmt,
        Tool::Java,
        Tool::Javac,
        Tool::Jar,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Tool::Php => "php",
            Tool::Composer => "composer",
            Tool::Node => "node",
            Tool::Npm => "npm",
            Tool::Npx => "npx",
            Tool::Mysql => "mysql",
            Tool::Mysqldump => "mysqldump",
            Tool::Psql => "psql",
            Tool::PgDump => "pg_dump",
            Tool::RedisCli => "redis-cli",
            Tool::Python => "python",
            Tool::Python3 => "python3",
            Tool::Pip => "pip",
            Tool::Pip3 => "pip3",
            Tool::Go => "go",
            Tool::Gofmt => "gofmt",
            Tool::Java => "java",
            Tool::Javac => "javac",
            Tool::Jar => "jar",
        }
    }

    /// `php`, `PHP` or `php.exe` → `Php`.
    pub fn from_name(name: &str) -> Option<Tool> {
        let name = name.to_ascii_lowercase();
        let name = name.strip_suffix(".exe").unwrap_or(&name);
        Tool::ALL.into_iter().find(|t| t.name() == name)
    }

    /// The runtime it comes with; `None` for the database clients.
    pub fn runtime(self) -> Option<RuntimeKind> {
        match self {
            Tool::Php | Tool::Composer => Some(RuntimeKind::Php),
            Tool::Node | Tool::Npm | Tool::Npx => Some(RuntimeKind::Node),
            Tool::Python | Tool::Python3 | Tool::Pip | Tool::Pip3 => Some(RuntimeKind::Python),
            Tool::Go | Tool::Gofmt => Some(RuntimeKind::Go),
            Tool::Java | Tool::Javac | Tool::Jar => Some(RuntimeKind::Java),
            Tool::Mysql | Tool::Mysqldump | Tool::Psql | Tool::PgDump | Tool::RedisCli => None,
        }
    }
}

/// Why a version was picked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "from", rename_all = "lowercase")]
pub enum Reason {
    Pinned { file: PathBuf },
    Site { domain: String },
    Default,
    Newest,
}

impl std::fmt::Display for Reason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Reason::Pinned { file } => write!(f, "pinned in {}", file.display()),
            Reason::Site { domain } => write!(f, "used by the site {domain}"),
            Reason::Default => f.write_str("the default version"),
            Reason::Newest => f.write_str("the newest installed version"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Choice {
    pub kind: RuntimeKind,
    pub version: String,
    pub reason: Reason,
}

/// Which version of `kind` to use in `dir`.
pub fn choose(
    kind: RuntimeKind,
    dir: &Path,
    settings: &Settings,
    sites: &[Site],
    runtimes: &RuntimeManager,
) -> Result<Choice> {
    let choice = |version: &str, reason| Choice {
        kind,
        version: version.to_string(),
        reason,
    };
    if let Some(pin) = pins::pinned(dir, kind)? {
        return Ok(choice(&pin.version, Reason::Pinned { file: pin.file }));
    }
    let site = sites
        .iter()
        .filter(|s| s.runtime.kind() == Some(kind) && dir.starts_with(&s.path))
        .max_by_key(|s| s.path.components().count());
    if let Some(site) = site {
        if let Some(version) = site_version(site) {
            let domain = site.domain();
            return Ok(choice(version, Reason::Site { domain }));
        }
    }
    if let Some(version) = settings.default_runtime(kind) {
        return Ok(choice(version, Reason::Default));
    }
    catalog::runtime(kind)
        .versions
        .iter()
        .find(|v| runtimes.get(kind, v).is_some())
        .map(|v| choice(v, Reason::Newest))
        .ok_or_else(|| {
            let id = kind.id();
            Error::Shell(format!(
                "no {} version is installed; install one with `stackyard runtimes install {id} <version>`",
                catalog::runtime(kind).name
            ))
        })
}

fn site_version(site: &Site) -> Option<&str> {
    site.runtime.version()
}

/// The runtimes a site's project uses: the one it is served with, Node.js
/// for the build tools most projects have (Vite, Tailwind), and any other
/// the folder pins. Only runtimes Stackyard can install.
pub fn site_runtimes(site: &Site) -> Vec<RuntimeKind> {
    let mut kinds: Vec<RuntimeKind> = site.runtime.kind().into_iter().collect();
    for kind in RuntimeKind::ALL {
        let wanted =
            kind == RuntimeKind::Node || pins::pinned(&site.path, kind).is_ok_and(|p| p.is_some());
        if wanted && !kinds.contains(&kind) {
            kinds.push(kind);
        }
    }
    kinds.retain(|k| catalog::runtime(*k).available);
    kinds
}

/// The version of `kind` a site runs, in the web server and in a terminal
/// in its folder alike: a version file in the project, else the version
/// saved with the site, else the default.
pub fn site_choice(
    site: &Site,
    kind: RuntimeKind,
    settings: &Settings,
    runtimes: &RuntimeManager,
) -> Choice {
    let choice = |version: String, reason| Choice {
        kind,
        version,
        reason,
    };
    if let Ok(Some(pin)) = pins::pinned(&site.path, kind) {
        return choice(pin.version, Reason::Pinned { file: pin.file });
    }
    if site.runtime.kind() == Some(kind) {
        if let Some(version) = site_version(site) {
            let domain = site.domain();
            return choice(version.to_string(), Reason::Site { domain });
        }
    }
    let reason = match settings.default_runtime(kind) {
        Some(_) => Reason::Default,
        None => Reason::Newest,
    };
    choice(
        crate::web::default_version(settings, runtimes, kind),
        reason,
    )
}

/// Picks the version of `kind` for a site; `None` goes back to the default.
///
/// The version a site is served with is saved with the site. Any other
/// runtime, and every runtime of a parked site, goes in the project's
/// `.stackyard` file, which also serves a terminal there. When a version
/// file already decides, `.stackyard` in the site's folder is updated so
/// it keeps deciding and the two never disagree.
pub fn set_site_version(
    store: &mut SiteStore,
    site: &Site,
    kind: RuntimeKind,
    version: Option<&str>,
) -> Result<()> {
    let family = catalog::runtime(kind);
    if let Some(version) = version {
        if !family.offers(version) {
            return Err(Error::UnknownVersion {
                name: family.name.into(),
                version: version.into(),
            });
        }
    }
    let saved = store
        .get_mut(&site.domain())
        .ok()
        .filter(|s| s.path == site.path && s.runtime.kind() == Some(kind));
    let Some(version) = version else {
        if saved.is_some() {
            return Err(Error::Shell(format!(
                "{} {} is served with {}; pick a version",
                site.domain(),
                family.name,
                family.name
            )));
        }
        return pins::unpin(&site.path, kind);
    };
    let pinned = pins::pinned(&site.path, kind).map_or(true, |p| p.is_some());
    match saved {
        Some(saved) => {
            saved.runtime.set_version(version);
            store.save()?;
            if pinned {
                pins::pin(&site.path, kind, version)?;
            }
        }
        None => {
            pins::pin(&site.path, kind, version)?;
        }
    }
    Ok(())
}

/// How to run a tool: the real program, its arguments and the environment to add.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launch {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    /// Folder put first on `PATH`, so programs the tool starts (`npm` running
    /// `node`, `composer` running `php`) use the same version.
    pub path_first: PathBuf,
    pub env: Vec<(String, OsString)>,
    /// e.g. `PHP 8.3 (pinned in /code/app/.stackyard)`.
    pub summary: String,
}

impl Launch {
    /// `PATH` with [`Launch::path_first`] in front.
    pub fn path_var(&self) -> OsString {
        let rest = std::env::var_os("PATH").unwrap_or_default();
        let mut parts = vec![self.path_first.clone()];
        parts.extend(std::env::split_paths(&rest).filter(|p| *p != self.path_first));
        std::env::join_paths(parts).unwrap_or(rest)
    }
}

/// Where `npm install -g` puts packages. Not the Node.js version's own
/// folder, which an update or reinstall of that version replaces: on
/// Windows the usual `%APPDATA%\npm`, which Node.js's own installer uses and
/// puts on the user's Path, so the commands work in every terminal; else a
/// folder of Stackyard's.
pub fn npm_global_dir(home: &Path) -> PathBuf {
    match Platform::current().os {
        Os::Windows => {
            dirs::data_dir().map_or_else(|| home.join("tools/npm-global"), |d| d.join("npm"))
        }
        _ => home.join("tools/npm-global"),
    }
}

/// The folder npm puts those packages' commands in.
pub fn npm_global_bin(home: &Path, os: Os) -> PathBuf {
    match os {
        Os::Windows => npm_global_dir(home),
        _ => npm_global_dir(home).join("bin"),
    }
}

/// `NPM_CONFIG_PREFIX` for [`npm_global_dir`], unless one is set already.
pub fn npm_prefix_env(home: &Path) -> Option<(String, OsString)> {
    let own = ["NPM_CONFIG_PREFIX", "npm_config_prefix"]
        .iter()
        .any(|name| std::env::var_os(name).is_some_and(|v| !v.is_empty()));
    (!own).then(|| {
        (
            "NPM_CONFIG_PREFIX".to_string(),
            npm_global_dir(home).into_os_string(),
        )
    })
}

/// Folders a Stackyard terminal puts first on its `PATH`: the shims, the
/// commands of global npm packages, and the default Node.js version's own
/// folder, where `npm install -g` put them before [`npm_global_dir`].
pub fn terminal_path(home: &Path, settings: &Settings, os: Os) -> Vec<PathBuf> {
    let mut dirs = vec![shims::bin_dir(home), npm_global_bin(home, os)];
    if let Some(node) = RuntimeManager::new(home).default_for(settings, RuntimeKind::Node) {
        dirs.push(match os {
            Os::Windows => node.path,
            _ => node.path.join("bin"),
        });
    }
    dirs
}

/// Everything needed to work out which program a shim runs.
pub struct Context {
    pub home: PathBuf,
    pub settings: Settings,
    pub sites: Vec<Site>,
    pub os: Os,
}

impl Context {
    pub fn load(home: &Path) -> Result<Self> {
        Ok(Self {
            home: home.to_owned(),
            settings: Settings::load_from(&home.join("settings.toml"))?,
            sites: SiteStore::load_from(&home.join("sites.toml"))?
                .list()
                .to_vec(),
            os: Platform::current().os,
        })
    }

    pub fn launch(&self, tool: Tool, dir: &Path, args: Vec<OsString>) -> Result<Launch> {
        match tool.runtime() {
            Some(kind) => self.runtime_launch(tool, kind, dir, args),
            None => self.database_launch(tool, args),
        }
    }

    fn runtime_launch(
        &self,
        tool: Tool,
        kind: RuntimeKind,
        dir: &Path,
        args: Vec<OsString>,
    ) -> Result<Launch> {
        let runtimes = RuntimeManager::new(&self.home);
        // Python, Go and Java are often installed already. Until Stackyard
        // has a version of its own (or with the module off), the commands
        // run that one, so putting the shims on PATH never hides it.
        let falls_through = matches!(
            kind,
            RuntimeKind::Python | RuntimeKind::Go | RuntimeKind::Java
        );
        let has_own = self.settings.module_enabled(kind.id())
            && catalog::runtime(kind)
                .versions
                .iter()
                .any(|v| runtimes.get(kind, v).is_some());
        if falls_through && !has_own && pins::pinned(dir, kind).ok().flatten().is_none() {
            if let Some(program) = self.elsewhere_on_path(tool.name()) {
                return Ok(Launch {
                    path_first: program.parent().map(Path::to_path_buf).unwrap_or_default(),
                    summary: format!("{} from your computer", tool.name()),
                    program,
                    args,
                    env: Vec::new(),
                });
            }
        }
        self.settings.require_module(kind.id())?;
        let choice = choose(kind, dir, &self.settings, &self.sites, &runtimes)?;
        let name = catalog::runtime(kind).name;
        let installed = runtimes.get(kind, &choice.version).ok_or_else(|| {
            Error::Shell(format!(
                "{name} {} is {}, but it is not installed. Install it with `stackyard runtimes install {} {}`",
                choice.version,
                choice.reason,
                kind.id(),
                choice.version
            ))
        })?;
        let bin = installed
            .binary
            .parent()
            .map_or_else(|| installed.path.clone(), Path::to_path_buf);
        let exe = if self.os == Os::Windows { ".exe" } else { "" };
        let mut program = installed.binary.clone();
        let mut full = Vec::new();
        match tool {
            // A sibling of the main program, e.g. `bin/gofmt` next to `bin/go`.
            Tool::Gofmt | Tool::Javac | Tool::Jar => {
                program = bin.join(format!("{}{exe}", tool.name()));
            }
            Tool::Pip | Tool::Pip3 => full.extend(["-m".into(), "pip".into()]),
            Tool::Composer => full.push(composer_phar(&self.home).into_os_string()),
            Tool::Npm | Tool::Npx => {
                let modules = match self.os {
                    Os::Windows => installed.path.join("node_modules"),
                    _ => installed.path.join("lib/node_modules"),
                };
                let script = format!("{}-cli.js", tool.name());
                full.push(modules.join("npm/bin").join(script).into_os_string());
            }
            _ => {}
        }
        full.extend(args);
        // PHP on Windows reads no php.ini of its own, so without this the
        // terminal had no openssl, curl or zip and Composer could not
        // download. Point it at the one the web server uses.
        let mut env = Vec::new();
        if kind == RuntimeKind::Php {
            let ini =
                crate::web::php_ini_file(&self.home, &choice.version, &installed.path, self.os)?;
            if let Some(dir) = ini.parent() {
                env.push(("PHPRC".to_string(), dir.as_os_str().to_owned()));
            }
        }
        if kind == RuntimeKind::Node {
            env.extend(npm_prefix_env(&self.home));
        }
        if kind == RuntimeKind::Java {
            env.push((
                "JAVA_HOME".to_string(),
                crate::runtimes::java_home(&installed.path, self.os).into_os_string(),
            ));
        }
        Ok(Launch {
            program,
            args: full,
            path_first: bin,
            env,
            summary: format!("{name} {} ({})", choice.version, choice.reason),
        })
    }

    /// `name` in a folder on PATH other than Stackyard's own commands.
    pub fn elsewhere_on_path(&self, name: &str) -> Option<PathBuf> {
        let own = shims::bin_dir(&self.home);
        let own = own.canonicalize().unwrap_or(own);
        let path = std::env::var_os("PATH")?;
        let names: Vec<String> = match self.os {
            Os::Windows => [".exe", ".cmd", ".bat"]
                .iter()
                .map(|ext| format!("{name}{ext}"))
                .collect(),
            _ => vec![name.to_string()],
        };
        std::env::split_paths(&path)
            .filter(|dir| dir.canonicalize().unwrap_or_else(|_| dir.clone()) != own)
            .flat_map(|dir| names.iter().map(move |n| dir.join(n)))
            .find(|p| p.is_file())
    }

    /// The port a service listens on, which moves when another program has
    /// the usual one.
    fn service_port(&self, id: &str) -> u16 {
        catalog::service(id).map_or(0, |def| crate::services::port(&self.home, def))
    }

    fn database_launch(&self, tool: Tool, args: Vec<OsString>) -> Result<Launch> {
        match tool {
            Tool::Psql | Tool::PgDump => {
                let env = [
                    ("PGHOST", "127.0.0.1".to_string()),
                    ("PGPORT", self.service_port("postgres").to_string()),
                    ("PGUSER", crate::services::postgres::USER.to_string()),
                ];
                return self.service_tool("postgres", tool, args, &env);
            }
            Tool::RedisCli => {
                // redis-cli has no setting for the port, only `-p`.
                let port = self.service_port("redis");
                let args = if port == 6379 {
                    args
                } else {
                    [OsString::from("-p"), OsString::from(port.to_string())]
                        .into_iter()
                        .chain(args)
                        .collect()
                };
                return self.service_tool("redis", tool, args, &[]);
            }
            _ => {}
        }
        let services = ServiceManager::new(&self.home);
        let usable = |id: &&str| self.settings.module_enabled(id) && services.get(id).is_some();
        let candidates: Vec<&str> = ["mysql", "mariadb"].into_iter().filter(usable).collect();
        // Prefer the one that is running: only it has a socket file. On
        // Windows there are no sockets, and both may keep port 3306 in
        // their settings, so ask whichever server answers on a port which
        // kind it is: MySQL's mysqldump fails on a MariaDB server.
        let socket = |id: &str| services.data_dir(id).join("mysql.sock");
        let port_of =
            |id: &str| catalog::service(id).map_or(0, |def| crate::services::port(&self.home, def));
        let id = candidates
            .iter()
            .copied()
            .find(|id| socket(id).exists())
            .or_else(|| {
                if candidates.len() < 2 {
                    return None;
                }
                candidates
                    .iter()
                    .copied()
                    .find(|id| mysql_flavor(port_of(id)) == Some(*id))
            })
            .or(candidates.first().copied())
            .ok_or_else(|| {
                Error::Shell(format!(
                    "{} comes with MySQL or MariaDB; install one with `stackyard services install mysql`",
                    tool.name()
                ))
            })?;
        self.mysql_tool(id, tool, args)
    }

    /// `mysql` or `mysqldump` from one server's own folder, `mysql` or
    /// `mariadb`, set up to talk to that server; for a site whose database
    /// is known to be in it, whichever of the two is switched on.
    pub fn mysql_launch(&self, id: &str, tool: Tool, args: Vec<OsString>) -> Result<Launch> {
        if !matches!(id, "mysql" | "mariadb") || !matches!(tool, Tool::Mysql | Tool::Mysqldump) {
            return Err(Error::Shell(format!(
                "{} has no {} program",
                id,
                tool.name()
            )));
        }
        self.mysql_tool(id, tool, args)
    }

    fn mysql_tool(&self, id: &str, tool: Tool, args: Vec<OsString>) -> Result<Launch> {
        let services = ServiceManager::new(&self.home);
        let socket = |id: &str| services.data_dir(id).join("mysql.sock");
        let def = catalog::service(id)
            .ok_or_else(|| Error::Shell(format!("{id} is not a database server")))?;
        if services.get(id).is_none() {
            return Err(Error::Shell(format!("{} is not installed", def.name)));
        }
        let bin = services.dir(def).join("bin");
        let exe = if self.os == Os::Windows { ".exe" } else { "" };
        // MariaDB names its clients `mariadb` and `mariadb-dump`; older
        // archives only have the `mysql` names.
        let names: &[&str] = match tool {
            Tool::Mysql => &["mysql", "mariadb"],
            _ => &["mysqldump", "mariadb-dump"],
        };
        let program = names
            .iter()
            .map(|n| bin.join(format!("{n}{exe}")))
            .find(|p| p.exists())
            .ok_or_else(|| Error::Shell(format!("{} has no {} program", def.name, tool.name())))?;
        let mut env = vec![(
            "MYSQL_TCP_PORT".to_string(),
            OsString::from(crate::services::port(&self.home, def).to_string()),
        )];
        if self.os != Os::Windows {
            env.push(("MYSQL_UNIX_PORT".into(), socket(id).into_os_string()));
        }
        Ok(Launch {
            program,
            args,
            path_first: bin,
            env,
            summary: format!("{} {}", def.name, def.version),
        })
    }
}

/// Which kind of server answers on `port`, from the greeting a MySQL or
/// MariaDB server sends as soon as a client connects: `mariadb` when its
/// version says MariaDB, else `mysql`. `None` when nothing answers.
pub fn mysql_flavor(port: u16) -> Option<&'static str> {
    use std::io::Read;
    use std::net::{Ipv4Addr, SocketAddr, TcpStream};
    use std::time::Duration;
    if port == 0 {
        return None;
    }
    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_millis(300)).ok()?;
    stream
        .set_read_timeout(Some(Duration::from_millis(800)))
        .ok()?;
    let mut greeting = [0u8; 160];
    let read = stream.read(&mut greeting).ok()?;
    flavor_of_greeting(&greeting[..read])
}

/// The kind of server from its first packet: 4 header bytes, protocol
/// version 10, then the version text, e.g. `5.5.5-10.11.8-MariaDB` or
/// `8.4.3`.
fn flavor_of_greeting(packet: &[u8]) -> Option<&'static str> {
    let body = packet.get(4..)?;
    if body.first() != Some(&10) {
        return None;
    }
    let version = &body[1..];
    let end = version
        .iter()
        .position(|b| *b == 0)
        .unwrap_or(version.len());
    let version = String::from_utf8_lossy(&version[..end]);
    Some(if version.contains("MariaDB") {
        "mariadb"
    } else {
        "mysql"
    })
}

impl Context {
    /// A client program in an installed service's `bin` folder, e.g. `psql`.
    fn service_tool(
        &self,
        id: &str,
        tool: Tool,
        args: Vec<OsString>,
        env: &[(&str, String)],
    ) -> Result<Launch> {
        let services = ServiceManager::new(&self.home);
        let def = catalog::service(id).expect("a catalog service");
        if !self.settings.module_enabled(id) || services.get(id).is_none() {
            return Err(Error::Shell(format!(
                "{} comes with {}; install it with `stackyard services install {id}`",
                tool.name(),
                def.name
            )));
        }
        let bin = services.dir(def).join("bin");
        let exe = if self.os == Os::Windows { ".exe" } else { "" };
        let program = bin.join(format!("{}{exe}", tool.name()));
        if !program.exists() {
            return Err(Error::Shell(format!(
                "{} has no {} program",
                def.name,
                tool.name()
            )));
        }
        Ok(Launch {
            program,
            args,
            path_first: bin,
            env: env
                .iter()
                .map(|(k, v)| (k.to_string(), OsString::from(v)))
                .collect(),
            summary: format!("{} {}", def.name, def.version),
        })
    }
}

const COMPOSER: &str = "https://getcomposer.org/download/latest-stable/composer.phar";

pub fn composer_phar(home: &Path) -> PathBuf {
    home.join("tools/composer/composer.phar")
}

/// Downloads the latest stable Composer, checked against its published
/// SHA-256. `STACKYARD_COMPOSER_MIRROR` points at another copy.
pub fn install_composer(home: &Path, progress: &mut dyn FnMut(Step)) -> Result<PathBuf> {
    let url = std::env::var("STACKYARD_COMPOSER_MIRROR").unwrap_or_else(|_| COMPOSER.into());
    let http = Http::new();
    progress(Step::Resolving);
    let sums = http.get_text(&format!("{url}.sha256sum"))?;
    let checksum = sums
        .split_whitespace()
        .next()
        .filter(|c| crate::download::is_sha256(c))
        .ok_or_else(|| Error::Download {
            url: format!("{url}.sha256sum"),
            message: "no SHA-256 checksum in it".into(),
        })?
        .to_ascii_lowercase();
    let dest = composer_phar(home);
    http.download(&url, &dest, &checksum, &mut |done, total| {
        progress(Step::Downloading { done, total })
    })?;
    Ok(dest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tells_mysql_from_mariadb_by_greeting() {
        let packet = |version: &str| {
            let mut p = vec![0, 0, 0, 0, 10];
            p.extend_from_slice(version.as_bytes());
            p.push(0);
            p.extend_from_slice(&[1, 2, 3]);
            p
        };
        assert_eq!(
            flavor_of_greeting(&packet("5.5.5-10.11.8-MariaDB")),
            Some("mariadb")
        );
        assert_eq!(
            flavor_of_greeting(&packet("11.8.3-MariaDB-log")),
            Some("mariadb")
        );
        assert_eq!(flavor_of_greeting(&packet("8.4.3")), Some("mysql"));
        assert_eq!(flavor_of_greeting(&[0, 0, 0, 0, 0xff, 1]), None);
        assert_eq!(flavor_of_greeting(&[1, 2]), None);
    }

    #[test]
    fn asks_a_listening_server_its_kind() {
        use std::io::Write;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            s.write_all(b"\x2a\0\0\0\x0a5.5.5-10.11.8-MariaDB\0")
                .unwrap();
        });
        assert_eq!(mysql_flavor(port), Some("mariadb"));
        server.join().unwrap();
    }
    use crate::sites::SiteRuntime;

    fn site(name: &str, path: &Path, version: &str) -> Site {
        Site {
            name: name.into(),
            tld: ".stack".into(),
            path: path.to_owned(),
            runtime: SiteRuntime::Php {
                version: version.into(),
            },
            https: true,
            dev_server: false,
            settings: Default::default(),
        }
    }

    /// Pretends `version` of `kind` is installed under `home`.
    fn fake_install(home: &Path, kind: RuntimeKind, version: &str) -> PathBuf {
        let runtimes = RuntimeManager::new(home);
        let dir = runtimes.dir(kind, version);
        let binary = dir.join(crate::runtimes::binary_path(kind, Platform::current().os));
        std::fs::create_dir_all(binary.parent().unwrap()).unwrap();
        std::fs::write(&binary, "").unwrap();
        crate::install::write_receipt_for_tests(&dir, &format!("{version}.1"));
        binary
    }

    #[test]
    fn knows_its_names() {
        assert_eq!(Tool::from_name("PHP.EXE"), Some(Tool::Php));
        assert_eq!(Tool::from_name("npx"), Some(Tool::Npx));
        assert_eq!(Tool::from_name("stackyard"), None);
        for tool in Tool::ALL {
            assert_eq!(Tool::from_name(tool.name()), Some(tool));
        }
    }

    #[test]
    fn picks_pin_then_site_then_default_then_newest() {
        let home = crate::testutil::tempdir();
        let code = crate::testutil::tempdir();
        let shop = code.join("shop");
        let deep = shop.join("src/Http");
        std::fs::create_dir_all(&deep).unwrap();
        let runtimes = RuntimeManager::new(&home);
        let mut settings = Settings::default();
        let sites = vec![site("shop", &shop, "8.2")];
        let php = RuntimeKind::Php;

        let err = choose(php, &code, &settings, &[], &runtimes).unwrap_err();
        assert!(err.to_string().contains("no PHP version is installed"));

        fake_install(&home, php, "8.3");
        fake_install(&home, php, "8.1");
        let newest = choose(php, &code, &settings, &sites, &runtimes).unwrap();
        assert_eq!(
            (newest.version.as_str(), newest.reason),
            ("8.3", Reason::Newest)
        );

        settings.set_default_runtime(php, Some("8.1"));
        let default = choose(php, &code, &settings, &sites, &runtimes).unwrap();
        assert_eq!(
            (default.version.as_str(), default.reason),
            ("8.1", Reason::Default)
        );

        let by_site = choose(php, &deep, &settings, &sites, &runtimes).unwrap();
        assert_eq!(by_site.version, "8.2");
        assert_eq!(
            by_site.reason,
            Reason::Site {
                domain: "shop.stack".into()
            }
        );

        pins::pin(&shop, php, "8.4").unwrap();
        let pinned = choose(php, &deep, &settings, &sites, &runtimes).unwrap();
        assert_eq!(pinned.version, "8.4");
        assert!(matches!(pinned.reason, Reason::Pinned { .. }));
    }

    #[test]
    fn sites_pick_their_versions() {
        let home = crate::testutil::tempdir();
        let code = crate::testutil::tempdir();
        let runtimes = RuntimeManager::new(&home);
        let settings = Settings::default();
        let mut store = SiteStore::load_from(&home.join("sites.toml")).unwrap();
        let shop = store.add(site("shop", &code, "8.2")).unwrap().clone();
        let php = RuntimeKind::Php;
        let node = RuntimeKind::Node;
        assert_eq!(site_runtimes(&shop), [php, node]);

        // The served runtime is saved with the site, no file in the project.
        set_site_version(&mut store, &shop, php, Some("8.4")).unwrap();
        let saved = SiteStore::load_from(&home.join("sites.toml")).unwrap();
        let shop = saved.list()[0].clone();
        let choice = site_choice(&shop, php, &settings, &runtimes);
        assert_eq!(choice.version, "8.4");
        assert!(matches!(choice.reason, Reason::Site { .. }));
        assert!(!code.join(pins::PIN_FILE).exists());
        assert!(set_site_version(&mut store, &shop, php, None).is_err());
        assert!(set_site_version(&mut store, &shop, php, Some("5.6")).is_err());

        // Node.js for the build tools goes in `.stackyard`, so `npm` in a
        // terminal there uses it too.
        set_site_version(&mut store, &shop, node, Some("22")).unwrap();
        let choice = site_choice(&shop, node, &settings, &runtimes);
        assert_eq!(choice.version, "22");
        assert!(matches!(choice.reason, Reason::Pinned { .. }));
        let sites = store.list().to_vec();
        assert_eq!(
            choose(node, &code, &settings, &sites, &runtimes)
                .unwrap()
                .version,
            "22"
        );
        set_site_version(&mut store, &shop, node, None).unwrap();
        assert!(!code.join(pins::PIN_FILE).exists());

        // A version file already decides: it is kept in step with the pick.
        std::fs::write(code.join(".php-version"), "8.3\n").unwrap();
        assert_eq!(site_choice(&shop, php, &settings, &runtimes).version, "8.3");
        set_site_version(&mut store, &shop, php, Some("8.5")).unwrap();
        let shop = store.list()[0].clone();
        assert_eq!(site_choice(&shop, php, &settings, &runtimes).version, "8.5");
        assert_eq!(
            choose(php, &code, &settings, &sites, &runtimes)
                .unwrap()
                .version,
            "8.5"
        );
    }

    #[test]
    fn launches_python_go_and_java() {
        let home = crate::testutil::tempdir();
        let project = crate::testutil::tempdir();
        let mut settings = Settings::default();
        for id in ["python", "go", "java"] {
            settings.set_module(id, true).unwrap();
        }
        settings.save_to(&home.join("settings.toml")).unwrap();
        let python = fake_install(&home, RuntimeKind::Python, "3.13");
        fake_install(&home, RuntimeKind::Go, "1.26");
        let java = fake_install(&home, RuntimeKind::Java, "21");
        let context = Context::load(&home).unwrap();

        let pip = context
            .launch(Tool::Pip3, &project, vec!["install".into()])
            .unwrap();
        assert_eq!(pip.program, python);
        assert_eq!(
            pip.args,
            vec![OsString::from("-m"), "pip".into(), "install".into()]
        );

        let exe = std::env::consts::EXE_SUFFIX;
        let gofmt = context.launch(Tool::Gofmt, &project, vec![]).unwrap();
        assert!(gofmt.program.ends_with(format!("bin/gofmt{exe}")));
        assert!(gofmt.summary.starts_with("Go 1.26"), "{}", gofmt.summary);

        let javac = context.launch(Tool::Javac, &project, vec![]).unwrap();
        assert_eq!(javac.program, java.with_file_name(format!("javac{exe}")));
        let java_home = javac.env.iter().find(|(k, _)| k == "JAVA_HOME").unwrap();
        assert_eq!(
            PathBuf::from(&java_home.1),
            crate::runtimes::java_home(
                &RuntimeManager::new(&home).dir(RuntimeKind::Java, "21"),
                Platform::current().os
            )
        );
    }

    #[test]
    fn python_go_and_java_fall_back_to_the_computers_own() {
        let home = crate::testutil::tempdir();
        let project = crate::testutil::tempdir();
        let mut context = Context::load(&home).unwrap();
        context.os = Platform::current().os;
        // Nothing installed by Stackyard: whatever else is on PATH runs, or
        // the usual "not installed" message when there is none.
        match context.elsewhere_on_path("go") {
            Some(system) => {
                let go = context.launch(Tool::Go, &project, vec![]).unwrap();
                assert_eq!(go.program, system);
                assert!(go.summary.contains("from your computer"));
            }
            None => assert!(context.launch(Tool::Go, &project, vec![]).is_err()),
        }
        // A pinned version never falls back.
        pins::pin(&project, RuntimeKind::Go, "1.26").unwrap();
        assert!(context.launch(Tool::Go, &project, vec![]).is_err());
    }

    #[test]
    fn launches_the_real_programs() {
        let home = crate::testutil::tempdir();
        let project = crate::testutil::tempdir();
        let php = fake_install(&home, RuntimeKind::Php, "8.4");
        let node = fake_install(&home, RuntimeKind::Node, "22");
        let context = Context::load(&home).unwrap();

        let run = context
            .launch(Tool::Php, &project, vec!["-v".into()])
            .unwrap();
        assert_eq!(run.program, php);
        assert_eq!(run.args, vec![OsString::from("-v")]);
        assert_eq!(run.path_first, php.parent().unwrap());
        assert!(run.summary.starts_with("PHP 8.4"), "{}", run.summary);
        let path = run.path_var();
        assert_eq!(std::env::split_paths(&path).next().unwrap(), run.path_first);

        let composer = context
            .launch(Tool::Composer, &project, vec!["install".into()])
            .unwrap();
        assert_eq!(composer.program, php);
        assert_eq!(
            composer.args,
            vec![composer_phar(&home).into_os_string(), "install".into()]
        );

        let npm = context.launch(Tool::Npm, &project, vec![]).unwrap();
        assert_eq!(npm.program, node);
        let script = PathBuf::from(&npm.args[0]);
        assert!(
            script.ends_with("node_modules/npm/bin/npm-cli.js"),
            "{script:?}"
        );
        // `npm install -g` goes where a Node.js update leaves it alone.
        if std::env::var_os("NPM_CONFIG_PREFIX").is_none()
            && std::env::var_os("npm_config_prefix").is_none()
        {
            let prefix = (
                "NPM_CONFIG_PREFIX".to_string(),
                npm_global_dir(&home).into(),
            );
            assert!(npm.env.contains(&prefix), "{:?}", npm.env);
            assert!(!npm_global_dir(&home).starts_with(home.join("runtimes")));
        }
        let first = terminal_path(&home, &context.settings, Platform::current().os);
        assert_eq!(first[0], shims::bin_dir(&home));
        assert_eq!(first[1], npm_global_bin(&home, Platform::current().os));

        pins::pin(&project, RuntimeKind::Node, "20").unwrap();
        let err = context.launch(Tool::Node, &project, vec![]).unwrap_err();
        let err = err.to_string();
        assert!(err.contains("Node.js 20 is pinned in"), "{err}");
        assert!(err.contains("stackyard runtimes install node 20"), "{err}");
    }

    #[test]
    fn respects_switched_off_modules() {
        let home = crate::testutil::tempdir();
        fake_install(&home, RuntimeKind::Php, "8.4");
        let mut context = Context::load(&home).unwrap();
        context.settings.set_module("php", false).unwrap();
        let err = context.launch(Tool::Php, &home, vec![]).unwrap_err();
        assert!(matches!(err, Error::ModuleOff { .. }));
    }

    #[test]
    fn finds_the_database_client() {
        let home = crate::testutil::tempdir();
        let context = Context::load(&home).unwrap();
        let err = context.launch(Tool::Mysql, &home, vec![]).unwrap_err();
        assert!(err.to_string().contains("stackyard services install mysql"));

        let services = ServiceManager::new(&home);
        let dir = services.dir(catalog::service("mysql").unwrap());
        let exe = if context.os == Os::Windows {
            ".exe"
        } else {
            ""
        };
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        std::fs::write(dir.join(format!("bin/mysql{exe}")), "").unwrap();
        crate::install::write_receipt_for_tests(&dir, "8.4.7");
        let run = context
            .launch(Tool::Mysql, &home, vec!["-uroot".into()])
            .unwrap();
        assert_eq!(run.program, dir.join(format!("bin/mysql{exe}")));
        assert!(run
            .env
            .iter()
            .any(|(k, v)| k == "MYSQL_TCP_PORT" && v == "3306"));
        assert_eq!(run.summary, "MySQL 8.4");
    }
}
