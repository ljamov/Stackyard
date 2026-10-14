//! Tauri commands the React interface calls. They are thin wrappers over
//! `stackyard-core`; errors come back to the interface as plain strings.

use std::collections::HashSet;
use std::sync::Mutex;

use serde::Serialize;
use stackyard_core::catalog::RUNTIMES;
use stackyard_core::catalog::{self, RuntimeKind, SERVICES};
use stackyard_core::detect::{self, Needs};
use stackyard_core::dns::{self, DnsServer};
use stackyard_core::modules::{ModuleGroup, MODULES};
use stackyard_core::runtimes::{RuntimeManager, Step};
use stackyard_core::services::ServiceManager;
use stackyard_core::settings::Settings;
use stackyard_core::sites::{Site, SiteRuntime, SiteStore};
use stackyard_core::supervisor::{ProcessState, Supervisor};
use stackyard_core::system::{self, Elevate, Resolver};
use stackyard_core::web::{self, Backend, Ports, ServedSite};
use stackyard_core::{domain, paths, platform::Platform};
use tauri::{AppHandle, Emitter, Manager, RunEvent, State};

type CmdResult<T> = Result<T, String>;

#[derive(Serialize)]
struct SiteView {
    name: String,
    domain: String,
    url: String,
    /// The `.localhost` address, which works before system setup.
    local_url: String,
    path: String,
    root: String,
    runtime: String,
    framework: Option<&'static str>,
    https: bool,
    /// Found in a parked folder (remove it by unparking the folder).
    parked: bool,
    /// Why the site cannot work yet, if it cannot.
    problem: Option<String>,
}

impl SiteView {
    fn new(served: &ServedSite, ports: Ports) -> Self {
        let site = &served.site;
        let (runtime, problem) = match &served.backend {
            Backend::Php {
                version, installed, ..
            } => (
                format!("PHP {version}"),
                (!installed).then(|| format!("PHP {version} is not installed")),
            ),
            Backend::Node { version, port } => {
                (format!("Node {version} · dev server on port {port}"), None)
            }
            Backend::Static => ("Static".to_string(), None),
        };
        let problem = problem.or_else(|| {
            (!served.root.is_dir()).then(|| format!("{} does not exist", served.root.display()))
        });
        Self {
            name: site.name.clone(),
            domain: site.domain(),
            url: ports.url(&site.domain(), site.https),
            local_url: ports.url(&served.localhost(), site.https),
            path: site.path.display().to_string(),
            root: served.root.display().to_string(),
            runtime,
            framework: served.framework,
            https: site.https,
            parked: served.parked,
            problem,
        }
    }
}

fn site_views() -> CmdResult<Vec<SiteView>> {
    let manager = ServiceManager::open();
    let ports = Ports::current(manager.home());
    let sites = manager.sites().map_err(err)?;
    Ok(sites.iter().map(|s| SiteView::new(s, ports)).collect())
}

#[derive(Serialize)]
struct TldView {
    tld: String,
    warning: Option<&'static str>,
}

fn err(e: stackyard_core::Error) -> String {
    e.to_string()
}

#[derive(Serialize)]
struct RuntimeView {
    kind: RuntimeKind,
    name: &'static str,
    available: bool,
    versions: Vec<VersionView>,
}

#[derive(Serialize)]
struct VersionView {
    version: &'static str,
    /// Exact installed release, e.g. `8.4.26`, or null when not installed.
    installed: Option<String>,
    default: bool,
    installing: bool,
}

/// Installs in progress, as `kind/version`, so a double click cannot start two.
#[derive(Default)]
struct Installs(Mutex<HashSet<String>>);

#[derive(Clone, Serialize)]
struct InstallProgress {
    kind: RuntimeKind,
    version: String,
    #[serde(flatten)]
    step: Step,
}

fn runtime_views(installs: &Installs) -> CmdResult<Vec<RuntimeView>> {
    let settings = Settings::load().map_err(err)?;
    let manager = RuntimeManager::open();
    let busy = installs.0.lock().unwrap().clone();
    Ok(RUNTIMES
        .iter()
        .map(|family| RuntimeView {
            kind: family.kind,
            name: family.name,
            available: family.available,
            versions: family
                .versions
                .iter()
                .map(|&version| VersionView {
                    version,
                    installed: manager.get(family.kind, version).map(|i| i.release),
                    default: settings.default_runtime(family.kind) == Some(version),
                    installing: busy.contains(&format!("{}/{version}", family.kind.id())),
                })
                .collect(),
        })
        .collect())
}

fn parse_kind(kind: &str) -> CmdResult<RuntimeKind> {
    RuntimeKind::from_id(kind).ok_or_else(|| format!("unknown runtime {kind:?}"))
}

#[tauri::command]
fn list_runtimes(installs: State<'_, Installs>) -> CmdResult<Vec<RuntimeView>> {
    runtime_views(&installs)
}

/// Runs on a worker thread; progress arrives as `runtime-progress` events.
#[tauri::command]
async fn install_runtime(
    app: AppHandle,
    kind: String,
    version: String,
) -> CmdResult<Vec<RuntimeView>> {
    let kind = parse_kind(&kind)?;
    Settings::load()
        .map_err(err)?
        .require_module(kind.id())
        .map_err(err)?;
    let key = format!("{}/{version}", kind.id());
    if !app
        .state::<Installs>()
        .0
        .lock()
        .unwrap()
        .insert(key.clone())
    {
        return Err(format!(
            "{} {version} is already being installed",
            catalog::runtime(kind).name
        ));
    }
    let worker = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let manager = RuntimeManager::open();
        manager.install(kind, &version, &mut |step| {
            let _ = worker.emit(
                "runtime-progress",
                InstallProgress {
                    kind,
                    version: version.clone(),
                    step,
                },
            );
        })?;
        let mut settings = Settings::load()?;
        if manager.fix_default(&mut settings, kind) {
            settings.save()?;
        }
        Ok::<_, stackyard_core::Error>(())
    })
    .await;
    app.state::<Installs>().0.lock().unwrap().remove(&key);
    result.map_err(|e| e.to_string())?.map_err(err)?;
    runtime_views(&app.state::<Installs>())
}

#[tauri::command]
fn uninstall_runtime(
    kind: String,
    version: String,
    installs: State<'_, Installs>,
) -> CmdResult<Vec<RuntimeView>> {
    let kind = parse_kind(&kind)?;
    let manager = RuntimeManager::open();
    manager.uninstall(kind, &version).map_err(err)?;
    let mut settings = Settings::load().map_err(err)?;
    if manager.fix_default(&mut settings, kind) {
        settings.save().map_err(err)?;
    }
    runtime_views(&installs)
}

#[tauri::command]
fn set_default_runtime(
    kind: String,
    version: String,
    installs: State<'_, Installs>,
) -> CmdResult<Vec<RuntimeView>> {
    let kind = parse_kind(&kind)?;
    let mut settings = Settings::load().map_err(err)?;
    RuntimeManager::open()
        .set_default(&mut settings, kind, &version)
        .map_err(err)?;
    settings.save().map_err(err)?;
    runtime_views(&installs)
}

#[derive(Serialize)]
struct ServiceView {
    id: &'static str,
    name: &'static str,
    version: &'static str,
    ports: Vec<u16>,
    role: &'static str,
    web: Option<String>,
    /// Exact installed release, or null when not installed.
    installed: Option<String>,
    installing: bool,
    #[serde(flatten)]
    state: ProcessState,
}

#[derive(Clone, Serialize)]
struct ServiceProgress {
    id: String,
    #[serde(flatten)]
    step: Step,
}

fn service_views(supervisor: &Supervisor, installs: &Installs) -> Vec<ServiceView> {
    let manager = ServiceManager::open();
    let busy = installs.0.lock().unwrap().clone();
    let web_ports = Ports::current(manager.home());
    SERVICES
        .iter()
        .map(|def| ServiceView {
            id: def.id,
            name: def.name,
            version: def.version,
            // The web server's ports depend on what this computer allows.
            ports: if def.id == "caddy" {
                vec![web_ports.http, web_ports.https]
            } else {
                def.ports.to_vec()
            },
            role: def.role,
            web: if def.id == "caddy" {
                Some(web_ports.url("localhost", false))
            } else {
                def.web.map(str::to_string)
            },
            installed: manager.get(def.id).map(|i| i.release),
            installing: busy.contains(&format!("service/{}", def.id)),
            state: supervisor.state(def.id),
        })
        .collect()
}

#[tauri::command]
fn list_services(
    supervisor: State<'_, Supervisor>,
    installs: State<'_, Installs>,
) -> Vec<ServiceView> {
    service_views(&supervisor, &installs)
}

/// Runs on a worker thread; progress arrives as `service-progress` events.
#[tauri::command]
async fn install_service(app: AppHandle, id: String) -> CmdResult<Vec<ServiceView>> {
    Settings::load()
        .map_err(err)?
        .require_module(&id)
        .map_err(err)?;
    let key = format!("service/{id}");
    if !app
        .state::<Installs>()
        .0
        .lock()
        .unwrap()
        .insert(key.clone())
    {
        return Err(format!("{id} is already being installed"));
    }
    let worker = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        ServiceManager::open().install(&id, &mut |step| {
            let _ = worker.emit(
                "service-progress",
                ServiceProgress {
                    id: id.clone(),
                    step,
                },
            );
        })
    })
    .await;
    app.state::<Installs>().0.lock().unwrap().remove(&key);
    result.map_err(|e| e.to_string())?.map_err(err)?;
    Ok(service_views(
        &app.state::<Supervisor>(),
        &app.state::<Installs>(),
    ))
}

#[tauri::command]
fn uninstall_service(
    id: String,
    supervisor: State<'_, Supervisor>,
    installs: State<'_, Installs>,
) -> CmdResult<Vec<ServiceView>> {
    let manager = ServiceManager::open();
    if id == "caddy" {
        manager.stop_web(&supervisor).map_err(err)?;
    } else if supervisor.state(&id) != ProcessState::Stopped {
        supervisor.stop(&id).map_err(err)?;
    }
    manager.uninstall(&id).map_err(err)?;
    Ok(service_views(&supervisor, &installs))
}

/// Preparing a database the first time takes a few seconds, so this runs
/// on a worker thread.
#[tauri::command]
async fn start_service(app: AppHandle, id: String) -> CmdResult<Vec<ServiceView>> {
    Settings::load()
        .map_err(err)?
        .require_module(&id)
        .map_err(err)?;
    let worker = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let manager = ServiceManager::open();
        let supervisor = worker.state::<Supervisor>();
        if id == "caddy" {
            manager.start_web(&supervisor)
        } else {
            supervisor.start(manager.spec(&id)?).map(|_| ())
        }
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(err)?;
    Ok(service_views(
        &app.state::<Supervisor>(),
        &app.state::<Installs>(),
    ))
}

#[tauri::command]
fn stop_service(
    id: String,
    supervisor: State<'_, Supervisor>,
    installs: State<'_, Installs>,
) -> CmdResult<Vec<ServiceView>> {
    if id == "caddy" {
        ServiceManager::open().stop_web(&supervisor).map_err(err)?;
    } else {
        supervisor.stop(&id).map_err(err)?;
    }
    Ok(service_views(&supervisor, &installs))
}

/// Opens a local page (a site or a service's web interface) in the default
/// browser. Only addresses on this computer are allowed, so the interface
/// cannot be used to launch arbitrary programs or URLs.
#[tauri::command]
fn open_local_page(url: String) -> CmdResult<()> {
    let endings = ServiceManager::open().endings().map_err(err)?;
    if !web::is_local_url(&url, &endings) {
        return Err(format!("not a local page: {url}"));
    }
    #[cfg(target_os = "macos")]
    let mut cmd = std::process::Command::new("open");
    #[cfg(target_os = "windows")]
    let mut cmd = {
        let mut c = std::process::Command::new("rundll32");
        c.arg("url.dll,FileProtocolHandler");
        c
    };
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut cmd = std::process::Command::new("xdg-open");
    cmd.arg(&url)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("could not open {url}: {e}"))
}

#[tauri::command]
fn service_log(id: String, supervisor: State<'_, Supervisor>) -> CmdResult<Vec<String>> {
    if catalog::service(&id).is_none() && !id.starts_with("php-") {
        return Err(format!("unknown service {id:?}"));
    }
    supervisor.tail_log(&id, 200).map_err(err)
}

#[tauri::command]
fn get_tld() -> CmdResult<TldView> {
    let tld = Settings::load().map_err(err)?.tld;
    Ok(TldView {
        warning: domain::tld_warning(&tld),
        tld,
    })
}

#[tauri::command]
fn set_tld(value: String, app: AppHandle) -> CmdResult<TldView> {
    let mut settings = Settings::load().map_err(err)?;
    settings.set_tld(&value).map_err(err)?;
    settings.save().map_err(err)?;
    sites_changed(&app);
    Ok(TldView {
        warning: domain::tld_warning(&settings.tld),
        tld: settings.tld,
    })
}

#[derive(Serialize)]
struct ModuleView {
    id: &'static str,
    name: &'static str,
    group: ModuleGroup,
    description: &'static str,
    on_by_default: bool,
    /// Can be switched on here: built, and offered for this system.
    available: bool,
    enabled: bool,
}

fn module_views(settings: &Settings) -> Vec<ModuleView> {
    MODULES
        .iter()
        .map(|def| ModuleView {
            id: def.id,
            name: def.name,
            group: def.group,
            description: def.description,
            on_by_default: def.on_by_default,
            available: def.available_here(),
            enabled: settings.module_enabled(def.id),
        })
        .collect()
}

#[tauri::command]
fn list_modules() -> CmdResult<Vec<ModuleView>> {
    Ok(module_views(&Settings::load().map_err(err)?))
}

#[tauri::command]
fn set_module(
    id: String,
    enabled: bool,
    supervisor: State<'_, Supervisor>,
) -> CmdResult<Vec<ModuleView>> {
    let mut settings = Settings::load().map_err(err)?;
    settings.set_module(&id, enabled).map_err(err)?;
    settings.save().map_err(err)?;
    if !enabled && supervisor.state(&id) != ProcessState::Stopped {
        supervisor.stop(&id).map_err(err)?;
    }
    Ok(module_views(&settings))
}

#[tauri::command]
fn service_states(supervisor: State<'_, Supervisor>) -> Vec<(String, ProcessState)> {
    supervisor.states()
}

#[tauri::command]
fn list_sites() -> CmdResult<Vec<SiteView>> {
    site_views()
}

#[tauri::command]
fn add_site(
    app: AppHandle,
    name: String,
    path: String,
    runtime: String,
    https: bool,
) -> CmdResult<Vec<SiteView>> {
    let settings = Settings::load().map_err(err)?;
    let path = paths::expand_home(std::path::Path::new(&path));
    let runtime = parse_runtime(&runtime, &path, &settings)?;
    let mut store = SiteStore::load().map_err(err)?;
    store
        .add(Site {
            name,
            tld: settings.tld,
            path,
            runtime,
            https,
        })
        .map_err(err)?;
    store.save().map_err(err)?;
    sites_changed(&app);
    site_views()
}

#[tauri::command]
fn remove_site(app: AppHandle, name: String) -> CmdResult<Vec<SiteView>> {
    let mut store = SiteStore::load().map_err(err)?;
    store.remove(&name).map_err(err)?;
    store.save().map_err(err)?;
    sites_changed(&app);
    site_views()
}

#[tauri::command]
fn list_parked() -> CmdResult<Vec<String>> {
    let settings = Settings::load().map_err(err)?;
    Ok(settings
        .parked
        .iter()
        .map(|p| p.display().to_string())
        .collect())
}

#[tauri::command]
fn park_folder(app: AppHandle, path: String) -> CmdResult<Vec<String>> {
    let mut settings = Settings::load().map_err(err)?;
    settings.park(std::path::Path::new(&path)).map_err(err)?;
    settings.save().map_err(err)?;
    sites_changed(&app);
    list_parked()
}

#[tauri::command]
fn unpark_folder(app: AppHandle, path: String) -> CmdResult<Vec<String>> {
    let mut settings = Settings::load().map_err(err)?;
    settings.unpark(std::path::Path::new(&path)).map_err(err)?;
    settings.save().map_err(err)?;
    sites_changed(&app);
    list_parked()
}

/// After sites or domain endings change: answer the new endings, and
/// restart a running web server so it serves the new list. Runs in the
/// background so the interface stays responsive.
fn sites_changed(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let manager = ServiceManager::open();
        if let (Some(server), Ok(endings)) = (
            app.state::<Dns>().0.lock().unwrap().as_ref(),
            manager.endings(),
        ) {
            server.set_endings(&endings);
        }
        let supervisor = app.state::<Supervisor>();
        if matches!(supervisor.state("caddy"), ProcessState::Running { .. }) {
            if let Err(e) = manager.start_web(&supervisor) {
                eprintln!("could not restart the web server: {e}");
            }
        }
    });
}

/// Accepts the labels the Sites screen offers: `Auto`, `PHP 8.3`,
/// `Node 22`, `Static`. `Auto` looks at the project folder.
fn parse_runtime(
    label: &str,
    path: &std::path::Path,
    settings: &Settings,
) -> CmdResult<SiteRuntime> {
    let node_port = || match detect::detect(path).needs {
        Needs::Node { port } => port,
        _ => 3000,
    };
    match label.split_once(' ') {
        Some(("PHP", version)) => Ok(SiteRuntime::Php {
            version: version.to_string(),
        }),
        Some(("Node", version)) => Ok(SiteRuntime::Node {
            version: version.to_string(),
            port: node_port(),
        }),
        None if label == "Static" => Ok(SiteRuntime::Static),
        None if label == "Auto" => Ok(web::runtime_for(path, settings, &RuntimeManager::open())),
        _ => Err(format!("unknown runtime {label:?}")),
    }
}

/// The local DNS server, on macOS and Linux once it can bind its port.
struct Dns(Mutex<Option<DnsServer>>);

#[derive(Serialize)]
struct SystemView {
    /// What setup would change; empty when everything is set up.
    needed: Vec<String>,
    waiting_for_certificate: bool,
    /// How local names are found: resolver files, systemd-resolved or the hosts file.
    resolver: Resolver,
    /// Whether the local DNS server is answering.
    dns_running: bool,
}

fn system_view(app: &AppHandle) -> CmdResult<SystemView> {
    let manager = ServiceManager::open();
    let wanted = manager.system_wanted().map_err(err)?;
    let status = wanted.status(&system::applied(manager.home()));
    Ok(SystemView {
        needed: status.needed,
        waiting_for_certificate: status.waiting_for_certificate,
        resolver: wanted.resolver,
        dns_running: app.state::<Dns>().0.lock().unwrap().is_some(),
    })
}

#[tauri::command]
fn system_status(app: AppHandle) -> CmdResult<SystemView> {
    system_view(&app)
}

/// Shows the system's password prompt, then applies setup. A running web
/// server restarts afterwards, since it may now use the standard ports.
#[tauri::command]
async fn run_system_setup(app: AppHandle) -> CmdResult<SystemView> {
    let worker = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let manager = ServiceManager::open();
        let wanted = manager.system_wanted()?;
        system::apply(manager.home(), &wanted, Elevate::Dialog)?;
        let supervisor = worker.state::<Supervisor>();
        if matches!(supervisor.state("caddy"), ProcessState::Running { .. }) {
            manager.start_web(&supervisor)?;
        }
        Ok::<_, stackyard_core::Error>(())
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(err)?;
    system_view(&app)
}

/// Starts the DNS server where local names are routed to it.
fn start_dns() -> Option<DnsServer> {
    let resolver = Resolver::for_this_system(Platform::current().os);
    if resolver == Resolver::HostsFile {
        return None;
    }
    let endings = ServiceManager::open().endings().ok()?;
    match DnsServer::start(dns::PORT, &endings) {
        Ok(server) => Some(server),
        Err(e) => {
            eprintln!("local domains are unavailable: {e}");
            None
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .manage(Supervisor::new(paths::logs_dir()))
        .manage(Installs::default())
        .manage(Dns(Mutex::new(start_dns())))
        .invoke_handler(tauri::generate_handler![
            list_runtimes,
            install_runtime,
            uninstall_runtime,
            set_default_runtime,
            list_services,
            install_service,
            uninstall_service,
            start_service,
            stop_service,
            service_log,
            open_local_page,
            list_modules,
            set_module,
            service_states,
            get_tld,
            set_tld,
            list_sites,
            add_site,
            remove_site,
            list_parked,
            park_folder,
            unpark_folder,
            system_status,
            run_system_setup
        ])
        .build(tauri::generate_context!())
        .expect("error while building Stackyard");
    app.run(|handle, event| {
        // Never leave services running after Stackyard quits.
        if let RunEvent::Exit = event {
            handle.state::<Supervisor>().stop_all();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_runtime_labels() {
        let settings = Settings::default();
        let folder = std::env::temp_dir().join("stackyard-no-such-project");
        let parse = |label| parse_runtime(label, &folder, &settings);
        assert_eq!(
            parse("PHP 8.3").unwrap(),
            SiteRuntime::Php {
                version: "8.3".into()
            }
        );
        assert_eq!(
            parse("Node 22").unwrap(),
            SiteRuntime::Node {
                version: "22".into(),
                port: 3000
            }
        );
        assert_eq!(parse("Static").unwrap(), SiteRuntime::Static);
        // An empty folder holds plain files.
        assert_eq!(parse("Auto").unwrap(), SiteRuntime::Static);
        assert!(parse("Ruby 3").is_err());
    }
}
