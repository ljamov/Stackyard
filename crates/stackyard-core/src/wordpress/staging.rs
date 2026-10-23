//! Stagings: named copies of a WordPress site that live next to it, such
//! as `checkout.shop.stack` for trying a new checkout on `shop.stack`.
//!
//! A staging is made from the main site, another staging or the live site.
//! It gets its own folder (`shop@checkout` beside `shop`), its own database
//! (`shop_checkout`), every address rewritten, `WP_ENVIRONMENT_TYPE` set to
//! `staging`, the must-use plugin's "Staging: checkout" label, and a base
//! snapshot ([`super::snapshot`]) that merges compare against later.
//!
//! Files are copied with [`super::copy`], so on disks that share blocks
//! the copy is instant. The media folder can be left out: the staging then
//! copies each missing file from the site it was made from when a page
//! asks for it.
//!
//! Destroying a staging moves its folder, database and snapshots to
//! `wordpress/trash/` for [`TRASH_DAYS`] days, so it can be brought back.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::env::{self, Environment, Kind};
use super::live::{self, CloneOptions, OnProgress, Progress, Source, Stage};
use super::{copy, muplugin, replace, snapshot, WpSite};
use crate::databases::Client;
use crate::remote::{RemoteStore, Sessions};
use crate::sites::{Site, SiteDatabase, SiteStore};
use crate::{domain, Error, Result};

pub const TRASH_DAYS: u64 = 7;
const MEDIA: &str = "wp-content/uploads";

fn fail(message: impl Into<String>) -> Error {
    Error::WordPress(message.into())
}

fn io(path: &Path) -> impl FnOnce(std::io::Error) -> Error + '_ {
    move |source| Error::Io {
        path: path.to_owned(),
        source,
    }
}

/// What a new staging is made from.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum MadeFrom {
    /// The main site or one of its stagings, by domain.
    Site { domain: String },
    /// The live site the main site was cloned from.
    Live,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct NewStaging {
    /// The main site's domain, e.g. `shop.stack`.
    pub parent: String,
    /// The staging's name, e.g. `checkout` for `checkout.shop.stack`.
    pub name: String,
    pub from: MadeFrom,
    /// Copy `wp-content/uploads` too; when false, media is copied from the
    /// site it was made from as pages ask for it.
    #[serde(default = "yes")]
    pub copy_media: bool,
}

fn yes() -> bool {
    true
}

/// A staging as the Environments tab lists it.
#[derive(Debug, Clone, Serialize)]
pub struct Staging {
    pub site: Site,
    pub domain: String,
    pub url: String,
    pub made_from: Option<String>,
    pub created_at: u64,
    pub shared_media: bool,
}

/// Where a staging of `parent` named `name` goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Place {
    pub tld: String,
    pub domain: String,
    pub folder: PathBuf,
    pub database: String,
}

/// The main site of a copy: itself, or a staging's parent.
pub fn main_of(home: &Path, domain: &str) -> String {
    env::load(home, domain)
        .ok()
        .flatten()
        .filter(|e| e.kind == Kind::Staging)
        .and_then(|e| e.parent)
        .unwrap_or_else(|| domain.to_string())
}

fn site_of(store: &SiteStore, domain: &str) -> Result<Site> {
    store
        .list()
        .iter()
        .find(|s| s.domain() == domain)
        .cloned()
        .ok_or_else(|| Error::SiteNotFound(domain.to_string()))
}

/// The database a site uses: from its settings, or its `wp-config.php`.
fn database_of(site: &Site) -> Result<String> {
    match &site.settings.database {
        Some(db) if db.server == "mysql" => Ok(db.name.clone()),
        _ => WpSite::open(site)?.database(),
    }
}

/// The database of a staging named `name`, e.g. `shop_checkout`.
pub fn database_name(parent: &Site, name: &str) -> Result<String> {
    let database = format!(
        "{}_{}",
        database_of(parent)?,
        crate::starters::database_name(name.trim())
    );
    crate::databases::check_name(&database)?;
    Ok(database)
}

/// Works out the staging's domain, folder and database, and checks none
/// of them is taken.
pub fn place(home: &Path, parent: &Site, name: &str) -> Result<Place> {
    let name = name.trim();
    domain::validate_site_name(name)?;
    let tld = domain::normalize_tld(&parent.domain())?;
    let domain = domain::site_domain(name, &tld);
    let store = SiteStore::load_from(&home.join("sites.toml"))?;
    if store.list().iter().any(|s| s.domain() == domain) {
        return Err(Error::SiteExists(domain));
    }
    let base = parent
        .path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| parent.name.clone());
    let folder = parent.path.with_file_name(format!("{base}@{name}"));
    if folder.exists() {
        return Err(fail(format!(
            "{} already exists; pick another name or move that folder",
            folder.display()
        )));
    }
    let database = database_name(parent, name)?;
    let client = Client::open(home)?;
    if client.databases()?.iter().any(|d| d.name == database) {
        return Err(fail(format!(
            "a database named {database} already exists; pick another name"
        )));
    }
    Ok(Place {
        tld,
        domain,
        folder,
        database,
    })
}

/// The stagings of a main site, oldest first.
pub fn list(home: &Path, parent: &str) -> Result<Vec<Staging>> {
    let store = SiteStore::load_from(&home.join("sites.toml"))?;
    let mut out: Vec<Staging> = store
        .list()
        .iter()
        .filter_map(|site| {
            let env = env::load(home, &site.domain()).ok().flatten()?;
            (env.kind == Kind::Staging && env.parent.as_deref() == Some(parent)).then(|| Staging {
                domain: site.domain(),
                url: site.url(),
                made_from: env.made_from.clone(),
                created_at: env.created_at,
                shared_media: env.shared_media.is_some(),
                site: site.clone(),
            })
        })
        .collect();
    out.sort_by_key(|s| s.created_at);
    Ok(out)
}

fn say(progress: OnProgress, stage: Stage, message: &str, done: u64, total: u64) -> Result<()> {
    if progress(Progress {
        stage,
        message: message.to_string(),
        done,
        total,
    }) {
        Ok(())
    } else {
        Err(fail("stopped"))
    }
}

/// Makes a staging. The database server must be running. A staging made
/// from the live site goes through [`live::clone`] with the main site's
/// saved live connection.
pub fn create(
    home: &Path,
    sessions: &Sessions,
    new: &NewStaging,
    progress: OnProgress,
) -> Result<Site> {
    let store = SiteStore::load_from(&home.join("sites.toml"))?;
    let parent_domain = main_of(home, &new.parent);
    let parent = site_of(&store, &parent_domain)?;
    let place = place(home, &parent, &new.name)?;
    let parent_env = env::load(home, &parent_domain)?.unwrap_or_default();
    let site = match &new.from {
        MadeFrom::Live => {
            let live = parent_env
                .live
                .clone()
                .ok_or_else(|| fail(format!("{parent_domain} was not cloned from a live site")))?;
            let id = live.connection.clone().ok_or_else(|| {
                fail("the live site came from backup files; make the staging from the local site instead")
            })?;
            let connection = RemoteStore::load_from(&home.join("remote.toml"))?
                .get(&id)?
                .clone();
            let options = CloneOptions {
                name: new.name.trim().to_string(),
                folder: Some(place.folder.clone()),
                root: Some(live.path.clone()),
                url: Some(live.url.clone()),
                media: live.media,
                https: parent.https,
                staging_of: Some(parent_domain.clone()),
            };
            live::clone(
                home,
                sessions,
                &Source::Server(Box::new(connection)),
                &options,
                progress,
            )?
            .site
        }
        MadeFrom::Site { domain } => {
            if main_of(home, domain) != parent_domain {
                return Err(fail(format!(
                    "{domain} is not {parent_domain} or one of its stagings"
                )));
            }
            let source = site_of(&store, domain)?;
            let made = copy_site(home, &source, &parent, &place, new, &parent_env, progress);
            if made.is_err() {
                let _ = std::fs::remove_dir_all(&place.folder);
                if let Ok(client) = Client::open(home) {
                    let _ = client.drop(&place.database);
                }
                if let Ok(mut store) = SiteStore::load_from(&home.join("sites.toml")) {
                    if store.remove(&place.domain).is_ok() {
                        let _ = store.save();
                    }
                }
                env::forget(home, &place.domain);
            }
            made?
        }
    };
    say(
        progress,
        Stage::Finishing,
        "Taking the base snapshot…",
        0,
        0,
    )?;
    let wp = WpSite::open(&site)?;
    snapshot::take_base(home, &wp)?;
    Ok(site)
}

fn copy_site(
    home: &Path,
    source: &Site,
    parent: &Site,
    place: &Place,
    new: &NewStaging,
    parent_env: &Environment,
    progress: OnProgress,
) -> Result<Site> {
    let source_wp = WpSite::open(source)?;
    // The WordPress folder may sit below the project folder (Bedrock).
    let media_rel = source_wp
        .wp_content()
        .join("uploads")
        .strip_prefix(&source.path)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| MEDIA.to_string());
    let skipped: Vec<&str> = if new.copy_media {
        Vec::new()
    } else {
        vec![media_rel.as_str()]
    };
    let total = copy::tree_size(&source.path, &skipped);
    say(progress, Stage::Files, "Copying the files…", 0, total)?;
    let mut stopped = false;
    copy::copy_tree(&source.path, &place.folder, &skipped, &mut |done| {
        if !stopped
            && !progress(Progress {
                stage: Stage::Files,
                message: "Copying the files…".into(),
                done,
                total,
            })
        {
            stopped = true;
        }
    })
    .map_err(io(&place.folder))?;
    if stopped {
        return Err(fail("stopped"));
    }

    say(progress, Stage::Database, "Copying the database…", 0, 0)?;
    let client = Client::open(home)?;
    let work = home
        .join("tmp")
        .join(format!("staging-{}", place.domain.replace('.', "-")));
    std::fs::create_dir_all(&work).map_err(io(&work))?;
    let result = (|| {
        let dump = work.join("source.sql.gz");
        client.dump_to(&source_wp.database()?, &[], &dump)?;
        let rewritten = work.join("staging.sql.gz");
        let local_url = Site {
            name: new.name.trim().to_string(),
            tld: place.tld.clone(),
            ..source.clone()
        }
        .url();
        // The address WordPress has, which may differ from the local one
        // (a site installed under another name, say).
        let source_url = super::home_url(home, &source_wp);
        let rules = replace::Rules::site_move(&source_url, &local_url);
        let input = flate2::read::GzDecoder::new(std::fs::File::open(&dump).map_err(io(&dump))?);
        let output = flate2::write::GzEncoder::new(
            std::fs::File::create(&rewritten).map_err(io(&rewritten))?,
            flate2::Compression::fast(),
        );
        replace::rewrite(input, output, &rules).map_err(io(&rewritten))?;
        say(progress, Stage::Loading, "Loading the database…", 0, 0)?;
        client.load(&place.database, &rewritten)?;
        Ok::<_, Error>(local_url)
    })();
    let _ = std::fs::remove_dir_all(&work);
    let local_url = result?;

    say(progress, Stage::Finishing, "Setting up the staging…", 0, 0)?;
    let config_file = place.folder.join(
        source_wp
            .config_file
            .strip_prefix(&source.path)
            .unwrap_or(Path::new("wp-config.php")),
    );
    let text = std::fs::read_to_string(&config_file).map_err(io(&config_file))?;
    let port = crate::catalog::service(&client.server().id)
        .map_or(3306, |def| crate::services::port(home, def));
    let (text, _) = live::local_config(&text, &place.database, port, &local_url);
    let text = super::config::set_define(&text, "WP_ENVIRONMENT_TYPE", "'staging'");
    std::fs::write(&config_file, text).map_err(io(&config_file))?;

    let mut settings = source.settings.clone();
    settings.database = Some(SiteDatabase {
        server: "mysql".into(),
        name: place.database.clone(),
    });
    let mut store = SiteStore::load_from(&home.join("sites.toml"))?;
    let site = store
        .add(Site {
            name: new.name.trim().to_string(),
            tld: place.tld.clone(),
            path: place.folder.clone(),
            runtime: source.runtime.clone(),
            https: source.https,
            dev_server: false,
            settings,
        })?
        .clone();
    store.save()?;
    let source_env = env::load(home, &source.domain())?.unwrap_or_default();
    let shared_media = if new.copy_media {
        None
    } else {
        // Media the source itself shares or loads from live is found the
        // same way, through its folder and the live site.
        Some(
            source_env
                .shared_media
                .clone()
                .unwrap_or_else(|| source_wp.wp_content().join("uploads")),
        )
    };
    env::save(
        home,
        &site.domain(),
        &Environment {
            kind: Kind::Staging,
            live: parent_env.live.clone(),
            safe: parent_env.safe,
            parent: Some(parent.domain()),
            made_from: Some(source.domain()),
            created_at: env::now(),
            shared_media,
        },
    )?;
    let wp = WpSite::open(&site)?;
    muplugin::install(&wp, &muplugin::Options::for_site(home, &wp))?;
    Ok(site)
}

// ---------------------------------------------------------------------------
// Trash

/// A destroyed staging, kept for [`TRASH_DAYS`] days.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Trashed {
    pub id: String,
    pub site: Site,
    pub env: Environment,
    pub database: String,
    pub trashed_at: u64,
}

fn trash_dir(home: &Path) -> PathBuf {
    home.join("wordpress").join("trash")
}

/// Moves a folder, copying when it can't be renamed (another disk).
fn move_folder(from: &Path, to: &Path) -> Result<()> {
    if std::fs::rename(from, to).is_ok() {
        return Ok(());
    }
    copy::copy_tree(from, to, &[], &mut |_| {}).map_err(io(to))?;
    std::fs::remove_dir_all(from).map_err(io(from))
}

/// Destroys a staging: its folder, database and snapshots go to the trash,
/// and the site is removed. Only stagings can be destroyed this way.
pub fn destroy(home: &Path, domain: &str) -> Result<Trashed> {
    let env = env::load(home, domain)?
        .filter(|e| e.kind == Kind::Staging)
        .ok_or_else(|| fail(format!("{domain} is not a staging")))?;
    let mut store = SiteStore::load_from(&home.join("sites.toml"))?;
    let site = site_of(&store, domain)?;
    let database = database_of(&site)?;
    let trashed = Trashed {
        id: format!("{}-{}", domain.replace('.', "-"), env::now()),
        site: site.clone(),
        env,
        database: database.clone(),
        trashed_at: env::now(),
    };
    let folder = trash_dir(home).join(&trashed.id);
    std::fs::create_dir_all(&folder).map_err(io(&folder))?;
    let client = Client::open(home)?;
    client.dump_to(&database, &[], &folder.join("database.sql.gz"))?;
    let file = folder.join("trashed.json");
    let text = serde_json::to_vec_pretty(&trashed)
        .map_err(|e| fail(format!("could not save {}: {e}", file.display())))?;
    std::fs::write(&file, text).map_err(io(&file))?;
    if site.path.exists() {
        move_folder(&site.path, &folder.join("files"))?;
    }
    let snapshots = snapshot::dir(home, domain);
    if snapshots.exists() {
        move_folder(&snapshots, &folder.join("snapshots"))?;
    }
    client.drop(&database)?;
    store.remove(domain)?;
    store.save()?;
    env::forget(home, domain);
    purge(home);
    Ok(trashed)
}

/// Destroyed stagings, newest first. Ones older than [`TRASH_DAYS`] days
/// are deleted first.
pub fn trash(home: &Path) -> Vec<Trashed> {
    purge(home);
    let mut all: Vec<Trashed> = std::fs::read_dir(trash_dir(home))
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|e| std::fs::read(e.path().join("trashed.json")).ok())
                .filter_map(|b| serde_json::from_slice(&b).ok())
                .collect()
        })
        .unwrap_or_default();
    all.sort_by_key(|t| std::cmp::Reverse(t.trashed_at));
    all
}

fn trashed(home: &Path, id: &str) -> Result<(Trashed, PathBuf)> {
    if id.is_empty() || id.contains(['/', '\\']) || id.starts_with('.') {
        return Err(fail(format!("{id} is not in the trash")));
    }
    let folder = trash_dir(home).join(id);
    let bytes = std::fs::read(folder.join("trashed.json")).map_err(|_| {
        fail(format!(
            "{id} is not in the trash; it may have been deleted"
        ))
    })?;
    let trashed = serde_json::from_slice(&bytes)
        .map_err(|e| fail(format!("{} is damaged: {e}", folder.display())))?;
    Ok((trashed, folder))
}

/// Brings a destroyed staging back. The database server must be running.
pub fn restore(home: &Path, id: &str) -> Result<Site> {
    let (trashed, folder) = trashed(home, id)?;
    let domain = trashed.site.domain();
    let mut store = SiteStore::load_from(&home.join("sites.toml"))?;
    if store.list().iter().any(|s| s.domain() == domain) {
        return Err(Error::SiteExists(domain));
    }
    if trashed.site.path.exists() {
        return Err(fail(format!(
            "{} exists again; move it away to bring the staging back",
            trashed.site.path.display()
        )));
    }
    let client = Client::open(home)?;
    if client
        .databases()?
        .iter()
        .any(|d| d.name == trashed.database)
    {
        return Err(fail(format!(
            "a database named {} exists again; remove it to bring the staging back",
            trashed.database
        )));
    }
    client.load(&trashed.database, &folder.join("database.sql.gz"))?;
    if folder.join("files").exists() {
        move_folder(&folder.join("files"), &trashed.site.path)?;
    }
    if folder.join("snapshots").exists() {
        let to = snapshot::dir(home, &domain);
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent).map_err(io(parent))?;
        }
        move_folder(&folder.join("snapshots"), &to)?;
    }
    let site = store.add(trashed.site.clone())?.clone();
    store.save()?;
    env::save(home, &domain, &trashed.env)?;
    let _ = std::fs::remove_dir_all(&folder);
    Ok(site)
}

/// Deletes a destroyed staging for good.
pub fn delete(home: &Path, id: &str) -> Result<()> {
    let (_, folder) = trashed(home, id)?;
    std::fs::remove_dir_all(&folder).map_err(io(&folder))
}

fn purge(home: &Path) {
    let cutoff = env::now().saturating_sub(TRASH_DAYS * 86_400);
    let Ok(entries) = std::fs::read_dir(trash_dir(home)) else {
        return;
    };
    for entry in entries.flatten() {
        let old = std::fs::read(entry.path().join("trashed.json"))
            .ok()
            .and_then(|b| serde_json::from_slice::<Trashed>(&b).ok())
            .is_none_or(|t| t.trashed_at < cutoff);
        if old {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::Sessions;
    use crate::sites::SiteRuntime;

    fn shop(home: &Path) -> Site {
        let folder = home.join("www").join("shop");
        std::fs::create_dir_all(&folder).unwrap();
        Site {
            name: "shop".into(),
            tld: ".stack".into(),
            path: folder,
            runtime: SiteRuntime::Static,
            https: true,
            dev_server: false,
            settings: crate::sites::SiteSettings {
                database: Some(SiteDatabase {
                    server: "mysql".into(),
                    name: "shop".into(),
                }),
                ..Default::default()
            },
        }
    }

    #[test]
    fn a_staging_is_named_after_its_parent() {
        let home = crate::testutil::tempdir();
        let parent = shop(&home);
        let staging = Site {
            name: "checkout".into(),
            tld: domain::normalize_tld(&parent.domain()).unwrap(),
            ..parent.clone()
        };
        assert_eq!(staging.domain(), "checkout.shop.stack");
        assert_eq!(staging.key(), "checkout.shop");
        assert_eq!(parent.key(), "shop");
        assert_eq!(
            parent.path.with_file_name("shop@checkout"),
            home.join("www").join("shop@checkout")
        );
    }

    #[test]
    fn finds_the_main_site_of_a_staging() {
        let home = crate::testutil::tempdir();
        assert_eq!(main_of(&home, "shop.stack"), "shop.stack");
        env::save(
            &home,
            "checkout.shop.stack",
            &Environment {
                kind: Kind::Staging,
                parent: Some("shop.stack".into()),
                ..Environment::default()
            },
        )
        .unwrap();
        assert_eq!(main_of(&home, "checkout.shop.stack"), "shop.stack");
    }

    #[test]
    fn old_trash_is_deleted() {
        let home = crate::testutil::tempdir();
        let site = shop(&home);
        for (id, at) in [("new", env::now()), ("old", env::now() - 8 * 86_400)] {
            let folder = trash_dir(&home).join(id);
            std::fs::create_dir_all(&folder).unwrap();
            let t = Trashed {
                id: id.into(),
                site: site.clone(),
                env: Environment::default(),
                database: "shop_x".into(),
                trashed_at: at,
            };
            std::fs::write(folder.join("trashed.json"), serde_json::to_vec(&t).unwrap()).unwrap();
        }
        let left = trash(&home);
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].id, "new");
        assert!(trashed(&home, "../x").is_err());
        delete(&home, "new").unwrap();
        assert!(trash(&home).is_empty());
    }

    #[test]
    #[ignore = "needs a WordPress site added in Stackyard, named in STACKYARD_TEST_STAGING"]
    fn stagings_of_a_real_site() {
        let Ok(parent) = std::env::var("STACKYARD_TEST_STAGING") else {
            return;
        };
        let home = crate::paths::home();
        let sessions = Sessions::default();
        let mut seen = Vec::new();
        let mut progress = |p: Progress| {
            seen.push(p.stage);
            true
        };
        let home_option = |site: &Site| {
            WpSite::open(site)
                .unwrap()
                .wp(&home, super::super::Load::Core, &["option", "get", "home"])
                .unwrap()
                .trim()
                .to_string()
        };

        // From the main site, sharing its media.
        let first = create(
            &home,
            &sessions,
            &NewStaging {
                parent: parent.clone(),
                name: "try".into(),
                from: MadeFrom::Site {
                    domain: parent.clone(),
                },
                copy_media: false,
            },
            &mut progress,
        )
        .unwrap();
        assert_eq!(first.domain(), format!("try.{parent}"));
        assert!(seen.contains(&Stage::Files) && seen.contains(&Stage::Loading));
        assert_eq!(home_option(&first), first.url());
        let wp = WpSite::open(&first).unwrap();
        assert_eq!(wp.config.get("WP_ENVIRONMENT_TYPE"), Some("staging"));
        let db = wp.database().unwrap();
        assert!(db.ends_with("_try"), "{db}");
        let plugin = std::fs::read_to_string(muplugin::path(&wp)).unwrap();
        assert!(plugin.contains("Staging: try"));
        assert!(plugin.contains("STACKYARD_MEDIA_FOLDER"));
        assert!(snapshot::base(&home, &first.domain()).is_some());
        let base = snapshot::base(&home, &first.domain()).unwrap();
        assert!(base.manifest(&home).unwrap().contains_key("wp-load.php"));
        assert_eq!(list(&home, &parent).unwrap().len(), 1);
        assert_eq!(main_of(&home, &first.domain()), parent);

        // From that staging, with its own media.
        let second = create(
            &home,
            &sessions,
            &NewStaging {
                parent: first.domain(),
                name: "again".into(),
                from: MadeFrom::Site {
                    domain: first.domain(),
                },
                copy_media: true,
            },
            &mut |_| true,
        )
        .unwrap();
        assert_eq!(second.domain(), format!("again.{parent}"));
        assert_eq!(home_option(&second), second.url());
        let env = env::load(&home, &second.domain()).unwrap().unwrap();
        assert_eq!(env.made_from, Some(first.domain()));
        assert_eq!(env.parent, Some(parent.clone()));
        assert!(env.shared_media.is_none());

        // A snapshot puts changed, removed and added files back.
        let mut snap = snapshot::take(&home, &wp, "test").unwrap();
        snap.save_file(&home, &first.path, "index.php").unwrap();
        std::fs::write(first.path.join("index.php"), "changed").unwrap();
        snap.save_file(&home, &first.path, "added.txt").unwrap();
        std::fs::write(first.path.join("added.txt"), "new").unwrap();
        wp.wp(
            &home,
            super::super::Load::Core,
            &["option", "update", "blogname", "Changed"],
        )
        .unwrap();
        let before = snapshot::restore(&home, &wp, &snap.id).unwrap();
        assert_ne!(
            std::fs::read_to_string(first.path.join("index.php")).unwrap(),
            "changed"
        );
        assert!(!first.path.join("added.txt").exists());
        let name = wp
            .wp(
                &home,
                super::super::Load::Core,
                &["option", "get", "blogname"],
            )
            .unwrap();
        assert_ne!(name.trim(), "Changed");
        // Restoring can itself be undone: what it replaced or removed is kept.
        assert_eq!(before.saved, vec!["added.txt", "index.php"]);
        assert_eq!(snapshot::list(&home, &first.domain()).len(), 2);

        // Destroyed, brought back, destroyed for good.
        let trashed = destroy(&home, &second.domain()).unwrap();
        assert!(!second.path.exists());
        assert!(trash(&home).iter().any(|t| t.id == trashed.id));
        let back = restore(&home, &trashed.id).unwrap();
        assert_eq!(home_option(&back), second.url());
        assert!(snapshot::base(&home, &second.domain()).is_some());
        for staging in [&second, &first] {
            let t = destroy(&home, &staging.domain()).unwrap();
            delete(&home, &t.id).unwrap();
        }
        assert!(list(&home, &parent).unwrap().is_empty());
        assert!(destroy(&home, &parent).is_err());
    }
}
