//! Commands for a site's own settings, its backups, moving it to another
//! computer as one file, and public links. Anything slow runs on a worker
//! thread so the window stays responsive.

use std::path::PathBuf;
use std::sync::Arc;

use serde::Serialize;
use stackyard_core::catalog::RuntimeKind;
use stackyard_core::detect;
use stackyard_core::engine::Engine;
use stackyard_core::services::ServiceManager;
use stackyard_core::settings::Settings;
use stackyard_core::sitepack::{self, ImportOptions, Packed, SiteBackup};
use stackyard_core::sites::{Site, SiteDatabase, SiteSettings, SiteStore};
use stackyard_core::{paths, share};
use tauri::{AppHandle, Manager, State};

use crate::{err, site_views, sites_changed, CmdResult, SiteView};

/// A site by its domain, saved or parked.
pub(crate) fn find_site(domain: &str) -> CmdResult<(Site, bool)> {
    ServiceManager::open()
        .user_sites()
        .map_err(err)?
        .into_iter()
        .find(|s| s.domain() == domain)
        .map(|s| (s.site, s.parked))
        .ok_or_else(|| format!("no site named {domain}"))
}

fn require(module: &str) -> CmdResult<()> {
    Settings::load()
        .and_then(|s| s.require_module(module))
        .map_err(err)
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> CmdResult<T> + Send + 'static,
) -> CmdResult<T> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|e| e.to_string())?
}

#[derive(Serialize)]
pub struct SiteSettingsView {
    domain: String,
    tld: String,
    https: bool,
    settings: SiteSettings,
    /// Found in a parked folder, so its settings can't be changed until it is added.
    parked: bool,
    /// The folder served when none is picked, inside the project, e.g. `public`.
    detected_root: String,
    /// The database found in the project's `.env` or `wp-config.php`.
    detected_database: Option<SiteDatabase>,
    /// PHP settings apply (a PHP site).
    php: bool,
    /// The port a start command gets in `PORT`, if the site has one.
    port: Option<u16>,
}

#[tauri::command]
pub fn site_settings(domain: String) -> CmdResult<SiteSettingsView> {
    let (site, parked) = find_site(&domain)?;
    let detected = detect::detect(&site.path);
    let detected_database = sitepack::site_database(&Site {
        settings: SiteSettings::default(),
        ..site.clone()
    });
    Ok(SiteSettingsView {
        domain,
        tld: site.tld.clone(),
        https: site.https,
        detected_root: detected.root.to_string_lossy().replace('\\', "/"),
        detected_database,
        php: site.runtime.kind() == Some(RuntimeKind::Php),
        port: site.runtime.port(),
        parked,
        settings: site.settings,
    })
}

#[derive(Serialize)]
pub struct SavedSettings {
    sites: Vec<SiteView>,
    /// The site's domain after the change.
    domain: String,
}

/// Saves a site's domain ending, HTTPS and settings, then restarts what
/// serves it.
#[tauri::command]
pub fn save_site_settings(
    app: AppHandle,
    domain: String,
    tld: String,
    https: bool,
    settings: SiteSettings,
) -> CmdResult<SavedSettings> {
    require("sitesettings")?;
    let (_, parked) = find_site(&domain)?;
    if parked {
        return Err("This site comes from a parked folder. Add its folder as a site to change its settings.".into());
    }
    let mut store = SiteStore::load().map_err(err)?;
    let (before, after) = store
        .configure(&domain, &tld, https, settings)
        .map_err(err)?;
    store.save().map_err(err)?;
    if after.domain() != before.domain() {
        crate::remote::site_renamed(&before.domain(), &after.domain());
        stackyard_core::wordpress::env::rename(
            &stackyard_core::paths::home(),
            &before.domain(),
            &after.domain(),
        );
    }
    // PHP reads its settings only when it starts, so the site's own PHP
    // process starts again with the new ones.
    let engine = app.state::<Arc<Engine>>().inner().clone();
    if before.settings.php != after.settings.php {
        let own = format!("-{}", after.domain());
        for (id, state) in engine.supervisor().states() {
            let stopped = state == stackyard_core::supervisor::ProcessState::Stopped;
            if id.starts_with("php-") && id.ends_with(&own) && !stopped {
                let _ = engine.supervisor().stop(&id);
            }
        }
    }
    // A link to the old address stops; the new one is made by hand.
    if after.domain() != before.domain() || after.https != before.https {
        let _ = share::stop(engine.supervisor(), &before);
    }
    sites_changed(&app);
    Ok(SavedSettings {
        sites: site_views()?,
        domain: after.domain(),
    })
}

#[tauri::command]
pub fn site_backups(domain: String) -> Vec<SiteBackup> {
    sitepack::backups(&paths::home(), &domain)
}

#[tauri::command]
pub async fn backup_site(domain: String) -> CmdResult<Packed> {
    require("sitebackups")?;
    let (site, _) = find_site(&domain)?;
    blocking(move || sitepack::backup(&paths::home(), &site).map_err(err)).await
}

/// Puts a backup back; the site as it was is backed up first.
#[tauri::command]
pub async fn restore_site(app: AppHandle, domain: String, file: String) -> CmdResult<Packed> {
    require("sitebackups")?;
    let (site, _) = find_site(&domain)?;
    let home = paths::home();
    let path = sitepack::backup_path(&home, &domain, &file).map_err(err)?;
    let packed = blocking(move || sitepack::restore(&home, &site, &path).map_err(err)).await?;
    sites_changed(&app);
    Ok(packed)
}

#[tauri::command]
pub fn remove_site_backup(domain: String, file: String) -> CmdResult<Vec<SiteBackup>> {
    let home = paths::home();
    sitepack::remove_backup(&home, &domain, &file).map_err(err)?;
    Ok(sitepack::backups(&home, &domain))
}

/// Opens the folder the site's backups are in.
#[tauri::command]
pub fn open_site_backups(domain: String) -> CmdResult<()> {
    let dir = sitepack::backups_dir(&paths::home()).join(&domain);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    open_with_system(dir.as_os_str())
}

#[tauri::command]
pub async fn export_site(domain: String, dest: String) -> CmdResult<Packed> {
    require("sitetransfer")?;
    let (site, _) = find_site(&domain)?;
    let mut dest = PathBuf::from(dest);
    if dest.extension().is_none() {
        dest.set_extension("zip");
    }
    blocking(move || sitepack::pack(&paths::home(), &site, &dest).map_err(err)).await
}

#[derive(Serialize)]
pub struct SiteFileView {
    name: String,
    domain: String,
    runtime: Option<String>,
    database: Option<String>,
    created: u64,
    made_by: String,
    /// Where the files go when no folder is picked.
    folder: String,
}

/// What a site file holds, before it is imported.
#[tauri::command]
pub fn inspect_site_file(path: String) -> CmdResult<SiteFileView> {
    let manifest = sitepack::read_manifest(std::path::Path::new(&path)).map_err(err)?;
    let settings = Settings::load().map_err(err)?;
    let site = &manifest.site;
    let runtime = site.runtime.kind().map(|kind| {
        let name = stackyard_core::catalog::runtime(kind).name;
        format!("{name} {}", site.runtime.version().unwrap_or_default())
    });
    Ok(SiteFileView {
        domain: stackyard_core::domain::site_domain(&site.name, &site.tld),
        folder: settings
            .sites_folder(&paths::home())
            .join(&site.name)
            .display()
            .to_string(),
        name: site.name.clone(),
        runtime,
        database: manifest.database.map(|d| d.name),
        created: manifest.created,
        made_by: manifest.made_by,
    })
}

#[derive(Serialize)]
pub struct ImportedView {
    sites: Vec<SiteView>,
    domain: String,
    warning: Option<String>,
}

#[tauri::command]
pub async fn import_site(
    app: AppHandle,
    path: String,
    name: Option<String>,
    folder: Option<String>,
) -> CmdResult<ImportedView> {
    require("sitetransfer")?;
    let options = ImportOptions {
        name: name.map(|n| n.trim().to_string()),
        path: folder.filter(|f| !f.trim().is_empty()).map(PathBuf::from),
    };
    let (site, warning) = blocking(move || {
        sitepack::import(&paths::home(), std::path::Path::new(&path), options).map_err(err)
    })
    .await?;
    sites_changed(&app);
    Ok(ImportedView {
        sites: site_views()?,
        domain: site.domain(),
        warning,
    })
}

#[derive(Serialize)]
pub struct ShareView {
    /// The downloaded cloudflared's version; `None` until it is downloaded.
    cloudflared: Option<String>,
    links: Vec<share::Link>,
    /// Whether the web server is running, which links need.
    web_running: bool,
}

fn share_view(engine: &Engine) -> CmdResult<ShareView> {
    let home = paths::home();
    let sites: Vec<Site> = ServiceManager::open()
        .user_sites()
        .map_err(err)?
        .into_iter()
        .map(|s| s.site)
        .collect();
    Ok(ShareView {
        cloudflared: share::version(&home),
        links: share::links(engine.supervisor(), &sites),
        web_running: matches!(
            engine.state("caddy"),
            stackyard_core::supervisor::ProcessState::Running { .. }
        ),
    })
}

#[tauri::command]
pub fn share_status(engine: State<'_, Arc<Engine>>) -> CmdResult<ShareView> {
    share_view(&engine)
}

#[tauri::command]
pub async fn share_install(app: AppHandle) -> CmdResult<ShareView> {
    require("share")?;
    blocking(|| {
        share::install(&paths::home(), &mut |_| {})
            .map(|_| ())
            .map_err(err)
    })
    .await?;
    share_view(&app.state::<Arc<Engine>>())
}

#[tauri::command]
pub fn share_uninstall(engine: State<'_, Arc<Engine>>) -> CmdResult<ShareView> {
    for link in share_view(&engine)?.links {
        if let Ok((site, _)) = find_site(&link.site) {
            let _ = share::stop(engine.supervisor(), &site);
        }
    }
    share::uninstall(&paths::home()).map_err(err)?;
    share_view(&engine)
}

/// Makes a public link to a site and waits for its address.
#[tauri::command]
pub async fn share_start(app: AppHandle, domain: String) -> CmdResult<share::Link> {
    require("share")?;
    let (site, _) = find_site(&domain)?;
    let engine = app.state::<Arc<Engine>>().inner().clone();
    if !matches!(
        engine.state("caddy"),
        stackyard_core::supervisor::ProcessState::Running { .. }
    ) {
        return Err(
            "Start the web server first (Services › Caddy), so the link has something to show."
                .into(),
        );
    }
    blocking(move || {
        let home = paths::home();
        if share::program(&home).is_none() {
            share::install(&home, &mut |_| {}).map_err(err)?;
        }
        share::start(&home, engine.supervisor(), &site).map_err(err)
    })
    .await
}

/// Opens a site's public link in the browser. Only the address cloudflared
/// reported is opened, never one passed in.
#[tauri::command]
pub fn open_share_link(engine: State<'_, Arc<Engine>>, domain: String) -> CmdResult<()> {
    let link = share_view(&engine)?
        .links
        .into_iter()
        .find(|l| l.site == domain)
        .and_then(|l| l.url)
        .ok_or_else(|| format!("{domain} has no public link"))?;
    open_with_system(std::ffi::OsStr::new(&link))
}

/// Opens a folder in the file manager, or an address in the browser.
fn open_with_system(target: &std::ffi::OsStr) -> CmdResult<()> {
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
    cmd.arg(target)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("could not open {}: {e}", target.to_string_lossy()))
}

#[tauri::command]
pub fn share_stop(engine: State<'_, Arc<Engine>>, domain: String) -> CmdResult<ShareView> {
    let (site, _) = find_site(&domain)?;
    share::stop(engine.supervisor(), &site).map_err(err)?;
    share_view(&engine)
}
