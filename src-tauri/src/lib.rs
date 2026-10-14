//! Tauri commands the React interface calls. They are thin wrappers over
//! `stackyard-core`; errors come back to the interface as plain strings.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use serde::Serialize;
use stackyard_core::catalog::RUNTIMES;
use stackyard_core::catalog::{self, RuntimeKind, SERVICES};
use stackyard_core::control::{self, ControlServer, Owner, Request, Response};
use stackyard_core::detect::{self, Needs};
use stackyard_core::dns;
use stackyard_core::engine::Engine;
use stackyard_core::logs;
use stackyard_core::mcp;
use stackyard_core::modules::{ModuleGroup, MODULES};
use stackyard_core::runtimes::{RuntimeManager, Step};
use stackyard_core::scheduler::{self, TaskInput, TaskView};
use stackyard_core::services::ServiceManager;
use stackyard_core::settings::Settings;
use stackyard_core::shell::{path as user_path, shims};
use stackyard_core::sites::{Site, SiteRuntime, SiteStore};
use stackyard_core::supervisor::{ProcessState, Supervisor};
use stackyard_core::system::{self, Elevate, Resolver};
use stackyard_core::web::{self, Backend, Ports, ServedSite};
use stackyard_core::{domain, paths};
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
    engine: State<'_, Arc<Engine>>,
    installs: State<'_, Installs>,
) -> Vec<ServiceView> {
    service_views(engine.supervisor(), &installs)
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
        app.state::<Arc<Engine>>().supervisor(),
        &app.state::<Installs>(),
    ))
}

#[tauri::command]
fn uninstall_service(
    id: String,
    engine: State<'_, Arc<Engine>>,
    installs: State<'_, Installs>,
) -> CmdResult<Vec<ServiceView>> {
    engine.stop(&id).map_err(err)?;
    engine.manager().uninstall(&id).map_err(err)?;
    Ok(service_views(engine.supervisor(), &installs))
}

/// Preparing a database the first time takes a few seconds, so this runs
/// on a worker thread.
#[tauri::command]
async fn start_service(app: AppHandle, id: String) -> CmdResult<Vec<ServiceView>> {
    let engine = app.state::<Arc<Engine>>().inner().clone();
    tauri::async_runtime::spawn_blocking(move || engine.start(&id))
        .await
        .map_err(|e| e.to_string())?
        .map_err(err)?;
    Ok(service_views(
        app.state::<Arc<Engine>>().supervisor(),
        &app.state::<Installs>(),
    ))
}

#[tauri::command]
fn stop_service(
    id: String,
    engine: State<'_, Arc<Engine>>,
    installs: State<'_, Installs>,
) -> CmdResult<Vec<ServiceView>> {
    engine.stop(&id).map_err(err)?;
    Ok(service_views(engine.supervisor(), &installs))
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
fn service_log(id: String, engine: State<'_, Arc<Engine>>) -> CmdResult<Vec<String>> {
    if catalog::service(&id).is_none() && !id.starts_with("php-") {
        return Err(format!("unknown service {id:?}"));
    }
    engine.supervisor().tail_log(&id, 200).map_err(err)
}

#[tauri::command]
fn list_logs() -> Vec<logs::LogFile> {
    logs::list(&paths::home())
}

/// The end of a log (`from` left out), or what was written since `from`.
#[tauri::command]
fn read_log(id: String, from: Option<u64>, lines: usize) -> CmdResult<logs::Chunk> {
    Settings::load()
        .map_err(err)?
        .require_module("logs")
        .map_err(err)?;
    logs::read(&paths::home(), &id, from, lines.min(5000)).map_err(err)
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
    engine: State<'_, Arc<Engine>>,
) -> CmdResult<Vec<ModuleView>> {
    let mut settings = Settings::load().map_err(err)?;
    settings.set_module(&id, enabled).map_err(err)?;
    settings.save().map_err(err)?;
    if !enabled && catalog::service(&id).is_some() {
        engine.stop(&id).map_err(err)?;
    }
    if id == "shell" && !enabled {
        terminal_off().map_err(err)?;
    }
    Ok(module_views(&settings))
}

#[derive(Serialize)]
struct TerminalView {
    /// The `stackyard` command-line program was found next to the app.
    available: bool,
    /// The commands exist and new terminals will find them.
    on: bool,
    folder: String,
    /// Start-up files or the Windows Path that carry the setting.
    places: Vec<String>,
}

/// The `stackyard` program, which every terminal command leads back to.
/// It ships next to the app.
fn cli_program() -> Option<std::path::PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let cli = exe.with_file_name(format!("stackyard{}", std::env::consts::EXE_SUFFIX));
    cli.is_file().then_some(cli)
}

fn terminal_view() -> TerminalView {
    let home = paths::home();
    let bin = shims::bin_dir(&home);
    let status = user_path::status(&bin);
    TerminalView {
        available: cli_program().is_some(),
        on: shims::installed(&home) && status.on_path,
        folder: bin.display().to_string(),
        places: status.places,
    }
}

fn terminal_off() -> stackyard_core::Result<()> {
    let home = paths::home();
    user_path::remove(&shims::bin_dir(&home))?;
    shims::remove(&home)
}

/// After an update, points the terminal commands at the new `stackyard`.
fn refresh_terminal_commands() {
    std::thread::spawn(|| {
        let home = paths::home();
        if let Some(cli) = cli_program().filter(|cli| shims::outdated(&home, cli)) {
            if let Err(e) = shims::install(&home, &cli) {
                eprintln!("could not update the terminal commands: {e}");
            }
        }
    });
}

#[tauri::command]
async fn terminal_status() -> TerminalView {
    // Reading the Windows Path starts PowerShell; keep it off the UI thread.
    tauri::async_runtime::spawn_blocking(terminal_view)
        .await
        .unwrap_or_else(|_| terminal_view())
}

#[tauri::command]
async fn set_terminal(on: bool) -> CmdResult<TerminalView> {
    tauri::async_runtime::spawn_blocking(move || {
        if on {
            Settings::load()?.require_module("shell")?;
            let cli = cli_program().ok_or_else(|| {
                stackyard_core::Error::Shell(
                    "the stackyard command-line program is not next to the app".into(),
                )
            })?;
            let home = paths::home();
            shims::install(&home, &cli)?;
            user_path::add(&shims::bin_dir(&home))?;
        } else {
            terminal_off()?;
        }
        Ok(terminal_view())
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(err)
}

#[derive(Serialize)]
struct McpToolView {
    name: &'static str,
    title: &'static str,
    description: &'static str,
    /// Changes something, so the user decides whether AI tools may use it.
    action: bool,
    allowed: bool,
}

#[derive(Serialize)]
struct McpView {
    /// How to add Stackyard to AI tools; `None` when the `stackyard`
    /// program is not next to the app.
    setup: Option<mcp::ClientSetup>,
    tools: Vec<McpToolView>,
    /// What AI tools did, newest first.
    activity: Vec<mcp::activity::Entry>,
}

fn mcp_view() -> CmdResult<McpView> {
    let home = paths::home();
    let settings = Settings::load().map_err(err)?;
    let tools = mcp::TOOLS
        .iter()
        .map(|t| McpToolView {
            name: t.name,
            title: t.title,
            description: t.description,
            action: t.action,
            allowed: !t.action || settings.mcp.allow.contains(t.name),
        })
        .collect();
    Ok(McpView {
        setup: cli_program().map(|cli| mcp::client_setup(&home, &cli)),
        tools,
        activity: mcp::activity::recent(&home, 100),
    })
}

#[tauri::command]
fn mcp_status() -> CmdResult<McpView> {
    mcp_view()
}

#[tauri::command]
fn set_mcp_tool(name: String, allowed: bool) -> CmdResult<McpView> {
    let mut settings = Settings::load().map_err(err)?;
    mcp::set_allowed(&mut settings, &name, allowed).map_err(err)?;
    settings.save().map_err(err)?;
    mcp_view()
}

fn task_views() -> CmdResult<Vec<TaskView>> {
    Settings::load()
        .map_err(err)?
        .require_module("scheduler")
        .map_err(err)?;
    scheduler::views(&paths::home()).map_err(err)
}

#[tauri::command]
fn list_tasks() -> CmdResult<Vec<TaskView>> {
    task_views()
}

#[tauri::command]
fn add_task(task: TaskInput) -> CmdResult<Vec<TaskView>> {
    task_views()?;
    scheduler::add(&paths::home(), task).map_err(err)?;
    task_views()
}

#[tauri::command]
fn update_task(id: String, task: TaskInput) -> CmdResult<Vec<TaskView>> {
    task_views()?;
    scheduler::update(&paths::home(), &id, task).map_err(err)?;
    task_views()
}

#[tauri::command]
fn set_task_enabled(id: String, enabled: bool) -> CmdResult<Vec<TaskView>> {
    task_views()?;
    scheduler::set_enabled(&paths::home(), &id, enabled).map_err(err)?;
    task_views()
}

/// Stops the task first if it is running.
#[tauri::command]
fn remove_task(id: String, engine: State<'_, Arc<Engine>>) -> CmdResult<Vec<TaskView>> {
    task_views()?;
    if let Ok(scheduler) = engine.scheduler() {
        if scheduler.running().contains(&id) {
            let _ = scheduler.stop(&id);
            for _ in 0..80 {
                if !scheduler.running().contains(&id) {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        }
    }
    scheduler::remove(&paths::home(), &id).map_err(err)?;
    task_views()
}

#[tauri::command]
fn run_task(id: String, engine: State<'_, Arc<Engine>>) -> CmdResult<Vec<TaskView>> {
    task_views()?;
    engine.scheduler().map_err(err)?.run_now(&id).map_err(err)?;
    task_views()
}

#[tauri::command]
fn stop_task(id: String, engine: State<'_, Arc<Engine>>) -> CmdResult<Vec<TaskView>> {
    engine.scheduler().map_err(err)?.stop(&id).map_err(err)?;
    task_views()
}

#[derive(Serialize)]
struct SchedulePreview {
    description: Option<String>,
    /// The next few runs, in milliseconds since 1970.
    runs: Vec<u64>,
}

#[tauri::command]
fn preview_schedule(schedule: String) -> CmdResult<SchedulePreview> {
    Ok(SchedulePreview {
        runs: scheduler::preview(&schedule, 3).map_err(err)?,
        description: scheduler::cron::describe(&schedule),
    })
}

/// The end of a task's output.
#[tauri::command]
fn task_log(id: String) -> CmdResult<Vec<String>> {
    if !scheduler::is_id(&id) {
        return Err("there is no such task".into());
    }
    stackyard_core::supervisor::tail_file(&scheduler::log_path(&paths::home(), &id), 200)
        .map_err(err)
}

#[tauri::command]
fn service_states(engine: State<'_, Arc<Engine>>) -> Vec<(String, ProcessState)> {
    engine.supervisor().states()
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
    let engine = app.state::<Arc<Engine>>().inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        if let Err(e) = engine.sites_changed() {
            eprintln!("could not restart the web server: {e}");
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
        dns_running: app.state::<Arc<Engine>>().dns_running(),
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
        worker.state::<Arc<Engine>>().restart_web()
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(err)?;
    system_view(&app)
}

/// The connection that lets the CLI reach this app's services.
struct Control(Mutex<Option<ControlServer>>);

/// Whether the tray icon is up; without one, closing the window quits.
struct Tray(bool);

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let home = paths::home();
    // One process runs services at a time.
    let mut resume = Vec::new();
    if let Some(client) = control::find(&home) {
        match client.owner {
            // Already open: bring that window forward instead.
            Owner::App => {
                let _ = client.send(Request::Show);
                return;
            }
            // `stackyard start` or `stackyard up` runs them: take over.
            Owner::Daemon | Owner::Up => resume = take_over(&home, &client),
        }
    }
    refresh_terminal_commands();
    let engine = Arc::new(Engine::new(&home));
    if !resume.is_empty() {
        let engine = engine.clone();
        std::thread::spawn(move || {
            for id in resume {
                if let Err(e) = engine.start(&id) {
                    eprintln!("could not start {id} again: {e}");
                }
            }
        });
    }
    let app = tauri::Builder::default()
        .manage(engine.clone())
        .manage(Installs::default())
        .manage(Control(Mutex::new(None)))
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
            run_system_setup,
            terminal_status,
            set_terminal,
            mcp_status,
            set_mcp_tool,
            list_logs,
            read_log,
            list_tasks,
            add_task,
            update_task,
            set_task_enabled,
            remove_task,
            run_task,
            stop_task,
            preview_schedule,
            task_log
        ])
        .setup(move |app| {
            let tray = match build_tray(app.handle()) {
                Ok(()) => true,
                Err(e) => {
                    eprintln!("no tray icon: {e}");
                    false
                }
            };
            app.manage(Tray(tray));
            // Quit cleanly, stopping services, when the system asks (logout, `kill`).
            let handle = app.handle().clone();
            let _ = ctrlc::set_handler(move || handle.exit(0));
            let handle = app.handle().clone();
            let server = ControlServer::start(&paths::home(), Owner::App, move |request| {
                answer_cli(&handle, request)
            });
            match server {
                Ok(server) => *app.state::<Control>().0.lock().unwrap() = Some(server),
                Err(e) => eprintln!("the stackyard command cannot reach this app: {e}"),
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building Stackyard");
    app.run(|handle, event| match event {
        // Closing the window drops it (and its web view) to save memory;
        // Stackyard keeps running in the tray until Quit.
        RunEvent::ExitRequested {
            api, code: None, ..
        } if handle.state::<Tray>().0 => api.prevent_exit(),
        // macOS: clicking the Dock icon with no window open.
        #[cfg(target_os = "macos")]
        RunEvent::Reopen { .. } => show_window(handle),
        // Never leave services running after Stackyard quits.
        RunEvent::Exit => {
            handle.state::<Control>().0.lock().unwrap().take();
            handle.state::<Arc<Engine>>().shutdown();
        }
        _ => {}
    });
}

/// Asks `stackyard daemon` or `stackyard up` to stop and returns what it
/// was running, so the app can start the same services itself.
fn take_over(home: &std::path::Path, client: &control::Client) -> Vec<String> {
    let running = match client.send(Request::HandOver) {
        Ok(Response::HandedOver { running }) => running,
        _ => return Vec::new(),
    };
    // Wait until it has quit and let go of the local DNS port.
    let dns_free = || std::net::UdpSocket::bind(("127.0.0.1", dns::PORT)).is_ok();
    for _ in 0..50 {
        if control::find(home).is_none() && dns_free() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    running
}

/// Requests from the `stackyard` command while the app runs services.
fn answer_cli(app: &AppHandle, request: Request) -> Response {
    match request {
        Request::Show => {
            let handle = app.clone();
            let _ = app.run_on_main_thread(move || show_window(&handle));
            Response::Done
        }
        Request::HandOver => Response::Failed {
            message: "the Stackyard app is running services; quit it first".into(),
        },
        Request::SitesChanged => {
            let response = app.state::<Arc<Engine>>().handle(Request::SitesChanged);
            // A terminal or an AI tool changed the sites: show them.
            let _ = app.emit("sites-changed", ());
            response
        }
        other => app.state::<Arc<Engine>>().handle(other),
    }
}

/// Shows the window, creating it again if it was closed.
fn show_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        return;
    }
    let Some(config) = app.config().app.windows.first().cloned() else {
        return;
    };
    match tauri::WebviewWindowBuilder::from_config(app, &config).and_then(|b| b.build()) {
        Ok(window) => {
            let _ = window.set_focus();
        }
        Err(e) => eprintln!("could not open the window: {e}"),
    }
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
    use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

    let item = |id: &str, text: &str| MenuItem::with_id(app, id, text, true, None::<&str>);
    let menu = Menu::with_items(
        app,
        &[
            &item("open", "Open Stackyard")?,
            &PredefinedMenuItem::separator(app)?,
            &item("start-all", "Start all services")?,
            &item("stop-all", "Stop all services")?,
            &PredefinedMenuItem::separator(app)?,
            &item("quit", "Quit Stackyard")?,
        ],
    )?;
    let mut tray = TrayIconBuilder::with_id("main")
        .tooltip("Stackyard")
        .menu(&menu)
        // macOS shows the menu on a click; elsewhere a click opens the window.
        .show_menu_on_left_click(cfg!(target_os = "macos"))
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => show_window(app),
            "start-all" => {
                let engine = app.state::<Arc<Engine>>().inner().clone();
                std::thread::spawn(move || {
                    let (_, problems) = engine.start_all();
                    for problem in problems {
                        eprintln!("{problem}");
                    }
                });
            }
            "stop-all" => app.state::<Arc<Engine>>().stop_all(),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                if !cfg!(target_os = "macos") {
                    show_window(tray.app_handle());
                }
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
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
