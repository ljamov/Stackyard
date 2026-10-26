//! What runs while Stackyard runs services: the processes, the web server
//! with PHP for the sites, and the local DNS server. Exactly one process
//! holds an `Engine` at a time (see [`crate::control`]); the app, the CLI
//! and the tray all start and stop things through it.

use std::path::Path;
use std::sync::Mutex;

use crate::catalog::{self, SERVICES};
use crate::control::{Request, Response, ServiceStatus};
use crate::dns::{self, DnsServer};
use crate::platform::Platform;
use crate::scheduler::Scheduler;
use crate::services::ServiceManager;
use crate::settings::Settings;
use crate::sitedb;
use crate::sites::Site;
use crate::supervisor::{ProcessState, Supervisor};
use crate::system::Resolver;
use crate::{Error, Result};

pub struct Engine {
    supervisor: Supervisor,
    manager: ServiceManager,
    dns: Mutex<Option<DnsServer>>,
    /// Runs scheduled tasks; absent in tests that build an engine by hand.
    scheduler: Option<Scheduler>,
}

impl Engine {
    /// Starts the local DNS server where local names are routed to it,
    /// and the task scheduler; services start only when asked.
    pub fn new(home: &Path) -> Self {
        let mut engine = Self::without_dns(home);
        engine.scheduler = Some(Scheduler::start(home));
        *engine.dns.lock().unwrap_or_else(|e| e.into_inner()) = start_dns(&engine.manager);
        engine
    }

    fn without_dns(home: &Path) -> Self {
        Self {
            supervisor: Supervisor::new(home.join("logs")),
            manager: ServiceManager::new(home),
            dns: Mutex::new(None),
            scheduler: None,
        }
    }

    pub fn supervisor(&self) -> &Supervisor {
        &self.supervisor
    }

    pub fn manager(&self) -> &ServiceManager {
        &self.manager
    }

    pub fn dns_running(&self) -> bool {
        self.dns.lock().unwrap_or_else(|e| e.into_inner()).is_some()
    }

    pub fn state(&self, id: &str) -> ProcessState {
        self.supervisor.state(id)
    }

    fn is_running(&self, id: &str) -> bool {
        matches!(self.state(id), ProcessState::Running { .. })
    }

    /// Starts a service; `caddy` serves the sites, with PHP for each version they use.
    pub fn start(&self, id: &str) -> Result<()> {
        let def = catalog::service(id).ok_or_else(|| Error::UnknownProcess(id.into()))?;
        Settings::load_from(&self.manager.home().join("settings.toml"))?.require_module(def.id)?;
        if self.manager.get(def.id).is_none() {
            return Err(Error::NotInstalled {
                name: def.name.into(),
                version: def.version.into(),
            });
        }
        if def.id == "caddy" {
            self.manager.start_web(&self.supervisor)?;
            self.start_site_databases();
            return Ok(());
        }
        // Its own port would look taken and move it.
        if self.is_running(def.id) {
            return Ok(());
        }
        // MySQL and MariaDB are alternatives: one runs at a time.
        if let Some(other) = SERVICES.iter().find(|other| {
            other.id != def.id
                && other.id != "caddy"
                && other.ports.iter().any(|p| def.ports.contains(p))
                && self.is_running(other.id)
        }) {
            return Err(Error::Alternative {
                name: def.name.into(),
                other: other.name.into(),
            });
        }
        self.supervisor.start(self.manager.spec(def.id)?)?;
        if matches!(def.id, "mysql" | "mariadb") {
            let port = crate::services::port(self.manager.home(), def);
            crate::phpmyadmin::use_database(self.manager.home(), port);
        }
        Ok(())
    }

    /// Starts the server that holds a site's database, stopping the other
    /// of MySQL and MariaDB first when that one has the port.
    pub fn start_site_database(&self, site: &Site) -> Result<()> {
        let running = |id: &str| self.is_running(id);
        let status = sitedb::status(self.manager.home(), site, &running).ok_or_else(|| {
            Error::Database(format!(
                "{} names no database Stackyard can find",
                site.domain()
            ))
        })?;
        let service = status.service.ok_or_else(|| {
            Error::Database(format!(
                "no installed database server has the database {} yet; install MySQL or MariaDB on the Services screen",
                status.database
            ))
        })?;
        if let Some(other) = &status.blocked_by_id {
            self.stop(other)?;
        }
        self.start(&service)
    }

    /// After the web server starts: the database servers of the sites set
    /// to start theirs with it. One whose port another server has is left
    /// alone, so a running database is never stopped behind the user's back.
    fn start_site_databases(&self) {
        let Ok(settings) = Settings::load_from(&self.manager.home().join("settings.toml")) else {
            return;
        };
        if settings.start_databases.is_empty() {
            return;
        }
        let Ok(sites) = self.manager.user_sites() else {
            return;
        };
        for served in sites {
            if !settings.start_databases.contains(&served.domain()) {
                continue;
            }
            let running = |id: &str| self.is_running(id);
            let Some(status) = sitedb::status(self.manager.home(), &served.site, &running) else {
                continue;
            };
            match (&status.service, &status.blocked_by) {
                (Some(service), None) if !status.running && status.installed => {
                    if let Err(e) = self.start(service) {
                        eprintln!("{}: {e}", served.domain());
                    }
                }
                (Some(_), Some(other)) if !status.running => eprintln!(
                    "{}: its database server can't start while {other} runs",
                    served.domain()
                ),
                _ => {}
            }
        }
    }

    pub fn stop(&self, id: &str) -> Result<()> {
        if catalog::service(id).is_none() {
            return Err(Error::UnknownProcess(id.into()));
        }
        if id == "caddy" {
            return self.manager.stop_web(&self.supervisor);
        }
        if self.state(id) != ProcessState::Stopped {
            self.supervisor.stop(id)?;
        }
        Ok(())
    }

    /// Starts every installed service that is switched on, skipping one
    /// whose ports another of them already uses (MySQL and MariaDB).
    /// Returns what was started and what could not be, with why.
    pub fn start_all(&self) -> (Vec<String>, Vec<String>) {
        let settings =
            Settings::load_from(&self.manager.home().join("settings.toml")).unwrap_or_default();
        let mut taken: Vec<u16> = Vec::new();
        let (mut started, mut problems) = (Vec::new(), Vec::new());
        for def in SERVICES {
            if !settings.module_enabled(def.id) || self.manager.get(def.id).is_none() {
                continue;
            }
            if def.id != "caddy" && def.ports.iter().any(|p| taken.contains(p)) {
                continue;
            }
            match self.start(def.id) {
                Ok(()) => {
                    taken.extend(def.ports);
                    started.push(def.id.to_string());
                }
                Err(e) => problems.push(format!("{}: {e}", def.name)),
            }
        }
        (started, problems)
    }

    pub fn stop_all(&self) {
        self.supervisor.stop_all();
    }

    /// Before quitting: stops running tasks and every service.
    pub fn shutdown(&self) {
        if let Some(scheduler) = &self.scheduler {
            scheduler.shutdown();
        }
        self.stop_all();
    }

    /// The task scheduler, for "Run now" and "Stop".
    pub fn scheduler(&self) -> Result<&Scheduler> {
        self.scheduler
            .as_ref()
            .ok_or_else(|| Error::Task("the scheduler is not running".into()))
    }

    /// The services that are running (or restarting), by id.
    pub fn running(&self) -> Vec<String> {
        SERVICES
            .iter()
            .filter(|def| {
                matches!(
                    self.state(def.id),
                    ProcessState::Running { .. } | ProcessState::Restarting { .. }
                )
            })
            .map(|def| def.id.to_string())
            .collect()
    }

    pub fn status(&self) -> Vec<ServiceStatus> {
        SERVICES
            .iter()
            .map(|def| ServiceStatus {
                id: def.id.into(),
                name: def.name.into(),
                state: self.state(def.id),
            })
            .collect()
    }

    /// After sites or domain endings change: answer the new endings, and
    /// restart a running web server so it serves the new list.
    pub fn sites_changed(&self) -> Result<()> {
        // A new site's name goes into the hosts file (Windows) right away.
        if let Ok(wanted) = self.manager.system_wanted() {
            if let Err(e) = crate::system::sync_hosts(self.manager.home(), &wanted) {
                eprintln!("{e}");
            }
        }
        if let Some(server) = self.dns.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            server.set_endings(&self.manager.endings()?);
        }
        if self.is_running("caddy") {
            self.manager.start_web(&self.supervisor)?;
        }
        Ok(())
    }

    /// Watches the parked folders while the engine lives: a project folder
    /// added, renamed or removed there becomes (or stops being) a site
    /// within a few seconds, without a restart. Calls `changed` after the
    /// web server has the new list.
    pub fn watch_parked(
        engine: &std::sync::Arc<Self>,
        home: &Path,
        changed: impl Fn() + Send + 'static,
    ) {
        let weak = std::sync::Arc::downgrade(engine);
        let settings_file = home.join("settings.toml");
        let seen = move || -> Vec<(std::path::PathBuf, Vec<String>)> {
            let settings = Settings::load_from(&settings_file).unwrap_or_default();
            settings
                .parked
                .iter()
                .map(|folder| {
                    let names = crate::web::project_folders(folder)
                        .into_iter()
                        .map(|(name, _)| name)
                        .collect();
                    (folder.clone(), names)
                })
                .collect()
        };
        std::thread::spawn(move || {
            let mut last = seen();
            loop {
                std::thread::sleep(std::time::Duration::from_secs(3));
                let Some(engine) = weak.upgrade() else {
                    return;
                };
                let now = seen();
                // Parking or unparking already restarts the web server.
                let same_folders = now.iter().map(|(f, _)| f).eq(last.iter().map(|(f, _)| f));
                if now != last && same_folders {
                    if let Err(e) = engine.sites_changed() {
                        eprintln!("could not restart the web server: {e}");
                    }
                    changed();
                }
                last = now;
            }
        });
    }

    /// Restarts a running web server, e.g. after system setup freed ports 80 and 443.
    pub fn restart_web(&self) -> Result<()> {
        if self.is_running("caddy") {
            self.manager.start_web(&self.supervisor)?;
        }
        Ok(())
    }

    /// Answers a request from another process. `Show` and `HandOver` need
    /// the owner's own handling; here they only report.
    pub fn handle(&self, request: Request) -> Response {
        let result = match request {
            Request::Status | Request::Show => Ok(()),
            Request::Start { ids } => ids.iter().try_for_each(|id| self.start(id)),
            Request::StartAll => {
                let (_, problems) = self.start_all();
                match problems.is_empty() {
                    true => Ok(()),
                    false => Err(Error::Control(problems.join("; "))),
                }
            }
            Request::Stop { ids } => ids.iter().try_for_each(|id| self.stop(id)),
            Request::StopAll => {
                self.stop_all();
                Ok(())
            }
            Request::SitesChanged => self.sites_changed(),
            Request::HandOver => {
                let running = self.running();
                self.stop_all();
                return Response::HandedOver { running };
            }
        };
        match result {
            Ok(()) => Response::Services {
                services: self.status(),
            },
            Err(e) => Response::Failed {
                message: e.to_string(),
            },
        }
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.stop_all();
    }
}

fn start_dns(manager: &ServiceManager) -> Option<DnsServer> {
    if Resolver::for_this_system(Platform::current().os) == Resolver::HostsFile {
        return None;
    }
    let endings = manager.endings().ok()?;
    match DnsServer::start(dns::PORT, &endings) {
        Ok(server) => Some(server),
        Err(e) => {
            eprintln!("local domains are unavailable: {e}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explains_what_it_cannot_start() {
        let home = crate::testutil::tempdir();
        let engine = Engine::without_dns(&home);
        let err = engine.start("mailpit").unwrap_err();
        assert!(matches!(err, Error::ModuleOff { .. }), "{err}");
        let err = engine.start("caddy").unwrap_err();
        assert!(err.to_string().contains("not installed"), "{err}");
        assert!(engine.start("nope").is_err());
        assert!(engine.stop("nope").is_err());
        assert!(
            engine.stop("caddy").is_ok(),
            "stopping what is stopped is fine"
        );

        let (started, problems) = engine.start_all();
        assert!(started.is_empty() && problems.is_empty());
        assert!(engine.running().is_empty());
        assert!(matches!(
            engine.handle(Request::Start {
                ids: vec!["caddy".into()]
            }),
            Response::Failed { .. }
        ));
        assert_eq!(
            engine.handle(Request::HandOver),
            Response::HandedOver { running: vec![] }
        );
        match engine.handle(Request::Status) {
            Response::Services { services } => assert_eq!(services.len(), SERVICES.len()),
            other => panic!("{other:?}"),
        }
    }
}
