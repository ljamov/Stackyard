//! Updates to Stackyard itself. Releases carry a signature made with a key
//! only the maintainer holds; the public half is in `tauri.conf.json`, and
//! an update whose signature doesn't match it is never installed. A build
//! without a public key never checks.
//!
//! Nothing installs on its own: a check (at start, every half hour, and
//! when the window comes forward, while the "updates" module is on) only
//! finds a newer version. Stackyard
//! then says so with a system notification, a tray menu item and a notice
//! in the window, and the person installs it from there or from Settings.
//!
//! What a release brings comes from `CHANGELOG.md`: the release workflow
//! puts the new version's section in `latest.json`, and the section for
//! this build's own version is built in, shown once after an update.

use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use stackyard_core::engine::Engine;
use stackyard_core::settings::Settings;
use tauri::{AppHandle, Emitter, Manager, Runtime};
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_updater::{Update, UpdaterExt};

const EVERY: Duration = Duration::from_secs(30 * 60);
/// A window coming forward checks again at most this often.
const ON_FOCUS: Duration = Duration::from_secs(10 * 60);
const CHANGELOG: &str = include_str!("../../CHANGELOG.md");

#[derive(Default)]
pub struct Updates(Mutex<State>);

impl Updates {
    /// `just_updated`: this start follows an update made from the app.
    pub fn new(just_updated: bool) -> Self {
        Updates(Mutex::new(State {
            just_updated,
            ..State::default()
        }))
    }
}

#[derive(Default)]
struct State {
    found: Option<Update>,
    checking: bool,
    installing: bool,
    checked_at: Option<u64>,
    /// When the last check began, successful or not.
    started_at: Option<std::time::Instant>,
    problem: Option<String>,
    /// The version the person was last told about, so each one is
    /// announced once per run.
    announced: Option<String>,
    just_updated: bool,
}

#[derive(Clone, Serialize)]
pub struct Available {
    pub version: String,
    pub notes: Option<String>,
}

#[derive(Clone, Serialize)]
pub struct UpdateView {
    pub current: String,
    /// This build knows the key updates are signed with.
    pub configured: bool,
    pub checking: bool,
    pub installing: bool,
    pub available: Option<Available>,
    /// Milliseconds since 1970.
    pub checked_at: Option<u64>,
    pub problem: Option<String>,
    /// What this version brought, from the changelog.
    pub whats_new: Option<String>,
    /// Stackyard has just restarted into this version after an update.
    pub just_updated: bool,
}

#[derive(Clone, Serialize)]
struct Progress {
    done: u64,
    total: Option<u64>,
}

/// Whether this build can check: it was given the public key.
pub fn configured<R: Runtime>(app: &AppHandle<R>) -> bool {
    app.config()
        .plugins
        .0
        .get("updater")
        .and_then(|u| u.get("pubkey"))
        .and_then(|k| k.as_str())
        .is_some_and(|k| !k.trim().is_empty())
}

pub fn view<R: Runtime>(app: &AppHandle<R>) -> UpdateView {
    let state = app.state::<Updates>();
    let state = state.0.lock().unwrap();
    let current = app.package_info().version.to_string();
    UpdateView {
        whats_new: notes_for(CHANGELOG, &current),
        just_updated: state.just_updated,
        current,
        configured: configured(app),
        checking: state.checking,
        installing: state.installing,
        available: state.found.as_ref().map(|u| Available {
            version: u.version.clone(),
            notes: u.body.clone().filter(|b| !b.trim().is_empty()),
        }),
        checked_at: state.checked_at,
        problem: state.problem.clone(),
    }
}

/// Looks for a newer version and remembers what it found.
pub async fn check<R: Runtime>(app: &AppHandle<R>) -> UpdateView {
    if !configured(app) {
        return view(app);
    }
    {
        let state = app.state::<Updates>();
        let mut state = state.0.lock().unwrap();
        if state.checking || state.installing {
            drop(state);
            return view(app);
        }
        state.checking = true;
        state.started_at = Some(std::time::Instant::now());
    }
    let result = match app.updater() {
        Ok(updater) => updater.check().await,
        Err(e) => Err(e),
    };
    {
        let state = app.state::<Updates>();
        let mut state = state.0.lock().unwrap();
        state.checking = false;
        state.checked_at = Some(now_ms());
        match result {
            Ok(found) => {
                state.found = found;
                state.problem = None;
            }
            Err(e) => state.problem = Some(explain(&e)),
        }
    }
    let view = view(app);
    // The window shows when it last checked, and the update when there is one.
    let _ = app.emit("update-available", view.clone());
    if let Some(available) = &view.available {
        let first_time = {
            let state = app.state::<Updates>();
            let mut state = state.0.lock().unwrap();
            let first = state.announced.as_deref() != Some(available.version.as_str());
            state.announced = Some(available.version.clone());
            first
        };
        if first_time {
            announce(app, &available.version);
        }
    }
    view
}

/// Tells the person about a new version: in the tray menu, and with a
/// system notification unless they are looking at Stackyard already (the
/// window shows its own notice).
fn announce<R: Runtime>(app: &AppHandle<R>, version: &str) {
    crate::set_tray_update(app, Some(version));
    let looking = app
        .get_webview_window("main")
        .is_some_and(|w| w.is_visible().unwrap_or(false) && w.is_focused().unwrap_or(false));
    if looking {
        return;
    }
    let shown = app
        .notification()
        .builder()
        .title(format!("Stackyard {version} is available"))
        .body("Open Stackyard to see what's new and update.")
        .show();
    if let Err(e) = shown {
        eprintln!("could not show the update notification: {e}");
    }
}

/// The person has seen what this version brought.
pub fn seen_whats_new<R: Runtime>(app: &AppHandle<R>) -> UpdateView {
    app.state::<Updates>().0.lock().unwrap().just_updated = false;
    view(app)
}

/// Downloads the update, stops every service, installs it and starts the
/// new version. On Windows the installer closes Stackyard itself.
pub async fn install<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    let update = {
        let state = app.state::<Updates>();
        let mut state = state.0.lock().unwrap();
        if state.installing {
            return Err("The update is already being installed.".into());
        }
        let Some(update) = state.found.clone() else {
            return Err("There is no update to install. Check for updates first.".into());
        };
        state.installing = true;
        update
    };
    let mut done = 0u64;
    let emitter = app.clone();
    let bytes = update
        .download(
            |chunk, total| {
                done += chunk as u64;
                let _ = emitter.emit("update-progress", Progress { done, total });
            },
            || {},
        )
        .await;
    let result = bytes.map_err(|e| explain(&e)).and_then(|bytes| {
        let engine = app.state::<Arc<Engine>>();
        crate::wordpress::set_quitting(true);
        remember_running(&engine.running());
        let _ = std::fs::write(updated_file(), app.package_info().version.to_string());
        engine.shutdown();
        update.install(bytes).map_err(|e| {
            crate::wordpress::set_quitting(false);
            let _ = std::fs::remove_file(resume_file());
            let _ = std::fs::remove_file(updated_file());
            explain(&e)
        })
    });
    match result {
        Ok(()) => app.restart(),
        Err(message) => {
            let state = app.state::<Updates>();
            let mut state = state.0.lock().unwrap();
            state.installing = false;
            state.problem = Some(message.clone());
            Err(message)
        }
    }
}

fn resume_file() -> std::path::PathBuf {
    stackyard_core::paths::home()
        .join("run")
        .join("resume-after-update.json")
}

fn updated_file() -> std::path::PathBuf {
    stackyard_core::paths::home()
        .join("run")
        .join("updated-from.txt")
}

/// Whether this start follows an update installed from the app, once: the
/// record is removed as it is read.
pub fn take_just_updated() -> bool {
    let file = updated_file();
    let found = file.exists();
    let _ = std::fs::remove_file(file);
    found
}

/// The section of the changelog headed `## <version>`, without the heading.
fn notes_for(changelog: &str, version: &str) -> Option<String> {
    let mut lines = changelog.lines();
    lines.find(|line| {
        line.strip_prefix("## ")
            .and_then(|rest| rest.trim().strip_prefix(version))
            .is_some_and(|after| after.is_empty() || after.starts_with([' ', '(', '-']))
    })?;
    let section: Vec<&str> = lines.take_while(|line| !line.starts_with("## ")).collect();
    let text = section.join("\n").trim().to_string();
    (!text.is_empty()).then_some(text)
}

fn remember_running(ids: &[String]) {
    let file = resume_file();
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(json) = serde_json::to_string(ids) {
        let _ = std::fs::write(file, json);
    }
}

/// The services that ran before an update restarted Stackyard, once: the
/// record is removed as it is read.
pub fn take_resume() -> Vec<String> {
    let file = resume_file();
    let ids = std::fs::read_to_string(&file)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default();
    let _ = std::fs::remove_file(file);
    ids
}

/// Checks right after start, then every half hour, while the "updates"
/// module is on.
pub fn watch<R: Runtime>(app: &AppHandle<R>) {
    if !configured(app) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio_sleep(Duration::from_secs(3)).await;
        loop {
            let on = Settings::load().is_ok_and(|s| s.module_enabled("updates"));
            if on {
                check(&app).await;
            }
            tokio_sleep(EVERY).await;
        }
    });
}

/// The window came forward: check again unless a check began in the last
/// ten minutes, so someone who left Stackyard open sees a new release
/// without restarting it.
pub fn on_focus<R: Runtime>(app: &AppHandle<R>) {
    if !configured(app) {
        return;
    }
    let due = {
        let state = app.state::<Updates>();
        let state = state.0.lock().unwrap();
        !state.checking
            && !state.installing
            && state.started_at.is_some_and(|at| at.elapsed() >= ON_FOCUS)
    };
    if !due || !Settings::load().is_ok_and(|s| s.module_enabled("updates")) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        check(&app).await;
    });
}

async fn tokio_sleep(duration: Duration) {
    // A blocking sleep on a worker thread keeps tokio's timer feature out.
    let _ = tauri::async_runtime::spawn_blocking(move || std::thread::sleep(duration)).await;
}

fn explain(e: &tauri_plugin_updater::Error) -> String {
    use tauri_plugin_updater::Error;
    match e {
        Error::Reqwest(_) | Error::Network(_) => {
            "Couldn't reach the update server. Check your connection and try again.".into()
        }
        Error::Minisign(_) | Error::SignatureUtf8(_) | Error::Base64(_) => {
            "The update's signature doesn't match Stackyard's key, so it was not installed.".into()
        }
        Error::ReleaseNotFound | Error::TargetNotFound(_) | Error::TargetsNotFound(..) => {
            "No update is published for this system yet.".into()
        }
        other => other.to_string(),
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOG: &str = "# What's new\n\nIntro.\n\n## 0.3.10\n\n- Ten.\n\n## 0.3.1 (2026-10-16)\n\n- One.\n- Two.\n\n## 0.3.0\n\n## 0.2.0\n- Old.\n";

    #[test]
    fn finds_a_version_section() {
        assert_eq!(notes_for(LOG, "0.3.1").as_deref(), Some("- One.\n- Two."));
        assert_eq!(notes_for(LOG, "0.3.10").as_deref(), Some("- Ten."));
        assert_eq!(notes_for(LOG, "0.2.0").as_deref(), Some("- Old."));
        assert_eq!(notes_for(LOG, "0.3.0"), None);
        assert_eq!(notes_for(LOG, "0.4.0"), None);
    }
}
