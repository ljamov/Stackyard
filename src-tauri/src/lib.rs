//! Tauri commands the React interface calls. They are thin wrappers over
//! `stackyard-core`; errors come back to the interface as plain strings.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use serde::Serialize;
use stackyard_core::catalog::RUNTIMES;
use stackyard_core::catalog::{self, RuntimeKind, SERVICES};
use stackyard_core::cleanup;
use stackyard_core::control::{self, ControlServer, Owner, Request, Response};
use stackyard_core::databases::{
    self, Backup, Client as DbClient, Database, ResultSet, Server, Table,
};
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
use stackyard_core::setup;
use stackyard_core::shell::{path as user_path, shims};
use stackyard_core::sites::{Site, SiteRuntime, SiteStore};
use stackyard_core::supervisor::{ProcessState, Supervisor};
use stackyard_core::system::{self, Elevate, Resolver};
use stackyard_core::web::{self, Backend, Ports, ServedSite};
use stackyard_core::{domain, images, paths, phpmyadmin, starters};
use tauri::{AppHandle, Emitter, Manager, RunEvent, Runtime, State};

mod updates;

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
    let sites = manager.user_sites().map_err(err)?;
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
                .versions_on(manager.platform().os)
                .map(|version| VersionView {
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
    SERVICES
        .iter()
        .map(|def| ServiceView {
            id: def.id,
            name: def.name,
            version: def.version,
            // Ports move when another program has the usual ones.
            ports: stackyard_core::services::ports(manager.home(), def),
            role: def.role,
            web: stackyard_core::services::web_page(manager.home(), def),
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

/// Runs database work off the interface thread: queries can take a while.
async fn with_db<T: Send + 'static>(
    work: impl FnOnce(&DbClient) -> stackyard_core::Result<T> + Send + 'static,
) -> CmdResult<T> {
    tauri::async_runtime::spawn_blocking(move || {
        Settings::load()?.require_module("databases")?;
        work(&DbClient::open(&paths::home())?)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(err)
}

#[derive(Serialize)]
struct DbServerView {
    server: Option<Server>,
    /// Why there is no server to use, e.g. none is installed.
    problem: Option<String>,
}

#[tauri::command]
async fn db_server() -> CmdResult<DbServerView> {
    tauri::async_runtime::spawn_blocking(|| {
        let opened = Settings::load()
            .and_then(|s| s.require_module("databases"))
            .and_then(|_| DbClient::open(&paths::home()));
        match opened {
            Ok(client) => DbServerView {
                server: Some(client.server()),
                problem: None,
            },
            Err(e) => DbServerView {
                server: None,
                problem: Some(e.to_string()),
            },
        }
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
async fn db_list() -> CmdResult<Vec<Database>> {
    with_db(|c| c.databases()).await
}

#[tauri::command]
async fn db_create(name: String) -> CmdResult<Vec<Database>> {
    with_db(move |c| {
        c.create(&name)?;
        c.databases()
    })
    .await
}

#[tauri::command]
async fn db_drop(name: String) -> CmdResult<Vec<Database>> {
    with_db(move |c| {
        c.drop(&name)?;
        c.databases()
    })
    .await
}

#[tauri::command]
async fn db_tables(database: String) -> CmdResult<Vec<Table>> {
    with_db(move |c| c.tables(&database)).await
}

#[tauri::command]
async fn db_browse(
    database: String,
    table: String,
    offset: u64,
    limit: usize,
) -> CmdResult<ResultSet> {
    with_db(move |c| c.browse(&database, &table, offset, limit)).await
}

#[tauri::command]
async fn db_query(database: Option<String>, sql: String) -> CmdResult<Vec<ResultSet>> {
    with_db(move |c| c.query(database.as_deref(), &sql)).await
}

#[tauri::command]
async fn db_backup(database: String) -> CmdResult<Vec<Backup>> {
    with_db(move |c| {
        c.backup(&database)?;
        Ok(databases::backups(&paths::home()))
    })
    .await
}

#[tauri::command]
fn db_backups() -> Vec<Backup> {
    databases::backups(&paths::home())
}

#[tauri::command]
async fn db_restore(
    database: String,
    file: String,
    into: Option<String>,
) -> CmdResult<Vec<Database>> {
    with_db(move |c| {
        c.restore(&database, &file, into.as_deref())?;
        c.databases()
    })
    .await
}

#[tauri::command]
fn db_remove_backup(database: String, file: String) -> CmdResult<Vec<Backup>> {
    databases::remove_backup(&paths::home(), &database, &file).map_err(err)?;
    Ok(databases::backups(&paths::home()))
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

#[derive(Serialize)]
struct PhpMyAdminView {
    /// The installed release, e.g. `5.2.2`.
    installed: Option<String>,
    enabled: bool,
    installing: bool,
    /// `https://phpmyadmin.stack`, and the `.localhost` address that works
    /// before local domains are set up.
    url: Option<String>,
    local_url: Option<String>,
    problem: Option<String>,
}

fn phpmyadmin_view(installs: &Installs) -> PhpMyAdminView {
    let home = paths::home();
    let enabled = Settings::load().is_ok_and(|s| s.module_enabled(phpmyadmin::MODULE));
    let dir = phpmyadmin::dir(&home).display().to_string();
    let manager = ServiceManager::new(&home);
    let ports = Ports::current(&home);
    let site = manager
        .sites()
        .unwrap_or_default()
        .iter()
        .map(|s| SiteView::new(s, ports))
        .find(|s| s.name == phpmyadmin::SITE && s.path == dir);
    PhpMyAdminView {
        installed: phpmyadmin::installed(&home),
        enabled,
        installing: installs.0.lock().unwrap().contains(phpmyadmin::MODULE),
        url: site.as_ref().map(|s| s.url.clone()),
        local_url: site.as_ref().map(|s| s.local_url.clone()),
        problem: site.and_then(|s| s.problem),
    }
}

#[tauri::command]
fn phpmyadmin_status(installs: State<'_, Installs>) -> PhpMyAdminView {
    phpmyadmin_view(&installs)
}

/// Downloads phpMyAdmin, switches its module on and adds its site.
#[tauri::command]
async fn phpmyadmin_install(app: AppHandle) -> CmdResult<PhpMyAdminView> {
    if !app
        .state::<Installs>()
        .0
        .lock()
        .unwrap()
        .insert(phpmyadmin::MODULE.into())
    {
        return Err("phpMyAdmin is already being installed".into());
    }
    let worker = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        phpmyadmin::install(&paths::home(), &mut |step| {
            let _ = worker.emit(
                "service-progress",
                ServiceProgress {
                    id: phpmyadmin::MODULE.into(),
                    step,
                },
            );
        })?;
        let mut settings = Settings::load()?;
        settings.set_module(phpmyadmin::MODULE, true)?;
        settings.save()
    })
    .await;
    app.state::<Installs>()
        .0
        .lock()
        .unwrap()
        .remove(phpmyadmin::MODULE);
    result.map_err(|e| e.to_string())?.map_err(err)?;
    sites_changed(&app);
    Ok(phpmyadmin_view(&app.state::<Installs>()))
}

#[tauri::command]
fn phpmyadmin_uninstall(app: AppHandle) -> CmdResult<PhpMyAdminView> {
    phpmyadmin::uninstall(&paths::home()).map_err(err)?;
    let mut settings = Settings::load().map_err(err)?;
    settings
        .set_module(phpmyadmin::MODULE, false)
        .map_err(err)?;
    settings.save().map_err(err)?;
    sites_changed(&app);
    Ok(phpmyadmin_view(&app.state::<Installs>()))
}

/// Starts the web server and the installed database server when they are
/// not running, then opens phpMyAdmin in the browser.
#[tauri::command]
async fn phpmyadmin_open(app: AppHandle) -> CmdResult<()> {
    let engine = app.state::<Arc<Engine>>().inner().clone();
    let url = tauri::async_runtime::spawn_blocking(move || {
        let running = |id: &str| matches!(engine.state(id), ProcessState::Running { .. });
        if !running("caddy") {
            engine.start("caddy").map_err(err)?;
        }
        let database = ["mysql", "mariadb"]
            .into_iter()
            .find(|id| engine.manager().get(id).is_some());
        if let Some(id) =
            database.filter(|id| !running(id) && !running("mysql") && !running("mariadb"))
        {
            engine.start(id).map_err(err)?;
        }
        let view = phpmyadmin_view(&Installs::default());
        view.local_url
            .ok_or_else(|| "phpMyAdmin is not installed, or its module is off".to_string())
    })
    .await
    .map_err(|e| e.to_string())??;
    open_local_page(url)
}

#[tauri::command]
fn setup_status() -> setup::Status {
    setup::status()
}

/// Saves what first-run setup picked. When Stackyard's folder changes, the
/// app restarts to move its files there; the interface then carries on.
#[tauri::command]
fn setup_choose(
    app: AppHandle,
    home: String,
    sites_folder: String,
    install: Vec<String>,
) -> CmdResult<bool> {
    let choices = setup::Choices {
        sites_folder: paths::expand_home(std::path::Path::new(sites_folder.trim())),
        install,
    };
    let home = paths::expand_home(std::path::Path::new(home.trim()));
    let restart = setup::choose(&home, choices).map_err(err)?;
    if restart {
        std::thread::spawn(move || {
            // Let the answer reach the interface first.
            std::thread::sleep(std::time::Duration::from_millis(300));
            app.restart();
        });
    }
    Ok(restart)
}

/// Applies the saved picks: modules on or off, the sites folder parked.
#[tauri::command]
fn setup_apply(app: AppHandle) -> CmdResult<()> {
    let choices = paths::Location::load()
        .and_then(|l| l.choices)
        .ok_or("nothing was picked in setup")?;
    setup::apply(&choices).map_err(err)?;
    sites_changed(&app);
    Ok(())
}

#[tauri::command]
fn setup_finish() -> CmdResult<()> {
    setup::finish().map_err(err)
}

/// Where new sites get their folder.
#[tauri::command]
fn sites_folder() -> CmdResult<String> {
    let settings = Settings::load().map_err(err)?;
    Ok(settings.sites_folder(&paths::home()).display().to_string())
}

/// Empties every log so each starts again from nothing.
#[tauri::command]
fn clear_logs() -> CmdResult<usize> {
    Settings::load()
        .map_err(err)?
        .require_module("logs")
        .map_err(err)?;
    logs::clear(&paths::home()).map_err(err)
}

/// Sizing the data folder reads every file in it, so this runs on a worker
/// thread.
#[tauri::command]
async fn cleanup_scan() -> CmdResult<cleanup::Report> {
    tauri::async_runtime::spawn_blocking(|| cleanup::scan(&paths::home()))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn cleanup_run(app: AppHandle, ids: Vec<String>) -> CmdResult<cleanup::Outcome> {
    let busy = app.state::<Installs>().0.lock().unwrap().len();
    if busy > 0 {
        return Err("Wait for the install that is running to finish, then clean up.".into());
    }
    tauri::async_runtime::spawn_blocking(move || cleanup::clean(&paths::home(), &ids))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn update_status(app: AppHandle) -> updates::UpdateView {
    updates::view(&app)
}

#[tauri::command]
async fn check_for_update(app: AppHandle) -> updates::UpdateView {
    updates::check(&app).await
}

#[tauri::command]
async fn install_update(app: AppHandle) -> CmdResult<()> {
    updates::install(&app).await
}

#[tauri::command]
fn seen_whats_new(app: AppHandle) -> updates::UpdateView {
    updates::seen_whats_new(&app)
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

/// After Stackyard's folder moved: the PATH entry and the commands follow it.
fn repoint_terminal(old: &std::path::Path, new: &std::path::Path) {
    let (old_bin, new_home) = (shims::bin_dir(old), new.to_path_buf());
    std::thread::spawn(move || {
        if !user_path::status(&old_bin).on_path {
            return;
        }
        let new_bin = shims::bin_dir(&new_home);
        let result = user_path::remove(&old_bin).and_then(|_| {
            if let Some(cli) = cli_program() {
                shims::install(&new_home, &cli)?;
            }
            user_path::add(&new_bin)
        });
        if let Err(e) = result {
            eprintln!("could not move the terminal commands: {e}");
        }
    });
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
    let path = match path.trim() {
        "" => settings.sites_folder(&paths::home()).join(&name),
        typed => paths::expand_home(std::path::Path::new(typed)),
    };
    // A new project starts as an empty folder.
    std::fs::create_dir_all(&path)
        .map_err(|e| format!("could not create {}: {e}", path.display()))?;
    let runtime = parse_runtime(&runtime, &path, &settings)?;
    let mut store = SiteStore::load().map_err(err)?;
    store
        .add(Site {
            name,
            tld: settings.tld,
            path,
            runtime,
            https,
            dev_server: false,
        })
        .map_err(err)?;
    store.save().map_err(err)?;
    sites_changed(&app);
    site_views()
}

#[derive(Serialize)]
struct ImagesFound {
    count: usize,
    size: u64,
}

/// How many JPG and PNG images the chosen files and folders hold.
#[tauri::command]
async fn find_images(paths: Vec<String>) -> CmdResult<ImagesFound> {
    tauri::async_runtime::spawn_blocking(move || {
        let paths: Vec<std::path::PathBuf> = paths.iter().map(std::path::PathBuf::from).collect();
        let found = images::find(&paths);
        ImagesFound {
            count: found.len(),
            size: found
                .iter()
                .map(|s| std::fs::metadata(&s.path).map_or(0, |m| m.len()))
                .sum(),
        }
    })
    .await
    .map_err(|e| e.to_string())
}

#[derive(Clone, Serialize)]
struct ImagesProgress {
    done: usize,
    total: usize,
    result: images::FileResult,
}

/// Makes the copies the options ask for, reporting each image as it is done.
#[tauri::command]
async fn optimize_images(
    app: AppHandle,
    paths: Vec<String>,
    options: images::Options,
) -> CmdResult<Vec<images::FileResult>> {
    Settings::load()
        .map_err(err)?
        .require_module(images::MODULE)
        .map_err(err)?;
    images::check(&options).map_err(err)?;
    if !app
        .state::<Installs>()
        .0
        .lock()
        .unwrap()
        .insert(images::MODULE.into())
    {
        return Err("Images are already being optimized".into());
    }
    let worker = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let paths: Vec<std::path::PathBuf> = paths.iter().map(std::path::PathBuf::from).collect();
        let sources = images::find(&paths);
        let total = sources.len();
        images::optimize_all(&sources, &options, &mut |done, result| {
            let _ = worker.emit(
                "images-progress",
                ImagesProgress {
                    done,
                    total,
                    result: result.clone(),
                },
            );
        })
    })
    .await;
    app.state::<Installs>()
        .0
        .lock()
        .unwrap()
        .remove(images::MODULE);
    result.map_err(|e| e.to_string())
}

#[derive(Clone, Serialize)]
struct StarterProgress {
    name: String,
    message: String,
}

#[tauri::command]
fn list_starters() -> Vec<starters::StarterInfo> {
    starters::list()
}

/// Makes a new site from a starter (WordPress, Laravel, Astro), starting
/// what it needs: MySQL or MariaDB for WordPress, and the web server so the
/// site opens right away.
#[tauri::command]
async fn add_starter_site(
    app: AppHandle,
    name: String,
    starter: String,
    path: String,
    https: bool,
) -> CmdResult<Vec<SiteView>> {
    let starter = starters::Starter::from_id(&starter)
        .ok_or_else(|| format!("unknown starter {starter:?}"))?;
    domain::validate_site_name(&name).map_err(err)?;
    let settings = Settings::load().map_err(err)?;
    let tld = domain::normalize_tld(&settings.tld).map_err(err)?;
    let domain_name = domain::site_domain(&name, &tld);
    if ServiceManager::open()
        .sites()
        .map_err(err)?
        .iter()
        .any(|s| s.domain() == domain_name)
    {
        return Err(stackyard_core::Error::SiteExists(domain_name).to_string());
    }
    if !app
        .state::<Installs>()
        .0
        .lock()
        .unwrap()
        .insert(format!("starter-{name}"))
    {
        return Err(format!("{domain_name} is already being made"));
    }
    let engine = app.state::<Arc<Engine>>().inner().clone();
    let worker = app.clone();
    let site_name = name.clone();
    let result = tauri::async_runtime::spawn_blocking(move || -> CmdResult<()> {
        let home = paths::home();
        let folder = match path.trim() {
            "" => settings.sites_folder(&home).join(&site_name),
            typed => paths::expand_home(std::path::Path::new(typed)),
        };
        let running = |id: &str| matches!(engine.state(id), ProcessState::Running { .. });
        if starter.needs_database() && !running("mysql") && !running("mariadb") {
            let id = ["mysql", "mariadb"]
                .into_iter()
                .find(|id| engine.manager().get(id).is_some())
                .ok_or_else(|| {
                    format!(
                        "{} needs MySQL or MariaDB. Install one under Services first.",
                        starter.name()
                    )
                })?;
            engine.start(id).map_err(err)?;
        }
        let existed = folder.exists();
        let made = starters::create(&home, starter, &site_name, &folder, &mut |message| {
            let _ = worker.emit(
                "starter-progress",
                StarterProgress {
                    name: site_name.clone(),
                    message,
                },
            );
        });
        let made = match made {
            Ok(made) => made,
            Err(e) => {
                // Leave nothing half-made, so trying again works.
                if !existed {
                    let _ = std::fs::remove_dir_all(&folder);
                }
                return Err(err(e));
            }
        };
        let mut store = SiteStore::load().map_err(err)?;
        store
            .add(Site {
                name: site_name.clone(),
                tld,
                path: folder,
                runtime: made.runtime,
                https,
                dev_server: made.dev_server,
            })
            .map_err(err)?;
        store.save().map_err(err)?;
        if !running("caddy") {
            engine.start("caddy").map_err(err)?;
        }
        Ok(())
    })
    .await;
    app.state::<Installs>()
        .0
        .lock()
        .unwrap()
        .remove(&format!("starter-{name}"));
    result.map_err(|e| e.to_string())??;
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
    let mut home = paths::home();
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
    // First-run setup picked another folder: move everything there now,
    // before anything opens a file in it.
    if let Some((old, new)) = setup::move_if_pending() {
        repoint_terminal(&old, &new);
        home = new;
    }
    let just_updated = updates::take_just_updated();
    for id in updates::take_resume() {
        if !resume.contains(&id) {
            resume.push(id);
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
        .manage(updates::Updates::new(just_updated))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
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
            list_starters,
            find_images,
            optimize_images,
            add_starter_site,
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
            clear_logs,
            setup_status,
            phpmyadmin_status,
            phpmyadmin_install,
            phpmyadmin_uninstall,
            phpmyadmin_open,
            setup_choose,
            setup_apply,
            setup_finish,
            sites_folder,
            db_server,
            db_list,
            db_create,
            db_drop,
            db_tables,
            db_browse,
            db_query,
            db_backup,
            db_backups,
            db_restore,
            db_remove_backup,
            list_tasks,
            add_task,
            update_task,
            set_task_enabled,
            remove_task,
            run_task,
            stop_task,
            preview_schedule,
            task_log,
            cleanup_scan,
            cleanup_run,
            update_status,
            check_for_update,
            install_update,
            seen_whats_new
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
            updates::watch(app.handle());
            let handle = app.handle().clone();
            Engine::watch_parked(&app.state::<Arc<Engine>>(), &paths::home(), move || {
                let _ = handle.emit("sites-changed", ());
            });
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

/// The tray menu, with an item for the update when one is available.
fn tray_menu<R: Runtime>(
    app: &AppHandle<R>,
    update: Option<&str>,
) -> tauri::Result<tauri::menu::Menu<R>> {
    use tauri::menu::{IsMenuItem, Menu, MenuItem, PredefinedMenuItem};

    let item = |id: &str, text: &str| MenuItem::with_id(app, id, text, true, None::<&str>);
    let open = item("open", "Open Stackyard")?;
    let update = update
        .map(|version| item("update", &format!("Update to {version}…")))
        .transpose()?;
    let start = item("start-all", "Start all services")?;
    let stop = item("stop-all", "Stop all services")?;
    let quit = item("quit", "Quit Stackyard")?;
    let (gap1, gap2) = (
        PredefinedMenuItem::separator(app)?,
        PredefinedMenuItem::separator(app)?,
    );
    let mut items: Vec<&dyn IsMenuItem<R>> = vec![&open];
    if let Some(update) = &update {
        items.push(update);
    }
    items.extend([&gap1 as &dyn IsMenuItem<R>, &start, &stop, &gap2, &quit]);
    Menu::with_items(app, &items)
}

/// Shows or removes the tray's update item.
pub(crate) fn set_tray_update<R: Runtime>(app: &AppHandle<R>, version: Option<&str>) {
    let Some(tray) = app.tray_by_id("main") else {
        return;
    };
    match tray_menu(app, version) {
        Ok(menu) => {
            let _ = tray.set_menu(Some(menu));
            if let Some(version) = version {
                let _ = tray.set_tooltip(Some(format!("Stackyard: {version} is available")));
            }
        }
        Err(e) => eprintln!("could not update the tray menu: {e}"),
    }
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

    let menu = tray_menu(app, None)?;
    let mut tray = TrayIconBuilder::with_id("main")
        .tooltip("Stackyard")
        .menu(&menu)
        // macOS shows the menu on a click; elsewhere a click opens the window.
        .show_menu_on_left_click(cfg!(target_os = "macos"))
        .on_menu_event(|app, event| match event.id.as_ref() {
            // The window's notice has the update and its Update button.
            "open" | "update" => show_window(app),
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
