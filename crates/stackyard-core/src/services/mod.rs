//! Installs services (web server, databases, mail catcher) and turns them
//! into processes the supervisor can run.
//!
//! Layout inside Stackyard's data folder:
//! - `services/<id>/<line>/`: the program, replaced on update
//! - `data/<id>/`: databases and state, kept across updates and uninstalls
//! - `config/<id>/`: config files Stackyard writes before each start

mod caddy;
mod github;
mod mailpit;
mod mariadb;
mod mysql;

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::catalog::{self, ServiceDef};
use crate::download::Http;
use crate::install::{self, Release, Step};
use crate::modules;
use crate::platform::{Os, Platform};
use crate::supervisor::ProcessSpec;
use crate::{paths, Error, Result};

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct InstalledService {
    pub id: &'static str,
    /// The release line, e.g. `8.4`.
    pub version: &'static str,
    /// The exact release, e.g. `8.4.6`.
    pub release: String,
    pub path: PathBuf,
}

/// Everything a service needs to build its command line.
pub(crate) struct Context<'a> {
    pub def: &'static ServiceDef,
    pub os: Os,
    /// Where the program is installed.
    pub install: &'a Path,
    /// Where it keeps its data; created before `spec` is called.
    pub data: PathBuf,
    /// Where Stackyard writes its config; created before `spec` is called.
    pub config: PathBuf,
}

impl Context<'_> {
    pub fn program(&self) -> PathBuf {
        self.install.join(program(self.def.id, self.os))
    }

    pub fn base_spec(&self, args: Vec<String>) -> ProcessSpec {
        ProcessSpec {
            id: self.def.id.to_string(),
            name: self.def.name.to_string(),
            program: self.program(),
            args,
            env: Vec::new(),
            cwd: Some(self.data.clone()),
            ports: self.def.ports.to_vec(),
            restart_on_crash: true,
        }
    }
}

pub struct ServiceManager {
    home: PathBuf,
    downloads: PathBuf,
    http: Http,
    platform: Platform,
}

impl ServiceManager {
    pub fn open() -> Self {
        Self::new(&paths::home())
    }

    pub fn new(home: &Path) -> Self {
        Self {
            home: home.to_owned(),
            downloads: home.join("downloads"),
            http: Http::new(),
            platform: Platform::current(),
        }
    }

    pub fn dir(&self, def: &ServiceDef) -> PathBuf {
        self.home.join("services").join(def.id).join(def.version)
    }

    pub fn data_dir(&self, id: &str) -> PathBuf {
        self.home.join("data").join(id)
    }

    pub fn get(&self, id: &str) -> Option<InstalledService> {
        let def = catalog::service(id)?;
        let path = self.dir(def);
        let receipt = install::read_receipt(&path)?;
        Some(InstalledService {
            id: def.id,
            version: def.version,
            release: receipt.release,
            path,
        })
    }

    /// Looks up the newest release in the service's line for this computer.
    pub fn resolve(&self, id: &str) -> Result<Release> {
        let def = offered(id)?;
        let p = self.platform;
        match def.id {
            "caddy" => caddy::resolve(&self.http, def, p),
            "mailpit" => mailpit::resolve(&self.http, p),
            "mysql" => mysql::resolve(&self.http, def, p),
            "mariadb" => mariadb::resolve(&self.http, def, p),
            _ => Err(Error::UnknownModule(id.to_string())),
        }
    }

    pub fn install(&self, id: &str, progress: &mut dyn FnMut(Step)) -> Result<InstalledService> {
        progress(Step::Resolving);
        let release = self.resolve(id)?;
        self.install_release(id, &release, progress)
    }

    pub fn install_release(
        &self,
        id: &str,
        release: &Release,
        progress: &mut dyn FnMut(Step),
    ) -> Result<InstalledService> {
        let def = offered(id)?;
        install::install(
            &self.http,
            &self.downloads,
            release,
            &self.dir(def),
            program(def.id, self.platform.os),
            progress,
        )?;
        self.get(id).ok_or_else(|| not_installed(def))
    }

    /// Removes the program. Its data folder is kept, so reinstalling brings
    /// databases back.
    pub fn uninstall(&self, id: &str) -> Result<()> {
        let def = offered(id)?;
        if self.get(id).is_none() {
            return Err(not_installed(def));
        }
        install::remove(&self.dir(def))
    }

    /// Prepares config and data (creating a fresh database on first start)
    /// and returns the process to run. Can take several seconds the first
    /// time, so call it off the interface thread.
    pub fn spec(&self, id: &str) -> Result<ProcessSpec> {
        let def = offered(id)?;
        let installed = self.get(id).ok_or_else(|| not_installed(def))?;
        let ctx = Context {
            def,
            os: self.platform.os,
            install: &installed.path,
            data: self.data_dir(def.id),
            config: self.home.join("config").join(def.id),
        };
        for dir in [&ctx.data, &ctx.config] {
            std::fs::create_dir_all(dir).map_err(|source| Error::Io {
                path: dir.clone(),
                source,
            })?;
        }
        match def.id {
            "caddy" => caddy::spec(&ctx),
            "mailpit" => mailpit::spec(&ctx),
            "mysql" => mysql::spec(&ctx),
            "mariadb" => mariadb::spec(&ctx),
            _ => Err(Error::UnknownModule(id.to_string())),
        }
    }
}

/// The main program inside an installed service folder.
pub fn program(id: &str, os: Os) -> &'static str {
    let windows = os == Os::Windows;
    match (id, windows) {
        ("caddy", false) => "caddy",
        ("caddy", true) => "caddy.exe",
        ("mailpit", false) => "mailpit",
        ("mailpit", true) => "mailpit.exe",
        ("mysql", false) => "bin/mysqld",
        ("mysql", true) => "bin/mysqld.exe",
        ("mariadb", false) => "bin/mariadbd",
        ("mariadb", true) => "bin/mariadbd.exe",
        _ => "",
    }
}

/// A catalog service whose module exists and is offered on this system.
fn offered(id: &str) -> Result<&'static ServiceDef> {
    let def = catalog::service(id).ok_or_else(|| Error::UnknownModule(id.to_string()))?;
    let module = modules::find(id).ok_or_else(|| Error::UnknownModule(id.to_string()))?;
    if !module.available {
        return Err(Error::ModuleUnavailable(def.name.into()));
    }
    if !module.available_here() {
        return Err(Error::NotOnThisSystem {
            name: def.name.into(),
            system: Platform::current().os.to_string(),
        });
    }
    Ok(def)
}

fn not_installed(def: &ServiceDef) -> Error {
    Error::NotInstalled {
        name: def.name.into(),
        version: def.version.into(),
    }
}

/// Writes a config file only when its contents changed.
pub(crate) fn write_config(path: &Path, text: &str) -> Result<()> {
    if std::fs::read_to_string(path).is_ok_and(|old| old == text) {
        return Ok(());
    }
    std::fs::write(path, text).map_err(|source| Error::Io {
        path: path.to_owned(),
        source,
    })
}

/// Paths in config files and arguments, with forward slashes everywhere
/// (Windows accepts them, and they need no escaping).
pub(crate) fn slash(path: &Path) -> String {
    path.display().to_string().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::archive::fixtures;
    use crate::download::{sha256_file, testserver};

    /// A stand-in Mailpit: a script that ignores its options and waits.
    fn fake_mailpit(home: &Path) -> Release {
        let path = home.join("mailpit.tar.gz");
        fixtures::tar_gz(
            &path,
            &[
                (
                    "mailpit",
                    b"#!/bin/sh\necho mailpit up\nexec sleep 30\n",
                    0o755,
                ),
                ("mailpit.exe", b"MZ", 0o755),
                ("LICENSE", b"MIT", 0o644),
            ],
        );
        let checksum = sha256_file(&path).unwrap();
        let base = testserver::serve(std::fs::read(&path).unwrap());
        Release {
            version: "1.2.3".into(),
            url: format!("{base}/mailpit.tar.gz"),
            file_name: "mailpit-test.tar.gz".into(),
            checksum,
        }
    }

    #[test]
    fn installs_builds_a_spec_and_uninstalls() {
        let home = crate::testutil::tempdir();
        let manager = ServiceManager::new(&home);
        assert!(manager.get("mailpit").is_none());
        assert!(matches!(
            manager.spec("mailpit"),
            Err(Error::NotInstalled { .. })
        ));

        let installed = manager
            .install_release("mailpit", &fake_mailpit(&home), &mut |_| {})
            .unwrap();
        assert_eq!(installed.release, "1.2.3");
        assert_eq!(installed.path, home.join("services/mailpit/1"));

        let spec = manager.spec("mailpit").unwrap();
        assert_eq!(spec.ports, vec![1025, 8025]);
        assert!(spec.args.contains(&"127.0.0.1:8025".to_string()));
        assert!(home.join("data/mailpit").is_dir());
        assert!(spec.program.starts_with(&installed.path));

        // Data survives an uninstall.
        std::fs::write(home.join("data/mailpit/mailpit.db"), b"mail").unwrap();
        manager.uninstall("mailpit").unwrap();
        assert!(manager.get("mailpit").is_none());
        assert!(home.join("data/mailpit/mailpit.db").is_file());
    }

    #[test]
    fn rejects_unknown_services() {
        let manager = ServiceManager::new(&crate::testutil::tempdir());
        assert!(matches!(
            manager.resolve("nginx"),
            Err(Error::UnknownModule(_))
        ));
        assert!(matches!(manager.spec("../x"), Err(Error::UnknownModule(_))));
    }

    #[test]
    fn caddy_gets_its_own_config_and_storage() {
        let home = crate::testutil::tempdir();
        let def = catalog::service("caddy").unwrap();
        let install = home.join("services/caddy/2");
        let ctx = Context {
            def,
            os: Os::Linux,
            install: &install,
            data: home.join("data/caddy"),
            config: home.join("config/caddy"),
        };
        std::fs::create_dir_all(&ctx.config).unwrap();
        let spec = caddy::spec(&ctx).unwrap();
        let caddyfile = std::fs::read_to_string(ctx.config.join("Caddyfile")).unwrap();
        assert!(caddyfile.contains("http://127.0.0.1:8080"), "{caddyfile}");
        assert!(caddyfile.contains(&slash(&ctx.data)));
        assert_eq!(spec.args[0], "run");
        assert!(spec.env.iter().any(|(k, _)| k == "XDG_DATA_HOME"));
    }

    /// Runs the stand-in through the real supervisor.
    #[cfg(unix)]
    #[test]
    fn starts_under_the_supervisor() {
        let home = crate::testutil::tempdir();
        let manager = ServiceManager::new(&home);
        manager
            .install_release("mailpit", &fake_mailpit(&home), &mut |_| {})
            .unwrap();
        let mut spec = manager.spec("mailpit").unwrap();
        spec.ports.clear(); // the real ports may be busy on a dev machine
        let supervisor = crate::supervisor::Supervisor::new(home.join("logs"));
        supervisor.start(spec).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(300));
        assert!(matches!(
            supervisor.state("mailpit"),
            crate::supervisor::ProcessState::Running { .. }
        ));
        supervisor.stop("mailpit").unwrap();
        let log = supervisor.tail_log("mailpit", 10).unwrap();
        assert!(log.iter().any(|l| l.contains("mailpit up")), "{log:?}");
    }

    /// Installs the real service, starts it and waits for its first port;
    /// returns the supervisor and the folder so the caller can check more.
    /// Run with `cargo test -p stackyard-core -- --ignored live_`.
    fn live_start(id: &str) -> Option<(crate::supervisor::Supervisor, PathBuf)> {
        let def = catalog::service(id).unwrap();
        let module = modules::find(id).unwrap();
        if !module.available_here() {
            eprintln!("{id} is not offered on this system; skipping");
            return None;
        }
        let home = crate::testutil::tempdir();
        let manager = ServiceManager::new(&home);
        let installed = manager.install(id, &mut |_| {}).unwrap();
        eprintln!("{id} {} installed", installed.release);
        let spec = manager.spec(id).unwrap();
        let supervisor = crate::supervisor::Supervisor::new(home.join("logs"));
        supervisor.start(spec).unwrap();
        let open = crate::ports::wait_until_open(def.ports[0], std::time::Duration::from_secs(90));
        let log = supervisor.tail_log(id, 40).unwrap_or_default();
        assert!(
            open,
            "{id} did not open port {}:\n{}",
            def.ports[0],
            log.join("\n")
        );
        Some((supervisor, home))
    }

    fn live_stop(id: &str, supervisor: crate::supervisor::Supervisor, home: PathBuf) {
        supervisor.stop(id).unwrap();
        assert!(!matches!(
            supervisor.state(id),
            crate::supervisor::ProcessState::Running { .. }
        ));
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    #[ignore]
    fn live_runs_caddy() {
        let Some((supervisor, home)) = live_start("caddy") else {
            return;
        };
        let page = Http::new().get_text("http://127.0.0.1:8080/").unwrap();
        assert!(page.contains("Caddy is running"), "{page}");
        live_stop("caddy", supervisor, home);
    }

    #[test]
    #[ignore]
    fn live_runs_mailpit() {
        let Some((supervisor, home)) = live_start("mailpit") else {
            return;
        };
        assert!(crate::ports::wait_until_open(
            8025,
            std::time::Duration::from_secs(30)
        ));
        let info = Http::new()
            .get_text("http://127.0.0.1:8025/api/v1/info")
            .unwrap();
        assert!(info.contains("Version"), "{info}");
        live_stop("mailpit", supervisor, home);
    }

    #[test]
    #[ignore]
    fn live_runs_mariadb() {
        let Some((supervisor, home)) = live_start("mariadb") else {
            return;
        };
        assert!(home.join("data/mariadb/db/mysql").is_dir());
        live_stop("mariadb", supervisor, home);
    }
}
