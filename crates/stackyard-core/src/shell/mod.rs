//! Stackyard Shell: `php`, `node`, `npm`, `composer` and `mysql` commands
//! that run the version each project asks for.
//!
//! The commands are small "shims" in `<home>/bin` that all lead back to the
//! `stackyard` program. It sees which name it was started as, works out the
//! version for the current folder and runs the real program from there:
//!
//! 1. the nearest `.stackyard` file that pins one ([`pins`]),
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
}

impl Tool {
    pub const ALL: [Tool; 10] = [
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
    match &site.runtime {
        crate::sites::SiteRuntime::Php { version } => Some(version),
        crate::sites::SiteRuntime::Node { version, .. } => Some(version),
        crate::sites::SiteRuntime::Static => None,
    }
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
        self.settings.require_module(kind.id())?;
        let runtimes = RuntimeManager::new(&self.home);
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
        let mut full = Vec::new();
        match tool {
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
        Ok(Launch {
            program: installed.binary,
            args: full,
            path_first: bin,
            env,
            summary: format!("{name} {} ({})", choice.version, choice.reason),
        })
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
        // Prefer the one that is running: only it has a socket file.
        let socket = |id: &str| services.data_dir(id).join("mysql.sock");
        let id = candidates
            .iter()
            .copied()
            .find(|id| socket(id).exists())
            .or(candidates.first().copied())
            .ok_or_else(|| {
                Error::Shell(format!(
                    "{} comes with MySQL or MariaDB; install one with `stackyard services install mysql`",
                    tool.name()
                ))
            })?;
        let def = catalog::service(id).expect("listed above");
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
