//! Commands for a site's own settings, its backups, moving it to another
//! computer as one file, and public links. Anything slow runs on a worker
//! thread so the window stays responsive.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Serialize;
use stackyard_core::catalog::RuntimeKind;
use stackyard_core::engine::Engine;
use stackyard_core::services::ServiceManager;
use stackyard_core::settings::Settings;
use stackyard_core::sitepack::{self, ImportOptions, PackStep, Packed, SiteBackup};
use stackyard_core::sites::{Site, SiteDatabase, SiteSettings, SiteStore};
use stackyard_core::{detect, editors};
use stackyard_core::{paths, share};
use tauri::{AppHandle, Emitter, Manager, State};

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

/// What a site backup, export, restore or import is doing, sent to the
/// window as `site-pack-progress`.
#[derive(Clone, Serialize)]
struct PackProgress {
    /// The site's domain, or the file being imported.
    site: String,
    #[serde(flatten)]
    step: PackStep,
}

/// Sends pack steps to the window: every new stage and the last file, and
/// otherwise at most ten a second, so a big site doesn't flood it.
fn pack_progress(app: &AppHandle, site: String) -> impl FnMut(PackStep) + Send + 'static {
    let app = app.clone();
    let mut last: Option<(PackStep, Instant)> = None;
    move |step| {
        let finished = matches!(
            step,
            PackStep::Files { done, total } | PackStep::Unpacking { done, total } if done == total
        );
        let due = match last {
            Some((before, at)) => {
                std::mem::discriminant(&before) != std::mem::discriminant(&step)
                    || finished
                    || at.elapsed() >= Duration::from_millis(100)
            }
            None => true,
        };
        if due {
            last = Some((step, Instant::now()));
            let _ = app.emit(
                "site-pack-progress",
                PackProgress {
                    site: site.clone(),
                    step,
                },
            );
        }
    }
}

#[derive(Serialize)]
pub struct SiteSettingsView {
    domain: String,
    tld: String,
    https: bool,
    settings: SiteSettings,
    /// Found in a parked folder; saving its settings saves it as a site.
    parked: bool,
    /// The folder served when none is picked, inside the project, e.g. `public`.
    detected_root: String,
    /// The database found in the project's `.env` or `wp-config.php`.
    detected_database: Option<SiteDatabase>,
    /// PHP settings apply (a PHP site).
    php: bool,
    /// The port a start command gets in `PORT`, if the site has one.
    port: Option<u16>,
    /// The project folder.
    folder: String,
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
        folder: site.path.display().to_string(),
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
    // One from a parked folder is saved first, like one made in the app.
    let mut store = crate::saved_store(&domain)?;
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
        let home = stackyard_core::paths::home();
        if let Ok(mut order) = stackyard_core::siteorder::SiteOrder::load(&home) {
            order.rename(&before.domain(), &after.domain());
            let _ = order.save();
        }
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

/// Stops what runs from a site's folder (its dev or app server and its own
/// PHP process), so the folder can change; the web server starts them again.
pub(crate) fn stop_site_processes(engine: &Engine, site: &Site) {
    let dev = stackyard_core::web::dev_server_id(&site.key());
    let own = format!("-{}", site.domain());
    for (id, state) in engine.supervisor().states() {
        let stopped = state == stackyard_core::supervisor::ProcessState::Stopped;
        let its_own = id == dev || (id.starts_with("php-") && id.ends_with(&own));
        if its_own && !stopped {
            let _ = engine.supervisor().stop(&id);
        }
    }
}

/// A site that was added (not one from a parked folder), to change its folder.
fn added_site(domain: &str) -> CmdResult<Site> {
    let (site, parked) = find_site(domain)?;
    if parked {
        return Err("This site comes from a parked folder, so its folder is where it is. Move the folder in your file manager instead.".into());
    }
    Ok(site)
}

/// Points a site at another folder, e.g. a copy on another drive. Its
/// settings, database and connections stay as they are.
#[tauri::command]
pub fn change_site_folder(
    app: AppHandle,
    domain: String,
    path: String,
) -> CmdResult<Vec<SiteView>> {
    let site = added_site(&domain)?;
    let mut store = SiteStore::load().map_err(err)?;
    let engine = app.state::<Arc<Engine>>().inner().clone();
    stop_site_processes(&engine, &site);
    store
        .set_folder(&domain, std::path::Path::new(path.trim()))
        .map_err(err)?;
    store.save().map_err(err)?;
    stackyard_core::thumbnails::forget(&paths::home(), &domain);
    sites_changed(&app);
    site_views()
}

/// How a site move is going, sent to the window as `site-move-progress`.
#[derive(Clone, Serialize)]
struct MoveProgress {
    site: String,
    done: u64,
    total: u64,
}

#[derive(Serialize)]
pub struct MovedSite {
    sites: Vec<SiteView>,
    /// Where the site's folder is now.
    path: String,
    /// Some of the old folder could not be removed; the new one is complete.
    left_behind: Option<String>,
}

/// Moves a site's folder into `into` (e.g. a folder on another drive) and
/// points the site there. The old folder goes only once everything is copied.
#[tauri::command]
pub async fn move_site(app: AppHandle, domain: String, into: String) -> CmdResult<MovedSite> {
    let site = added_site(&domain)?;
    let engine = app.state::<Arc<Engine>>().inner().clone();
    let worker = app.clone();
    let name = domain.clone();
    let (path, left) = blocking(move || {
        stop_site_processes(&engine, &site);
        let mut last = Instant::now() - Duration::from_secs(1);
        let into = paths::expand_home(std::path::Path::new(into.trim()));
        stackyard_core::sites::move_folder(&site.path, &into, &mut |done, total| {
            if done == total || last.elapsed() >= Duration::from_millis(100) {
                last = Instant::now();
                let _ = worker.emit(
                    "site-move-progress",
                    MoveProgress {
                        site: name.clone(),
                        done,
                        total,
                    },
                );
            }
        })
        .map(|(path, left)| (path, left.then(|| site.path.display().to_string())))
        .map_err(err)
    })
    .await?;
    let mut store = SiteStore::load().map_err(err)?;
    store.set_folder(&domain, &path).map_err(err)?;
    store.save().map_err(err)?;
    sites_changed(&app);
    Ok(MovedSite {
        sites: site_views()?,
        path: path.display().to_string(),
        left_behind: left,
    })
}

#[tauri::command]
pub fn site_backups(domain: String) -> Vec<SiteBackup> {
    sitepack::backups(&paths::home(), &domain)
}

#[tauri::command]
pub async fn backup_site(app: AppHandle, domain: String) -> CmdResult<Packed> {
    require("sitebackups")?;
    let (site, _) = find_site(&domain)?;
    let mut progress = pack_progress(&app, domain);
    blocking(move || sitepack::backup(&paths::home(), &site, &mut progress).map_err(err)).await
}

/// Puts a backup back; the site as it was is backed up first.
#[tauri::command]
pub async fn restore_site(app: AppHandle, domain: String, file: String) -> CmdResult<Packed> {
    require("sitebackups")?;
    let (site, _) = find_site(&domain)?;
    let home = paths::home();
    let path = sitepack::backup_path(&home, &domain, &file).map_err(err)?;
    let mut progress = pack_progress(&app, domain);
    let packed =
        blocking(move || sitepack::restore(&home, &site, &path, &mut progress).map_err(err))
            .await?;
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

/// Opens the site's own folder in the file manager.
#[tauri::command]
pub fn open_site_folder(domain: String) -> CmdResult<()> {
    let (site, _) = find_site(&domain)?;
    open_with_system(stackyard_core::projects::folder(&site).as_os_str())
}

/// The code editors, which are installed, and the one in use.
#[tauri::command]
pub fn code_editors() -> CmdResult<editors::Editors> {
    let settings = Settings::load().map_err(err)?;
    Ok(editors::list(settings.editor.as_deref()))
}

/// Picks the editor Open in editor uses; `None` goes back to the default.
#[tauri::command]
pub fn set_code_editor(id: Option<String>) -> CmdResult<editors::Editors> {
    if let Some(id) = &id {
        editors::check(id).map_err(err)?;
    }
    let mut settings = Settings::load().map_err(err)?;
    settings.editor = id;
    settings.save().map_err(err)?;
    Ok(editors::list(settings.editor.as_deref()))
}

/// Opens the site's folder in the code editor picked in Settings.
#[tauri::command]
pub async fn open_site_in_editor(domain: String) -> CmdResult<()> {
    let (site, _) = find_site(&domain)?;
    blocking(move || {
        let settings = Settings::load().map_err(err)?;
        let current = editors::list(settings.editor.as_deref()).current;
        editors::open(&current, stackyard_core::projects::folder(&site)).map_err(err)
    })
    .await
}

#[tauri::command]
pub async fn export_site(app: AppHandle, domain: String, dest: String) -> CmdResult<Packed> {
    require("sitetransfer")?;
    let (site, _) = find_site(&domain)?;
    let mut dest = PathBuf::from(dest);
    if dest.extension().is_none() {
        dest.set_extension("zip");
    }
    let mut progress = pack_progress(&app, domain);
    blocking(move || sitepack::pack(&paths::home(), &site, &dest, &mut progress).map_err(err)).await
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
    let mut progress = pack_progress(&app, path.clone());
    let (site, warning) = blocking(move || {
        sitepack::import(
            &paths::home(),
            std::path::Path::new(&path),
            options,
            &mut progress,
        )
        .map_err(err)
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
    let worker = app.clone();
    blocking(move || {
        share::install(&paths::home(), &mut |step| {
            share_progress(&worker, "", step)
        })
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
            share::install(&home, &mut |step| share_progress(&app, &domain, step)).map_err(err)?;
        }
        let _ = app.emit(
            "share-progress",
            serde_json::json!({ "site": domain, "step": "connecting" }),
        );
        share::start(&home, engine.supervisor(), &site).map_err(err)
    })
    .await
}

/// Sends cloudflared's download progress to the window as `share-progress`;
/// `site` is the site the link is for, or empty.
fn share_progress(app: &AppHandle, site: &str, step: stackyard_core::install::Step) {
    let mut value = serde_json::to_value(step).unwrap_or_default();
    if let Some(fields) = value.as_object_mut() {
        fields.insert("site".into(), site.into());
    }
    let _ = app.emit("share-progress", value);
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
pub(crate) fn open_with_system(target: &std::ffi::OsStr) -> CmdResult<()> {
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

/// A WordPress theme or plugin project's version and where its zips go,
/// for its Manage tab.
#[derive(Serialize)]
pub struct WpPackageView {
    #[serde(flatten)]
    package: stackyard_core::wppackage::Package,
    /// The folder built zips go in.
    builds: String,
    /// The WordPress Stackyard made for it before, while it is still there.
    old_wordpress: Option<String>,
}

fn wp_package_site(domain: &str) -> CmdResult<(Site, stackyard_core::projects::ProjectKind)> {
    let (site, _) = find_site(domain)?;
    let kind = site.settings.project.kind;
    if !kind.wp_package() {
        return Err(format!("{domain} is not a WordPress theme or plugin"));
    }
    Ok((site, kind))
}

fn wp_package_view(site: &Site) -> CmdResult<WpPackageView> {
    let home = paths::home();
    let folder = stackyard_core::projects::folder(site);
    Ok(WpPackageView {
        package: stackyard_core::wppackage::read(site.settings.project.kind, folder)
            .map_err(err)?,
        builds: stackyard_core::wppackage::builds_dir(&home)
            .display()
            .to_string(),
        old_wordpress: stackyard_core::projects::old_wordpress(&home, site)
            .map(|p| p.display().to_string()),
    })
}

#[tauri::command]
pub fn wp_package(domain: String) -> CmdResult<WpPackageView> {
    let (site, _) = wp_package_site(&domain)?;
    wp_package_view(&site)
}

/// Sets the version in the theme's or plugin's header (and readme.txt).
#[tauri::command]
pub fn wp_package_set_version(domain: String, version: String) -> CmdResult<WpPackageView> {
    let (site, kind) = wp_package_site(&domain)?;
    stackyard_core::wppackage::set_version(kind, stackyard_core::projects::folder(&site), &version)
        .map_err(err)?;
    wp_package_view(&site)
}

/// Packs the theme or plugin into a zip WordPress installs from.
#[tauri::command]
pub async fn wp_package_build(domain: String) -> CmdResult<stackyard_core::wppackage::Built> {
    let (site, kind) = wp_package_site(&domain)?;
    blocking(move || {
        let out = stackyard_core::wppackage::builds_dir(&paths::home());
        stackyard_core::wppackage::build_zip(kind, stackyard_core::projects::folder(&site), &out)
            .map_err(err)
    })
    .await
}

/// Opens the folder built zips go in.
#[tauri::command]
pub fn open_wp_builds() -> CmdResult<()> {
    let out = stackyard_core::wppackage::builds_dir(&paths::home());
    std::fs::create_dir_all(&out)
        .map_err(|e| format!("could not create {}: {e}", out.display()))?;
    open_with_system(out.as_os_str())
}

/// Removes the WordPress Stackyard made for a theme or plugin before. Its
/// database, if any, stays on the Databases screen.
#[tauri::command]
pub async fn wp_remove_old_wordpress(domain: String) -> CmdResult<WpPackageView> {
    let (site, _) = wp_package_site(&domain)?;
    blocking(move || {
        stackyard_core::projects::remove_old_wordpress(&paths::home(), &site).map_err(err)?;
        wp_package_view(&site)
    })
    .await
}
