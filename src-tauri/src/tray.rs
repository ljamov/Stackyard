//! The tray icon by the clock and its menu: open the window, start and stop
//! services one by one or all at once, open sites, make a new site, open a
//! terminal and jump to a screen.
//!
//! Nothing here polls. The supervisor says when a service starts, stops or
//! crashes, and the commands that add sites or install services say so too;
//! a worker thread then waits a moment for more changes and updates the
//! menu once. When only states changed, the items are updated in place;
//! the menu is rebuilt only when what it lists changed.

use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use stackyard_core::ai::terminal;
use stackyard_core::catalog::SERVICES;
use stackyard_core::engine::Engine;
use stackyard_core::settings::Settings;
use stackyard_core::supervisor::ProcessState;
use stackyard_core::{paths, phpmyadmin};
use tauri::menu::{CheckMenuItem, IsMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{AppHandle, Emitter, Manager, Runtime, Wry};
use tauri_plugin_notification::NotificationExt;

/// Sites listed in the menu; the rest are on the Sites screen.
const MAX_SITES: usize = 20;
/// How long to wait for more changes before updating the menu, so starting
/// everything updates it once rather than once per process.
const SETTLE: Duration = Duration::from_millis(150);

/// Screens the Go to submenu opens, with the module each needs.
const SCREENS: &[(&str, Option<&str>)] = &[
    ("Dashboard", None),
    ("Sites", None),
    ("Services", None),
    ("Databases", Some("databases")),
    ("Runtimes", None),
    ("Settings", None),
];

pub(crate) struct Tray {
    /// Whether the icon is up; without one, closing the window quits.
    pub up: bool,
    update: Mutex<Option<String>>,
    kick: SyncSender<()>,
    wake: Mutex<Option<Receiver<()>>>,
    shown: Mutex<Option<Shown>>,
    /// What the window should do when it opens, for New site and Go to.
    action: Mutex<Option<String>>,
}

/// The menu on show and the items that change with service states.
struct Shown {
    model: Model,
    services: Submenu<Wry>,
    items: Vec<CheckMenuItem<Wry>>,
    start_all: MenuItem<Wry>,
    stop_all: MenuItem<Wry>,
}

#[derive(Clone, PartialEq)]
struct Model {
    update: Option<String>,
    services: Vec<ServiceItem>,
    /// Name and domain of each site listed.
    sites: Vec<(String, String)>,
    more_sites: bool,
    phpmyadmin: bool,
    terminal: bool,
    screens: Vec<&'static str>,
}

#[derive(Clone, PartialEq)]
struct ServiceItem {
    id: &'static str,
    name: &'static str,
    state: ProcessState,
}

impl ServiceItem {
    fn running(&self) -> bool {
        matches!(
            self.state,
            ProcessState::Running { .. } | ProcessState::Restarting { .. }
        )
    }

    fn label(&self) -> String {
        match self.state {
            ProcessState::Restarting { .. } => format!("{} (restarting)", self.name),
            ProcessState::Crashed { .. } => format!("{} (stopped by a problem)", self.name),
            _ => self.name.to_string(),
        }
    }
}

impl Model {
    fn read(app: &AppHandle, update: Option<String>) -> Self {
        let engine = app.state::<Arc<Engine>>();
        let home = paths::home();
        let settings = Settings::load().unwrap_or_default();
        let services = SERVICES
            .iter()
            .filter(|def| settings.module_enabled(def.id) && engine.manager().get(def.id).is_some())
            .map(|def| ServiceItem {
                id: def.id,
                name: def.name,
                state: engine.state(def.id),
            })
            .collect();
        // In the order the Sites screen shows them.
        let mut sites: Vec<(String, String)> = crate::site_views()
            .unwrap_or_default()
            .into_iter()
            .map(|s| (s.name, s.domain))
            .collect();
        let more_sites = sites.len() > MAX_SITES;
        sites.truncate(MAX_SITES);
        Self {
            update,
            services,
            sites,
            more_sites,
            phpmyadmin: settings.module_enabled(phpmyadmin::MODULE)
                && phpmyadmin::installed(&home).is_some(),
            terminal: settings.module_enabled("shell") && crate::cli_program().is_some(),
            screens: SCREENS
                .iter()
                .filter(|(_, module)| module.is_none_or(|m| settings.module_enabled(m)))
                .map(|(screen, _)| *screen)
                .collect(),
        }
    }

    /// Whether `other` lists the same things, so only states differ.
    fn same_items(&self, other: &Self) -> bool {
        let ids = |m: &Self| m.services.iter().map(|s| s.id).collect::<Vec<_>>();
        Self {
            services: Vec::new(),
            ..self.clone()
        } == Self {
            services: Vec::new(),
            ..other.clone()
        } && ids(self) == ids(other)
    }

    fn running(&self) -> usize {
        self.services.iter().filter(|s| s.running()).count()
    }

    fn services_title(&self) -> String {
        match (self.running(), self.services.len()) {
            (_, 0) => "Services".into(),
            (running, all) => format!("Services ({running} of {all} running)"),
        }
    }

    fn tooltip(&self) -> String {
        if let Some(version) = &self.update {
            return format!("Stackyard: {version} is available");
        }
        match self.running() {
            0 => "Stackyard: no services running".into(),
            1 => "Stackyard: 1 service running".into(),
            n => format!("Stackyard: {n} services running"),
        }
    }
}

/// Puts the icon up; `Tray::up` says whether that worked. [`follow`]
/// starts filling in the menu once the app holds the result.
pub(crate) fn build(app: &AppHandle) -> Tray {
    let (kick, wake) = sync_channel(1);
    let up = match build_icon(app) {
        Ok(()) => true,
        Err(e) => {
            eprintln!("no tray icon: {e}");
            false
        }
    };
    Tray {
        up,
        update: Mutex::new(None),
        kick,
        wake: Mutex::new(up.then_some(wake)),
        shown: Mutex::new(None),
        action: Mutex::new(None),
    }
}

/// Keeps the menu up to date from now on, on a thread that sleeps until
/// something changes.
pub(crate) fn follow(app: &AppHandle) {
    let tray = app.state::<Tray>();
    let Some(wake) = tray.wake.lock().unwrap().take() else {
        return;
    };
    // Any start, stop or crash, whoever caused it.
    let changes = tray.kick.clone();
    app.state::<Arc<Engine>>().supervisor().on_change(move || {
        let _ = changes.try_send(());
    });
    let handle = app.clone();
    let started = std::thread::Builder::new()
        .name("stackyard-tray".into())
        .spawn(move || keep_up(handle, wake));
    match started {
        Ok(_) => {
            let _ = tray.kick.try_send(());
        }
        Err(e) => eprintln!("the tray menu will not follow service changes: {e}"),
    }
}

/// Something the menu lists may have changed: sites, installs, modules.
pub(crate) fn changed<R: Runtime>(app: &AppHandle<R>) {
    if let Some(tray) = app.try_state::<Tray>() {
        let _ = tray.kick.try_send(());
    }
}

/// Shows or removes the update item.
pub(crate) fn set_update<R: Runtime>(app: &AppHandle<R>, version: Option<&str>) {
    if let Some(tray) = app.try_state::<Tray>() {
        *tray.update.lock().unwrap() = version.map(str::to_string);
        let _ = tray.kick.try_send(());
    }
}

/// What the window was asked to do from the tray, once.
#[tauri::command]
pub(crate) fn take_tray_action(tray: tauri::State<'_, Tray>) -> Option<String> {
    tray.action.lock().unwrap().take()
}

/// Waits for changes and brings the menu up to date; sleeps otherwise.
fn keep_up(app: AppHandle, wake: Receiver<()>) {
    while wake.recv().is_ok() {
        std::thread::sleep(SETTLE);
        while wake.try_recv().is_ok() {}
        let tray = app.state::<Tray>();
        let model = Model::read(&app, tray.update.lock().unwrap().clone());
        let mut shown = tray.shown.lock().unwrap();
        match shown.as_ref() {
            // Also when nothing changed: a click ticks an item by itself,
            // and a start that failed has to take the tick away again.
            Some(old) if old.model.same_items(&model) => {
                if let Err(e) = update_states(old, &model) {
                    eprintln!("could not update the tray menu: {e}");
                }
                if let Some(old) = shown.as_mut() {
                    old.model = model;
                }
            }
            _ => match show(&app, model) {
                Ok(new) => *shown = Some(new),
                Err(e) => eprintln!("could not update the tray menu: {e}"),
            },
        }
        if let (Some(icon), Some(shown)) = (app.tray_by_id("main"), shown.as_ref()) {
            let _ = icon.set_tooltip(Some(shown.model.tooltip()));
        }
    }
}

fn update_states(shown: &Shown, model: &Model) -> tauri::Result<()> {
    for (item, service) in shown.items.iter().zip(&model.services) {
        item.set_checked(service.running())?;
        item.set_text(service.label())?;
    }
    shown.services.set_text(model.services_title())?;
    shown
        .start_all
        .set_enabled(model.running() < model.services.len())?;
    shown.stop_all.set_enabled(model.running() > 0)?;
    Ok(())
}

/// Builds the whole menu and puts it on the icon.
fn show(app: &AppHandle, model: Model) -> tauri::Result<Shown> {
    let item = |id: &str, text: &str| MenuItem::with_id(app, id, text, true, None::<&str>);
    let gap = || PredefinedMenuItem::separator(app);

    let open = item("open", "Open Stackyard")?;
    let update = model
        .update
        .as_ref()
        .map(|version| item("update", &format!("Update to {version}…")))
        .transpose()?;

    let start_all = MenuItem::with_id(
        app,
        "start-all",
        "Start all services",
        model.running() < model.services.len(),
        None::<&str>,
    )?;
    let stop_all = MenuItem::with_id(
        app,
        "stop-all",
        "Stop all services",
        model.running() > 0,
        None::<&str>,
    )?;
    let items = model
        .services
        .iter()
        .map(|s| {
            CheckMenuItem::with_id(
                app,
                format!("service:{}", s.id),
                s.label(),
                true,
                s.running(),
                None::<&str>,
            )
        })
        .collect::<tauri::Result<Vec<_>>>()?;
    let none_installed =
        MenuItem::with_id(app, "none", "No services installed", false, None::<&str>)?;
    let manage = item("screen:Services", "Manage services…")?;
    let services = Submenu::with_id(app, "services", model.services_title(), true)?;
    if items.is_empty() {
        services.append(&none_installed)?;
    }
    for service in &items {
        services.append(service)?;
    }
    services.append_items(&[&gap()?, &manage])?;

    let sites = Submenu::with_id(app, "sites", "Open site", true)?;
    for (name, domain) in &model.sites {
        sites.append(&MenuItem::with_id(
            app,
            format!("site:{domain}"),
            format!("{name}  ({domain})"),
            true,
            None::<&str>,
        )?)?;
    }
    if model.sites.is_empty() {
        sites.append(&MenuItem::with_id(
            app,
            "no-sites",
            "No sites yet",
            false,
            None::<&str>,
        )?)?;
    }
    let all_sites = item(
        "screen:Sites",
        if model.more_sites {
            "All sites…"
        } else {
            "Manage sites…"
        },
    )?;
    sites.append_items(&[&gap()?, &all_sites])?;

    let new_site = item("new-site", "New site or app…")?;
    let folder = item("sites-folder", "Open sites folder")?;
    let terminal = model
        .terminal
        .then(|| item("terminal", "Open terminal"))
        .transpose()?;
    let phpmyadmin = model
        .phpmyadmin
        .then(|| item("phpmyadmin", "Open phpMyAdmin"))
        .transpose()?;

    let go = Submenu::with_id(app, "go", "Go to", true)?;
    for screen in &model.screens {
        go.append(&item(&format!("screen:{screen}"), screen)?)?;
    }
    let quit = item("quit", "Quit Stackyard")?;

    let (gap1, gap2, gap3, gap4) = (gap()?, gap()?, gap()?, gap()?);
    let mut all: Vec<&dyn IsMenuItem<Wry>> = vec![&open];
    if let Some(update) = &update {
        all.push(update);
    }
    all.extend([
        &gap1 as &dyn IsMenuItem<Wry>,
        &start_all,
        &stop_all,
        &services,
        &gap2,
        &sites,
        &new_site,
        &folder,
    ]);
    if let Some(terminal) = &terminal {
        all.push(terminal);
    }
    if let Some(phpmyadmin) = &phpmyadmin {
        all.push(phpmyadmin);
    }
    all.extend([&gap3 as &dyn IsMenuItem<Wry>, &go, &gap4, &quit]);
    let menu = Menu::with_items(app, &all)?;
    if let Some(icon) = app.tray_by_id("main") {
        icon.set_menu(Some(menu))?;
    }
    Ok(Shown {
        model,
        services,
        items,
        start_all,
        stop_all,
    })
}

fn build_icon(app: &AppHandle) -> tauri::Result<()> {
    use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

    // A first menu at once; the worker fills in services and sites.
    let open = MenuItem::with_id(app, "open", "Open Stackyard", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Stackyard", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &PredefinedMenuItem::separator(app)?, &quit])?;
    let mut tray = TrayIconBuilder::with_id("main")
        .tooltip("Stackyard")
        .menu(&menu)
        // macOS shows the menu on a click; elsewhere a click opens the window.
        .show_menu_on_left_click(cfg!(target_os = "macos"))
        .on_menu_event(|app, event| on_menu(app, event.id.as_ref()))
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                if !cfg!(target_os = "macos") {
                    crate::show_window(tray.app_handle());
                }
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

/// Runs on the main thread, so anything slow goes to a worker.
fn on_menu(app: &AppHandle, id: &str) {
    let engine = || app.state::<Arc<Engine>>().inner().clone();
    match id {
        // The window's notice has the update and its Update button.
        "open" | "update" => crate::show_window(app),
        "start-all" => {
            let (engine, app) = (engine(), app.clone());
            std::thread::spawn(move || {
                let (_, problems) = engine.start_all();
                if !problems.is_empty() {
                    tell(&app, "Some services did not start", &problems.join("\n"));
                }
            });
        }
        // Stopping waits for each process to finish.
        "stop-all" => {
            let engine = engine();
            std::thread::spawn(move || engine.stop_all());
        }
        "new-site" => open_at(app, "new-site"),
        "sites-folder" => {
            let app = app.clone();
            std::thread::spawn(move || {
                let folder = Settings::load()
                    .map(|s| s.sites_folder(&paths::home()))
                    .map_err(crate::err)
                    .and_then(|folder| {
                        std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
                        crate::sitetools::open_with_system(folder.as_os_str())
                    });
                if let Err(e) = folder {
                    tell(&app, "Could not open the sites folder", &e);
                }
            });
        }
        "terminal" => {
            let app = app.clone();
            std::thread::spawn(move || {
                if let Err(e) = open_terminal() {
                    tell(&app, "Could not open a terminal", &e);
                }
            });
        }
        "phpmyadmin" => {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = crate::open_phpmyadmin(app.clone(), None).await {
                    tell(&app, "Could not open phpMyAdmin", &e);
                }
            });
        }
        "quit" => app.exit(0),
        id => {
            if let Some(service) = id.strip_prefix("service:") {
                toggle(app, engine(), service.to_string());
            } else if let Some(domain) = id.strip_prefix("site:") {
                open_site(app, domain.to_string());
            } else if let Some(screen) = id.strip_prefix("screen:") {
                open_at(app, &format!("screen:{screen}"));
            }
        }
    }
}

/// Starts a stopped service, or stops a running one.
fn toggle(app: &AppHandle, engine: Arc<Engine>, id: String) {
    let app = app.clone();
    std::thread::spawn(move || {
        let running = matches!(
            engine.state(&id),
            ProcessState::Running { .. } | ProcessState::Restarting { .. }
        );
        let done = if running {
            engine.stop(&id)
        } else {
            engine.start(&id)
        };
        if let Err(e) = done {
            let name = SERVICES
                .iter()
                .find(|d| d.id == id)
                .map_or(id.as_str(), |d| d.name);
            let what = if running { "stop" } else { "start" };
            tell(&app, &format!("Could not {what} {name}"), &e.to_string());
        }
        // A failed start changes no state, so the tick would stay wrong.
        changed(&app);
    });
}

/// Opens a site in the browser, starting the web server first if needed.
fn open_site(app: &AppHandle, domain: String) {
    let (app, engine) = (app.clone(), app.state::<Arc<Engine>>().inner().clone());
    std::thread::spawn(move || {
        let opened = (|| {
            let site = crate::site_views()?
                .into_iter()
                .find(|s| s.domain == domain)
                .ok_or_else(|| format!("{domain} is no longer a site"))?;
            if !matches!(engine.state("caddy"), ProcessState::Running { .. })
                && engine.manager().get("caddy").is_some()
            {
                engine.start("caddy").map_err(crate::err)?;
            }
            crate::open_page(site.url)
        })();
        if let Err(e) = opened {
            tell(&app, &format!("Could not open {domain}"), &e);
        }
    });
}

/// Opens the window and has it show a screen or the New site form. The
/// window takes the action when it loads, or now if it is open already.
fn open_at(app: &AppHandle, action: &str) {
    *app.state::<Tray>().action.lock().unwrap() = Some(action.to_string());
    crate::show_window(app);
    let _ = app.emit("tray-action", ());
}

/// A terminal window in the sites folder running Stackyard Shell, where
/// `php`, `node` and the rest use each project's versions.
fn open_terminal() -> Result<(), String> {
    let cli =
        crate::cli_program().ok_or("the stackyard command-line program is not next to the app")?;
    let folder = Settings::load()
        .map_err(crate::err)?
        .sites_folder(&paths::home());
    std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    terminal::open(&terminal::Window {
        title: "Stackyard Shell".into(),
        folder,
        program: cli,
        args: vec!["shell".into()],
        path: std::env::var_os("PATH").unwrap_or_default(),
        path_first: Vec::new(),
    })
    .map_err(crate::err)
}

/// Says what went wrong, since a tray click has no window to show it in.
fn tell(app: &AppHandle, title: &str, body: &str) {
    eprintln!("{title}: {body}");
    let shown = app.notification().builder().title(title).body(body).show();
    if let Err(e) = shown {
        eprintln!("could not show a notification: {e}");
    }
}
