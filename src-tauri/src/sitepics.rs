//! Commands for a site's picture (Sites screen, WordPress view) and for the
//! database server it needs.

use std::path::Path;
use std::sync::{Arc, Mutex};

use base64::Engine as _;
use stackyard_core::databases::Client as DbClient;
use stackyard_core::engine::Engine;
use stackyard_core::settings::Settings;
use stackyard_core::sitedb::{self, SiteDb};
use stackyard_core::supervisor::ProcessState;
use stackyard_core::wordpress::{self, WpSite};
use stackyard_core::{paths, sitesize, thumbnails};
use tauri::{AppHandle, Manager};

use crate::{err, site_views, sitetools::find_site, CmdResult};

/// One browser at a time takes screenshots.
static TAKING: Mutex<()> = Mutex::new(());

fn data_url(jpeg: &[u8]) -> String {
    format!(
        "data:image/jpeg;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(jpeg)
    )
}

fn read_url(path: &Path) -> Option<String> {
    std::fs::read(path).ok().map(|b| data_url(&b))
}

/// A picture of the site's home page, as a `data:` URL; `None` when there
/// is none to show. A missing or day-old picture is taken again while the
/// web server runs (or always with `refresh`); a WordPress site without
/// one shows its theme's picture. With `refresh`, failing to take one is an
/// error that says why, so the person who asked knows what to fix.
#[tauri::command]
pub async fn site_thumbnail(
    app: AppHandle,
    domain: String,
    refresh: bool,
) -> CmdResult<Option<String>> {
    let engine = app.state::<Arc<Engine>>().inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let home = paths::home();
        // Pictures kept by earlier versions could be blank white boxes.
        if let Some((path, _)) = thumbnails::cached(&home, &domain) {
            if std::fs::read(&path).is_ok_and(|b| thumbnails::is_blank(&b)) {
                thumbnails::forget(&home, &domain);
            }
        }
        let cached = thumbnails::cached(&home, &domain);
        if let Some((path, true)) = &cached {
            if !refresh {
                return Ok(read_url(path));
            }
        }
        // A site without its database would only show its error page.
        let caddy = matches!(engine.state("caddy"), ProcessState::Running { .. });
        let database = status_of(&engine, &domain)
            .ok()
            .flatten()
            .is_none_or(|db| db.running);
        let why = if !caddy {
            "start the web server (Caddy) first".to_string()
        } else if !database {
            "start the site's database server first".to_string()
        } else if let Some(browser) = thumbnails::browser() {
            let url = site_views()?
                .into_iter()
                .find(|s| s.domain == domain)
                .map(|s| s.local_url)
                .ok_or_else(|| format!("no site named {domain}"))?;
            let _one = TAKING.lock().unwrap_or_else(|e| e.into_inner());
            match thumbnails::capture(&home, &browser, &domain, &url) {
                Ok(path) => return Ok(read_url(&path)),
                Err(e) => {
                    eprintln!("picture of {domain}: {e}");
                    e.to_string()
                }
            }
        } else {
            "no Edge, Chrome, Chromium or Brave was found to take it".to_string()
        };
        // Asked for a new picture: say why there is none.
        if refresh {
            return Err(format!("No picture of {domain}: {why}"));
        }
        // A blank page leaves no picture behind; look again.
        let cached = thumbnails::cached(&home, &domain);
        if let Some((path, _)) = cached {
            return Ok(read_url(&path));
        }
        Ok(theme_picture(&home, &domain))
    })
    .await
    .map_err(|e| e.to_string())?
}

fn theme_picture(home: &Path, domain: &str) -> Option<String> {
    let (site, _) = find_site(domain).ok()?;
    let wp = WpSite::open(&site).ok()?;
    let file = wordpress::active_theme_screenshot(home, &wp)?;
    let jpeg = thumbnails::small_jpeg(&std::fs::read(file).ok()?, 480, 300).ok()?;
    Some(data_url(&jpeg))
}

/// A theme's own picture, as a `data:` URL, for the Themes list.
#[tauri::command]
pub async fn wp_theme_screenshot(domain: String, theme: String) -> CmdResult<Option<String>> {
    tauri::async_runtime::spawn_blocking(move || {
        let (site, _) = find_site(&domain)?;
        let wp = WpSite::open(&site).map_err(err)?;
        Ok(wordpress::theme_screenshot(&wp, &theme)
            .and_then(|file| std::fs::read(file).ok())
            .and_then(|bytes| thumbnails::small_jpeg(&bytes, 480, 360).ok())
            .map(|jpeg| data_url(&jpeg)))
    })
    .await
    .map_err(|e| e.to_string())?
}

fn status_of(engine: &Engine, domain: &str) -> CmdResult<Option<SiteDb>> {
    let (site, _) = find_site(domain)?;
    let running = |id: &str| matches!(engine.state(id), ProcessState::Running { .. });
    Ok(sitedb::status(&paths::home(), &site, &running))
}

/// The database a site uses, the server that holds it and whether it runs.
#[tauri::command]
pub fn site_database(app: AppHandle, domain: String) -> CmdResult<Option<SiteDb>> {
    status_of(&app.state::<Arc<Engine>>(), &domain)
}

/// The site's size as last measured, at once; `None` until it is measured.
#[tauri::command]
pub fn site_size(domain: String) -> Option<sitesize::SiteSize> {
    sitesize::cached(&paths::home(), &domain)
}

/// Every site's size as last measured, by domain, for the Sites list.
#[tauri::command]
pub fn site_sizes() -> std::collections::BTreeMap<String, sitesize::SiteSize> {
    sitesize::all_cached(&paths::home())
}

/// Measures the site's files and database now (on a worker thread) and keeps the answer.
#[tauri::command]
pub async fn measure_site_size(app: AppHandle, domain: String) -> CmdResult<sitesize::SiteSize> {
    let engine = app.state::<Arc<Engine>>().inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let (site, _) = find_site(&domain)?;
        let db = status_of(&engine, &domain)?;
        let home = paths::home();
        // PostgreSQL has no folder per database; the running server says.
        let running = db.as_ref().is_some_and(|d| d.running);
        let pg_bytes = |name: &str| {
            if !running {
                return None;
            }
            DbClient::open_server(&home, Some("postgres"))
                .and_then(|c| c.databases())
                .ok()?
                .into_iter()
                .find(|d| d.name == name)
                .map(|d| d.size)
        };
        sitesize::measure(&home, &site, db.as_ref(), &pg_bytes).map_err(err)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Starts the server with the site's database (stopping the other of MySQL
/// and MariaDB first when it has the port).
#[tauri::command]
pub async fn start_site_database(app: AppHandle, domain: String) -> CmdResult<Option<SiteDb>> {
    let engine = app.state::<Arc<Engine>>().inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let (site, _) = find_site(&domain)?;
        engine.start_site_database(&site).map_err(err)?;
        status_of(&engine, &domain)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Whether the site's database server starts along with the web server.
#[tauri::command]
pub fn set_site_database_autostart(
    app: AppHandle,
    domain: String,
    on: bool,
) -> CmdResult<Option<SiteDb>> {
    let mut settings = Settings::load().map_err(err)?;
    if on {
        settings.start_databases.insert(domain.clone());
    } else {
        settings.start_databases.remove(&domain);
    }
    settings.save().map_err(err)?;
    status_of(&app.state::<Arc<Engine>>(), &domain)
}
