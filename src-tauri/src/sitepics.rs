//! Commands for a site's picture (Sites screen, WordPress view) and for the
//! database server it needs.

use std::path::Path;
use std::sync::{Arc, Mutex};

use base64::Engine as _;
use stackyard_core::engine::Engine;
use stackyard_core::settings::Settings;
use stackyard_core::sitedb::{self, SiteDb};
use stackyard_core::supervisor::ProcessState;
use stackyard_core::wordpress::{self, WpSite};
use stackyard_core::{paths, thumbnails};
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
/// one shows its theme's picture.
#[tauri::command]
pub async fn site_thumbnail(
    app: AppHandle,
    domain: String,
    refresh: bool,
) -> CmdResult<Option<String>> {
    let engine = app.state::<Arc<Engine>>().inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let home = paths::home();
        let cached = thumbnails::cached(&home, &domain);
        if let Some((path, true)) = &cached {
            if !refresh {
                return Ok(read_url(path));
            }
        }
        // A site without its database would only show its error page.
        let serving = matches!(engine.state("caddy"), ProcessState::Running { .. })
            && status_of(&engine, &domain)
                .ok()
                .flatten()
                .is_none_or(|db| db.running);
        if serving {
            if let Some(browser) = thumbnails::browser() {
                let url = site_views()?
                    .into_iter()
                    .find(|s| s.domain == domain)
                    .map(|s| s.local_url)
                    .ok_or_else(|| format!("no site named {domain}"))?;
                let _one = TAKING.lock().unwrap_or_else(|e| e.into_inner());
                match thumbnails::capture(&home, &browser, &domain, &url) {
                    Ok(path) => return Ok(read_url(&path)),
                    Err(e) => eprintln!("picture of {domain}: {e}"),
                }
            }
        }
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
