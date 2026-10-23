//! Which database server a site's database lives in, and whether it runs.
//!
//! MySQL and MariaDB keep their databases in separate data folders and
//! only one of them runs at a time (they share port 3306), so a WordPress
//! site made while MariaDB ran shows "Error establishing a database
//! connection" while MySQL runs. Each MySQL or MariaDB database is a folder
//! in the server's data folder, so the folders tell which server holds it
//! without starting either.

use std::path::Path;

use serde::Serialize;

use crate::catalog;
use crate::services::ServiceManager;
use crate::sitepack::site_database;
use crate::sites::Site;

/// What a site needs from the database servers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SiteDb {
    /// The database's name, e.g. `wp_shop`.
    pub database: String,
    /// The service that holds it: `mysql`, `mariadb` or `postgres`;
    /// `None` when no installed server has it yet.
    pub service: Option<String>,
    /// e.g. `MariaDB`.
    pub service_name: Option<String>,
    /// Whether that service is installed.
    pub installed: bool,
    pub running: bool,
    /// Another server running on the same port, which has to stop first
    /// (MySQL while the site's database is in MariaDB), by name.
    pub blocked_by: Option<String>,
    /// The id of that other server.
    pub blocked_by_id: Option<String>,
    /// Whether Stackyard starts the server along with the web server.
    pub start_with_web: bool,
}

/// MySQL's name for a database folder: letters, digits and `_` as they
/// are, anything else as `@` and four hex digits (`-` is `@002d`).
fn folder_name(database: &str) -> String {
    let mut out = String::with_capacity(database.len());
    for c in database.chars() {
        if c.is_ascii_alphanumeric() || c == '_' {
            out.push(c);
        } else {
            out.push_str(&format!("@{:04x}", c as u32));
        }
    }
    out
}

/// Whether `service`'s data folder holds a database called `database`.
/// Names are compared without case: Windows keeps them lowercase.
pub fn holds(home: &Path, service: &str, database: &str) -> bool {
    let dir = ServiceManager::new(home).data_dir(service).join("db");
    let want = folder_name(database).to_lowercase();
    std::fs::read_dir(&dir)
        .map(|entries| {
            entries.flatten().any(|e| {
                e.file_type().is_ok_and(|t| t.is_dir())
                    && e.file_name().to_string_lossy().to_lowercase() == want
            })
        })
        .unwrap_or(false)
}

/// The site's database and the server it is in; `None` when the site has
/// no database Stackyard knows of. `running` says whether a service runs.
pub fn status(home: &Path, site: &Site, running: &dyn Fn(&str) -> bool) -> Option<SiteDb> {
    let db = site_database(site)?;
    let manager = ServiceManager::new(home);
    let service = if db.server == "postgres" {
        Some("postgres")
    } else {
        // The server that has its folder; else the installed one, if only
        // one is (the database is made on the first start of WordPress).
        let candidates = ["mariadb", "mysql"];
        candidates
            .into_iter()
            .find(|id| holds(home, id, &db.name))
            .or_else(|| {
                let installed: Vec<&str> = candidates
                    .into_iter()
                    .filter(|id| manager.get(id).is_some())
                    .collect();
                match installed.as_slice() {
                    [one] => Some(*one),
                    _ => installed.iter().copied().find(|id| running(id)),
                }
            })
    };
    let def = service.and_then(catalog::service);
    let blocked_by = def
        .filter(|_| service.is_some_and(|s| !running(s)))
        .and_then(|def| {
            catalog::SERVICES.iter().find(|other| {
                other.id != def.id
                    && other.id != "caddy"
                    && other.ports.iter().any(|p| def.ports.contains(p))
                    && running(other.id)
            })
        });
    Some(SiteDb {
        database: db.name,
        service: service.map(str::to_string),
        service_name: def.map(|d| d.name.to_string()),
        installed: service.is_some_and(|s| manager.get(s).is_some()),
        running: service.is_some_and(running),
        blocked_by: blocked_by.map(|d| d.name.to_string()),
        blocked_by_id: blocked_by.map(|d| d.id.to_string()),
        start_with_web: crate::settings::Settings::load_from(&home.join("settings.toml"))
            .is_ok_and(|s| s.start_databases.contains(&site.domain())),
    })
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

    #[test]
    fn names_folders_like_mysql() {
        assert_eq!(folder_name("wp_shop"), "wp_shop");
        assert_eq!(folder_name("my-shop"), "my@002dshop");
    }

    #[test]
    fn finds_the_server_that_holds_the_database() {
        let home = tempdir();
        let project = home.join("www").join("shop");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(
            project.join("wp-config.php"),
            "<?php define( 'DB_NAME', 'my-shop' );",
        )
        .unwrap();
        std::fs::create_dir_all(home.join("data/mariadb/db/my@002dshop")).unwrap();
        std::fs::create_dir_all(home.join("data/mysql/db/other")).unwrap();
        let site = site(&project);

        let mysql_runs = |id: &str| id == "mysql";
        let found = status(&home, &site, &mysql_runs).unwrap();
        assert_eq!(found.database, "my-shop");
        assert_eq!(found.service.as_deref(), Some("mariadb"));
        assert_eq!(found.service_name.as_deref(), Some("MariaDB"));
        assert!(!found.running);
        assert_eq!(found.blocked_by_id.as_deref(), Some("mysql"));

        let mariadb_runs = |id: &str| id == "mariadb";
        let found = status(&home, &site, &mariadb_runs).unwrap();
        assert!(found.running);
        assert_eq!(found.blocked_by, None);
    }

    #[test]
    fn no_database_no_status() {
        let home = tempdir();
        assert_eq!(status(&home, &site(&home), &|_| false), None);
    }
}
