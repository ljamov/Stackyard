//! Checks a WordPress site's database and repairs what a copy from another
//! server often gets wrong: a wp-config.php that still logs in to the live
//! server, a database that is missing or named differently, a table prefix
//! that doesn't match the tables, missing core tables, ID columns that
//! lost AUTO_INCREMENT (new posts then all get ID 0), roles and user
//! rights stored under another prefix, the site address still pointing at
//! the old site, damaged tables and old `utf8` tables.
//!
//! [`check`] only looks; [`repair`] backs up wp-config.php and the
//! database, then fixes what it can and says what is left.

use std::path::{Path, PathBuf};

use serde::Serialize;

use super::{config, live, read_text, write_text, Load, WpSite};
use crate::databases::{self, Client};
use crate::services::ServiceManager;
use crate::sites::Site;
use crate::{catalog, Error, Result};

/// WordPress's own tables on every site, without the prefix.
const CORE_TABLES: [&str; 12] = [
    "commentmeta",
    "comments",
    "links",
    "options",
    "postmeta",
    "posts",
    "term_relationships",
    "term_taxonomy",
    "termmeta",
    "terms",
    "usermeta",
    "users",
];

/// Core tables and their ID column, which WordPress makes AUTO_INCREMENT.
const ID_COLUMNS: [(&str, &str); 11] = [
    ("commentmeta", "meta_id"),
    ("comments", "comment_ID"),
    ("links", "link_id"),
    ("options", "option_id"),
    ("postmeta", "meta_id"),
    ("posts", "ID"),
    ("term_taxonomy", "term_taxonomy_id"),
    ("termmeta", "meta_id"),
    ("terms", "term_id"),
    ("usermeta", "umeta_id"),
    ("users", "ID"),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Ok,
    /// Wrong, and [`repair`] fixes it.
    Fix,
    /// Fixed by this repair.
    Fixed,
    /// Wrong, and needs the user (or failed to fix).
    Problem,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Finding {
    /// What was checked, e.g. `Login`.
    pub check: String,
    pub status: Status,
    pub text: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Report {
    pub database: String,
    /// e.g. `MariaDB`.
    pub server: Option<String>,
    pub findings: Vec<Finding>,
    /// The database backup taken before repairing (Databases screen).
    pub backup: Option<String>,
    /// Where the old wp-config.php was saved, when it was changed.
    pub config_backup: Option<String>,
}

impl Report {
    fn add(&mut self, check: &str, status: Status, text: impl Into<String>) {
        self.findings.push(Finding {
            check: check.into(),
            status,
            text: text.into(),
        });
    }
}

/// Looks for problems without changing anything.
pub fn check(home: &Path, site: &Site) -> Result<Report> {
    run(home, site, false)
}

/// Fixes what [`check`] finds, after backing up wp-config.php and the
/// database.
pub fn repair(home: &Path, site: &Site) -> Result<Report> {
    run(home, site, true)
}

/// `Fix` while checking, `Fixed` while repairing.
fn fixing(apply: bool) -> Status {
    if apply {
        Status::Fixed
    } else {
        Status::Fix
    }
}

/// One table as information_schema describes it.
struct TableInfo {
    name: String,
    engine: String,
    collation: String,
}

fn tables(client: &Client, database: &str) -> Result<Vec<TableInfo>> {
    let rows = client.rows(
        database,
        &format!(
            "SELECT TABLE_NAME, COALESCE(ENGINE, ''), COALESCE(TABLE_COLLATION, '') \
             FROM information_schema.TABLES WHERE TABLE_SCHEMA = {} AND TABLE_TYPE = 'BASE TABLE' \
             ORDER BY TABLE_NAME",
            databases::literal(database)
        ),
    )?;
    Ok(rows
        .rows
        .into_iter()
        .map(|r| {
            let at = |i: usize| r.get(i).cloned().flatten().unwrap_or_default();
            TableInfo {
                name: at(0),
                engine: at(1),
                collation: at(2),
            }
        })
        .collect())
}

/// The prefixes of the WordPress sites in a list of tables: `wp_` when
/// there are `wp_options`, `wp_posts` and `wp_users`. A multisite's
/// subsites (`wp_2_options`) have no users table of their own.
fn prefixes(names: &[&str]) -> Vec<String> {
    let mut found: Vec<String> = names
        .iter()
        .filter_map(|n| n.strip_suffix("options"))
        .filter(|p| {
            names.contains(&format!("{p}posts").as_str())
                && names.contains(&format!("{p}users").as_str())
        })
        .map(str::to_string)
        .collect();
    found.sort();
    found.dedup();
    found
}

/// The server, port and login a `DB_HOST` names: `127.0.0.1:3307` is
/// local on 3307, `localhost:/run/mysqld/mysqld.sock` is a socket (only
/// on the server it came from), `db.example.com` is another computer.
fn host_port(host: &str) -> (bool, Option<u16>) {
    let (name, rest) = match host.rsplit_once(':') {
        // `[::1]:3306` and plain `::1`.
        Some((name, rest)) if !name.is_empty() && !name.ends_with(':') => (name, Some(rest)),
        _ => (host, None),
    };
    let name = name.trim_start_matches('[').trim_end_matches(']');
    let local = ["localhost", "127.0.0.1", "::1"].contains(&name) || host == "::1";
    match rest {
        None => (local, None),
        Some(rest) => match rest.parse::<u16>() {
            Ok(port) => (local, Some(port)),
            // A socket path only works on the server it came from.
            Err(_) => (false, None),
        },
    }
}

/// A name for a new local database from the site's name: `my_shop` for
/// `my-shop`, never one `taken` already has.
fn local_name(site: &Site, taken: &[String]) -> String {
    let base: String = site
        .name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    let base = if base.is_empty() {
        "wordpress".to_string()
    } else {
        base
    };
    let mut name = base.clone();
    let mut n = 2;
    while taken.iter().any(|t| t.eq_ignore_ascii_case(&name)) {
        name = format!("{base}_{n}");
        n += 1;
    }
    name
}

/// Saves the current wp-config.php under `backups/wp-config/`, outside the
/// site so the live password in it is never served.
fn save_config(home: &Path, wp: &WpSite) -> Result<PathBuf> {
    let dir = databases::backups_dir(home).join("wp-config");
    std::fs::create_dir_all(&dir).map_err(|source| Error::Io {
        path: dir.clone(),
        source,
    })?;
    let file = dir.join(format!("{}-{}.php", wp.site.domain(), databases::stamp()));
    std::fs::copy(&wp.config_file, &file).map_err(|source| Error::Io {
        path: file.clone(),
        source,
    })?;
    Ok(file)
}

fn run(home: &Path, site: &Site, apply: bool) -> Result<Report> {
    let wp = WpSite::open(site)?;
    let mut report = Report {
        database: wp.database().unwrap_or_default(),
        ..Report::default()
    };

    // The server: the one holding the database, else the running one.
    let services = ServiceManager::new(home);
    let port_of = |id: &str| catalog::service(id).map_or(0, |d| crate::services::port(home, d));
    let runs = |id: &str| crate::shell::mysql_flavor(port_of(id)) == Some(id);
    let installed: Vec<&str> = ["mariadb", "mysql"]
        .into_iter()
        .filter(|id| services.get(id).is_some())
        .collect();
    if installed.is_empty() {
        report.add(
            "Server",
            Status::Problem,
            "Neither MySQL nor MariaDB is installed. Install one on the Services screen.",
        );
        return Ok(report);
    }
    let holder = installed
        .iter()
        .copied()
        .find(|id| !report.database.is_empty() && crate::sitedb::holds(home, id, &report.database));
    let service = holder
        .or_else(|| installed.iter().copied().find(|id| runs(id)))
        .unwrap_or(installed[0]);
    let name = catalog::service(service).map_or(service, |d| d.name);
    report.server = Some(name.to_string());
    if !runs(service) {
        report.add(
            "Server",
            Status::Problem,
            format!("{name} is not running. Start it, then check again."),
        );
        return Ok(report);
    }
    let client = Client::open_service(home, service)?;
    let port = port_of(service);
    report.add(
        "Server",
        Status::Ok,
        format!("{name} is running on port {port}."),
    );

    // The database: the one wp-config.php names, else one on this server
    // that holds this site's tables, else a new empty one.
    let all: Vec<String> = client.databases()?.into_iter().map(|d| d.name).collect();
    let mut prefix = wp.prefix().to_string();
    let named = report.database.clone();
    let mut database = all
        .iter()
        .find(|d| !named.is_empty() && d.eq_ignore_ascii_case(&named))
        .cloned();
    // A new database: made now while repairing, or to be made.
    let mut created = false;
    if database.is_some() {
        report.add(
            "Database",
            Status::Ok,
            format!("{name} has the database {named}."),
        );
    } else {
        let wanted = local_name(site, &[]);
        let found = all.iter().find(|d| {
            d.eq_ignore_ascii_case(&wanted)
                && tables(&client, d).is_ok_and(|t| {
                    let names: Vec<&str> = t.iter().map(|t| t.name.as_str()).collect();
                    !prefixes(&names).is_empty()
                })
        });
        match found {
            Some(other) => {
                report.add(
                    "Database",
                    fixing(apply),
                    format!("{name} has no database {named}, but {other} holds this site's tables; wp-config.php is pointed at it."),
                );
                database = Some(other.clone());
            }
            None => {
                let new = local_name(site, &all);
                if apply {
                    client.create(&new)?;
                }
                created = true;
                report.add(
                    "Database",
                    Status::Problem,
                    format!(
                        "{name} has no database {}. {} Load the site's .sql file under Database › Import, or clone the site from live again.",
                        if named.is_empty() { "for this site".to_string() } else { named.clone() },
                        if apply {
                            format!("An empty database {new} was made for it.")
                        } else {
                            format!("Repair makes an empty database {new} for it.")
                        }
                    ),
                );
                database = Some(new);
            }
        }
    }
    let database = database.expect("set above");

    // The table prefix.
    let list = if created {
        Vec::new()
    } else {
        tables(&client, &database)?
    };
    let names: Vec<&str> = list.iter().map(|t| t.name.as_str()).collect();
    let mut has_tables = !names.is_empty();
    if names.contains(&format!("{prefix}options").as_str()) {
        report.add(
            "Prefix",
            Status::Ok,
            format!("The tables start with {prefix}, as wp-config.php says."),
        );
    } else {
        let found = prefixes(&names);
        match found.as_slice() {
            [one] => {
                report.add(
                    "Prefix",
                    fixing(apply),
                    format!("wp-config.php says the tables start with {prefix}, but they start with {one}; $table_prefix is set to {one}."),
                );
                prefix = one.clone();
            }
            [] if names.is_empty() => {
                has_tables = false;
                if !created {
                    report.add(
                        "Tables",
                        Status::Problem,
                        format!("The database {database} is empty. Load the site's .sql file under Database › Import, or clone the site from live again."),
                    );
                }
            }
            [] => {
                has_tables = false;
                report.add(
                    "Prefix",
                    Status::Problem,
                    format!("{database} has tables, but no WordPress ones starting with {prefix}."),
                );
            }
            many => {
                has_tables = false;
                report.add(
                    "Prefix",
                    Status::Problem,
                    format!(
                        "{database} holds more than one WordPress ({}); set $table_prefix in wp-config.php to the right one.",
                        many.join(", ")
                    ),
                );
            }
        }
    }

    // The login in wp-config.php: what this server takes, on its port.
    let user = wp.config.get("DB_USER").unwrap_or("").to_string();
    let password = wp.config.get("DB_PASSWORD").unwrap_or("").to_string();
    let host = wp.config.get("DB_HOST").unwrap_or("localhost").to_string();
    let (local, named_port) = host_port(&host);
    let login_ok = local
        && named_port.unwrap_or(3306) == port
        // Into the database itself: an anonymous user can log in to the
        // server but not open it. A database still to be made is opened
        // as root.
        && if created {
            user == "root"
        } else {
            client.can_log_in(&user, &password, port, Some(&database))
        };
    if login_ok {
        report.add(
            "Login",
            Status::Ok,
            format!("wp-config.php logs in to {name} as {user}."),
        );
    } else {
        let why = if !local {
            format!("wp-config.php logs in to {host}, the server it was copied from")
        } else if named_port.unwrap_or(3306) != port {
            format!(
                "wp-config.php uses port {}, but {name} is on {port}",
                named_port.unwrap_or(3306)
            )
        } else {
            format!("{name} doesn't take the login in wp-config.php (user {user})")
        };
        report.add(
            "Login",
            fixing(apply),
            format!(
                "{why}; it is changed to root on 127.0.0.1:{port}, as Stackyard's other sites use."
            ),
        );
    }
    let config_name = wp.config.get("DB_NAME").unwrap_or("");
    let name_ok = config_name.eq_ignore_ascii_case(&database);
    if !name_ok && login_ok {
        report.add(
            "Database",
            fixing(apply),
            format!("wp-config.php names the database {config_name}; it is changed to {database}."),
        );
    }

    // wp-config.php changes.
    let config_changes = !login_ok || !name_ok || prefix != wp.prefix();
    let mut wp = wp;
    if apply && config_changes {
        let saved = save_config(home, &wp)?;
        report.config_backup = Some(saved.display().to_string());
        let mut text = read_text(&wp.config_file)?;
        if !login_ok {
            text = live::local_config(&text, &database, port, &site.url()).0;
        } else if !name_ok {
            text = config::set_define(&text, "DB_NAME", &config::quote(&database));
        }
        if prefix != wp.prefix() {
            text = config::set_table_prefix(&text, &prefix);
        }
        write_text(&wp.config_file, &text)?;
        wp = WpSite::open(site)?;
    }
    if !has_tables {
        return Ok(report);
    }

    // From here on the fixes change the database: back it up once.
    let mut backup: Option<String> = None;
    let mut backed_up = |report: &mut Report| -> Result<()> {
        if apply && backup.is_none() {
            let file = client.backup(&database)?.file;
            report.backup = Some(file.clone());
            backup = Some(file);
        }
        Ok(())
    };
    let sql = |text: &str| client.query(Some(&database), text);
    let table = |t: &str| format!("{prefix}{t}");

    // Damaged tables.
    if !list.is_empty() {
        let idents: Vec<String> = list.iter().map(|t| databases::ident(&t.name)).collect();
        let checked = sql(&format!("CHECK TABLE {}", idents.join(", ")))?;
        let mut bad: Vec<String> = Vec::new();
        for row in checked.into_iter().flat_map(|s| s.rows) {
            let at = |i: usize| row.get(i).cloned().flatten().unwrap_or_default();
            let (full, kind, text) = (at(0), at(2), at(3));
            let short = full.rsplit('.').next().unwrap_or(&full).to_string();
            let fine = kind.eq_ignore_ascii_case("status") && text.eq_ignore_ascii_case("OK")
                || kind.eq_ignore_ascii_case("note")
                || kind.eq_ignore_ascii_case("warning");
            if !fine && !bad.contains(&short) {
                bad.push(short);
            }
        }
        if bad.is_empty() {
            report.add(
                "Tables",
                Status::Ok,
                format!("All {} tables check out.", list.len()),
            );
        }
        for t in bad {
            let engine = list
                .iter()
                .find(|x| x.name == t)
                .map(|x| x.engine.clone())
                .unwrap_or_default();
            let repairable = ["MyISAM", "Aria"]
                .iter()
                .any(|e| engine.eq_ignore_ascii_case(e));
            if !repairable {
                report.add(
                    "Tables",
                    Status::Problem,
                    format!("{t} is damaged and {engine} tables can't be repaired in place; restore a backup or clone the site again."),
                );
            } else if apply {
                backed_up(&mut report)?;
                match sql(&format!("REPAIR TABLE {}", databases::ident(&t))) {
                    Ok(_) => report.add(
                        "Tables",
                        Status::Fixed,
                        format!("{t} was damaged and is repaired."),
                    ),
                    Err(e) => report.add(
                        "Tables",
                        Status::Problem,
                        format!("{t} is damaged and couldn't be repaired: {e}"),
                    ),
                }
            } else {
                report.add(
                    "Tables",
                    Status::Fix,
                    format!("{t} is damaged; repair fixes it."),
                );
            }
        }
    }

    // Missing core tables, made with WordPress's own table definitions.
    let missing: Vec<&str> = CORE_TABLES
        .iter()
        .copied()
        .filter(|t| !names.contains(&table(t).as_str()))
        .collect();
    if !missing.is_empty() {
        let list = missing
            .iter()
            .map(|t| table(t))
            .collect::<Vec<_>>()
            .join(", ");
        if apply {
            backed_up(&mut report)?;
            let code = "require_once ABSPATH . 'wp-admin/includes/upgrade.php'; dbDelta( wp_get_db_schema( 'all' ) );";
            match wp.wp(home, Load::Core, &["eval", code]) {
                Ok(_) => report.add(
                    "Core tables",
                    Status::Fixed,
                    format!("Missing tables made: {list}."),
                ),
                Err(e) => report.add(
                    "Core tables",
                    Status::Problem,
                    format!("Missing tables {list} couldn't be made: {e}"),
                ),
            }
        } else {
            report.add(
                "Core tables",
                Status::Fix,
                format!("Missing: {list}; repair makes them."),
            );
        }
    } else {
        report.add(
            "Core tables",
            Status::Ok,
            "All of WordPress's own tables are there.",
        );
    }

    // ID columns without AUTO_INCREMENT or a primary key.
    let id_rows = client
        .rows(
            &database,
            &format!(
                "SELECT TABLE_NAME, COLUMN_NAME, COLUMN_TYPE, COLUMN_KEY, EXTRA FROM information_schema.COLUMNS \
                 WHERE TABLE_SCHEMA = {} AND TABLE_NAME IN ({})",
                databases::literal(&database),
                ID_COLUMNS
                    .iter()
                    .map(|(t, _)| databases::literal(&table(t)))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        )?
        .rows;
    let mut ids_ok = true;
    for (t, column) in ID_COLUMNS {
        let full = table(t);
        let Some(row) = id_rows.iter().find(|r| {
            r.first().cloned().flatten().as_deref() == Some(full.as_str())
                && r.get(1)
                    .cloned()
                    .flatten()
                    .is_some_and(|c| c.eq_ignore_ascii_case(column))
        }) else {
            continue;
        };
        let at = |i: usize| row.get(i).cloned().flatten().unwrap_or_default();
        let (kind, key, extra) = (at(2), at(3), at(4));
        if extra.to_ascii_lowercase().contains("auto_increment") {
            continue;
        }
        ids_ok = false;
        let ident = databases::ident(&full);
        let col = databases::ident(column);
        if !apply {
            report.add(
                "IDs",
                Status::Fix,
                format!("{full}.{column} lost AUTO_INCREMENT, so new rows get ID 0; repair puts it back."),
            );
            continue;
        }
        let duplicates = sql(&format!(
            "SELECT COUNT(*) - COUNT(DISTINCT {col}) FROM {ident} WHERE {col} <> 0"
        ))?
        .into_iter()
        .next()
        .and_then(|s| s.rows.into_iter().next())
        .and_then(|r| r.into_iter().next().flatten())
        .and_then(|n| n.parse::<u64>().ok())
        .unwrap_or(0);
        if duplicates > 0 {
            report.add(
                "IDs",
                Status::Problem,
                format!("{full}.{column} lost AUTO_INCREMENT and has {duplicates} repeated IDs; fix those in phpMyAdmin first."),
            );
            continue;
        }
        backed_up(&mut report)?;
        let primary = if key.eq_ignore_ascii_case("PRI") {
            String::new()
        } else {
            format!(", ADD PRIMARY KEY ({col})")
        };
        let fix = format!(
            "SET @sy_next := (SELECT COALESCE(MAX({col}), 0) FROM {ident}); \
             UPDATE {ident} SET {col} = (@sy_next := @sy_next + 1) WHERE {col} = 0; \
             ALTER TABLE {ident} MODIFY {col} {kind} NOT NULL AUTO_INCREMENT{primary};"
        );
        match sql(&fix) {
            Ok(_) => report.add(
                "IDs",
                Status::Fixed,
                format!("{full}.{column} has AUTO_INCREMENT again; rows with ID 0 got new IDs."),
            ),
            Err(e) => report.add(
                "IDs",
                Status::Problem,
                format!("{full}.{column} lost AUTO_INCREMENT and it couldn't be put back: {e}"),
            ),
        }
    }
    if ids_ok && missing.len() < CORE_TABLES.len() {
        report.add(
            "IDs",
            Status::Ok,
            "ID columns count up by themselves (AUTO_INCREMENT).",
        );
    }

    // Roles and user rights, which WordPress keys by table prefix.
    let options = databases::ident(&table("options"));
    let usermeta = databases::ident(&table("usermeta"));
    if names.contains(&table("options").as_str()) {
        let roles: Vec<String> = sql(&format!(
            "SELECT option_name FROM {options} WHERE option_name LIKE '%user\\_roles'"
        ))?
        .into_iter()
        .flat_map(|s| s.rows)
        .filter_map(|r| r.into_iter().next().flatten())
        .collect();
        let own = table("user_roles");
        let others: Vec<&String> = roles.iter().filter(|r| **r != own).collect();
        if roles.contains(&own) {
            report.add("Roles", Status::Ok, "User roles are there.");
        } else if let [other] = others.as_slice() {
            if apply {
                backed_up(&mut report)?;
                sql(&format!(
                    "UPDATE {options} SET option_name = {} WHERE option_name = {}",
                    databases::literal(&own),
                    databases::literal(other)
                ))?;
            }
            report.add(
                "Roles",
                fixing(apply),
                format!("User roles are stored as {other}, not {own}, so nobody can open wp-admin; renamed."),
            );
        } else if apply {
            backed_up(&mut report)?;
            let code = "require_once ABSPATH . 'wp-admin/includes/upgrade.php'; populate_roles();";
            match wp.wp(home, Load::Core, &["eval", code]) {
                Ok(_) => report.add(
                    "Roles",
                    Status::Fixed,
                    "User roles were missing; WordPress's default roles are back.",
                ),
                Err(e) => report.add(
                    "Roles",
                    Status::Problem,
                    format!("User roles are missing and couldn't be made: {e}"),
                ),
            }
        } else {
            report.add(
                "Roles",
                Status::Fix,
                "User roles are missing; repair puts back WordPress's default roles.",
            );
        }
    }
    if names.contains(&table("usermeta").as_str()) {
        let keys: Vec<String> = sql(&format!(
            "SELECT DISTINCT meta_key FROM {usermeta} WHERE meta_key LIKE '%capabilities'"
        ))?
        .into_iter()
        .flat_map(|s| s.rows)
        .filter_map(|r| r.into_iter().next().flatten())
        .collect();
        let own = table("capabilities");
        let old: Vec<&str> = keys
            .iter()
            .filter_map(|k| k.strip_suffix("capabilities"))
            .filter(|p| p.ends_with('_') && *p != prefix)
            .collect();
        if keys.contains(&own) || keys.is_empty() {
            report.add(
                "User rights",
                Status::Ok,
                "Users' rights match the table prefix.",
            );
        } else if let [old] = old.as_slice() {
            if apply {
                backed_up(&mut report)?;
                sql(&format!(
                    "UPDATE {usermeta} SET meta_key = CONCAT({new}, SUBSTRING(meta_key, {from})) \
                     WHERE LEFT(meta_key, {len}) = BINARY {old_lit}",
                    new = databases::literal(&prefix),
                    from = old.len() + 1,
                    len = old.len(),
                    old_lit = databases::literal(old),
                ))?;
            }
            report.add(
                "User rights",
                fixing(apply),
                format!("Users' rights are stored under {old}, not {prefix}, so logging in gives no access; renamed."),
            );
        } else {
            report.add(
                "User rights",
                Status::Problem,
                format!("No user has rights under {prefix}; add an admin on the Users tab."),
            );
        }
    }

    // Old `utf8` (3-byte) tables, which can't hold emoji; utf8mb4 holds
    // everything utf8 does, so converting loses nothing.
    let old_utf8: Vec<&TableInfo> = list
        .iter()
        .filter(|t| t.collation.starts_with("utf8_") || t.collation.starts_with("utf8mb3_"))
        .collect();
    let latin: Vec<&str> = list
        .iter()
        .filter(|t| t.collation.starts_with("latin1"))
        .map(|t| t.name.as_str())
        .collect();
    if !old_utf8.is_empty() {
        let names = old_utf8
            .iter()
            .map(|t| t.name.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        let mut failed = Vec::new();
        if apply {
            backed_up(&mut report)?;
            for t in &old_utf8 {
                if let Err(e) = sql(&format!(
                    "ALTER TABLE {} CONVERT TO CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_520_ci",
                    databases::ident(&t.name)
                )) {
                    failed.push(format!("{} ({e})", t.name));
                }
            }
        }
        if failed.is_empty() {
            report.add(
                "Characters",
                fixing(apply),
                format!("Tables in the old 3-byte utf8, which can't store emoji, go to utf8mb4: {names}."),
            );
        } else {
            report.add(
                "Characters",
                Status::Problem,
                format!("These tables couldn't go to utf8mb4: {}", failed.join(", ")),
            );
        }
    }
    if !latin.is_empty() {
        report.add(
            "Characters",
            Status::Problem,
            format!("These tables are latin1: {}. They are left as they are, since converting them can garble text that is already stored wrongly.", latin.join(", ")),
        );
    }
    if old_utf8.is_empty() && latin.is_empty() {
        report.add("Characters", Status::Ok, "Tables use utf8mb4.");
    }

    // The site address: a copy that still says the live address sends
    // every page to the live site.
    let wanted = site.url();
    let home_option = if names.contains(&table("options").as_str()) {
        sql(&format!(
            "SELECT option_value FROM {options} WHERE option_name = 'home'"
        ))?
        .into_iter()
        .flat_map(|s| s.rows)
        .find_map(|r| r.into_iter().next().flatten())
    } else {
        None
    };
    match home_option.map(|h| h.trim_end_matches('/').to_string()) {
        Some(old)
            if !old.is_empty() && old != wanted && !wp.config.defines.contains_key("WP_HOME") =>
        {
            if apply {
                backed_up(&mut report)?;
                match super::search_replace(home, &wp, &old, &wanted, true, false) {
                    Ok(done) => report.add(
                        "Address",
                        Status::Fixed,
                        format!(
                            "The site's address was {old}; changed to {wanted} in {} places.",
                            done.report.replaced
                        ),
                    ),
                    Err(e) => report.add(
                        "Address",
                        Status::Problem,
                        format!("The address {old} couldn't be changed to {wanted}: {e}"),
                    ),
                }
            } else {
                report.add(
                    "Address",
                    Status::Fix,
                    format!("The site's address is {old}, so pages send you there; repair changes it to {wanted} everywhere."),
                );
            }
        }
        Some(_) => report.add(
            "Address",
            Status::Ok,
            format!("The site's address is {wanted}."),
        ),
        None => {}
    }

    if report.findings.iter().all(|f| f.status == Status::Ok) && apply {
        // Nothing needed doing; no backup was taken.
        report.backup = None;
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_db_host() {
        assert_eq!(host_port("127.0.0.1:3307"), (true, Some(3307)));
        assert_eq!(host_port("localhost"), (true, None));
        assert_eq!(
            host_port("localhost:/run/mysqld/mysqld.sock"),
            (false, None)
        );
        assert_eq!(host_port("db.example.com:3306"), (false, Some(3306)));
        assert_eq!(host_port("::1"), (true, None));
        assert_eq!(host_port("[::1]:3307"), (true, Some(3307)));
    }

    #[test]
    fn finds_wordpress_prefixes() {
        let names = [
            "ids_options",
            "ids_posts",
            "ids_users",
            "ids_2_options",
            "ids_2_posts",
            "other",
        ];
        assert_eq!(prefixes(&names), vec!["ids_".to_string()]);
        assert!(prefixes(&["wp_posts"]).is_empty());
    }
}

/// Runs against MySQL or MariaDB running in `STACKYARD_HOME`:
/// `STACKYARD_TEST_REPAIR=1 cargo test -p stackyard-core -- --ignored repairs_a_broken_copy`.
/// It makes a database `repairtest` and drops it again.
#[cfg(test)]
mod server_tests {
    use super::*;

    #[test]
    #[ignore = "needs MySQL or MariaDB running in STACKYARD_HOME"]
    fn repairs_a_broken_copy() {
        if std::env::var_os("STACKYARD_TEST_REPAIR").is_none() {
            return;
        }
        let home = crate::paths::home();
        let dir = crate::testutil::tempdir();
        super::super::tests::fake_wordpress(&dir);
        // A copy whose wp-config.php still has the live login and database,
        // with the old table prefix.
        std::fs::write(
            dir.join("wp-config.php"),
            "<?php\ndefine( 'DB_NAME', 'liveuser_db' );\ndefine( 'DB_USER', 'liveuser' );\n\
             define( 'DB_PASSWORD', 'secret' );\ndefine( 'DB_HOST', 'localhost:/run/mysqld/mysqld.sock' );\n\
             $table_prefix = 'wp_';\n/* That's all, stop editing! */\nrequire_once ABSPATH . 'wp-settings.php';\n",
        )
        .unwrap();
        let mut site = super::super::tests::site_at(&dir);
        site.name = "repairtest".into();

        let client = Client::open(&home).unwrap();
        let _ = client.drop("repairtest");
        client.create("repairtest").unwrap();
        // Tables with the prefix ids_, IDs without AUTO_INCREMENT, roles
        // and rights under another prefix, an old utf8 table, the live address.
        let mut sql = String::from(
            "CREATE TABLE ids_options (option_id bigint(20) unsigned NOT NULL, option_name varchar(191) NOT NULL, \
               option_value longtext NOT NULL, autoload varchar(20) NOT NULL DEFAULT 'yes', UNIQUE KEY option_name (option_name)) DEFAULT CHARSET=utf8mb4;\
             CREATE TABLE ids_posts (ID bigint(20) unsigned NOT NULL, post_title text NOT NULL, guid varchar(255) NOT NULL DEFAULT '') DEFAULT CHARSET=utf8mb4;\
             CREATE TABLE ids_users (ID bigint(20) unsigned NOT NULL AUTO_INCREMENT, user_login varchar(60) NOT NULL, PRIMARY KEY (ID)) DEFAULT CHARSET=utf8mb4;\
             CREATE TABLE ids_usermeta (umeta_id bigint(20) unsigned NOT NULL AUTO_INCREMENT, user_id bigint(20) unsigned NOT NULL DEFAULT 0, \
               meta_key varchar(255) DEFAULT NULL, meta_value longtext, PRIMARY KEY (umeta_id)) DEFAULT CHARSET=utf8mb4;\
             CREATE TABLE ids_terms (term_id bigint(20) unsigned NOT NULL AUTO_INCREMENT, name varchar(200) NOT NULL DEFAULT '', PRIMARY KEY (term_id)) DEFAULT CHARSET=utf8mb3;",
        );
        for t in [
            "commentmeta",
            "comments",
            "links",
            "postmeta",
            "term_relationships",
            "term_taxonomy",
            "termmeta",
        ] {
            sql.push_str(&format!(
                "CREATE TABLE ids_{t} (id bigint(20) unsigned NOT NULL AUTO_INCREMENT, PRIMARY KEY (id)) DEFAULT CHARSET=utf8mb4;"
            ));
        }
        sql.push_str(
            "INSERT INTO ids_options VALUES (1, 'siteurl', 'https://live.example.com', 'yes'), \
               (2, 'home', 'https://live.example.com', 'yes'), (3, 'old_user_roles', 'a:0:{}', 'yes'), (0, 'blogname', 'Shop', 'yes');\
             INSERT INTO ids_posts VALUES (1, 'Hello', 'https://live.example.com/?p=1'), (0, 'Lost', '');\
             INSERT INTO ids_users (user_login) VALUES ('admin');\
             INSERT INTO ids_usermeta (user_id, meta_key, meta_value) VALUES (1, 'old_capabilities', 'a:1:{s:13:\"administrator\";b:1;}'), (1, 'old_user_level', '10');",
        );
        client.query(Some("repairtest"), &sql).unwrap();

        let found = check(&home, &site).unwrap();
        let status = |r: &Report, check: &str| -> Vec<Status> {
            r.findings
                .iter()
                .filter(|f| f.check == check)
                .map(|f| f.status)
                .collect()
        };
        for name in [
            "Database",
            "Prefix",
            "Login",
            "Roles",
            "User rights",
            "Characters",
            "Address",
        ] {
            assert!(
                status(&found, name).contains(&Status::Fix),
                "{name}: {found:#?}"
            );
        }
        assert_eq!(
            status(&found, "IDs"),
            vec![Status::Fix, Status::Fix],
            "{found:#?}"
        );

        let done = repair(&home, &site).unwrap();
        assert!(
            done.findings
                .iter()
                .all(|f| f.status != Status::Problem && f.status != Status::Fix),
            "{done:#?}"
        );
        assert!(
            done.backup.is_some() && done.config_backup.is_some(),
            "{done:#?}"
        );

        let again = check(&home, &site).unwrap();
        assert!(
            again.findings.iter().all(|f| f.status == Status::Ok),
            "{again:#?}"
        );
        let wp = WpSite::open(&site).unwrap();
        assert_eq!(wp.prefix(), "ids_");
        assert_eq!(wp.config.get("DB_NAME"), Some("repairtest"));
        assert_eq!(wp.config.get("DB_USER"), Some("root"));
        assert_eq!(
            wp.option(&home, "home").unwrap().as_deref(),
            Some("https://repairtest.stack")
        );
        assert!(wp.option(&home, "ids_user_roles").unwrap().is_some());
        let ids = wp
            .sql(&home, "SELECT ID FROM ids_posts ORDER BY ID")
            .unwrap();
        assert_eq!(ids.len(), 2);
        assert!(ids.iter().all(|r| r[0].as_deref() != Some("0")), "{ids:?}");
        wp.sql(&home, "INSERT INTO ids_posts (post_title) VALUES ('New')")
            .unwrap();
        let _ = std::fs::remove_file(done.config_backup.unwrap());
        client.drop("repairtest").unwrap();

        // A copy whose database never came along: an empty one is made and
        // wp-config.php points at it, ready for an import.
        site.name = "repairempty".into();
        std::fs::write(
            dir.join("wp-config.php"),
            "<?php\ndefine( 'DB_NAME', 'gone_db' );\ndefine( 'DB_USER', 'gone' );\n$table_prefix = 'wp_';\n",
        )
        .unwrap();
        let _ = client.drop("repairempty");
        let found = check(&home, &site).unwrap();
        assert_eq!(
            status(&found, "Database"),
            vec![Status::Problem],
            "{found:#?}"
        );
        let done = repair(&home, &site).unwrap();
        assert!(
            done.findings
                .iter()
                .any(|f| f.text.contains("empty database repairempty")),
            "{done:#?}"
        );
        let again = check(&home, &site).unwrap();
        assert_eq!(status(&again, "Database"), vec![Status::Ok], "{again:#?}");
        assert_eq!(status(&again, "Login"), vec![Status::Ok], "{again:#?}");
        assert_eq!(
            status(&again, "Tables"),
            vec![Status::Problem],
            "{again:#?}"
        );
        let _ = std::fs::remove_file(done.config_backup.unwrap());
        client.drop("repairempty").unwrap();
    }
}
