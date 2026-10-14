//! Tauri commands the React interface calls. They are thin wrappers over
//! `stackyard-core`; errors come back to the interface as plain strings.

use std::collections::HashSet;
use std::sync::Mutex;

use serde::Serialize;
use stackyard_core::catalog::RUNTIMES;
use stackyard_core::catalog::{self, RuntimeKind, SERVICES};
use stackyard_core::modules::{ModuleGroup, MODULES};
use stackyard_core::runtimes::{RuntimeManager, Step};
use stackyard_core::services::ServiceManager;
use stackyard_core::settings::Settings;
use stackyard_core::sites::{Site, SiteRuntime, SiteStore};
use stackyard_core::supervisor::{ProcessState, Supervisor};
use stackyard_core::{domain, paths};
use tauri::{AppHandle, Emitter, Manager, RunEvent, State};

type CmdResult<T> = Result<T, String>;

#[derive(Serialize)]
struct SiteView {
    name: String,
    domain: String,
    url: String,
    path: String,
    runtime: String,
    https: bool,
}

impl From<&Site> for SiteView {
    fn from(site: &Site) -> Self {
        let runtime = match &site.runtime {
            SiteRuntime::Php { version } => format!("PHP {version}"),
            SiteRuntime::Node { version, .. } => format!("Node {version}"),
            SiteRuntime::Static => "Static".to_string(),
        };
        Self {
            name: site.name.clone(),
            domain: site.domain(),
            url: site.url(),
            path: site.path.display().to_string(),
            runtime,
            https: site.https,
        }
    }
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
    ports: &'static [u16],
    role: &'static str,
    web: Option<&'static str>,
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
    SERVICES
        .iter()
        .map(|def| ServiceView {
            id: def.id,
            name: def.name,
            version: def.version,
            ports: def.ports,
            role: def.role,
            web: def.web,
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
    if supervisor.state(&id) != ProcessState::Stopped {
        supervisor.stop(&id).map_err(err)?;
    }
    ServiceManager::open().uninstall(&id).map_err(err)?;
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
    let spec = tauri::async_runtime::spawn_blocking(move || ServiceManager::open().spec(&id))
        .await
        .map_err(|e| e.to_string())?
        .map_err(err)?;
    app.state::<Supervisor>().start(spec).map_err(err)?;
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
    supervisor.stop(&id).map_err(err)?;
    Ok(service_views(&supervisor, &installs))
}

/// Opens a local page (a service's web interface) in the default browser.
/// Only loopback addresses are allowed, so the interface cannot be used to
/// launch arbitrary programs or URLs.
#[tauri::command]
fn open_local_page(url: String) -> CmdResult<()> {
    let local = [
        "http://127.0.0.1:",
        "http://localhost:",
        "https://127.0.0.1:",
        "https://localhost:",
    ];
    let safe = url
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b":/.-_?=&%".contains(&b));
    if !safe || !local.iter().any(|p| url.starts_with(p)) {
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
    if catalog::service(&id).is_none() {
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
fn set_tld(value: String) -> CmdResult<TldView> {
    let mut settings = Settings::load().map_err(err)?;
    settings.set_tld(&value).map_err(err)?;
    settings.save().map_err(err)?;
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
    let store = SiteStore::load().map_err(err)?;
    Ok(store.list().iter().map(SiteView::from).collect())
}

#[tauri::command]
fn add_site(name: String, path: String, runtime: String, https: bool) -> CmdResult<SiteView> {
    let settings = Settings::load().map_err(err)?;
    let runtime = parse_runtime(&runtime)?;
    let mut store = SiteStore::load().map_err(err)?;
    let view = SiteView::from(
        store
            .add(Site {
                name,
                tld: settings.tld,
                path: path.into(),
                runtime,
                https,
            })
            .map_err(err)?,
    );
    store.save().map_err(err)?;
    Ok(view)
}

#[tauri::command]
fn remove_site(name: String) -> CmdResult<()> {
    let mut store = SiteStore::load().map_err(err)?;
    store.remove(&name).map_err(err)?;
    store.save().map_err(err)
}

/// Accepts the labels the Sites screen offers: `PHP 8.3`, `Node 22`, `Static`.
fn parse_runtime(label: &str) -> CmdResult<SiteRuntime> {
    match label.split_once(' ') {
        Some(("PHP", version)) => Ok(SiteRuntime::Php {
            version: version.to_string(),
        }),
        Some(("Node", version)) => Ok(SiteRuntime::Node {
            version: version.to_string(),
            port: 3000,
        }),
        None if label == "Static" => Ok(SiteRuntime::Static),
        _ => Err(format!("unknown runtime {label:?}")),
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .manage(Supervisor::new(paths::logs_dir()))
        .manage(Installs::default())
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
            remove_site
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
        assert_eq!(
            parse_runtime("PHP 8.3").unwrap(),
            SiteRuntime::Php {
                version: "8.3".into()
            }
        );
        assert!(matches!(
            parse_runtime("Node 22").unwrap(),
            SiteRuntime::Node { .. }
        ));
        assert_eq!(parse_runtime("Static").unwrap(), SiteRuntime::Static);
        assert!(parse_runtime("Ruby 3").is_err());
    }
}
