//! Commands for the AI tools screen: install, update, remove and open AI
//! coding tools. npm runs on a worker thread and reports as `ai-progress`
//! events.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::Serialize;
use stackyard_core::ai::{self, Status};
use stackyard_core::paths;
use stackyard_core::settings::Settings;
use tauri::{AppHandle, Emitter, Manager};

use crate::{err, CmdResult};

/// Tools being installed or updated, so a double click cannot start two.
#[derive(Default)]
pub struct AiInstalls(Mutex<HashSet<String>>);

#[derive(Serialize)]
pub struct AiToolView {
    #[serde(flatten)]
    status: Status,
    installing: bool,
}

#[derive(Clone, Serialize)]
struct AiProgress {
    id: String,
    text: String,
}

fn views(app: &AppHandle, statuses: Vec<Status>) -> Vec<AiToolView> {
    let busy = app.state::<AiInstalls>().0.lock().unwrap().clone();
    statuses
        .into_iter()
        .map(|status| AiToolView {
            installing: busy.contains(status.tool.id),
            status,
        })
        .collect()
}

fn require_ai() -> CmdResult<()> {
    Settings::load()
        .map_err(err)?
        .require_module("ai")
        .map_err(err)
}

/// The tools and which are installed; no network.
#[tauri::command]
pub fn ai_tools(app: AppHandle) -> Vec<AiToolView> {
    views(&app, ai::list(&paths::home()))
}

/// Like [`ai_tools`], with the newest version of each from npm.
#[tauri::command]
pub async fn ai_tools_check(app: AppHandle) -> CmdResult<Vec<AiToolView>> {
    let statuses = tauri::async_runtime::spawn_blocking(|| {
        let mut statuses = ai::list(&paths::home());
        ai::check_updates(&mut statuses);
        statuses
    })
    .await
    .map_err(|e| e.to_string())?;
    Ok(views(&app, statuses))
}

/// Installs a tool, or updates it to the newest version.
#[tauri::command]
pub async fn ai_tool_install(app: AppHandle, id: String) -> CmdResult<Vec<AiToolView>> {
    require_ai()?;
    let tool = ai::find(&id).map_err(err)?;
    if !app
        .state::<AiInstalls>()
        .0
        .lock()
        .unwrap()
        .insert(id.clone())
    {
        return Err(format!("{} is already being installed", tool.name));
    }
    let worker = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        ai::install(&paths::home(), tool, &mut |text| {
            let _ = worker.emit(
                "ai-progress",
                AiProgress {
                    id: tool.id.to_string(),
                    text,
                },
            );
        })
    })
    .await;
    app.state::<AiInstalls>().0.lock().unwrap().remove(&id);
    result.map_err(|e| e.to_string())?.map_err(err)?;
    Ok(views(&app, ai::list(&paths::home())))
}

#[tauri::command]
pub fn ai_tool_remove(app: AppHandle, id: String) -> CmdResult<Vec<AiToolView>> {
    let tool = ai::find(&id).map_err(err)?;
    ai::uninstall(&paths::home(), tool).map_err(err)?;
    Ok(views(&app, ai::list(&paths::home())))
}

/// Opens the tool's own install and sign-in guide in the browser. Only the
/// guides Stackyard lists can be opened.
#[tauri::command]
pub fn ai_tool_docs(id: String) -> CmdResult<()> {
    let url = ai::find(&id).map_err(err)?.docs;
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
    cmd.arg(url)
        .spawn()
        .map(drop)
        .map_err(|e| format!("could not open {url}: {e}"))
}

/// Opens a terminal window in `folder` running the tool.
#[tauri::command]
pub async fn ai_tool_open(id: String, folder: String) -> CmdResult<()> {
    let tool = ai::find(&id).map_err(err)?;
    tauri::async_runtime::spawn_blocking(move || {
        ai::open(&paths::home(), tool, &PathBuf::from(folder))
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(err)
}
