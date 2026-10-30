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
use stackyard_core::projects::{self, Category, Project, ProjectKind};
use stackyard_core::runtimes::{RuntimeManager, Step};
use stackyard_core::scheduler::{self, TaskInput, TaskView};
use stackyard_core::services::ServiceManager;
use stackyard_core::settings::Settings;
use stackyard_core::setup;
use stackyard_core::shell::{self, path as user_path, shims};
use stackyard_core::siteorder::{self, SiteOrder};
use stackyard_core::sites::{Site, SiteRuntime, SiteStore};
use stackyard_core::supervisor::{ProcessState, Supervisor};
use stackyard_core::system::{self, Elevate, Resolver};
use stackyard_core::web::{self, Backend, Ports, ServedSite};
use stackyard_core::{
    domain, images, interface, languages, paths, phpmyadmin, starters, themes, videos,
};
use tauri::{AppHandle, Emitter, Manager, RunEvent, State};

mod ai;
mod applog;
mod console;
mod perf;
mod remote;
mod sitepics;
mod sitetools;
mod tray;
mod updates;
mod wordpress;

pub(crate) type CmdResult<T> = Result<T, String>;

#[derive(Serialize)]
pub(crate) struct SiteView {
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
    /// Has a local address of its own (`name.stack`); an app without one
    /// opens on its port or `.localhost` name, both in `url`.
    address: bool,
    /// Why the site cannot work yet, if it cannot.
    problem: Option<String>,
    /// The runtime versions the project uses, for the version pickers.
    versions: Vec<SiteVersionView>,
    project: ProjectView,
}

/// What a site is for, and the folder the editor, terminal and Git open.
#[derive(Serialize)]
struct ProjectView {
    kind: &'static str,
    /// The list it is in: `project`, `app` or `site`.
    category: &'static str,
    folder: String,
}

#[derive(Serialize)]
struct SiteVersionView {
    kind: RuntimeKind,
    version: String,
    installed: bool,
    /// `site` (saved with the site), `pinned`, `default` or `newest`.
    from: &'static str,
    /// The version file that decides, e.g. `.nvmrc`, when one does.
    file: Option<String>,
    /// Whether "Default" is a choice: not for the runtime a saved site is
    /// served with, which always names its version.
    can_unset: bool,
}

impl SiteVersionView {
    fn list(
        site: &Site,
        parked: bool,
        settings: &Settings,
        runtimes: &RuntimeManager,
    ) -> Vec<Self> {
        shell::site_runtimes(site)
            .into_iter()
            .filter(|kind| settings.module_enabled(kind.id()))
            .map(|kind| {
                let choice = shell::site_choice(site, kind, settings, runtimes);
                let (from, file) = match &choice.reason {
                    shell::Reason::Pinned { file } => (
                        "pinned",
                        file.file_name().map(|n| n.to_string_lossy().into_owned()),
                    ),
                    shell::Reason::Site { .. } => ("site", None),
                    shell::Reason::Default => ("default", None),
                    shell::Reason::Newest => ("newest", None),
                };
                Self {
                    kind,
                    installed: runtimes.get(kind, &choice.version).is_some(),
                    version: choice.version,
                    from,
                    file,
                    can_unset: parked || site.runtime.kind() != Some(kind),
                }
            })
            .collect()
    }
}

impl SiteView {
    fn new(
        served: &ServedSite,
        ports: Ports,
        settings: &Settings,
        runtimes: &RuntimeManager,
    ) -> Self {
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
            Backend::App {
                runtime,
                version,
                port,
            } => (
                format!("{} {version} · server on port {port}", site_label(*runtime)),
                served.parked.then(|| {
                    "Stackyard runs the server only for saved items: open Manage and save its settings or pick its type".to_string()
                }),
            ),
            Backend::Static => ("Static".to_string(), None),
        };
        let problem = problem.or_else(|| {
            (!served.root.is_dir()).then(|| format!("{} does not exist", served.root.display()))
        });
        let (url, local_url) = if site.settings.no_address {
            let url = match &served.backend {
                Backend::Node { port, .. } | Backend::App { port, .. } => {
                    format!("http://localhost:{port}")
                }
                _ => ports.url(&served.localhost(), false),
            };
            (url.clone(), url)
        } else {
            (
                ports.url(&site.domain(), site.https),
                ports.url(&served.localhost(), site.https),
            )
        };
        Self {
            name: site.name.clone(),
            domain: site.domain(),
            url,
            local_url,
            address: !site.settings.no_address,
            path: site.path.display().to_string(),
            root: served.root.display().to_string(),
            runtime,
            framework: served.framework,
            https: site.https && !site.settings.no_address,
            parked: served.parked,
            problem,
            versions: SiteVersionView::list(site, served.parked, settings, runtimes),
            project: ProjectView {
                kind: site.settings.project.kind.id(),
                category: site.settings.project.category().id(),
                folder: stackyard_core::projects::folder(site).display().to_string(),
            },
        }
    }
}

pub(crate) fn site_views() -> CmdResult<Vec<SiteView>> {
    let manager = ServiceManager::open();
    let ports = Ports::current(manager.home());
    let sites = manager.user_sites().map_err(err)?;
    let settings = Settings::load().map_err(err)?;
    let runtimes = RuntimeManager::new(manager.home());
    Ok(sites
        .iter()
        .map(|s| SiteView::new(s, ports, &settings, &runtimes))
        .collect())
}

#[derive(Serialize)]
struct TldView {
    tld: String,
    warning: Option<&'static str>,
}

pub(crate) fn err(e: stackyard_core::Error) -> String {
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
    /// How apps connect, e.g. `mongodb://127.0.0.1:27017`.
    connect: Option<String>,
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
            connect: stackyard_core::services::connect(manager.home(), def),
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
    tray::changed(&app);
    Ok(service_views(
        app.state::<Arc<Engine>>().supervisor(),
        &app.state::<Installs>(),
    ))
}

#[tauri::command]
fn uninstall_service(
    app: AppHandle,
    id: String,
    engine: State<'_, Arc<Engine>>,
    installs: State<'_, Installs>,
) -> CmdResult<Vec<ServiceView>> {
    engine.stop(&id).map_err(err)?;
    engine.manager().uninstall(&id).map_err(err)?;
    tray::changed(&app);
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
/// `server` is `postgres` for PostgreSQL; anything else is MySQL or MariaDB.
async fn with_db<T: Send + 'static>(
    server: Option<String>,
    work: impl FnOnce(&DbClient) -> stackyard_core::Result<T> + Send + 'static,
) -> CmdResult<T> {
    tauri::async_runtime::spawn_blocking(move || {
        Settings::load()?.require_module("databases")?;
        work(&DbClient::open_server(&paths::home(), server.as_deref())?)
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
async fn db_server(server: Option<String>) -> CmdResult<DbServerView> {
    tauri::async_runtime::spawn_blocking(move || {
        let opened = Settings::load()
            .and_then(|s| s.require_module("databases"))
            .and_then(|_| DbClient::open_server(&paths::home(), server.as_deref()));
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
async fn db_list(server: Option<String>) -> CmdResult<Vec<Database>> {
    with_db(server, |c| c.databases()).await
}

#[tauri::command]
async fn db_create(name: String, server: Option<String>) -> CmdResult<Vec<Database>> {
    with_db(server, move |c| {
        c.create(&name)?;
        c.databases()
    })
    .await
}

#[tauri::command]
async fn db_drop(name: String, server: Option<String>) -> CmdResult<Vec<Database>> {
    with_db(server, move |c| {
        c.drop(&name)?;
        c.databases()
    })
    .await
}

#[tauri::command]
async fn db_tables(database: String, server: Option<String>) -> CmdResult<Vec<Table>> {
    with_db(server, move |c| c.tables(&database)).await
}

#[tauri::command]
async fn db_browse(
    database: String,
    table: String,
    offset: u64,
    limit: usize,
    server: Option<String>,
) -> CmdResult<ResultSet> {
    with_db(server, move |c| c.browse(&database, &table, offset, limit)).await
}

#[tauri::command]
async fn db_query(
    database: Option<String>,
    sql: String,
    server: Option<String>,
) -> CmdResult<Vec<ResultSet>> {
    with_db(server, move |c| c.query(database.as_deref(), &sql)).await
}

#[tauri::command]
async fn db_backup(database: String, server: Option<String>) -> CmdResult<Vec<Backup>> {
    with_db(server, move |c| {
        c.backup(&database)?;
        Ok(databases::backups(&paths::home()))
    })
    .await
}

/// The servers the Databases screen can switch between.
#[tauri::command]
fn db_servers() -> Vec<&'static str> {
    databases::servers(&paths::home())
}

#[tauri::command]
fn db_backups() -> Vec<Backup> {
    databases::backups(&paths::home())
}

/// Opens the folder a database's backups are in, or the backups folder
/// when it has none yet.
#[tauri::command]
fn open_db_backups(database: String) -> CmdResult<()> {
    let home = paths::home();
    let root = databases::backups_dir(&home);
    let has_backups = databases::backups(&home)
        .iter()
        .any(|b| b.database == database);
    let dir = if has_backups {
        root.join(&database)
    } else {
        root
    };
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    sitetools::open_with_system(dir.as_os_str())
}

#[tauri::command]
async fn db_restore(
    database: String,
    file: String,
    into: Option<String>,
    server: Option<String>,
) -> CmdResult<Vec<Database>> {
    with_db(server, move |c| {
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
    open_page(url)
}

/// Opens a page of a local site in the browser; other addresses are refused.
pub(crate) fn open_page(url: String) -> CmdResult<()> {
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
    let settings = Settings::load().unwrap_or_default();
    let runtimes = RuntimeManager::new(&home);
    let site = manager
        .sites()
        .unwrap_or_default()
        .iter()
        .map(|s| SiteView::new(s, ports, &settings, &runtimes))
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
    open_phpmyadmin(app, None).await
}

/// Starts what phpMyAdmin needs and opens it, at `database` when given.
pub(crate) async fn open_phpmyadmin(app: AppHandle, database: Option<String>) -> CmdResult<()> {
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
    match database.filter(|d| {
        d.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    }) {
        Some(db) => open_page(format!(
            "{}/index.php?route=/database/structure&db={db}",
            url.trim_end_matches('/')
        )),
        None => open_page(url),
    }
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

/// Picks the folder new sites are created in.
#[tauri::command]
fn set_sites_folder(path: String) -> CmdResult<String> {
    let mut settings = Settings::load().map_err(err)?;
    let folder = settings
        .set_sites_folder(std::path::Path::new(path.trim()))
        .map_err(err)?;
    settings.save().map_err(err)?;
    Ok(folder.display().to_string())
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

/// Empties one log, leaving the others as they are.
#[tauri::command]
fn clear_log(id: String) -> CmdResult<bool> {
    Settings::load()
        .map_err(err)?
        .require_module("logs")
        .map_err(err)?;
    logs::clear_one(&paths::home(), &id).map_err(err)
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

/// Translation files the user put in the languages folder.
#[tauri::command]
fn language_files() -> languages::LanguageFiles {
    languages::load()
}

/// Opens the languages folder (made first) in the file manager.
#[tauri::command]
fn open_languages_folder() -> CmdResult<()> {
    let dir = languages::folder();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    #[cfg(target_os = "macos")]
    let mut cmd = std::process::Command::new("open");
    #[cfg(target_os = "windows")]
    let mut cmd = std::process::Command::new("explorer");
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut cmd = std::process::Command::new("xdg-open");
    cmd.arg(&dir).spawn().map_err(|e| e.to_string())?;
    Ok(())
}

/// The window's own settings, from the `[interface]` section of `settings.toml`.
#[tauri::command]
fn interface_settings() -> interface::InterfaceFile {
    interface::load_settings()
}

#[tauri::command]
fn save_interface_settings(values: interface::Table) -> CmdResult<()> {
    interface::save_settings(values).map_err(err)
}

/// The keyboard shortcuts from `keybindings.toml`.
#[tauri::command]
fn keybindings() -> interface::InterfaceFile {
    interface::load_keybindings()
}

#[tauri::command]
fn save_keybindings(values: interface::Table) -> CmdResult<()> {
    interface::save_keybindings(&values).map_err(err)
}

/// Shows `settings.toml` or `keybindings.toml` in the file manager, made
/// first when it does not exist yet, so it can be opened in an editor.
#[tauri::command]
fn show_interface_file(file: String) -> CmdResult<()> {
    let path = match file.as_str() {
        "settings" => paths::settings_file(),
        "keybindings" => interface::keybindings_file(),
        _ => return Err(format!("unknown file {file:?}")),
    };
    if !path.exists() {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        std::fs::write(&path, "").map_err(|e| e.to_string())?;
    }
    // Explorer reads `/select,"path"` itself, so the quotes are written here.
    #[cfg(target_os = "windows")]
    let result = {
        use std::os::windows::process::CommandExt;
        std::process::Command::new("explorer")
            .raw_arg(format!("/select,\"{}\"", path.display()))
            .spawn()
    };
    #[cfg(target_os = "macos")]
    let result = std::process::Command::new("open")
        .arg("-R")
        .arg(&path)
        .spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let result = std::process::Command::new("xdg-open")
        .arg(path.parent().unwrap_or(path.as_path()))
        .spawn();
    result.map(|_| ()).map_err(|e| e.to_string())
}

/// Color themes added to the `themes` folder, and the files that couldn't be used.
#[tauri::command]
fn theme_files() -> themes::ThemeFiles {
    themes::load()
}

/// Copies a VS Code theme (`.json`) or extension (`.vsix`) into the themes
/// folder; returns the ids of the themes it added.
#[tauri::command]
fn import_theme(path: std::path::PathBuf) -> CmdResult<Vec<String>> {
    themes::import(&path)
}

#[tauri::command]
fn remove_theme(id: String) -> CmdResult<()> {
    themes::remove(&id)
}

#[tauri::command]
fn open_themes_folder() -> CmdResult<()> {
    let dir = themes::folder();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    #[cfg(target_os = "macos")]
    let mut cmd = std::process::Command::new("open");
    #[cfg(target_os = "windows")]
    let mut cmd = std::process::Command::new("explorer");
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut cmd = std::process::Command::new("xdg-open");
    cmd.arg(&dir).spawn().map_err(|e| e.to_string())?;
    Ok(())
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
    app: AppHandle,
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
    tray::changed(&app);
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
pub(crate) fn cli_program() -> Option<std::path::PathBuf> {
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
            crate::applog::error(&format!("could not move the terminal commands: {e}"));
        }
    });
}

/// After an update, points the terminal commands at the new `stackyard`.
fn refresh_terminal_commands() {
    std::thread::spawn(|| {
        let home = paths::home();
        if let Some(cli) = cli_program().filter(|cli| shims::outdated(&home, cli)) {
            if let Err(e) = shims::install(&home, &cli) {
                crate::applog::error(&format!("could not update the terminal commands: {e}"));
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
    /// Off until the user allows it: it changes something, or reads what
    /// is in databases.
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

/// One entry of the Sites screen's order. A group always carries its
/// `sites`, even none: `site-order.toml` leaves an empty list out, and a
/// group without one broke the Sites screen (and with it the whole window).
#[derive(Debug, PartialEq, Serialize)]
#[serde(untagged)]
enum LayoutItem {
    Site {
        site: String,
    },
    Group {
        group: String,
        sites: Vec<String>,
        collapsed: bool,
        /// `app` or `site`: the list it shows in; absent shows it where its sites are.
        #[serde(skip_serializing_if = "Option::is_none")]
        list: Option<String>,
    },
}

impl From<siteorder::Item> for LayoutItem {
    fn from(item: siteorder::Item) -> Self {
        match item.group {
            Some(group) => LayoutItem::Group {
                group,
                sites: item.sites,
                collapsed: item.collapsed,
                list: item.list,
            },
            None => LayoutItem::Site {
                site: item.site.unwrap_or_default(),
            },
        }
    }
}

/// The sites' groups and order, for the Sites screen.
#[tauri::command]
fn site_layout() -> CmdResult<Vec<LayoutItem>> {
    let manager = ServiceManager::open();
    let order = SiteOrder::load(manager.home()).map_err(err)?;
    let domains: Vec<String> = manager
        .user_sites()
        .map_err(err)?
        .iter()
        .map(|s| s.site.domain())
        .collect();
    Ok(order
        .arranged(&domains)
        .into_iter()
        .map(LayoutItem::from)
        .collect())
}

/// Saves the groups and order the user arranged; every list follows it.
#[tauri::command]
fn set_site_layout(app: AppHandle, items: Vec<siteorder::Item>) -> CmdResult<Vec<LayoutItem>> {
    let mut order = SiteOrder::load(&paths::home()).map_err(err)?;
    order.set(items).map_err(err)?;
    order.save().map_err(err)?;
    // Only the lists change; the web server needn't restart.
    let _ = app.emit("sites-changed", ());
    tray::changed(&app);
    site_layout()
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
fn add_site(
    app: AppHandle,
    name: String,
    path: String,
    runtime: String,
    https: bool,
    kind: Option<String>,
    category: Option<String>,
    address: Option<bool>,
) -> CmdResult<Vec<SiteView>> {
    let category = parse_category(category.as_deref())?;
    let kind = match kind.as_deref() {
        None => ProjectKind::Website,
        Some(id) => {
            ProjectKind::from_id(id).ok_or_else(|| format!("unknown project kind {id:?}"))?
        }
    };
    if kind.in_wordpress() {
        return Err("a theme or plugin is made with its WordPress (add_project)".into());
    }
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
            settings: stackyard_core::sites::SiteSettings {
                project: Project {
                    kind,
                    source: None,
                    category,
                },
                no_address: address == Some(false),
                ..Default::default()
            },
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

fn require_videos() -> CmdResult<()> {
    Settings::load()
        .map_err(err)?
        .require_module(videos::MODULE)
        .map_err(err)
}

#[derive(Serialize)]
struct VideosStatus {
    #[serde(flatten)]
    ffmpeg: videos::Status,
    /// FFmpeg is being downloaded.
    installing: bool,
}

fn videos_view(installs: &Installs) -> VideosStatus {
    VideosStatus {
        ffmpeg: videos::status(&paths::home()),
        installing: installs.0.lock().unwrap().contains("ffmpeg"),
    }
}

/// Which FFmpeg the video optimizer uses, if any.
#[tauri::command]
async fn videos_status(app: AppHandle) -> CmdResult<VideosStatus> {
    tauri::async_runtime::spawn_blocking(move || videos_view(&app.state::<Installs>()))
        .await
        .map_err(|e| e.to_string())
}

/// Downloads FFmpeg into Stackyard's tools folder.
#[tauri::command]
async fn videos_install_ffmpeg(app: AppHandle) -> CmdResult<VideosStatus> {
    require_videos()?;
    if !app
        .state::<Installs>()
        .0
        .lock()
        .unwrap()
        .insert("ffmpeg".into())
    {
        return Err("FFmpeg is already being downloaded".into());
    }
    let worker = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        videos::install(&paths::home(), &mut |step| {
            let _ = worker.emit("ffmpeg-progress", step);
        })
    })
    .await;
    app.state::<Installs>().0.lock().unwrap().remove("ffmpeg");
    result.map_err(|e| e.to_string())?.map_err(err)?;
    Ok(videos_view(&app.state::<Installs>()))
}

#[tauri::command]
fn videos_remove_ffmpeg(app: AppHandle) -> CmdResult<VideosStatus> {
    videos::uninstall(&paths::home()).map_err(err)?;
    Ok(videos_view(&app.state::<Installs>()))
}

/// How many videos the chosen files and folders hold.
#[tauri::command]
async fn find_videos(paths: Vec<String>) -> CmdResult<ImagesFound> {
    tauri::async_runtime::spawn_blocking(move || {
        let paths: Vec<std::path::PathBuf> = paths.iter().map(std::path::PathBuf::from).collect();
        let found = videos::find(&paths);
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

/// Makes the copies the options ask for, reporting progress as it goes.
#[tauri::command]
async fn optimize_videos(
    app: AppHandle,
    paths: Vec<String>,
    options: videos::Options,
) -> CmdResult<Vec<videos::FileResult>> {
    require_videos()?;
    videos::check(&options).map_err(err)?;
    let ffmpeg = videos::ffmpeg(&paths::home())
        .ok_or("FFmpeg is not installed yet: download it at the top of the Videos screen")?;
    if !app
        .state::<Installs>()
        .0
        .lock()
        .unwrap()
        .insert(videos::MODULE.into())
    {
        return Err("Videos are already being optimized".into());
    }
    let worker = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let paths: Vec<std::path::PathBuf> = paths.iter().map(std::path::PathBuf::from).collect();
        let sources = videos::find(&paths);
        videos::optimize_all(&ffmpeg, &sources, &options, &mut |progress| {
            let _ = worker.emit("videos-progress", progress);
        })
    })
    .await;
    app.state::<Installs>()
        .0
        .lock()
        .unwrap()
        .remove(videos::MODULE);
    result.map_err(|e| e.to_string())
}

/// Stops optimizing; the copy being made is dropped.
#[tauri::command]
fn stop_videos() {
    videos::stop();
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
    category: Option<String>,
    address: Option<bool>,
) -> CmdResult<Vec<SiteView>> {
    let starter = starters::Starter::from_id(&starter)
        .ok_or_else(|| format!("unknown starter {starter:?}"))?;
    let category = parse_category(category.as_deref())?.or(Some(starter.category()));
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
        if starter.needs_database() {
            start_database(&engine, starter.name())?;
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
                settings: stackyard_core::sites::SiteSettings {
                    database: made.database.clone().map(|name| {
                        stackyard_core::sites::SiteDatabase {
                            server: "mysql".into(),
                            name,
                        }
                    }),
                    project: Project {
                        category,
                        ..Default::default()
                    },
                    no_address: address == Some(false),
                    ..Default::default()
                },
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

/// Starts MySQL or MariaDB, whichever is installed, unless one runs.
fn start_database(engine: &Engine, needed_by: &str) -> CmdResult<()> {
    let running = |id: &str| matches!(engine.state(id), ProcessState::Running { .. });
    if running("mysql") || running("mariadb") {
        return Ok(());
    }
    let id = ["mysql", "mariadb"]
        .into_iter()
        .find(|id| engine.manager().get(id).is_some())
        .ok_or_else(|| {
            format!("{needed_by} needs MySQL or MariaDB. Install one under Services first.")
        })?;
    engine.start(id).map_err(err)
}

/// Sends the steps of making a project to the window, as a starter's go.
fn project_progress(app: &AppHandle, name: &str) -> impl FnMut(String) + Send + 'static {
    let app = app.clone();
    let name = name.to_string();
    move |message| {
        let _ = app.emit(
            "starter-progress",
            StarterProgress {
                name: name.clone(),
                message,
            },
        );
    }
}

/// Makes a WordPress theme or plugin project: its folder (with a starting
/// theme or plugin when empty), the WordPress it is tried in, and the link
/// between them.
#[tauri::command]
async fn add_project(
    app: AppHandle,
    name: String,
    kind: String,
    path: String,
    https: bool,
) -> CmdResult<Vec<SiteView>> {
    let kind = ProjectKind::from_id(&kind)
        .filter(|k| k.in_wordpress())
        .ok_or_else(|| format!("{kind:?} is not made with a WordPress; add it as a site"))?;
    domain::validate_site_name(&name).map_err(err)?;
    let settings = Settings::load().map_err(err)?;
    settings.require_module("php").map_err(err)?;
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
    let key = format!("starter-{name}");
    if !app
        .state::<Installs>()
        .0
        .lock()
        .unwrap()
        .insert(key.clone())
    {
        return Err(format!("{domain_name} is already being made"));
    }
    let engine = app.state::<Arc<Engine>>().inner().clone();
    let mut progress = project_progress(&app, &name);
    let site_name = name.clone();
    let result = tauri::async_runtime::spawn_blocking(move || -> CmdResult<()> {
        let home = paths::home();
        let source = match path.trim() {
            "" => settings.sites_folder(&home).join(&site_name),
            typed => paths::expand_home(std::path::Path::new(typed)),
        };
        std::fs::create_dir_all(&source)
            .map_err(|e| format!("could not create {}: {e}", source.display()))?;
        projects::scaffold(kind, &source, &site_name).map_err(err)?;
        start_database(&engine, kind.name())?;
        let url = stackyard_core::web::Ports::current(&home).url(&domain_name, https);
        let ready =
            projects::prepare_wordpress(&home, &site_name, kind, &source, &url, &mut progress)
                .map_err(err)?;
        let mut store = SiteStore::load().map_err(err)?;
        store
            .add(Site {
                name: site_name.clone(),
                tld,
                path: ready.path,
                runtime: ready.runtime,
                https,
                dev_server: false,
                settings: stackyard_core::sites::SiteSettings {
                    database: Some(ready.database),
                    project: Project {
                        kind,
                        source: Some(source),
                        category: Some(Category::App),
                    },
                    ..Default::default()
                },
            })
            .map_err(err)?;
        store.save().map_err(err)?;
        if !matches!(engine.state("caddy"), ProcessState::Running { .. }) {
            engine.start("caddy").map_err(err)?;
        }
        Ok(())
    })
    .await;
    app.state::<Installs>().0.lock().unwrap().remove(&key);
    result.map_err(|e| e.to_string())??;
    sites_changed(&app);
    site_views()
}

/// Changes what a site is for: a theme or plugin gets its WordPress made
/// (or found again) and linked; going back to a website unlinks it.
#[tauri::command]
async fn set_project_kind(
    app: AppHandle,
    domain: String,
    kind: String,
) -> CmdResult<Vec<SiteView>> {
    let to = ProjectKind::from_id(&kind).ok_or_else(|| format!("unknown project kind {kind:?}"))?;
    let engine = app.state::<Arc<Engine>>().inner().clone();
    let mut store = saved_store(&domain)?;
    let name = store.get_mut(&domain).map_err(err)?.name.clone();
    let mut progress = project_progress(&app, &name);
    tauri::async_runtime::spawn_blocking(move || -> CmdResult<()> {
        if to.in_wordpress() {
            start_database(&engine, to.name())?;
        }
        let site = store.get_mut(&domain).map_err(err)?;
        projects::change(&paths::home(), site, to, &mut progress).map_err(err)?;
        store.save().map_err(err)
    })
    .await
    .map_err(|e| e.to_string())??;
    sites_changed(&app);
    site_views()
}

/// Moves a project, app or site to another list. One found in a parked
/// folder is saved first, as if it had been made in the app.
#[tauri::command]
fn set_site_category(app: AppHandle, domain: String, category: String) -> CmdResult<Vec<SiteView>> {
    let category =
        Category::from_id(&category).ok_or_else(|| format!("unknown type {category:?}"))?;
    let mut store = saved_store(&domain)?;
    store
        .get_mut(&domain)
        .map_err(err)?
        .settings
        .project
        .category = Some(category);
    store.save().map_err(err)?;
    sites_changed(&app);
    site_views()
}

/// `project`, `app` or `site`; `None` leaves it to the kind.
fn parse_category(id: Option<&str>) -> CmdResult<Option<Category>> {
    id.map(|id| Category::from_id(id).ok_or_else(|| format!("unknown type {id:?}")))
        .transpose()
}

/// The saved sites, with the one at `domain` among them: a site found in a
/// parked folder is saved as it is listed (and the store written), so it
/// can be changed like one made in the app.
pub(crate) fn saved_store(domain: &str) -> CmdResult<SiteStore> {
    let mut store = SiteStore::load().map_err(err)?;
    let served = ServiceManager::open()
        .user_sites()
        .map_err(err)?
        .into_iter()
        .find(|s| s.domain() == domain)
        .ok_or_else(|| format!("no site named {domain}"))?;
    if served.parked {
        store.adopt(&served.site).map_err(err)?;
        store.save().map_err(err)?;
    }
    Ok(store)
}

/// Picks the version of a runtime for a site, or `None` for the default,
/// and restarts the web server and dev server with it.
#[tauri::command]
fn set_site_version(
    app: AppHandle,
    domain: String,
    kind: String,
    version: Option<String>,
) -> CmdResult<Vec<SiteView>> {
    let kind = parse_kind(&kind)?;
    let served = ServiceManager::open()
        .user_sites()
        .map_err(err)?
        .into_iter()
        .find(|s| s.domain() == domain)
        .ok_or_else(|| format!("no site named {domain}"))?;
    let mut store = SiteStore::load().map_err(err)?;
    shell::set_site_version(&mut store, &served.site, kind, version.as_deref()).map_err(err)?;
    sites_changed(&app);
    site_views()
}

/// Takes a site off the list. With `delete_files` its folder goes too
/// (for a theme or plugin, also the folder it is built in); else the files
/// stay and a folder in a parked folder is hidden so it doesn't come back.
#[tauri::command]
fn remove_site(
    app: AppHandle,
    name: String,
    delete_files: Option<bool>,
) -> CmdResult<Vec<SiteView>> {
    let delete_files = delete_files.unwrap_or(false);
    let listed = ServiceManager::open()
        .user_sites()
        .map_err(err)?
        .into_iter()
        .find(|s| s.domain() == name || s.site.name == name)
        .ok_or_else(|| format!("no site named {name}"))?;
    let engine = app.state::<Arc<Engine>>().inner().clone();
    // Checked before the site is taken off the list, so a refused delete
    // changes nothing.
    let keep = if delete_files {
        let keep = kept_folders(&listed.site)?;
        stackyard_core::sites::check_delete(&listed.site, &keep).map_err(err)?;
        keep
    } else {
        Vec::new()
    };
    let removed = if listed.parked {
        listed.site
    } else {
        let mut store = SiteStore::load().map_err(err)?;
        let removed = store.remove(&name).map_err(err)?;
        store.save().map_err(err)?;
        removed
    };
    // A project in a parked folder (new sites go in one) would come straight
    // back as a parked site, so it is hidden from the list; its files stay.
    let mut settings = Settings::load().map_err(err)?;
    if settings.hide_parked(&removed.path) {
        settings.save().map_err(err)?;
    }
    remote::site_removed(&removed.domain());
    stackyard_core::wordpress::env::forget(&paths::home(), &removed.domain());
    stackyard_core::thumbnails::forget(&paths::home(), &removed.domain());
    stackyard_core::sitesize::forget(&paths::home(), &removed.domain());
    let _ = stackyard_core::share::stop(engine.supervisor(), &removed);
    let deleted = if delete_files {
        // What runs from the folder holds files open on Windows.
        sitetools::stop_site_processes(&engine, &removed);
        let deleted = stackyard_core::sites::delete_folders(&removed, &keep);
        // Gone for good: nothing is left to hide.
        if deleted.is_ok() {
            let hidden: Vec<_> = settings
                .hidden
                .iter()
                .filter(|h| stackyard_core::sites::same_folder(h, &removed.path))
                .cloned()
                .collect();
            let mut unhidden = false;
            for h in &hidden {
                unhidden |= settings.unhide(h);
            }
            if unhidden {
                settings.save().map_err(err)?;
            }
        }
        deleted.map_err(|e| format!("{} is removed from Stackyard, but {e}", removed.domain()))
    } else {
        Ok(())
    };
    sites_changed(&app);
    deleted?;
    site_views()
}

/// Folders deleting a site's files must leave alone: parked folders, the
/// folder new sites go in, the data home, the home folder, and every other
/// site's folders.
fn kept_folders(site: &stackyard_core::sites::Site) -> CmdResult<Vec<std::path::PathBuf>> {
    let settings = Settings::load().map_err(err)?;
    let home = paths::home();
    let mut keep = settings.parked.clone();
    keep.push(settings.sites_folder(&home));
    keep.push(home);
    if let Some(user) = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }) {
        keep.push(user.into());
    }
    for other in ServiceManager::open().user_sites().map_err(err)? {
        if other.site.domain() != site.domain() {
            keep.extend(stackyard_core::sites::site_folders(&other.site));
        }
    }
    Ok(keep)
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
/// Starts MySQL or MariaDB unless one is running, for work that needs a database.
pub(crate) fn ensure_database(engine: &Engine) -> CmdResult<()> {
    let running = |id: &str| matches!(engine.state(id), ProcessState::Running { .. });
    if running("mysql") || running("mariadb") {
        return Ok(());
    }
    let id = ["mysql", "mariadb"]
        .into_iter()
        .find(|id| engine.manager().get(id).is_some())
        .ok_or("this needs MySQL or MariaDB; install one under Services first")?;
    engine.start(id).map_err(err)
}

/// Starts the web server unless it is running.
pub(crate) fn ensure_web(engine: &Engine) -> CmdResult<()> {
    if matches!(engine.state("caddy"), ProcessState::Running { .. }) {
        return Ok(());
    }
    engine.start("caddy").map_err(err)
}

pub(crate) fn sites_changed(app: &AppHandle) {
    // Every list of sites in the window (the side bar, tabs, Home) reloads.
    let _ = app.emit("sites-changed", ());
    tray::changed(app);
    let engine = app.state::<Arc<Engine>>().inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        if let Err(e) = engine.sites_changed() {
            crate::applog::error(&format!("could not restart the web server: {e}"));
        }
    });
}

/// How the Sites screen names a runtime: `PHP`, `Node`, `Python`, `Go`, `Java`.
fn site_label(kind: RuntimeKind) -> &'static str {
    match kind {
        RuntimeKind::Node => "Node",
        other => catalog::runtime(other).name,
    }
}

/// Accepts the labels the Sites screen offers: `Auto`, `PHP 8.3`,
/// `Node 22`, `Python 3.13`, `Go 1.26`, `Java 21`, `Static`. `Auto` looks at
/// the project folder.
fn parse_runtime(
    label: &str,
    path: &std::path::Path,
    settings: &Settings,
) -> CmdResult<SiteRuntime> {
    let kind = |name: &str| {
        RuntimeKind::ALL
            .into_iter()
            .find(|k| site_label(*k) == name)
    };
    match label.split_once(' ') {
        Some((name, version)) if kind(name).is_some() => {
            let kind = kind(name).expect("checked");
            let port = match detect::detect(path).needs {
                Needs::Node { port } if kind == RuntimeKind::Node => port,
                Needs::App { runtime, port } if runtime == kind => port,
                _ if kind == RuntimeKind::Node => 3000,
                _ => 8000,
            };
            Ok(SiteRuntime::new(kind, version.to_string(), port))
        }
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
    /// Sites whose own name already works.
    ready_domains: Vec<String>,
}

fn system_view(app: &AppHandle) -> CmdResult<SystemView> {
    let manager = ServiceManager::open();
    let (wanted, status) = manager.system_status().map_err(err)?;
    Ok(SystemView {
        needed: status.needed,
        waiting_for_certificate: status.waiting_for_certificate,
        resolver: wanted.resolver,
        dns_running: app.state::<Arc<Engine>>().dns_running(),
        ready_domains: status.ready_domains,
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

/// Makes sure only one copy of the app runs. Opening Stackyard twice at
/// once (a double click) used to start two copies before either could see
/// the other, and both started MySQL on the same files. The lock is held
/// for the life of this process; `None` means another copy has it, and its
/// window was brought forward. A restart after an update waits here for the
/// old copy to finish quitting.
fn first_copy(home: &std::path::Path) -> Option<Option<std::fs::File>> {
    let path = std::env::temp_dir().join("stackyard-app.lock");
    let Ok(file) = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
    else {
        // No lock to be had: carry on as before.
        return Some(None);
    };
    for _ in 0..150 {
        match file.try_lock() {
            Ok(()) => return Some(Some(file)),
            Err(std::fs::TryLockError::WouldBlock) => {}
            Err(std::fs::TryLockError::Error(_)) => return Some(None),
        }
        if let Some(client) = control::find(home).filter(|c| c.owner == Owner::App) {
            let _ = client.send(Request::Show);
            return None;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    None
}

/// The connection that lets the CLI reach this app's services.
struct Control(Mutex<Option<ControlServer>>);

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    perf::started();
    let mut home = paths::home();
    applog::init(&home);
    // Held until this process ends.
    let Some(_first) = first_copy(&home) else {
        applog::info("another copy is open; showed its window and quit");
        return;
    };
    // One process runs services at a time.
    let mut resume = Vec::new();
    if let Some(client) = control::find(&home) {
        match client.owner {
            // Already open: bring that window forward instead.
            Owner::App => {
                let _ = client.send(Request::Show);
                applog::info("another copy is open; showed its window and quit");
                return;
            }
            // `stackyard start` or `stackyard up` runs them: take over.
            Owner::Daemon | Owner::Up => {
                applog::info("taking over from the background service");
                resume = take_over(&home, &client)
            }
        }
    }
    // First-run setup picked another folder: move everything there now,
    // before anything opens a file in it.
    if let Some((old, new)) = setup::move_if_pending() {
        applog::info(&format!(
            "moved the data from {} to {}",
            old.display(),
            new.display()
        ));
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
        applog::info(&format!("starting again: {}", resume.join(", ")));
        let engine = engine.clone();
        std::thread::spawn(move || {
            for id in resume {
                if let Err(e) = engine.start(&id) {
                    crate::applog::error(&format!("could not start {id} again: {e}"));
                }
            }
        });
    }
    perf::mark("engine");
    applog::info("services ready; opening the window");
    wordpress::count_restarts();
    let app = tauri::Builder::default()
        .manage(engine.clone())
        .manage(Installs::default())
        .manage(Control(Mutex::new(None)))
        .manage(updates::Updates::new(just_updated))
        .manage(remote::RemoteState::default())
        .manage(ai::AiInstalls::default())
        .manage(console::Consoles::default())
        .manage(wordpress::CloneState::default())
        .manage(wordpress::SyncState::default())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .on_page_load(|webview, payload| {
            if payload.event() == tauri::webview::PageLoadEvent::Finished {
                applog::info(&format!("window page loaded ({})", webview.label()));
                perf::page_loaded(webview);
            }
        })
        .on_window_event(|window, event| match event {
            tauri::WindowEvent::Focused(true) => updates::on_focus(window.app_handle()),
            tauri::WindowEvent::Destroyed => {
                applog::info(&format!("window {} closed", window.label()));
                release_memory()
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            console::console_open,
            console::console_write,
            console::console_resize,
            console::console_close,
            sitepics::site_thumbnail,
            sitepics::wp_theme_screenshot,
            sitepics::site_database,
            sitepics::site_size,
            sitepics::site_sizes,
            sitepics::measure_site_size,
            sitepics::start_site_database,
            sitepics::set_site_database_autostart,
            wordpress::wp_overview,
            wordpress::wp_disk_use,
            wordpress::wp_core_update,
            wordpress::wp_update_core,
            wordpress::wp_extensions,
            wordpress::wp_change_extension,
            wordpress::wp_install_extension,
            wordpress::wp_users,
            wordpress::wp_add_admin,
            wordpress::wp_set_password,
            wordpress::wp_login_as,
            wordpress::wp_set_debug,
            wordpress::wp_log,
            wordpress::wp_clear_log,
            wordpress::wp_search_replace,
            wordpress::wp_export_database,
            wordpress::wp_import_database,
            wordpress::wp_optimize_database,
            wordpress::wp_clear_transients,
            wordpress::wp_change_prefix,
            wordpress::wp_open_phpmyadmin,
            wordpress::wp_woo,
            wordpress::wp_woo_run_actions,
            wordpress::wp_woo_test_mode,
            wordpress::wp_woo_create,
            wordpress::wp_saved_connections,
            wordpress::wp_clone_inspect,
            wordpress::wp_clone,
            wordpress::wp_clone_check,
            wordpress::wp_clone_stop,
            wordpress::wp_clone_pause,
            wordpress::wp_clone_forget,
            wordpress::wp_clone_unfinished,
            wordpress::wp_clones_saved,
            wordpress::wp_environment,
            wordpress::wp_stagings,
            wordpress::wp_staging_create,
            wordpress::wp_staging_stop,
            wordpress::wp_staging_destroy,
            wordpress::wp_trash,
            wordpress::wp_trash_restore,
            wordpress::wp_trash_delete,
            wordpress::wp_snapshots,
            wordpress::wp_snapshot_restore,
            wordpress::wp_snapshot_remove,
            wordpress::wp_merge_changes,
            wordpress::wp_merge,
            wordpress::wp_posts_list,
            wordpress::wp_posts_plan,
            wordpress::wp_posts_move,
            wordpress::wp_sync_changes,
            wordpress::wp_push,
            wordpress::wp_pull,
            wordpress::wp_install,
            wordpress::wp_sync_stop,
            wordpress::wp_live_backups,
            wordpress::wp_live_backup_restore,
            wordpress::wp_live_backup_remove,
            wordpress::wp_live_read,
            wordpress::wp_live_read_at,
            wordpress::wp_live_posts,
            wordpress::wp_live_posts_plan,
            wordpress::wp_live_posts_move,
            sitetools::site_settings,
            sitetools::save_site_settings,
            sitetools::site_backups,
            sitetools::backup_site,
            sitetools::restore_site,
            sitetools::remove_site_backup,
            sitetools::open_site_backups,
            sitetools::open_site_folder,
            sitetools::change_site_folder,
            sitetools::move_site,
            sitetools::code_editors,
            sitetools::set_code_editor,
            sitetools::open_site_in_editor,
            sitetools::export_site,
            sitetools::inspect_site_file,
            sitetools::import_site,
            sitetools::share_status,
            sitetools::share_install,
            sitetools::share_uninstall,
            sitetools::share_start,
            sitetools::share_stop,
            sitetools::open_share_link,
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
            videos_status,
            videos_install_ffmpeg,
            videos_remove_ffmpeg,
            find_videos,
            optimize_videos,
            stop_videos,
            add_starter_site,
            add_project,
            set_project_kind,
            set_site_category,
            list_modules,
            set_module,
            service_states,
            get_tld,
            set_tld,
            language_files,
            open_languages_folder,
            interface_settings,
            save_interface_settings,
            keybindings,
            save_keybindings,
            show_interface_file,
            theme_files,
            import_theme,
            remove_theme,
            open_themes_folder,
            list_sites,
            site_layout,
            set_site_layout,
            set_site_version,
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
            clear_log,
            setup_status,
            phpmyadmin_status,
            phpmyadmin_install,
            phpmyadmin_uninstall,
            phpmyadmin_open,
            setup_choose,
            setup_apply,
            setup_finish,
            sites_folder,
            set_sites_folder,
            db_server,
            db_servers,
            db_list,
            db_create,
            db_drop,
            db_tables,
            db_browse,
            db_query,
            db_backup,
            db_backups,
            open_db_backups,
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
            seen_whats_new,
            ai::ai_tools,
            ai::ai_tools_check,
            ai::ai_tool_install,
            ai::ai_tool_remove,
            ai::ai_tool_open,
            ai::ai_tool_docs,
            remote::remote_connections,
            remote::remote_save,
            remote::remote_remove,
            remote::remote_trust,
            remote::remote_test,
            remote::remote_check,
            remote::remote_browse,
            remote::remote_plan,
            remote::remote_run,
            remote::remote_stop,
            remote::remote_local_folder,
            remote::git_status,
            remote::git_link,
            remote::git_fetch,
            remote::git_pull,
            remote::git_push,
            remote::git_commit,
            tray::take_tray_action,
            applog::app_log,
            applog::app_log_open,
            applog::app_log_path
        ])
        .setup(move |app| {
            perf::mark("setup");
            app.manage(tray::build(app.handle()));
            tray::follow(app.handle());
            // Quit cleanly, stopping services, when the system asks (logout, `kill`).
            let handle = app.handle().clone();
            let _ = ctrlc::set_handler(move || handle.exit(0));
            let handle = app.handle().clone();
            let server = ControlServer::start(&paths::home(), Owner::App, move |request| {
                answer_cli(&handle, request)
            });
            match server {
                Ok(server) => *app.state::<Control>().0.lock().unwrap() = Some(server),
                Err(e) => crate::applog::error(&format!(
                    "the stackyard command cannot reach this app: {e}"
                )),
            }
            updates::watch(app.handle());
            let handle = app.handle().clone();
            Engine::watch_parked(&app.state::<Arc<Engine>>(), &paths::home(), move || {
                let _ = handle.emit("sites-changed", ());
                tray::changed(&handle);
            });
            perf::mark("setup_done");
            applog::info("tray and control connection ready");
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building Stackyard");
    perf::mark("built");
    app.run(|handle, event| match event {
        // Closing the window drops it (and its web view) to save memory;
        // Stackyard keeps running in the tray until Quit.
        RunEvent::ExitRequested {
            api, code: None, ..
        } if handle.state::<tray::Tray>().up => api.prevent_exit(),
        // macOS: clicking the Dock icon with no window open.
        #[cfg(target_os = "macos")]
        RunEvent::Reopen { .. } => show_window(handle),
        // Never leave services running after Stackyard quits.
        RunEvent::Exit => {
            applog::info("quitting");
            wordpress::set_quitting(true);
            handle.state::<Control>().0.lock().unwrap().take();
            handle.state::<Arc<Engine>>().shutdown();
        }
        _ => {}
    });
}

/// Hands memory the closed window freed back to the system. glibc keeps
/// freed memory for reuse, which would leave the tray-only app holding most
/// of what the web view used.
fn release_memory() {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    std::thread::spawn(|| {
        // The web view lets go of some of it a moment after the window goes.
        for wait in [0, 3] {
            std::thread::sleep(std::time::Duration::from_secs(wait));
            // SAFETY: malloc_trim only returns unused heap pages to the system.
            unsafe {
                libc::malloc_trim(0);
            }
        }
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
            tray::changed(app);
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
        Err(e) => crate::applog::error(&format!("could not open the window: {e}")),
    }
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
        assert_eq!(
            parse("Python 3.13").unwrap(),
            SiteRuntime::Python {
                version: "3.13".into(),
                port: 8000
            }
        );
        assert_eq!(parse("Go 1.26").unwrap().kind(), Some(RuntimeKind::Go));
        assert_eq!(parse("Java 21").unwrap().version(), Some("21"));
        assert_eq!(parse("Static").unwrap(), SiteRuntime::Static);
        // An empty folder holds plain files.
        assert_eq!(parse("Auto").unwrap(), SiteRuntime::Static);
        assert!(parse("Ruby 3").is_err());
    }

    #[test]
    fn empty_groups_keep_their_site_list() {
        let json = serde_json::to_value(vec![
            LayoutItem::from(siteorder::Item::group("New group", &[])),
            LayoutItem::from(siteorder::Item::site("shop.stack")),
        ])
        .unwrap();
        assert_eq!(
            json,
            serde_json::json!([
                { "group": "New group", "sites": [], "collapsed": false },
                { "site": "shop.stack" }
            ])
        );
    }
}
