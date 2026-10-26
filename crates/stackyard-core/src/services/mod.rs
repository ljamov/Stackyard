//! Installs services (web server, databases, mail catcher) and turns them
//! into processes the supervisor can run.
//!
//! Layout inside Stackyard's data folder:
//! - `services/<id>/<line>/`: the program, replaced on update
//! - `data/<id>/`: databases and state, kept across updates and uninstalls
//! - `config/<id>/`: config files Stackyard writes before each start

mod caddy;
pub(crate) mod github;
mod mailpit;
mod mariadb;
mod meilisearch;
mod mongodb;
mod mysql;
mod nats;
pub mod postgres;
mod redis;
pub mod seaweedfs;

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::catalog::{self, ServiceDef};
use crate::download::Http;
use crate::install::{self, Release, Step};
use crate::modules;
use crate::platform::{Os, Platform};
use crate::runtimes::RuntimeManager;
use crate::settings::Settings;
use crate::supervisor::{ProcessSpec, ProcessState, Supervisor};
use crate::system::Wanted;
use crate::web::{self, Ports, ServedSite};
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
    /// The ports to listen on, in the order of `def.ports`.
    pub ports: Vec<u16>,
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
            ports: self.ports.clone(),
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
            "meilisearch" => meilisearch::resolve(&self.http, p),
            "postgres" => postgres::resolve(&self.http, def, p),
            "redis" => redis::resolve(&self.http, def, p),
            "mongodb" => mongodb::resolve(&self.http, def, p),
            "seaweedfs" => seaweedfs::resolve(&self.http, p),
            "nats" => nats::resolve(&self.http, p),
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
        // Before ports are checked: a leftover copy would hold them.
        crate::platform::stop_strays(&installed.path.join(program(def.id, self.platform.os)));
        let ctx = Context {
            def,
            os: self.platform.os,
            install: &installed.path,
            data: self.data_dir(def.id),
            config: self.home.join("config").join(def.id),
            ports: if def.id == "caddy" {
                Vec::new()
            } else {
                choose_ports(&self.home, def)?
            },
        };
        for dir in [&ctx.data, &ctx.config] {
            std::fs::create_dir_all(dir).map_err(|source| Error::Io {
                path: dir.clone(),
                source,
            })?;
        }
        match def.id {
            "caddy" => {
                let sites = self.sites()?;
                let ports = Ports::choose();
                ports.save(&self.home)?;
                caddy::spec(&ctx, ports, &web::caddy_sites(&sites))
            }
            "mailpit" => mailpit::spec(&ctx),
            "mysql" => mysql::spec(&ctx),
            "mariadb" => mariadb::spec(&ctx),
            "meilisearch" => meilisearch::spec(&ctx),
            "postgres" => postgres::spec(&ctx),
            "redis" => redis::spec(&ctx),
            "mongodb" => mongodb::spec(&ctx),
            "seaweedfs" => seaweedfs::spec(&ctx),
            "nats" => nats::spec(&ctx),
            _ => Err(Error::UnknownModule(id.to_string())),
        }
    }
}

/// The ports a service listens on: its usual ones, or the ones it moved to
/// because another program had those (saved in `config/<id>/ports`).
pub fn ports(home: &Path, def: &ServiceDef) -> Vec<u16> {
    if def.id == "caddy" {
        let web = Ports::current(home);
        return vec![web.http, web.https];
    }
    std::fs::read_to_string(ports_file(home, def))
        .ok()
        .and_then(|text| {
            text.split(',')
                .map(|p| p.trim().parse().ok())
                .collect::<Option<Vec<u16>>>()
        })
        .filter(|saved| saved.len() == def.ports.len())
        .unwrap_or_else(|| def.ports.to_vec())
}

/// The page a service shows in the browser, on the ports it really uses.
/// The web server's opens at `127.0.0.1`: browsers try IPv6 first for
/// `localhost`, where another local server may answer.
pub fn web_page(home: &Path, def: &ServiceDef) -> Option<String> {
    if def.id == "caddy" {
        return Some(Ports::current(home).url("127.0.0.1", false));
    }
    Some(follow_ports(home, def, def.web?))
}

/// How apps connect to a service, on the ports it really uses.
pub fn connect(home: &Path, def: &ServiceDef) -> Option<String> {
    Some(follow_ports(home, def, def.connect?))
}

fn follow_ports(home: &Path, def: &ServiceDef, text: &str) -> String {
    let mut text = text.to_string();
    for (usual, actual) in def.ports.iter().zip(ports(home, def)) {
        text = text.replace(&format!(":{usual}"), &format!(":{actual}"));
    }
    text
}

/// Drops endings that another one covers: a staging's `.shop.stack` is
/// already answered for by `.stack`, so it needs no system setup of its own.
fn covering(endings: Vec<String>) -> Vec<String> {
    endings
        .iter()
        .filter(|e| {
            !endings
                .iter()
                .any(|other| other != *e && e.ends_with(other.as_str()))
        })
        .cloned()
        .collect()
}

/// The first port, the one programs connect to (3306 for MySQL).
pub fn port(home: &Path, def: &ServiceDef) -> u16 {
    ports(home, def)[0]
}

fn ports_file(home: &Path, def: &ServiceDef) -> PathBuf {
    home.join("config").join(def.id).join("ports")
}

/// Before a start: the ports it used last time while they are free, else
/// the usual ones, else the next free ones after those (MySQL moves to 3307
/// when another MySQL has 3306). The choice is kept so apps' settings stay
/// right.
fn choose_ports(home: &Path, def: &ServiceDef) -> Result<Vec<u16>> {
    let last = ports(home, def);
    let mut chosen: Vec<u16> = Vec::new();
    for (&usual, &before) in def.ports.iter().zip(&last) {
        let free = [before, usual]
            .into_iter()
            .chain((1..=100).filter_map(|n| usual.checked_add(n)))
            .find(|p| !chosen.contains(p) && crate::ports::is_free(*p));
        // Nothing free nearby: keep the usual port and let the start explain.
        chosen.push(free.unwrap_or(before));
    }
    let file = ports_file(home, def);
    let io = |source| Error::Io {
        path: file.clone(),
        source,
    };
    if chosen == def.ports {
        if file.exists() {
            std::fs::remove_file(&file).map_err(io)?;
        }
    } else {
        let text: Vec<String> = chosen.iter().map(u16::to_string).collect();
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir).map_err(io)?;
        }
        std::fs::write(&file, text.join(",")).map_err(io)?;
    }
    Ok(chosen)
}

impl ServiceManager {
    /// Every site the web server serves.
    pub fn sites(&self) -> Result<Vec<ServedSite>> {
        let settings = Settings::load_from(&self.home.join("settings.toml"))?;
        web::sites(&self.home, &settings, &RuntimeManager::new(&self.home))
    }

    /// The sites the user made or parked, without the ones Stackyard adds
    /// itself (phpMyAdmin), for lists: in the order the user put them in.
    pub fn user_sites(&self) -> Result<Vec<ServedSite>> {
        let builtin = crate::phpmyadmin::dir(&self.home);
        let mut sites = self.sites()?;
        sites.retain(|s| s.site.path != builtin);
        // A damaged order file shouldn't hide the sites.
        if let Ok(order) = crate::siteorder::SiteOrder::load(&self.home) {
            order.sort(&mut sites, |s| s.site.domain());
        }
        Ok(sites)
    }

    /// The web server and the PHP processes its sites need, in start order
    /// (PHP first, so the first page view already works).
    pub fn web_specs(&self) -> Result<Vec<ProcessSpec>> {
        let sites = self.sites()?;
        let mut specs = web::php_processes(&self.home, &sites, &RuntimeManager::new(&self.home))?;
        specs.push(self.spec("caddy")?);
        specs.extend(web::dev_servers(&self.home, &sites));
        Ok(specs)
    }

    /// Starts the PHP processes the sites need and (re)starts the web server
    /// so it picks up site changes. PHP versions no site uses any more stop.
    pub fn start_web(&self, supervisor: &Supervisor) -> Result<()> {
        if supervisor.state("caddy") != ProcessState::Stopped {
            supervisor.stop("caddy")?;
        }
        let specs = self.web_specs()?;
        for (id, state) in supervisor.states() {
            let unused = !specs.iter().any(|s| s.id == id);
            if is_web_helper(&id) && unused && state != ProcessState::Stopped {
                supervisor.stop(&id)?;
            }
        }
        for spec in specs {
            // A dev server that can't start (its port taken by a copy the
            // user runs) shows in its log; the other sites still work.
            if spec.id.starts_with("dev-") {
                if let Err(e) = supervisor.start(spec) {
                    eprintln!("{e}");
                }
            } else {
                supervisor.start(spec)?;
            }
        }
        Ok(())
    }

    /// Stops the web server, every PHP process and every dev server.
    pub fn stop_web(&self, supervisor: &Supervisor) -> Result<()> {
        for (id, state) in supervisor.states() {
            if (id == "caddy" || is_web_helper(&id)) && state != ProcessState::Stopped {
                supervisor.stop(&id)?;
            }
        }
        Ok(())
    }

    /// Domain endings in use: the one for new sites, plus any older ones.
    pub fn endings(&self) -> Result<Vec<String>> {
        let settings = Settings::load_from(&self.home.join("settings.toml"))?;
        let mut endings = vec![settings.tld];
        for site in self.sites()? {
            if !endings.contains(&site.site.tld) {
                endings.push(site.site.tld);
            }
        }
        Ok(covering(endings))
    }

    /// What system setup should cover right now.
    pub fn system_wanted(&self) -> Result<Wanted> {
        let domains: Vec<String> = self.sites()?.iter().map(ServedSite::domain).collect();
        Ok(Wanted::new(
            self.platform.os,
            &self.endings()?,
            &domains,
            &self.root_certificate(),
        ))
    }

    /// Where Caddy keeps its local certificate authority's root.
    pub fn root_certificate(&self) -> PathBuf {
        caddy::root_certificate(&self.data_dir("caddy"))
    }

    pub fn home(&self) -> &Path {
        &self.home
    }
}

/// The processes that run only for the web server's sites.
fn is_web_helper(id: &str) -> bool {
    id.starts_with("php-") || id.starts_with("dev-")
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
        ("meilisearch", false) => "meilisearch",
        ("meilisearch", true) => "meilisearch.exe",
        ("postgres", false) => "bin/postgres",
        ("postgres", true) => "bin/postgres.exe",
        ("redis", _) => "bin/redis-server",
        ("mongodb", false) => "bin/mongod",
        ("mongodb", true) => "bin/mongod.exe",
        ("seaweedfs", false) => "weed",
        ("seaweedfs", true) => "weed.exe",
        ("nats", false) => "nats-server",
        ("nats", true) => "nats-server.exe",
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

    #[test]
    fn endings_under_another_ending_are_covered() {
        let all = covering(vec![".stack".into(), ".shop.stack".into(), ".test".into()]);
        assert_eq!(all, vec![".stack".to_string(), ".test".to_string()]);
    }
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

    /// Meilisearch is published as the bare program, not an archive.
    #[test]
    fn installs_a_bare_program() {
        let home = crate::testutil::tempdir();
        let manager = ServiceManager::new(&home);
        let body = b"#!/bin/sh\necho meilisearch up\nexec sleep 30\n".to_vec();
        let file = home.join("meili");
        std::fs::write(&file, &body).unwrap();
        let release = Release {
            version: "1.25.0".into(),
            url: format!("{}/meilisearch-linux-amd64", testserver::serve(body)),
            file_name: "meilisearch-linux-amd64".into(),
            checksum: sha256_file(&file).unwrap(),
        };
        let installed = manager
            .install_release("meilisearch", &release, &mut |_| {})
            .unwrap();
        assert_eq!(installed.release, "1.25.0");
        let program = installed
            .path
            .join(program("meilisearch", Platform::current().os));
        assert!(program.is_file());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&program).unwrap().permissions().mode();
            assert_eq!(mode & 0o111, 0o111);
        }

        let spec = manager.spec("meilisearch").unwrap();
        // 7700, or the next free port when something on this computer holds it.
        let [port] = spec.ports[..] else {
            panic!("one port expected: {:?}", spec.ports)
        };
        assert!(port >= 7700);
        assert!(spec.args.contains(&format!("127.0.0.1:{port}")));
        assert!(spec
            .args
            .contains(&slash(&home.join("data/meilisearch/data.ms"))));
        assert!(spec
            .env
            .iter()
            .any(|(k, v)| k == "MEILI_EXPERIMENTAL_DUMPLESS_UPGRADE" && v == "true"));
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

    /// Another MySQL (or Local by Flywheel's) on 3306 used to stop Stackyard's
    /// from starting; now it moves to the next free port and stays there.
    #[test]
    fn a_taken_port_moves_to_the_next_free_one() {
        let home = crate::testutil::tempdir();
        // A made-up service on a port this test holds.
        let held = std::net::TcpListener::bind(("0.0.0.0", 0)).unwrap();
        let usual = held.local_addr().unwrap().port();
        let def: &'static ServiceDef = Box::leak(Box::new(ServiceDef {
            id: "testdb",
            ports: Box::leak(vec![usual].into_boxed_slice()),
            web: Some("http://127.0.0.1:PORT"),
            ..*catalog::service("mysql").unwrap()
        }));
        let chosen = choose_ports(&home, def).unwrap();
        assert_ne!(chosen, vec![usual]);
        assert!(chosen[0] > usual && chosen[0] <= usual.saturating_add(100));
        assert_eq!(ports(&home, def), chosen, "kept for next time");
        // While it is free, the moved port is kept so apps stay pointed at it.
        drop(held);
        assert_eq!(choose_ports(&home, def).unwrap(), chosen);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn pages_follow_moved_ports() {
        let home = crate::testutil::tempdir();
        let mailpit = catalog::service("mailpit").unwrap();
        assert_eq!(
            web_page(&home, mailpit).as_deref(),
            Some("http://127.0.0.1:8025")
        );
        std::fs::create_dir_all(home.join("config/mailpit")).unwrap();
        std::fs::write(home.join("config/mailpit/ports"), "1026,8026").unwrap();
        assert_eq!(ports(&home, mailpit), vec![1026, 8026]);
        assert_eq!(
            web_page(&home, mailpit).as_deref(),
            Some("http://127.0.0.1:8026")
        );
        let caddy = catalog::service("caddy").unwrap();
        assert_eq!(web_page(&home, caddy).as_deref(), Some("http://127.0.0.1"));
        let _ = std::fs::remove_dir_all(&home);
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
            ports: Vec::new(),
        };
        std::fs::create_dir_all(&ctx.config).unwrap();
        let spec = caddy::spec(&ctx, Ports::FALLBACK, "\nsite.stack {\n}\n").unwrap();
        let caddyfile = std::fs::read_to_string(ctx.config.join("Caddyfile")).unwrap();
        assert!(caddyfile.contains("http_port 8080"), "{caddyfile}");
        assert!(caddyfile.contains("skip_install_trust"), "{caddyfile}");
        assert!(caddyfile.ends_with("site.stack {\n}\n"), "{caddyfile}");
        assert_eq!(spec.ports, vec![8080, 8443]);
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
        let port = spec.ports[0];
        let supervisor = crate::supervisor::Supervisor::new(home.join("logs"));
        supervisor.start(spec).unwrap();
        let open = crate::ports::wait_until_open(port, std::time::Duration::from_secs(90));
        let log = supervisor.tail_log(id, 40).unwrap_or_default();
        assert!(open, "{id} did not open port {}:\n{}", port, log.join("\n"));
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
        let port = Ports::current(&home).http;
        let page = Http::new()
            .get_text(&format!("http://127.0.0.1:{port}/"))
            .unwrap();
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
    fn live_runs_meilisearch() {
        let Some((supervisor, home)) = live_start("meilisearch") else {
            return;
        };
        let health = Http::new()
            .get_text("http://127.0.0.1:7700/health")
            .unwrap();
        assert!(health.contains("available"), "{health}");
        assert!(home.join("data/meilisearch/data.ms").is_dir());
        live_stop("meilisearch", supervisor, home);
    }

    #[test]
    #[ignore]
    fn live_runs_postgres() {
        // PostgreSQL refuses to run elevated, as Windows CI runners do; an
        // everyday (not "Run as administrator") account is fine.
        #[cfg(windows)]
        {
            let groups = std::process::Command::new("whoami")
                .arg("/groups")
                .output()
                .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
                .unwrap_or_default();
            // S-1-16-12288: the high (elevated) integrity level.
            if groups.contains("S-1-16-12288") {
                eprintln!("running elevated, which PostgreSQL refuses; skipping");
                return;
            }
        }
        let Some((supervisor, home)) = live_start("postgres") else {
            return;
        };
        // The Databases screen against it: queries, backup and restore.
        let mut settings = Settings::default();
        settings.set_module("postgres", true).unwrap();
        settings.save_to(&home.join("settings.toml")).unwrap();
        // Accepting connections a moment after the port opens.
        std::thread::sleep(std::time::Duration::from_secs(2));
        crate::databases::tests::exercise_postgres(&home);
        live_stop("postgres", supervisor, home);
    }

    #[test]
    #[ignore]
    fn live_runs_nats() {
        let Some((supervisor, home)) = live_start("nats") else {
            return;
        };
        assert!(crate::ports::wait_until_open(
            8222,
            std::time::Duration::from_secs(30)
        ));
        let info = Http::new().get_text("http://127.0.0.1:8222/jsz").unwrap();
        assert!(info.contains("\"config\""), "JetStream is on: {info}");
        live_stop("nats", supervisor, home);
    }

    #[test]
    #[ignore]
    fn live_runs_seaweedfs() {
        let Some((supervisor, home)) = live_start("seaweedfs") else {
            return;
        };
        // Unsigned requests are refused once the key exists.
        let mut answer = String::new();
        for _ in 0..60 {
            match Http::new().get_text("http://127.0.0.1:8333/") {
                Ok(text) => answer = text,
                Err(e) => answer = e.to_string(),
            }
            if answer.contains("AccessDenied") || answer.contains("403") {
                break;
            }
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
        let log = supervisor.tail_log("seaweedfs", 40).unwrap_or_default();
        assert!(
            answer.contains("AccessDenied") || answer.contains("403"),
            "{answer}\n{}",
            log.join("\n")
        );
        live_stop("seaweedfs", supervisor, home);
    }

    #[test]
    #[ignore]
    fn live_runs_mongodb() {
        let Some((supervisor, home)) = live_start("mongodb") else {
            return;
        };
        // Listening, and its storage engine made its files.
        let engine = home.join("data/mongodb/db/WiredTiger");
        for _ in 0..60 {
            if engine.is_file() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
        let log = supervisor.tail_log("mongodb", 40).unwrap_or_default();
        assert!(engine.is_file(), "{}", log.join("\n"));
        assert!(matches!(
            supervisor.state("mongodb"),
            crate::supervisor::ProcessState::Running { .. }
        ));
        live_stop("mongodb", supervisor, home);
    }

    #[test]
    #[ignore]
    fn live_runs_mariadb() {
        let Some((supervisor, home)) = live_start("mariadb") else {
            return;
        };
        assert!(home.join("data/mariadb/db/mysql").is_dir());
        // The Databases screen against it: queries, backup and restore.
        let mut settings = Settings::default();
        settings.set_module("mariadb", true).unwrap();
        settings.save_to(&home.join("settings.toml")).unwrap();
        crate::databases::tests::exercise(&home);
        live_stop("mariadb", supervisor, home);
    }
}
