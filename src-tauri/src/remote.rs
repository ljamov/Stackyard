//! Commands for a site's remote servers (SFTP, FTPS, FTP) and its Git
//! repository. Everything that talks to a server runs on a worker thread;
//! transfers report as `remote-progress` events and can be stopped.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use stackyard_core::git;
use stackyard_core::remote::{
    self, secrets, Compare, Connection, ConnectionInput, Direction, Listing, Outcome, Plan,
    PlannedFile, Progress, RemoteStore, Sessions,
};
use stackyard_core::services::ServiceManager;
use stackyard_core::settings::Settings;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::{err, CmdResult};

/// Open logins, and a stop switch for each connection that is busy.
#[derive(Default)]
pub struct RemoteState {
    sessions: Sessions,
    busy: Mutex<HashMap<String, Arc<AtomicBool>>>,
}

#[derive(Serialize)]
pub struct ConnectionView {
    #[serde(flatten)]
    connection: Connection,
    has_password: bool,
    busy: bool,
}

#[derive(Serialize)]
pub struct RemoteView {
    connections: Vec<ConnectionView>,
    /// Where passwords are kept, e.g. "Windows Credential Manager".
    secret_store: &'static str,
}

#[derive(Serialize)]
pub struct TestView {
    /// Where the server starts, when logging in worked.
    start_dir: Option<String>,
    error: Option<String>,
    /// A certificate the system does not trust, which the user may choose to.
    certificate: Option<String>,
    /// The server's key is not the one trusted before.
    key_changed: bool,
}

#[derive(Clone, Serialize)]
struct ProgressEvent {
    id: String,
    #[serde(flatten)]
    progress: Progress,
}

fn require(module: &str) -> CmdResult<()> {
    Settings::load()
        .and_then(|s| s.require_module(module))
        .map_err(err)
}

fn view(state: &RemoteState, site: &str) -> CmdResult<RemoteView> {
    let store = RemoteStore::load().map_err(err)?;
    let busy = state.busy.lock().unwrap();
    Ok(RemoteView {
        connections: store
            .for_site(site)
            .map(|c| ConnectionView {
                has_password: remote::has_password(&c.id),
                busy: busy.contains_key(&c.id),
                connection: c.clone(),
            })
            .collect(),
        secret_store: secrets::store_name(),
    })
}

fn connection(id: &str) -> CmdResult<Connection> {
    RemoteStore::load()
        .and_then(|s| s.get(id).cloned())
        .map_err(err)
}

/// The project folder of a site, parked ones included.
fn site_folder(domain: &str) -> CmdResult<PathBuf> {
    ServiceManager::open()
        .sites()
        .map_err(err)?
        .into_iter()
        .find(|s| s.site.domain() == domain)
        .map(|s| s.site.path)
        .ok_or_else(|| format!("no site named {domain}"))
}

/// Runs server work on a worker thread.
async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> CmdResult<T> + Send + 'static,
) -> CmdResult<T> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|e| e.to_string())?
}

/// Marks a connection busy for one transfer; a second at once is refused.
struct Busy {
    app: AppHandle,
    id: String,
    stop: Arc<AtomicBool>,
}

impl Busy {
    fn start(app: &AppHandle, id: &str) -> CmdResult<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let state = app.state::<RemoteState>();
        let mut busy = state.busy.lock().unwrap();
        if busy.contains_key(id) {
            return Err("this connection is busy; wait for it or stop it first".into());
        }
        busy.insert(id.to_string(), stop.clone());
        Ok(Self {
            app: app.clone(),
            id: id.to_string(),
            stop,
        })
    }

    /// Sends progress to the interface; false once the user pressed Stop.
    fn report(&self, progress: Progress) -> bool {
        let _ = self.app.emit(
            "remote-progress",
            ProgressEvent {
                id: self.id.clone(),
                progress,
            },
        );
        !self.stop.load(Ordering::Relaxed)
    }
}

impl Drop for Busy {
    fn drop(&mut self) {
        self.app
            .state::<RemoteState>()
            .busy
            .lock()
            .unwrap()
            .remove(&self.id);
    }
}

#[tauri::command]
pub fn remote_connections(site: String, state: State<'_, RemoteState>) -> CmdResult<RemoteView> {
    require("remote")?;
    view(&state, &site)
}

/// Adds a connection (no `id`) or changes one.
#[tauri::command]
pub fn remote_save(
    id: Option<String>,
    input: ConnectionInput,
    state: State<'_, RemoteState>,
) -> CmdResult<RemoteView> {
    require("remote")?;
    let site = input.site.clone();
    let mut store = RemoteStore::load().map_err(err)?;
    match id {
        Some(id) => {
            store.update(&id, input).map_err(err)?;
            state.sessions.close(&id);
        }
        None => {
            store.add(input).map_err(err)?;
        }
    }
    store.save().map_err(err)?;
    view(&state, &site)
}

#[tauri::command]
pub fn remote_remove(id: String, state: State<'_, RemoteState>) -> CmdResult<RemoteView> {
    let mut store = RemoteStore::load().map_err(err)?;
    let removed = store.remove(&id).map_err(err)?;
    store.save().map_err(err)?;
    state.sessions.close(&id);
    view(&state, &removed.site)
}

/// Trusts a certificate or server key (`None` forgets the trusted one).
#[tauri::command]
pub fn remote_trust(
    id: String,
    fingerprint: Option<String>,
    state: State<'_, RemoteState>,
) -> CmdResult<RemoteView> {
    let mut store = RemoteStore::load().map_err(err)?;
    store.set_server_key(&id, fingerprint).map_err(err)?;
    store.save().map_err(err)?;
    state.sessions.close(&id);
    let site = store.get(&id).map_err(err)?.site.clone();
    view(&state, &site)
}

#[tauri::command]
pub async fn remote_test(app: AppHandle, id: String) -> CmdResult<TestView> {
    require("remote")?;
    let connection = connection(&id)?;
    blocking(move || {
        let state = app.state::<RemoteState>();
        Ok(match state.sessions.test(&connection) {
            Ok(dir) => TestView {
                start_dir: Some(dir),
                error: None,
                certificate: None,
                key_changed: false,
            },
            Err(e) => TestView {
                start_dir: None,
                certificate: match &e {
                    stackyard_core::Error::RemoteCertificate { fingerprint, .. } => {
                        Some(fingerprint.clone())
                    }
                    _ => None,
                },
                key_changed: matches!(e, stackyard_core::Error::ServerKeyChanged { .. }),
                error: Some(e.to_string()),
            },
        })
    })
    .await
}

/// Connects one step at a time and says where it stops and what to try.
#[tauri::command]
pub async fn remote_check(
    app: AppHandle,
    id: String,
) -> CmdResult<stackyard_core::remote::check::Report> {
    require("remote")?;
    let connection = connection(&id)?;
    blocking(move || Ok(app.state::<RemoteState>().sessions.check(&connection, None))).await
}

#[tauri::command]
pub async fn remote_browse(app: AppHandle, id: String, dir: Option<String>) -> CmdResult<Listing> {
    require("remote")?;
    let connection = connection(&id)?;
    blocking(move || {
        app.state::<RemoteState>()
            .sessions
            .browse(&connection, dir.as_deref())
            .map_err(err)
    })
    .await
}

/// Works out what to transfer. `paths`, relative to the connection's
/// folders, limits it to those files and folders.
#[tauri::command]
pub async fn remote_plan(
    app: AppHandle,
    id: String,
    direction: Direction,
    compare: Option<Compare>,
    delete: bool,
    paths: Option<Vec<String>>,
) -> CmdResult<Plan> {
    require("remote")?;
    let connection = connection(&id)?;
    let local = remote::local_root(&connection, &site_folder(&connection.site)?);
    blocking(move || {
        let busy = Busy::start(&app, &id)?;
        app.state::<RemoteState>()
            .sessions
            .plan(
                &connection,
                &local,
                direction,
                compare.unwrap_or_default(),
                delete,
                paths.as_deref(),
                &mut |p| busy.report(p),
            )
            .map_err(err)
    })
    .await
}

#[tauri::command]
pub async fn remote_run(app: AppHandle, id: String, files: Vec<PlannedFile>) -> CmdResult<Outcome> {
    require("remote")?;
    let connection = connection(&id)?;
    let local = remote::local_root(&connection, &site_folder(&connection.site)?);
    blocking(move || {
        let busy = Busy::start(&app, &id)?;
        app.state::<RemoteState>()
            .sessions
            .run(&connection, &local, &files, &mut |p| busy.report(p))
            .map_err(err)
    })
    .await
}

#[tauri::command]
pub fn remote_stop(id: String, state: State<'_, RemoteState>) {
    if let Some(stop) = state.busy.lock().unwrap().get(&id) {
        stop.store(true, Ordering::Relaxed);
    }
}

/// The local folder a connection transfers, to start file pickers in.
#[tauri::command]
pub fn remote_local_folder(id: String) -> CmdResult<String> {
    let connection = connection(&id)?;
    let folder = remote::local_root(&connection, &site_folder(&connection.site)?);
    Ok(folder.display().to_string())
}

async fn with_git(
    site: String,
    work: impl FnOnce(&std::path::Path) -> stackyard_core::Result<git::Status> + Send + 'static,
) -> CmdResult<git::Status> {
    require("git")?;
    let folder = site_folder(&site)?;
    blocking(move || work(&folder).map_err(err)).await
}

#[tauri::command]
pub async fn git_status(site: String) -> CmdResult<git::Status> {
    with_git(site, git::status).await
}

#[tauri::command]
pub async fn git_link(site: String, url: String) -> CmdResult<git::Status> {
    with_git(site, move |dir| git::link(dir, &url)).await
}

#[tauri::command]
pub async fn git_fetch(site: String) -> CmdResult<git::Status> {
    with_git(site, git::fetch).await
}

#[tauri::command]
pub async fn git_pull(site: String) -> CmdResult<git::Status> {
    with_git(site, git::pull).await
}

#[tauri::command]
pub async fn git_push(site: String) -> CmdResult<git::Status> {
    with_git(site, git::push).await
}

#[tauri::command]
pub async fn git_commit(
    site: String,
    message: String,
    name: Option<String>,
    email: Option<String>,
) -> CmdResult<git::Status> {
    with_git(site, move |dir| {
        let identity = match (&name, &email) {
            (Some(n), Some(e)) => Some((n.as_str(), e.as_str())),
            _ => None,
        };
        git::commit(dir, &message, identity)
    })
    .await
}

/// Forgets a removed site's connections and their passwords.
pub fn site_removed(domain: &str) {
    if let Ok(mut store) = RemoteStore::load() {
        if store.for_site(domain).next().is_some() {
            store.forget_site(domain);
            let _ = store.save();
        }
    }
}

/// Keeps a site's connections when its domain changes.
pub fn site_renamed(old: &str, new: &str) {
    if let Ok(mut store) = RemoteStore::load() {
        if store.for_site(old).next().is_some() {
            store.rename_site(old, new);
            let _ = store.save();
        }
    }
}
