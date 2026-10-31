//! Commands for the WordPress tab of a site's tools. Each runs on a worker
//! thread: WP-CLI and database dumps can take a while.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use stackyard_core::engine::Engine;
use stackyard_core::paths;
use stackyard_core::remote::{self, Connection, ConnectionInput, RemoteStore, Sessions};
use stackyard_core::settings::Settings;
use stackyard_core::sites::Site;
use stackyard_core::wordpress::{
    self, connect, env, live, merge, posts, repair, snapshot, staging, sync, woo, CoreUpdate,
    DebugSettings, DiskUse, Extension, ExtensionAction, Kind, LogTail, Overview, Replaced, User,
    WpSite,
};

use tauri::{AppHandle, Emitter, Manager, State};

use crate::{err, sitetools::find_site, CmdResult};

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> CmdResult<T> + Send + 'static,
) -> CmdResult<T> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|e| e.to_string())?
}

/// The site, checked to be WordPress, with the module on.
fn open(domain: &str) -> CmdResult<WpSite> {
    Settings::load()
        .and_then(|s| s.require_module("wordpress"))
        .map_err(err)?;
    let (site, _) = find_site(domain)?;
    WpSite::open(&site).map_err(err)
}

/// Runs `work` with the site on a worker thread.
async fn with_site<T: Send + 'static>(
    domain: String,
    work: impl FnOnce(&std::path::Path, &WpSite) -> stackyard_core::Result<T> + Send + 'static,
) -> CmdResult<T> {
    blocking(move || {
        let wp = open(&domain)?;
        work(&paths::home(), &wp).map_err(err)
    })
    .await
}

#[tauri::command]
pub async fn wp_overview(domain: String) -> CmdResult<Overview> {
    with_site(domain, |home, wp| Ok(wordpress::overview(home, wp))).await
}

#[tauri::command]
pub async fn wp_disk_use(domain: String) -> CmdResult<DiskUse> {
    with_site(domain, |home, wp| Ok(wordpress::disk_use(home, wp))).await
}

#[tauri::command]
pub async fn wp_core_update(domain: String) -> CmdResult<CoreUpdate> {
    with_site(domain, wordpress::core_update).await
}

#[tauri::command]
pub async fn wp_update_core(domain: String) -> CmdResult<()> {
    with_site(domain, wordpress::update_core).await
}

#[tauri::command]
pub async fn wp_extensions(
    domain: String,
    kind: Kind,
    check_updates: bool,
) -> CmdResult<Vec<Extension>> {
    with_site(domain, move |home, wp| {
        wordpress::extensions(home, wp, kind, check_updates)
    })
    .await
}

#[tauri::command]
pub async fn wp_change_extension(
    domain: String,
    kind: Kind,
    name: String,
    action: ExtensionAction,
) -> CmdResult<Vec<Extension>> {
    with_site(domain, move |home, wp| {
        wordpress::change_extension(home, wp, kind, &name, action)?;
        wordpress::extensions(home, wp, kind, false)
    })
    .await
}

#[tauri::command]
pub async fn wp_install_extension(
    domain: String,
    kind: Kind,
    slug: String,
    activate: bool,
) -> CmdResult<Vec<Extension>> {
    with_site(domain, move |home, wp| {
        wordpress::install_extension(home, wp, kind, &slug, activate)?;
        wordpress::extensions(home, wp, kind, false)
    })
    .await
}

#[tauri::command]
pub async fn wp_users(domain: String) -> CmdResult<Vec<User>> {
    with_site(domain, wordpress::users).await
}

#[tauri::command]
pub async fn wp_add_admin(
    domain: String,
    login: String,
    email: String,
    password: String,
) -> CmdResult<Vec<User>> {
    with_site(domain, move |home, wp| {
        wordpress::add_admin(home, wp, &login, &email, &password)?;
        wordpress::users(home, wp)
    })
    .await
}

#[tauri::command]
pub async fn wp_set_password(domain: String, user: u64, password: String) -> CmdResult<()> {
    with_site(domain, move |home, wp| {
        wordpress::set_password(home, wp, user, &password)
    })
    .await
}

/// Makes a one-time login link and opens it in the browser.
#[tauri::command]
pub async fn wp_login_as(domain: String, user: u64) -> CmdResult<()> {
    let url = with_site(domain, move |home, wp| {
        wordpress::login_link(home, wp, user)
    })
    .await?;
    crate::open_page(url)
}

#[tauri::command]
pub async fn wp_set_debug(domain: String, settings: DebugSettings) -> CmdResult<Overview> {
    with_site(domain.clone(), move |_, wp| {
        wordpress::set_debug(wp, settings)
    })
    .await?;
    wp_overview(domain).await
}

#[tauri::command]
pub async fn wp_log(domain: String) -> CmdResult<LogTail> {
    with_site(domain, |_, wp| wordpress::read_log(wp, 256 * 1024)).await
}

#[tauri::command]
pub async fn wp_clear_log(domain: String) -> CmdResult<()> {
    with_site(domain, |_, wp| wordpress::clear_log(wp)).await
}

#[tauri::command]
pub async fn wp_search_replace(
    domain: String,
    from: String,
    to: String,
    addresses: bool,
    dry_run: bool,
) -> CmdResult<Replaced> {
    with_site(domain, move |home, wp| {
        wordpress::search_replace(home, wp, &from, &to, addresses, dry_run)
    })
    .await
}

#[tauri::command]
pub async fn wp_export_database(domain: String, path: PathBuf) -> CmdResult<()> {
    with_site(domain, move |home, wp| {
        wordpress::export_database(home, wp, &path)
    })
    .await
}

/// Loads a SQL file; returns the backup taken first.
#[tauri::command]
pub async fn wp_import_database(domain: String, path: PathBuf) -> CmdResult<String> {
    with_site(domain, move |home, wp| {
        wordpress::import_database(home, wp, &path)
    })
    .await
}

#[derive(Serialize)]
pub struct PrefixChanged {
    overview: Overview,
    backup: String,
}

/// Renames the tables to a new prefix; returns the backup taken first.
#[tauri::command]
pub async fn wp_change_prefix(domain: String, prefix: String) -> CmdResult<PrefixChanged> {
    let backup = with_site(domain.clone(), move |home, wp| {
        wordpress::change_prefix(home, wp, &prefix)
    })
    .await?;
    Ok(PrefixChanged {
        overview: wp_overview(domain).await?,
        backup,
    })
}

#[tauri::command]
pub async fn wp_optimize_database(domain: String) -> CmdResult<usize> {
    with_site(domain, wordpress::optimize_database).await
}

#[tauri::command]
pub async fn wp_clear_transients(domain: String) -> CmdResult<u64> {
    with_site(domain, wordpress::clear_transients).await
}

/// Looks for problems in the site's database and wp-config.php.
#[tauri::command]
pub async fn wp_check_database(domain: String) -> CmdResult<repair::Report> {
    blocking(move || {
        let (site, _) = find_site(&domain)?;
        repair::check(&paths::home(), &site).map_err(err)
    })
    .await
}

/// Fixes what [`wp_check_database`] found, after backing up.
#[tauri::command]
pub async fn wp_repair_database(domain: String) -> CmdResult<repair::Report> {
    blocking(move || {
        let (site, _) = find_site(&domain)?;
        repair::repair(&paths::home(), &site).map_err(err)
    })
    .await
}

/// The site's database in phpMyAdmin.
#[tauri::command]
pub async fn wp_open_phpmyadmin(app: tauri::AppHandle, domain: String) -> CmdResult<()> {
    let database = with_site(domain, |_, wp| wp.database()).await?;
    crate::open_phpmyadmin(app, Some(database)).await
}

#[derive(Serialize)]
pub struct WooView {
    status: woo::Status,
    gateways: Vec<woo::Gateway>,
}

#[tauri::command]
pub async fn wp_woo(domain: String) -> CmdResult<WooView> {
    with_site(domain, |home, wp| {
        Ok(WooView {
            status: woo::status(home, wp)?,
            gateways: woo::gateways(home, wp)?,
        })
    })
    .await
}

#[tauri::command]
pub async fn wp_woo_run_actions(domain: String) -> CmdResult<String> {
    with_site(domain, woo::run_due_actions).await
}

#[tauri::command]
pub async fn wp_woo_test_mode(domain: String, gateway: String, on: bool) -> CmdResult<WooView> {
    with_site(domain.clone(), move |home, wp| {
        woo::set_test_mode(home, wp, &gateway, on)
    })
    .await?;
    wp_woo(domain).await
}

/// Makes test customers (`kind` `customers`) or orders; returns what was made.
#[tauri::command]
pub async fn wp_woo_create(domain: String, kind: String, count: u32) -> CmdResult<Vec<String>> {
    with_site(domain, move |home, wp| match kind.as_str() {
        "customers" => woo::create_customers(home, wp, count),
        _ => woo::create_orders(home, wp, count),
    })
    .await
}

// ---------------------------------------------------------------------------
// Clone from a live server

/// Clones from live servers, any number at once, each named by the id of
/// the form that started it; and the staging being made.
#[derive(Default)]
pub struct CloneState {
    sessions: Sessions,
    /// Connections typed in, logged in but not saved yet, by clone.
    drafts: Mutex<HashMap<String, Connection>>,
    /// The clones running now, by clone.
    running: Mutex<HashMap<String, Running>>,
    /// The Stop switches of the forms looking for WordPress now, by clone.
    inspecting: Mutex<HashMap<String, Arc<AtomicBool>>>,
    staging_stop: Arc<AtomicBool>,
    staging_busy: AtomicBool,
}

/// A clone running now: its site name, its Stop switch, and whether it
/// was stopped to pause it.
struct Running {
    name: String,
    stop: Arc<AtomicBool>,
    pause: Arc<AtomicBool>,
}

/// Two clones can finish at once; the database and web server are started
/// by one at a time.
static STARTING: Mutex<()> = Mutex::new(());

fn ensure_database(engine: &Engine) -> CmdResult<()> {
    let _one = STARTING.lock().unwrap_or_else(|e| e.into_inner());
    crate::ensure_database(engine)
}

fn ensure_web(engine: &Engine) -> CmdResult<()> {
    let _one = STARTING.lock().unwrap_or_else(|e| e.into_inner());
    crate::ensure_web(engine)
}

/// Where to clone from, as the New site form fills it in.
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum SourceInput {
    /// A connection saved for another site.
    Saved { id: String },
    /// A connection typed in now.
    New { connection: Box<ConnectionInput> },
    /// Backup files on this computer.
    Files { files: Vec<String> },
    /// A Stackyard Connect code from the live site. Saved clones keep it
    /// in the keychain, not in `clones.json`: it copies the whole site.
    Code { code: String },
}

#[derive(Serialize)]
pub struct SavedConnection {
    id: String,
    name: String,
    site: String,
    host: String,
    user: String,
    protocol: remote::Protocol,
}

#[derive(Clone, Serialize)]
struct CloneEvent {
    /// The clone this is about; empty for a staging.
    job: String,
    name: String,
    #[serde(flatten)]
    progress: live::Progress,
}

fn require_clone() -> CmdResult<()> {
    let settings = Settings::load().map_err(err)?;
    settings.require_module("wordpress").map_err(err)?;
    settings.require_module("remote").map_err(err)
}

fn source(
    state: &CloneState,
    job: &str,
    input: SourceInput,
    inspecting: bool,
) -> CmdResult<live::Source> {
    match input {
        SourceInput::Saved { id } => {
            let connection = RemoteStore::load()
                .and_then(|s| s.get(&id).cloned())
                .map_err(err)?;
            Ok(live::Source::Server(Box::new(connection)))
        }
        SourceInput::New { mut connection } => {
            let draft = state.drafts.lock().unwrap().get(job).cloned();
            match draft {
                Some(draft) if !inspecting => Ok(live::Source::Server(Box::new(draft))),
                _ => {
                    // After a restart the password comes from the keychain.
                    let password = connection
                        .password
                        .take()
                        .or_else(|| remote::secrets::get(&clone_secret(job)).ok().flatten());
                    let mut draft = remote::draft(*connection).map_err(err)?;
                    forget_draft(state, job);
                    draft.server_key = state.sessions.login(&draft, password).map_err(err)?;
                    state
                        .drafts
                        .lock()
                        .unwrap()
                        .insert(job.to_string(), draft.clone());
                    Ok(live::Source::Server(Box::new(draft)))
                }
            }
        }
        SourceInput::Files { files } => Ok(live::Source::Files(
            files.into_iter().map(PathBuf::from).collect(),
        )),
        SourceInput::Code { code } => {
            let code = if code.is_empty() {
                remote::secrets::get(&clone_secret(job))
                    .ok()
                    .flatten()
                    .ok_or("the connection code was not kept; paste it again")?
            } else {
                code
            };
            Ok(live::Source::Connect(Box::new(
                connect::Link::parse(&code).map_err(err)?,
            )))
        }
    }
}

/// A clone kept in `wordpress/clones.json`, so it outlives the app: one
/// running when the app quit (for an update, say) starts again the next
/// time and continues where it stopped; a stopped or failed one is shown
/// again with its Continue button. A typed-in password is kept in the
/// system's keychain until the clone is done or closed.
#[derive(Clone, Serialize, Deserialize)]
pub struct SavedClone {
    job: String,
    /// Where it comes from, as the clone list shows it.
    from: String,
    source: SourceInput,
    options: live::CloneOptions,
    /// `running`, `paused`, `stopped` or `failed`.
    state: String,
    #[serde(default)]
    error: Option<String>,
    /// Still running in this app (the window was closed and opened again).
    #[serde(default, skip_deserializing)]
    active: bool,
    /// How many times in a row the app started it again by itself after
    /// quitting under it.
    #[serde(default)]
    starts: u32,
}

/// A clone the app closed under this many times in a row (it may be what
/// closes the app) waits for Try again instead of starting by itself.
const MAX_STARTS: u32 = 2;

/// How a clone ended, for a window that did not start it.
#[derive(Clone, Serialize)]
struct CloneEnded {
    job: String,
    result: Option<live::Cloned>,
    error: Option<String>,
    /// Paused, to go on with later; `error` is then empty.
    paused: bool,
}

static SAVED_CLONES: Mutex<()> = Mutex::new(());

/// Set when the app quits or installs an update: a clone that fails then
/// (its database stopped under it) stays `running`, so it starts again.
static QUITTING: AtomicBool = AtomicBool::new(false);

pub fn set_quitting(quitting: bool) {
    QUITTING.store(quitting, Ordering::SeqCst);
}

fn clones_file() -> PathBuf {
    paths::home().join("wordpress").join("clones.json")
}

/// The saved clones. One the app can't read is left out; a file it can't
/// read at all is kept beside as `clones.json.bad` and the list starts
/// empty, so a bad entry never stops the window from opening.
fn saved_clones() -> Vec<SavedClone> {
    let file = clones_file();
    let Ok(bytes) = std::fs::read(&file) else {
        return Vec::new();
    };
    let list: Vec<serde_json::Value> = match serde_json::from_slice(&bytes) {
        Ok(list) => list,
        Err(e) => {
            let bad = file.with_extension("json.bad");
            let _ = std::fs::rename(&file, &bad);
            crate::applog::error(&format!(
                "the saved clones could not be read ({e}); kept as {}",
                bad.display()
            ));
            return Vec::new();
        }
    };
    list.into_iter()
        .filter_map(|entry| match serde_json::from_value(entry) {
            Ok(clone) => Some(clone),
            Err(e) => {
                crate::applog::error(&format!("a saved clone was left out: {e}"));
                None
            }
        })
        .collect()
}

/// At start-up: counts one more start for each clone that was running
/// when the app last quit; one the app has quit under too often is marked
/// failed, for Try again, instead of starting by itself.
pub fn count_restarts() {
    let _guard = SAVED_CLONES.lock().unwrap_or_else(|e| e.into_inner());
    let mut list = saved_clones();
    let mut changed = false;
    for clone in list.iter_mut().filter(|c| c.state == "running") {
        changed = true;
        clone.starts += 1;
        if clone.starts > MAX_STARTS {
            crate::applog::error(&format!(
                "clone {} of {}: the app closed under it {} times, so it waits for Try again",
                clone.job, clone.options.name, MAX_STARTS
            ));
            clone.state = "failed".into();
            clone.starts = 0;
            clone.error = Some(format!(
                "Stackyard closed while cloning this site {MAX_STARTS} times in a row, so it was not started again. Press Try again to continue where it stopped."
            ));
        } else {
            crate::applog::info(&format!(
                "clone {} of {} starts again where it stopped",
                clone.job, clone.options.name
            ));
        }
    }
    if changed {
        write_clones(&list);
    }
}

fn write_clones(list: &[SavedClone]) {
    let file = clones_file();
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(json) = serde_json::to_vec_pretty(list) {
        let _ = std::fs::write(file, json);
    }
}

/// Puts `clone` in the saved list in place of its job's old entry, or
/// takes the job out when `clone` is `None`.
fn save_clone(job: &str, clone: Option<SavedClone>) {
    let _guard = SAVED_CLONES.lock().unwrap_or_else(|e| e.into_inner());
    let mut list = saved_clones();
    list.retain(|c| c.job != job);
    list.extend(clone);
    write_clones(&list);
}

fn clone_secret(job: &str) -> String {
    format!("clone-{job}")
}

fn forget_clone(job: &str) {
    save_clone(job, None);
    let _ = remote::secrets::delete(&clone_secret(job));
}

/// The clones kept from before the app last quit.
#[tauri::command]
pub fn wp_clones_saved(state: State<'_, CloneState>) -> Vec<SavedClone> {
    let running = state.running.lock().unwrap();
    let _guard = SAVED_CLONES.lock().unwrap_or_else(|e| e.into_inner());
    let mut list = saved_clones();
    for clone in &mut list {
        clone.active = running.contains_key(&clone.job);
    }
    list
}

/// Logs out of the connection typed in for this clone.
fn forget_draft(state: &CloneState, job: &str) {
    let old = state.drafts.lock().unwrap().remove(job);
    if let Some(old) = old {
        state.sessions.forget(&old.id);
    }
}

#[tauri::command]
pub fn wp_saved_connections() -> CmdResult<Vec<SavedConnection>> {
    require_clone()?;
    let store = RemoteStore::load().map_err(err)?;
    Ok(store
        .list()
        .iter()
        .map(|c| SavedConnection {
            id: c.id.clone(),
            name: c.name.clone(),
            site: c.site.clone(),
            host: c.host.clone(),
            user: c.user.clone(),
            protocol: c.protocol,
        })
        .collect())
}

/// Logs in (for a typed-in connection) and looks at the live site. Backup
/// files are unpacked first, saying how far as `wp-inspect-progress`
/// events; [`wp_clone_inspect_stop`] stops it.
#[tauri::command]
pub async fn wp_clone_inspect(
    app: AppHandle,
    job: String,
    source: SourceInput,
    root: Option<String>,
    url: Option<String>,
) -> CmdResult<live::Found> {
    require_clone()?;
    let stop = Arc::new(AtomicBool::new(false));
    app.state::<CloneState>()
        .inspecting
        .lock()
        .unwrap()
        .insert(job.clone(), stop.clone());
    let worker = app.clone();
    let id = job.clone();
    let result = blocking(move || {
        let state = worker.state::<CloneState>();
        let source = self::source(&state, &id, source, true)?;
        let mut last: Option<(live::Stage, String, Instant)> = None;
        live::inspect(
            &paths::home(),
            &state.sessions,
            &source,
            root.as_deref(),
            url.as_deref().filter(|u| !u.trim().is_empty()),
            &mut |progress| {
                // Ten updates a second at most, and every new step.
                let now = Instant::now();
                let due = match &last {
                    Some((stage, message, at)) => {
                        *stage != progress.stage
                            || *message != progress.message
                            || now.duration_since(*at) >= Duration::from_millis(100)
                    }
                    None => true,
                };
                if due {
                    last = Some((progress.stage, progress.message.clone(), now));
                    let _ = worker.emit(
                        "wp-inspect-progress",
                        CloneEvent {
                            job: id.clone(),
                            name: String::new(),
                            progress,
                        },
                    );
                }
                !stop.load(Ordering::SeqCst)
            },
        )
        .map_err(err)
    })
    .await;
    app.state::<CloneState>()
        .inspecting
        .lock()
        .unwrap()
        .remove(&job);
    result
}

/// Stops a Look for WordPress that is unpacking backup files.
#[tauri::command]
pub fn wp_clone_inspect_stop(state: State<'_, CloneState>, job: String) {
    if let Some(stop) = state.inspecting.lock().unwrap().get(&job) {
        stop.store(true, Ordering::SeqCst);
    }
}

/// Deletes what Look for WordPress unpacked of these backup files, when
/// their form is closed without cloning.
#[tauri::command]
pub async fn wp_backup_forget(files: Vec<String>) {
    let files: Vec<PathBuf> = files.into_iter().map(PathBuf::from).collect();
    let _ = blocking(move || {
        live::forget_backup(&paths::home(), &files);
        Ok(())
    })
    .await;
}

/// Checks the server login of a clone one step at a time, saying where it
/// stops and what to try.
#[tauri::command]
pub async fn wp_clone_check(
    app: AppHandle,
    source: SourceInput,
) -> CmdResult<remote::check::Report> {
    require_clone()?;
    blocking(move || {
        let state = app.state::<CloneState>();
        match source {
            SourceInput::Saved { id } => {
                let connection = RemoteStore::load()
                    .and_then(|s| s.get(&id).cloned())
                    .map_err(err)?;
                Ok(state.sessions.check(&connection, None))
            }
            SourceInput::New { mut connection } => {
                let password = connection.password.take();
                let draft = remote::draft(*connection).map_err(err)?;
                let report = state.sessions.check(&draft, Some(password));
                state.sessions.forget(&draft.id);
                Ok(report)
            }
            SourceInput::Files { .. } => Err("backup files need no connection".into()),
            SourceInput::Code { .. } => Err("a connection code needs no server login".into()),
        }
    })
    .await
}

/// Clones the live site into a new site, starting MySQL or MariaDB first
/// and the web server after. Other clones can run at the same time, each
/// under its own `job`. Progress comes as `wp-clone-progress` events.
#[tauri::command]
pub async fn wp_clone(
    app: AppHandle,
    job: String,
    source: SourceInput,
    options: live::CloneOptions,
    from: Option<String>,
) -> CmdResult<live::Cloned> {
    require_clone()?;
    let stop = Arc::new(AtomicBool::new(false));
    let pause = Arc::new(AtomicBool::new(false));
    {
        let state = app.state::<CloneState>();
        let mut running = state.running.lock().unwrap();
        let name = options.name.trim();
        if running.contains_key(&job) {
            return Err("this clone is already running".into());
        }
        if running.values().any(|r| r.name == name) {
            return Err(format!(
                "a clone named {name} is already running; wait for it or pick another name"
            ));
        }
        running.insert(
            job.clone(),
            Running {
                name: name.to_string(),
                stop: stop.clone(),
                pause: pause.clone(),
            },
        );
    }
    if let SourceInput::New { connection } = &source {
        if let Some(password) = connection.password.as_deref().filter(|p| !p.is_empty()) {
            let _ = remote::secrets::set(&clone_secret(&job), password);
        }
    }
    let kept_source = match &source {
        SourceInput::Code { code } => {
            if !code.is_empty() {
                let _ = remote::secrets::set(&clone_secret(&job), code);
            }
            SourceInput::Code {
                code: String::new(),
            }
        }
        other => other.clone(),
    };
    // Started again by the app after it quit under it: keep the count.
    let starts = {
        let _guard = SAVED_CLONES.lock().unwrap_or_else(|e| e.into_inner());
        saved_clones()
            .iter()
            .find(|c| c.job == job && c.state == "running")
            .map_or(0, |c| c.starts)
    };
    crate::applog::info(&format!(
        "clone {job} of {} from {} starts",
        options.name,
        from.as_deref().unwrap_or("")
    ));
    let mut saved = SavedClone {
        job: job.clone(),
        from: from.unwrap_or_default(),
        source: kept_source,
        options: options.clone(),
        state: "running".into(),
        error: None,
        active: false,
        starts,
    };
    save_clone(&job, Some(saved.clone()));
    let stopped = stop.clone();
    let worker = app.clone();
    let id = job.clone();
    let result = blocking(move || {
        let state = worker.state::<CloneState>();
        let engine = worker.state::<Arc<Engine>>().inner().clone();
        ensure_database(&engine)?;
        let source = self::source(&state, &id, source, false)?;
        let name = options.name.clone();
        let mut last: Option<(live::Stage, Instant)> = None;
        let cloned = live::clone(
            &paths::home(),
            &state.sessions,
            &source,
            &options,
            &mut |progress| {
                // Ten updates a second at most, and every new stage.
                let now = Instant::now();
                let due = match last {
                    Some((stage, at)) => {
                        stage != progress.stage
                            || now.duration_since(at) >= Duration::from_millis(100)
                    }
                    None => true,
                };
                if due {
                    last = Some((progress.stage, now));
                    let _ = worker.emit(
                        "wp-clone-progress",
                        CloneEvent {
                            job: id.clone(),
                            name: name.clone(),
                            progress,
                        },
                    );
                }
                !stop.load(Ordering::SeqCst)
            },
        )
        .map_err(err)?;
        forget_draft(&state, &id);
        ensure_web(&engine)?;
        Ok(cloned)
    })
    .await;
    app.state::<CloneState>()
        .running
        .lock()
        .unwrap()
        .remove(&job);
    match &result {
        Ok(_) => {
            crate::applog::info(&format!("clone {job} of {} is done", saved.options.name));
            forget_clone(&job);
            crate::sites_changed(&app);
        }
        Err(_) if QUITTING.load(Ordering::SeqCst) => return result,
        Err(_) if pause.load(Ordering::SeqCst) => {
            crate::applog::info(&format!("clone {job} of {} is paused", saved.options.name));
            saved.state = "paused".into();
            saved.error = None;
            saved.starts = 0;
            save_clone(&job, Some(saved));
        }
        Err(e) => {
            crate::applog::error(&format!("clone {job} of {}: {e}", saved.options.name));
            saved.state = if stopped.load(Ordering::SeqCst) {
                "stopped"
            } else {
                "failed"
            }
            .into();
            saved.error = Some(e.clone());
            save_clone(&job, Some(saved));
        }
    }
    let paused = result.is_err() && pause.load(Ordering::SeqCst);
    let _ = app.emit(
        "wp-clone-ended",
        CloneEnded {
            job,
            result: result.as_ref().ok().cloned(),
            error: result.as_ref().err().cloned().filter(|_| !paused),
            paused,
        },
    );
    if paused {
        return Err(PAUSED.into());
    }
    result
}

/// What a paused clone's call answers, so the window shows it paused.
const PAUSED: &str = "paused";

/// Stops one clone; it can be continued later.
#[tauri::command]
pub fn wp_clone_stop(state: State<'_, CloneState>, job: String) {
    if let Some(running) = state.running.lock().unwrap().get(&job) {
        running.stop.store(true, Ordering::SeqCst);
    }
}

/// Pauses one clone: its download stops at once (a file cut off halfway is
/// kept and goes on from there), so other clones get the connection's
/// speed, and Resume continues where it stopped, also after a restart.
#[tauri::command]
pub fn wp_clone_pause(state: State<'_, CloneState>, job: String) {
    if let Some(running) = state.running.lock().unwrap().get(&job) {
        running.pause.store(true, Ordering::SeqCst);
        running.stop.store(true, Ordering::SeqCst);
    }
}

/// Logs out of the server typed in for a clone the app is done with.
#[tauri::command]
pub fn wp_clone_forget(state: State<'_, CloneState>, job: String) {
    if !state.running.lock().unwrap().contains_key(&job) {
        forget_draft(&state, &job);
        forget_clone(&job);
    }
}

/// Whether a clone with this name stopped halfway and can be continued.
#[tauri::command]
pub fn wp_clone_unfinished(name: String) -> bool {
    live::unfinished(&paths::home(), name.trim())
}

/// How much room the drives a clone of `name` uses have.
#[tauri::command]
pub fn wp_clone_space(name: String, folder: Option<PathBuf>) -> CmdResult<live::Space> {
    live::space(&paths::home(), name.trim(), folder.as_deref()).map_err(err)
}

/// Where a site came from: its live site, when Stackyard cloned it.
#[tauri::command]
pub fn wp_environment(domain: String) -> CmdResult<Option<env::Environment>> {
    env::load(&paths::home(), &domain).map_err(err)
}

/// The live site of a copy, or of the main site a staging belongs to.
fn live_of(domain: &str) -> CmdResult<env::Live> {
    let home = paths::home();
    let of = |d: &str| env::load(&home, d).ok().flatten().and_then(|e| e.live);
    of(domain)
        .or_else(|| of(&staging::main_of(&home, domain)))
        .ok_or_else(|| format!("{domain} has no live site"))
}

/// Opens the live site a copy was cloned from in the browser, or its
/// wp-admin. Only the address Stackyard saved is opened, never one passed in.
#[tauri::command]
pub fn wp_open_live(domain: String, admin: bool) -> CmdResult<()> {
    let live = live_of(&domain)?;
    let url = live.url.trim_end_matches('/');
    if !url.starts_with("https://") && !url.starts_with("http://") {
        return Err(format!("{domain}'s live address is not a web address"));
    }
    let target = if admin {
        format!("{url}/wp-admin/")
    } else {
        format!("{url}/")
    };
    crate::sitetools::open_with_system(std::ffi::OsStr::new(&target))
}

/// How a copy reaches its live site, for the top of the Live site tab.
#[derive(Serialize)]
pub struct LiveLink {
    /// `sftp`, `ftps`, `ftp`, `plugin`, or `files` when the copy was
    /// cloned from backup files and has no way to the live site.
    kind: String,
    /// The saved connection's id, to check it or edit it.
    connection: Option<String>,
    host: String,
    user: String,
    /// The Stackyard Connect version on the live site, when it answered.
    plugin: Option<String>,
    /// Whether the code lets Stackyard change the live site (push).
    can_push: Option<bool>,
    /// Whether the live site answered; `None` when it wasn't asked (SFTP
    /// and FTP are checked with Check connection instead).
    works: Option<bool>,
    /// What went wrong, in the live site's or Stackyard's words.
    problem: Option<String>,
    /// The plugin said the code stopped working: it was stopped, replaced
    /// by a newer one, or ran out.
    expired: bool,
}

fn live_link(domain: &str) -> CmdResult<LiveLink> {
    let live = live_of(domain)?;
    let Some(id) = live.connection.clone() else {
        return Ok(LiveLink {
            kind: "files".into(),
            connection: None,
            host: String::new(),
            user: String::new(),
            plugin: None,
            can_push: None,
            works: None,
            problem: None,
            expired: false,
        });
    };
    let connection = RemoteStore::load()
        .map_err(err)?
        .get(&id)
        .map_err(err)?
        .clone();
    let kind = match connection.protocol {
        remote::Protocol::Sftp => "sftp",
        remote::Protocol::Ftps => "ftps",
        remote::Protocol::Ftp => "ftp",
        remote::Protocol::Plugin => "plugin",
    };
    let mut link = LiveLink {
        kind: kind.into(),
        connection: Some(id),
        host: connection.host.clone(),
        user: connection.user.clone(),
        plugin: None,
        can_push: None,
        works: None,
        problem: None,
        expired: false,
    };
    if connection.protocol != remote::Protocol::Plugin {
        return Ok(link);
    }
    let code = remote::input_of(&connection).map_err(err)?.password;
    let answer = code
        .ok_or_else(|| "no connection code is saved for it".to_string())
        .and_then(|code| connect::Link::parse(&code).map_err(err))
        .and_then(|l| {
            link.can_push = Some(l.write);
            l.info().map_err(err)
        });
    match answer {
        Ok(info) => {
            link.works = Some(true);
            link.plugin = Some(info.plugin);
        }
        Err(e) => {
            link.works = Some(false);
            link.expired = e.contains("not valid any more");
            link.problem = Some(e);
        }
    }
    Ok(link)
}

/// How this copy reaches its live site; with the plugin, asks the live
/// site whether the code still works.
#[tauri::command]
pub async fn wp_live_link(domain: String) -> CmdResult<LiveLink> {
    blocking(move || live_link(&domain)).await
}

/// `https://www.Shop.example/` → `shop.example`, to tell whether two
/// addresses are the same site.
fn same_site(a: &str, b: &str) -> bool {
    let plain = |u: &str| {
        let host = u.split("://").nth(1).unwrap_or(u);
        let host = host
            .split('/')
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase();
        host.strip_prefix("www.")
            .map(str::to_string)
            .unwrap_or(host)
    };
    plain(a) == plain(b)
}

/// Saves a new Stackyard Connect code for the copy's live site, in place of
/// the one that stopped working, and checks it. A copy made from backup
/// files gets its first connection this way, so it can push and pull.
#[tauri::command]
pub async fn wp_live_set_code(app: AppHandle, domain: String, code: String) -> CmdResult<LiveLink> {
    blocking(move || {
        let live = live_of(&domain)?;
        let link = connect::Link::parse(&code).map_err(err)?;
        if !live.url.is_empty()
            && !same_site(&link.url, &live.url)
            && !same_site(&link.site, &live.url)
        {
            return Err(format!(
                "this code is from {}, but {domain}'s live site is {}; make the code on {}",
                link.url, live.url, live.url
            ));
        }
        let mut store = RemoteStore::load().map_err(err)?;
        let Some(id) = live.connection.clone() else {
            return connect_backup_copy(&domain, &link, &mut store);
        };
        let mut input = remote::input_of(store.get(&id).map_err(err)?).map_err(err)?;
        input.protocol = Some(remote::Protocol::Plugin);
        input.password = Some(link.code().to_string());
        store.update(&id, input).map_err(err)?;
        store.save().map_err(err)?;
        app.state::<CloneState>().sessions.close(&id);
        app.state::<crate::remote::RemoteState>()
            .sessions
            .close(&id);
        live_link(&domain)
    })
    .await
}

/// Connects a copy made from backup files to its live site through
/// Stackyard Connect: saves the code as its Live connection and makes the
/// copy reach the site the way a clone with a code does.
fn connect_backup_copy(
    domain: &str,
    link: &connect::Link,
    store: &mut RemoteStore,
) -> CmdResult<LiveLink> {
    let home = paths::home();
    let mut environment = env::load(&home, domain)
        .map_err(err)?
        .ok_or_else(|| format!("{domain} is not a copy of a live site"))?;
    let Some(live) = environment.live.as_mut() else {
        return Err(format!("{domain} is not a copy of a live site"));
    };
    // Asks the plugin first, so a code that doesn't work saves nothing.
    let root = link.root().map_err(err)?;
    let saved = store
        .add(remote::ConnectionInput {
            site: domain.to_string(),
            name: "Live".into(),
            protocol: Some(remote::Protocol::Plugin),
            remote_dir: root.clone(),
            password: Some(link.code().to_string()),
            ..remote::ConnectionInput::default()
        })
        .map_err(err)?;
    store.save().map_err(err)?;
    if live.url.is_empty() {
        live.url = link.url.clone();
    }
    live.connection = Some(saved.id);
    live.path = root;
    live.route = env::Route::Helper;
    env::save(&home, domain, &environment).map_err(err)?;
    live_link(domain)
}

#[derive(Serialize)]
pub struct Environments {
    /// The main site: this site, or the one it is a staging of.
    main: String,
    stagings: Vec<staging::Staging>,
    /// The main site was cloned from a live server Stackyard can reach.
    from_live: bool,
}

/// The main site and its stagings, for the Environments tab.
#[tauri::command]
pub fn wp_stagings(domain: String) -> CmdResult<Environments> {
    open(&domain)?;
    let home = paths::home();
    let main = staging::main_of(&home, &domain);
    let from_live = env::load(&home, &main)
        .ok()
        .flatten()
        .and_then(|e| e.live)
        .is_some_and(|l| l.connection.is_some());
    Ok(Environments {
        stagings: staging::list(&home, &main).map_err(err)?,
        main,
        from_live,
    })
}

/// Makes a staging, starting MySQL or MariaDB first and the web server
/// after. Progress comes as `wp-staging-progress` events.
#[tauri::command]
pub async fn wp_staging_create(app: AppHandle, new: staging::NewStaging) -> CmdResult<Site> {
    open(&new.parent)?;
    {
        let state = app.state::<CloneState>();
        if state.staging_busy.swap(true, Ordering::SeqCst) {
            return Err("a copy is already being made; wait for it or stop it first".into());
        }
        state.staging_stop.store(false, Ordering::SeqCst);
    }
    let worker = app.clone();
    let result = blocking(move || {
        let state = worker.state::<CloneState>();
        let engine = worker.state::<Arc<Engine>>().inner().clone();
        ensure_database(&engine)?;
        let stop = state.staging_stop.clone();
        let name = new.name.clone();
        let site = staging::create(&paths::home(), &state.sessions, &new, &mut |progress| {
            let _ = worker.emit(
                "wp-staging-progress",
                CloneEvent {
                    job: String::new(),
                    name: name.clone(),
                    progress,
                },
            );
            !stop.load(Ordering::SeqCst)
        })
        .map_err(err)?;
        ensure_web(&engine)?;
        Ok(site)
    })
    .await;
    app.state::<CloneState>()
        .staging_busy
        .store(false, Ordering::SeqCst);
    if result.is_ok() {
        crate::sites_changed(&app);
    }
    result
}

/// Stops the staging being made.
#[tauri::command]
pub fn wp_staging_stop(state: State<'_, CloneState>) {
    state.staging_stop.store(true, Ordering::SeqCst);
}

/// Moves a staging to the trash.
#[tauri::command]
pub async fn wp_staging_destroy(app: AppHandle, domain: String) -> CmdResult<staging::Trashed> {
    open(&domain)?;
    let worker = app.clone();
    let trashed = blocking(move || {
        let engine = worker.state::<Arc<Engine>>().inner().clone();
        crate::ensure_database(&engine)?;
        let trashed = staging::destroy(&paths::home(), &domain).map_err(err)?;
        crate::ensure_web(&engine)?;
        Ok(trashed)
    })
    .await?;
    crate::sites_changed(&app);
    Ok(trashed)
}

#[tauri::command]
pub fn wp_trash() -> Vec<staging::Trashed> {
    staging::trash(&paths::home())
}

#[tauri::command]
pub async fn wp_trash_restore(app: AppHandle, id: String) -> CmdResult<Site> {
    let worker = app.clone();
    let site = blocking(move || {
        let engine = worker.state::<Arc<Engine>>().inner().clone();
        crate::ensure_database(&engine)?;
        let site = staging::restore(&paths::home(), &id).map_err(err)?;
        crate::ensure_web(&engine)?;
        Ok(site)
    })
    .await?;
    crate::sites_changed(&app);
    Ok(site)
}

#[tauri::command]
pub fn wp_trash_delete(id: String) -> CmdResult<()> {
    staging::delete(&paths::home(), &id).map_err(err)
}

#[derive(Serialize)]
pub struct Snapshots {
    list: Vec<snapshot::Snapshot>,
    /// When the staging was made, if it has a base snapshot.
    base: Option<u64>,
}

#[tauri::command]
pub fn wp_snapshots(domain: String) -> CmdResult<Snapshots> {
    open(&domain)?;
    let home = paths::home();
    Ok(Snapshots {
        list: snapshot::list(&home, &domain),
        base: snapshot::base(&home, &domain).map(|s| s.created_at),
    })
}

/// Puts the copy back as it was before the snapshot's step.
#[tauri::command]
pub async fn wp_snapshot_restore(app: AppHandle, domain: String, id: String) -> CmdResult<()> {
    let worker = app.clone();
    blocking(move || {
        let engine = worker.state::<Arc<Engine>>().inner().clone();
        crate::ensure_database(&engine)?;
        let wp = open(&domain)?;
        snapshot::restore(&paths::home(), &wp, &id).map_err(err)?;
        Ok(())
    })
    .await
}

#[tauri::command]
pub fn wp_snapshot_remove(domain: String, id: String) -> CmdResult<()> {
    snapshot::remove(&paths::home(), &domain, &id).map_err(err)
}

// ---------------------------------------------------------------------------
// Merge

/// What changed on a staging since it was made, against `target`.
#[tauri::command]
pub async fn wp_merge_changes(
    app: AppHandle,
    source: String,
    target: String,
) -> CmdResult<merge::ChangeList> {
    let worker = app.clone();
    blocking(move || {
        let engine = worker.state::<Arc<Engine>>().inner().clone();
        crate::ensure_database(&engine)?;
        merge::changes(&paths::home(), &source, &target).map_err(err)
    })
    .await
}

#[tauri::command]
pub async fn wp_merge(
    app: AppHandle,
    source: String,
    target: String,
    choice: merge::Choice,
) -> CmdResult<merge::Merged> {
    let worker = app.clone();
    blocking(move || {
        let engine = worker.state::<Arc<Engine>>().inner().clone();
        crate::ensure_database(&engine)?;
        merge::merge(&paths::home(), &source, &target, &choice).map_err(err)
    })
    .await
}

/// Posts and pages of a copy, for picking which to move.
#[tauri::command]
pub async fn wp_posts_list(
    app: AppHandle,
    domain: String,
    search: String,
) -> CmdResult<Vec<posts::PostSummary>> {
    let worker = app.clone();
    blocking(move || {
        let engine = worker.state::<Arc<Engine>>().inner().clone();
        crate::ensure_database(&engine)?;
        posts::list(&paths::home(), &domain, &search).map_err(err)
    })
    .await
}

/// What moving the picked posts would do, without doing it.
#[tauri::command]
pub async fn wp_posts_plan(
    app: AppHandle,
    source: String,
    target: String,
    choice: posts::MoveChoice,
) -> CmdResult<posts::MovePlan> {
    let worker = app.clone();
    blocking(move || {
        let engine = worker.state::<Arc<Engine>>().inner().clone();
        crate::ensure_database(&engine)?;
        posts::plan(&paths::home(), &source, &target, &choice).map_err(err)
    })
    .await
}

#[tauri::command]
pub async fn wp_posts_move(
    app: AppHandle,
    source: String,
    target: String,
    choice: posts::MoveChoice,
) -> CmdResult<posts::Moved> {
    let worker = app.clone();
    blocking(move || {
        let engine = worker.state::<Arc<Engine>>().inner().clone();
        crate::ensure_database(&engine)?;
        posts::move_posts(&paths::home(), &source, &target, &choice).map_err(err)
    })
    .await
}

// ---------------------------------------------------------------------------
// Push and pull

/// One push, pull or live read at a time; the stop button ends it.
#[derive(Default)]
pub struct SyncState {
    stop: Arc<AtomicBool>,
    busy: AtomicBool,
}

#[derive(Clone, Serialize)]
struct SyncEvent {
    domain: String,
    #[serde(flatten)]
    progress: live::Progress,
}

/// Runs `work` against the live site of `domain` on a worker thread, with
/// progress as `wp-sync-progress` events. Clones share the logged-in
/// sessions, so a password typed once is not asked again.
async fn with_live<T: Send + 'static>(
    app: AppHandle,
    domain: String,
    work: impl FnOnce(&std::path::Path, &Sessions, &str, live::OnProgress) -> stackyard_core::Result<T>
        + Send
        + 'static,
) -> CmdResult<T> {
    require_clone()?;
    {
        let state = app.state::<SyncState>();
        if state.busy.swap(true, Ordering::SeqCst) {
            return Err("another push or pull is running; wait for it or stop it first".into());
        }
        state.stop.store(false, Ordering::SeqCst);
    }
    let worker = app.clone();
    let result = blocking(move || {
        let engine = worker.state::<Arc<Engine>>().inner().clone();
        crate::ensure_database(&engine)?;
        let clones = worker.state::<CloneState>();
        let stop = worker.state::<SyncState>().stop.clone();
        let emitter = worker.clone();
        let name = domain.clone();
        work(&paths::home(), &clones.sessions, &domain, &mut |progress| {
            let _ = emitter.emit(
                "wp-sync-progress",
                SyncEvent {
                    domain: name.clone(),
                    progress,
                },
            );
            !stop.load(Ordering::SeqCst)
        })
        .map_err(err)
    })
    .await;
    app.state::<SyncState>().busy.store(false, Ordering::SeqCst);
    result
}

/// What changed on the side changes come from, since the copy and the live
/// site were last in step.
#[tauri::command]
pub async fn wp_sync_changes(
    app: AppHandle,
    domain: String,
    way: sync::Way,
) -> CmdResult<sync::SyncChanges> {
    with_live(app, domain, move |home, sessions, domain, progress| {
        sync::changes(home, sessions, domain, way, progress)
    })
    .await
}

#[tauri::command]
pub async fn wp_push(
    app: AppHandle,
    domain: String,
    choice: merge::Choice,
    confirm: String,
) -> CmdResult<sync::Pushed> {
    with_live(app, domain, move |home, sessions, domain, progress| {
        sync::push(home, sessions, domain, &choice, &confirm, progress)
    })
    .await
}

#[tauri::command]
pub async fn wp_pull(
    app: AppHandle,
    domain: String,
    choice: merge::Choice,
) -> CmdResult<sync::Pulled> {
    with_live(app, domain, move |home, sessions, domain, progress| {
        sync::pull(home, sessions, domain, &choice, progress)
    })
    .await
}

/// Puts a site made here on a host through one of its servers, replacing
/// what is there; it is then a copy of the host's site.
#[tauri::command]
pub async fn wp_install(
    app: AppHandle,
    domain: String,
    connection: String,
    url: String,
    confirm: String,
) -> CmdResult<sync::Installed> {
    with_live(app, domain, move |home, sessions, domain, progress| {
        sync::install(
            home,
            sessions,
            domain,
            &connection,
            &url,
            &confirm,
            progress,
        )
    })
    .await
}

#[tauri::command]
pub fn wp_sync_stop(state: State<'_, SyncState>) {
    state.stop.store(true, Ordering::SeqCst);
}

/// Backups of the live site taken before each push, newest first.
#[tauri::command]
pub fn wp_live_backups(domain: String) -> CmdResult<Vec<sync::LiveBackup>> {
    require_clone()?;
    Ok(sync::backups(&paths::home(), &domain))
}

#[tauri::command]
pub async fn wp_live_backup_restore(app: AppHandle, domain: String, id: String) -> CmdResult<()> {
    with_live(app, domain, move |home, sessions, domain, progress| {
        sync::restore_backup(home, sessions, domain, &id, progress)
    })
    .await
}

#[tauri::command]
pub fn wp_live_backup_remove(domain: String, id: String) -> CmdResult<()> {
    require_clone()?;
    sync::remove_backup(&paths::home(), &domain, &id).map_err(err)
}

/// Reads the live database into a local mirror, to pick single posts from.
#[tauri::command]
pub async fn wp_live_read(app: AppHandle, domain: String) -> CmdResult<u64> {
    with_live(app, domain, move |home, sessions, domain, progress| {
        sync::read_live(home, sessions, domain, progress)
    })
    .await
}

/// When the live database was last read, if ever.
#[tauri::command]
pub fn wp_live_read_at(domain: String) -> Option<u64> {
    sync::live_read_at(&paths::home(), &domain)
}

#[tauri::command]
pub async fn wp_live_posts(
    app: AppHandle,
    domain: String,
    search: String,
) -> CmdResult<Vec<posts::PostSummary>> {
    let worker = app.clone();
    blocking(move || {
        let engine = worker.state::<Arc<Engine>>().inner().clone();
        crate::ensure_database(&engine)?;
        sync::live_posts(&paths::home(), &domain, &search).map_err(err)
    })
    .await
}

#[tauri::command]
pub async fn wp_live_posts_plan(
    app: AppHandle,
    domain: String,
    way: sync::Way,
    choice: posts::MoveChoice,
) -> CmdResult<posts::MovePlan> {
    let worker = app.clone();
    blocking(move || {
        let engine = worker.state::<Arc<Engine>>().inner().clone();
        crate::ensure_database(&engine)?;
        sync::plan_posts(&paths::home(), &domain, way, &choice).map_err(err)
    })
    .await
}

#[tauri::command]
pub async fn wp_live_posts_move(
    app: AppHandle,
    domain: String,
    way: sync::Way,
    choice: posts::MoveChoice,
) -> CmdResult<sync::PostsMoved> {
    with_live(app, domain, move |home, sessions, domain, progress| {
        sync::move_posts(home, sessions, domain, way, &choice, progress)
    })
    .await
}
