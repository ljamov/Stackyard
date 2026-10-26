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
    self, env, live, merge, posts, snapshot, staging, sync, woo, CoreUpdate, DebugSettings,
    DiskUse, Extension, ExtensionAction, Kind, LogTail, Overview, Replaced, User, WpSite,
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
    /// The clones running now, by clone: the site name and the Stop switch.
    running: Mutex<HashMap<String, (String, Arc<AtomicBool>)>>,
    staging_stop: Arc<AtomicBool>,
    staging_busy: AtomicBool,
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
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum SourceInput {
    /// A connection saved for another site.
    Saved { id: String },
    /// A connection typed in now.
    New { connection: ConnectionInput },
    /// Backup files on this computer.
    Files { files: Vec<String> },
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
                    let password = connection.password.take();
                    let mut draft = remote::draft(connection).map_err(err)?;
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
    }
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

/// Logs in (for a typed-in connection) and looks at the live site.
#[tauri::command]
pub async fn wp_clone_inspect(
    app: AppHandle,
    job: String,
    source: SourceInput,
    root: Option<String>,
    url: Option<String>,
) -> CmdResult<live::Found> {
    require_clone()?;
    blocking(move || {
        let state = app.state::<CloneState>();
        let source = self::source(&state, &job, source, true)?;
        live::inspect(
            &paths::home(),
            &state.sessions,
            &source,
            root.as_deref(),
            url.as_deref().filter(|u| !u.trim().is_empty()),
        )
        .map_err(err)
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
) -> CmdResult<live::Cloned> {
    require_clone()?;
    let stop = Arc::new(AtomicBool::new(false));
    {
        let state = app.state::<CloneState>();
        let mut running = state.running.lock().unwrap();
        let name = options.name.trim();
        if running.contains_key(&job) {
            return Err("this clone is already running".into());
        }
        if running.values().any(|(n, _)| n == name) {
            return Err(format!(
                "a clone named {name} is already running; wait for it or pick another name"
            ));
        }
        running.insert(job.clone(), (name.to_string(), stop.clone()));
    }
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
    if result.is_ok() {
        crate::sites_changed(&app);
    }
    result
}

/// Stops one clone; it can be continued later.
#[tauri::command]
pub fn wp_clone_stop(state: State<'_, CloneState>, job: String) {
    if let Some((_, stop)) = state.running.lock().unwrap().get(&job) {
        stop.store(true, Ordering::SeqCst);
    }
}

/// Logs out of the server typed in for a clone the app is done with.
#[tauri::command]
pub fn wp_clone_forget(state: State<'_, CloneState>, job: String) {
    if !state.running.lock().unwrap().contains_key(&job) {
        forget_draft(&state, &job);
    }
}

/// Whether a clone with this name stopped halfway and can be continued.
#[tauri::command]
pub fn wp_clone_unfinished(name: String) -> bool {
    live::unfinished(&paths::home(), name.trim())
}

/// Where a site came from: its live site, when Stackyard cloned it.
#[tauri::command]
pub fn wp_environment(domain: String) -> CmdResult<Option<env::Environment>> {
    env::load(&paths::home(), &domain).map_err(err)
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
