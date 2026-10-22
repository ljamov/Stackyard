//! Commands for the WordPress tab of a site's tools. Each runs on a worker
//! thread: WP-CLI and database dumps can take a while.

use std::path::PathBuf;

use serde::Serialize;
use stackyard_core::paths;
use stackyard_core::settings::Settings;
use stackyard_core::wordpress::{
    self, woo, CoreUpdate, DebugSettings, DiskUse, Extension, ExtensionAction, Kind, LogTail,
    Overview, Replaced, User, WpSite,
};

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
pub async fn wp_extensions(domain: String, kind: Kind) -> CmdResult<Vec<Extension>> {
    with_site(domain, move |home, wp| {
        wordpress::extensions(home, wp, kind)
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
        wordpress::extensions(home, wp, kind)
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
        wordpress::extensions(home, wp, kind)
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
