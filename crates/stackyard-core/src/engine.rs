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
use crate::services::ServiceManager;
use crate::settings::Settings;
use crate::supervisor::{ProcessState, Supervisor};
use crate::system::Resolver;
use crate::{Error, Result};

pub struct Engine {
    supervisor: Supervisor,
    manager: ServiceManager,
    dns: Mutex<Option<DnsServer>>,
}

impl Engine {
    /// Starts the local DNS server where local names are routed to it;
    /// services start only when asked.
    pub fn new(home: &Path) -> Self {
        let engine = Self::without_dns(home);
        *engine.dns.lock().unwrap_or_else(|e| e.into_inner()) = start_dns(&engine.manager);
        engine
    }

    fn without_dns(home: &Path) -> Self {
        Self {
            supervisor: Supervisor::new(home.join("logs")),
            manager: ServiceManager::new(home),
            dns: Mutex::new(None),
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
            self.manager.start_web(&self.supervisor)
        } else {
            self.supervisor
                .start(self.manager.spec(def.id)?)
                .map(|_| ())
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
        if let Some(server) = self.dns.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            server.set_endings(&self.manager.endings()?);
        }
        if self.is_running("caddy") {
            self.manager.start_web(&self.supervisor)?;
        }
        Ok(())
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
