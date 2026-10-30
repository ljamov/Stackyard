//! How much room a site takes: its database and its files, for WordPress
//! by part (core, themes, plugins, media). Measuring walks the whole
//! project folder, so it runs only when asked; the last answer is kept in
//! `cache/sitesize/<domain>.json` and shown straight away next time.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::services::ServiceManager;
use crate::sitedb::{self, SiteDb};
use crate::sites::Site;
use crate::wordpress::helper::Sizes;
use crate::Result;

/// A site's size when it was last measured.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SiteSize {
    /// The files split into WordPress's parts; `false` puts them all in
    /// `files.other`.
    pub wordpress: bool,
    pub files: Sizes,
    /// The database's name, when the site has one.
    pub database: Option<String>,
    /// What the database takes on disk; `None` when it could not be read
    /// (say a PostgreSQL that is not running).
    pub database_bytes: Option<u64>,
    /// Files and database together.
    pub total: u64,
    /// When it was measured, in seconds since 1970.
    pub measured: u64,
}

fn dir(home: &Path) -> PathBuf {
    home.join("cache").join("sitesize")
}

fn file(home: &Path, domain: &str) -> PathBuf {
    dir(home).join(format!("{domain}.json"))
}

/// The last measurement of the site, if there is one.
pub fn cached(home: &Path, domain: &str) -> Option<SiteSize> {
    let text = std::fs::read_to_string(file(home, domain)).ok()?;
    serde_json::from_str(&text).ok()
}

/// The last measurements of every site that has one, by domain.
pub fn all_cached(home: &Path) -> std::collections::BTreeMap<String, SiteSize> {
    let Ok(entries) = std::fs::read_dir(dir(home)) else {
        return Default::default();
    };
    entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let domain = name.strip_suffix(".json")?.to_string();
            let size = serde_json::from_str(&std::fs::read_to_string(e.path()).ok()?).ok()?;
            Some((domain, size))
        })
        .collect()
}

/// Forgets a removed site's measurement.
pub fn forget(home: &Path, domain: &str) {
    let _ = std::fs::remove_file(file(home, domain));
}

/// Measures the site's files and database and keeps the answer. `db` is
/// where its database lives (see [`sitedb::status`]); `pg_bytes` reads a
/// PostgreSQL database's size from the running server.
pub fn measure(
    home: &Path,
    site: &Site,
    db: Option<&SiteDb>,
    pg_bytes: &dyn Fn(&str) -> Option<u64>,
) -> Result<SiteSize> {
    let wp_root = crate::wordpress::root_of(&site.path);
    let mut files = Sizes {
        complete: true,
        ..Sizes::default()
    };
    let skip: Vec<String> = crate::wordpress::live::SKIP
        .iter()
        .map(|s| s.to_string())
        .collect();
    walk(&site.path, &mut |path, bytes| match &wp_root {
        Some(root) => match path.strip_prefix(root) {
            Ok(rel) => files.add(&rel.to_string_lossy().replace('\\', "/"), bytes, &skip),
            Err(_) => files.add("", bytes, &[]),
        },
        None => files.add("", bytes, &[]),
    });
    if let Some(root) = &wp_root {
        count_items(root, &mut files, &skip);
    }
    let database_bytes = db.and_then(|db| match db.service.as_deref() {
        Some("postgres") => pg_bytes(&db.database),
        Some(service) => {
            let folder = ServiceManager::new(home)
                .data_dir(service)
                .join("db")
                .join(sitedb::folder_name(&db.database));
            folder.is_dir().then(|| folder_bytes(&folder))
        }
        None => None,
    });
    let size = SiteSize {
        wordpress: wp_root.is_some(),
        total: files.total().bytes + files.skipped.bytes + database_bytes.unwrap_or(0),
        files,
        database: db.map(|d| d.database.clone()),
        database_bytes,
        measured: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
    };
    let path = file(home, &site.domain());
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&path, serde_json::to_vec(&size).unwrap_or_default());
    Ok(size)
}

/// Counts the theme folders and plugins (folders and single-file ones).
fn count_items(root: &Path, files: &mut Sizes, skip: &[String]) {
    for (dir, part) in [
        ("themes", &mut files.themes),
        ("plugins", &mut files.plugins),
    ] {
        let Ok(entries) = std::fs::read_dir(root.join("wp-content").join(dir)) else {
            continue;
        };
        part.items = entries
            .flatten()
            .filter(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                match e.file_type() {
                    Ok(t) if t.is_dir() => !skip.iter().any(|s| s == &name),
                    Ok(t) if t.is_file() => {
                        dir == "plugins" && name.ends_with(".php") && name != "index.php"
                    }
                    _ => false,
                }
            })
            .count() as u64;
    }
}

fn folder_bytes(dir: &Path) -> u64 {
    let mut total = 0;
    walk(dir, &mut |_, bytes| total += bytes);
    total
}

/// Calls `visit` with every file under `dir` and its size; links are not followed.
fn walk(dir: &Path, visit: &mut dyn FnMut(&Path, u64)) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let path = entry.path();
        if kind.is_dir() {
            walk(&path, visit);
        } else if kind.is_file() {
            visit(&path, entry.metadata().map(|m| m.len()).unwrap_or(0));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sites::{SiteRuntime, SiteSettings};
    use crate::testutil::tempdir;

    fn site(dir: &Path) -> Site {
        Site {
            name: "shop".into(),
            tld: ".stack".into(),
            path: dir.to_path_buf(),
            runtime: SiteRuntime::Php {
                version: "8.4".into(),
            },
            https: true,
            dev_server: false,
            settings: SiteSettings::default(),
        }
    }

    fn write(root: &Path, rel: &str, bytes: usize) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, vec![b'x'; bytes]).unwrap();
    }

    #[test]
    fn measures_a_wordpress_site_and_its_database_folder() {
        let home = tempdir();
        let project = tempdir();
        for (rel, bytes) in [
            ("wp-load.php", 10),
            ("wp-includes/version.php", 30),
            ("wp-content/themes/one/style.css", 40),
            ("wp-content/plugins/shop/shop.php", 200),
            ("wp-content/plugins/hello.php", 7),
            ("wp-content/uploads/a.jpg", 1000),
            ("wp-content/cache/page.html", 500),
        ] {
            write(&project, rel, bytes);
        }
        let db_dir = ServiceManager::new(&home).data_dir("mariadb").join("db");
        write(&db_dir, "wp_shop/wp_posts.ibd", 4096);
        let db = SiteDb {
            database: "wp_shop".into(),
            service: Some("mariadb".into()),
            service_name: Some("MariaDB".into()),
            installed: true,
            running: false,
            blocked_by: None,
            blocked_by_id: None,
            start_with_web: false,
            found: true,
        };
        let size = measure(&home, &site(&project), Some(&db), &|_| None).unwrap();
        assert!(size.wordpress);
        assert_eq!(size.files.core.bytes, 40);
        assert_eq!((size.files.themes.items, size.files.themes.bytes), (1, 40));
        assert_eq!(
            (size.files.plugins.items, size.files.plugins.bytes),
            (2, 207)
        );
        assert_eq!(size.files.uploads.bytes, 1000);
        assert_eq!(size.files.skipped.bytes, 500);
        assert_eq!(size.database_bytes, Some(4096));
        assert_eq!(size.total, 40 + 40 + 207 + 1000 + 500 + 4096);
        assert_eq!(cached(&home, "shop.stack"), Some(size.clone()));
        assert_eq!(all_cached(&home).get("shop.stack"), Some(&size));
        forget(&home, "shop.stack");
        assert_eq!(cached(&home, "shop.stack"), None);
    }

    #[test]
    fn puts_other_projects_files_together() {
        let home = tempdir();
        let project = tempdir();
        write(&project, "package.json", 20);
        write(&project, "src/app.ts", 80);
        let size = measure(&home, &site(&project), None, &|_| None).unwrap();
        assert!(!size.wordpress);
        assert_eq!((size.files.other.files, size.files.other.bytes), (2, 100));
        assert_eq!(size.database_bytes, None);
        assert_eq!(size.total, 100);
    }
}
