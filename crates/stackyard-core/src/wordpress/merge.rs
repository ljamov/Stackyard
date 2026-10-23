//! Merging a staging into another copy of the same site: its main site or
//! another staging.
//!
//! [`changes`] lists what changed on the staging since it was made (its
//! base snapshot): files, plugins and the theme switched on or off, and
//! settings. Anything the target also changed since then, differently, is
//! a conflict the user decides. [`merge`] then applies what was picked, at
//! one of three [`Level`]s, after a snapshot of the target so the merge can
//! be undone.
//!
//! Databases can't be merged row by row: both copies number new posts and
//! orders from the same counters. So the levels go from safe to whole:
//!
//! - **Code** moves files and which plugins and theme are on; the target
//!   keeps its whole database.
//! - **Settings** adds the options picked from the list (WordPress,
//!   WooCommerce, widgets, theme settings).
//! - **Everything** replaces the target's database with the staging's,
//!   but keeps the target's users, orders and customers (both kinds of
//!   WooCommerce store: High-Performance Order Storage and orders kept as
//!   posts). Staging posts whose numbers clash with orders the target took
//!   meanwhile are given new numbers, with their references changed.
//!
//! Single posts and pages are moved with [`super::posts`].

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::snapshot::{self, Facts, FileEntry, Manifest};
use super::{posts, replace, staging, ExtensionAction, Kind, WpSite};
use crate::databases::{ident, literal, Client};
use crate::sites::{Site, SiteStore};
use crate::{Error, Result};

fn fail(message: impl Into<String>) -> Error {
    Error::WordPress(message.into())
}

fn io(path: &Path) -> impl FnOnce(std::io::Error) -> Error + '_ {
    move |source| Error::Io {
        path: path.to_owned(),
        source,
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    #[default]
    Code,
    Settings,
    Everything,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Change {
    Added,
    Changed,
    Removed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FileChange {
    /// Relative to the site's folder, `/`-separated.
    pub path: String,
    /// What it belongs to: `plugin:woocommerce`, `theme:storefront`,
    /// `mu-plugins`, `media`, `languages`, `content`, `core` or `other`.
    pub group: String,
    pub change: Change,
    /// How the target changed it since the staging was made, if it did.
    pub target: Option<Change>,
    /// Changed on both sides, differently.
    pub conflict: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Switch {
    /// `plugin:woocommerce` or `theme:storefront`.
    pub id: String,
    pub title: String,
    /// On (activated) or off on the staging.
    pub on: bool,
    pub conflict: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SettingChange {
    pub name: String,
    pub change: Change,
    pub conflict: bool,
    /// The start of each side's value, for showing.
    pub staging: Option<String>,
    pub target: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Store {
    /// High-Performance Order Storage; otherwise orders are posts.
    pub hpos: bool,
    pub orders: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChangeList {
    pub source: String,
    pub target: String,
    /// When the staging was made.
    pub base_at: u64,
    pub files: Vec<FileChange>,
    pub switches: Vec<Switch>,
    pub settings: Vec<SettingChange>,
    /// The target's store, when it runs WooCommerce.
    pub store: Option<Store>,
    /// Posts and pages made on the target since the staging was made: an
    /// Everything merge replaces them with the staging's content.
    pub target_new_posts: u64,
    /// Users added on the staging: an Everything merge keeps the target's.
    pub staging_new_users: u64,
}

/// What to merge, picked from the [`ChangeList`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct Choice {
    pub level: Level,
    /// Files to take from the staging (added, changed or removed there).
    #[serde(default)]
    pub files: Vec<String>,
    /// Switch ids to apply.
    #[serde(default)]
    pub switches: Vec<String>,
    /// Options to take from the staging (Settings level).
    #[serde(default)]
    pub settings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Merged {
    /// The target's snapshot from before the merge, to undo it.
    pub snapshot: String,
    pub files: usize,
    pub switches: usize,
    pub settings: usize,
    pub notes: Vec<String>,
}

/// Files a merge never touches: each copy's own config, Stackyard's plugin
/// and leftovers.
pub(super) fn kept_apart(rel: &str, content: &str) -> bool {
    matches!(rel, "wp-config.php" | "wp-config.live.php" | ".user.ini")
        || rel == format!("{content}/mu-plugins/stackyard.php")
        || rel == format!("{content}/debug.log")
        || rel == format!("{content}/object-cache.php.stackyard-off")
        || rel.starts_with(&format!("{content}/upgrade/"))
        || rel.starts_with(&format!("{content}/cache/"))
        || rel.ends_with(".stackyard-part")
}

/// What a file belongs to, for grouping the list.
pub fn group_of(rel: &str, content: &str) -> String {
    let under = |dir: &str| rel.strip_prefix(&format!("{content}/{dir}/"));
    let first = |rest: &str| -> String {
        let name = rest.split('/').next().unwrap_or(rest);
        name.strip_suffix(".php").unwrap_or(name).to_string()
    };
    if let Some(rest) = under("plugins") {
        return format!("plugin:{}", first(rest));
    }
    if let Some(rest) = under("themes") {
        return format!("theme:{}", first(rest));
    }
    if under("mu-plugins").is_some() {
        return "mu-plugins".into();
    }
    if under("uploads").is_some() {
        return "media".into();
    }
    if under("languages").is_some() {
        return "languages".into();
    }
    if rel.starts_with(&format!("{content}/")) {
        return "content".into();
    }
    let core = rel.starts_with("wp-admin/")
        || rel.starts_with("wp-includes/")
        || (!rel.contains('/') && rel.starts_with("wp-") && rel.ends_with(".php"))
        || matches!(
            rel,
            "index.php" | "xmlrpc.php" | "license.txt" | "readme.html"
        );
    if core {
        "core".into()
    } else {
        "other".into()
    }
}

/// The same file on both sides: same checksum, or for media the same size.
pub(super) fn same(a: Option<&FileEntry>, b: Option<&FileEntry>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => match (&a.hash, &b.hash) {
            (Some(x), Some(y)) => x == y,
            _ => a.size == b.size,
        },
        _ => false,
    }
}

fn change_of(base: Option<&FileEntry>, now: Option<&FileEntry>) -> Option<Change> {
    if same(base, now) {
        return None;
    }
    Some(match (base, now) {
        (None, _) => Change::Added,
        (_, None) => Change::Removed,
        _ => Change::Changed,
    })
}

/// Compares three file lists: the base, the staging now and the target now.
pub fn file_changes(
    base: &Manifest,
    source: &Manifest,
    target: &Manifest,
    content: &str,
) -> Vec<FileChange> {
    let paths: BTreeSet<&String> = base
        .keys()
        .chain(source.keys())
        .chain(target.keys())
        .collect();
    paths
        .into_iter()
        .filter(|rel| !kept_apart(rel, content))
        .filter_map(|rel| {
            let (b, s, t) = (base.get(rel), source.get(rel), target.get(rel));
            let change = change_of(b, s)?;
            if same(s, t) {
                return None;
            }
            let target_change = change_of(b, t);
            Some(FileChange {
                path: rel.clone(),
                group: group_of(rel, content),
                change,
                target: target_change,
                conflict: target_change.is_some(),
            })
        })
        .collect()
}

/// Options a merge leaves alone: each copy's address, what Stackyard and
/// WordPress keep for themselves, caches, and the store's order storage.
pub(super) fn option_kept_apart(name: &str) -> bool {
    const NAMES: &[&str] = &[
        "siteurl",
        "home",
        "active_plugins",
        "template",
        "stylesheet",
        "current_theme",
        "cron",
        "rewrite_rules",
        "recently_activated",
        "db_version",
        "db_upgraded",
        "initial_db_version",
        "upload_path",
        "upload_url_path",
        "uninstall_plugins",
        "auto_updater.lock",
        "core_updater.lock",
        "can_compress_scripts",
        "theme_switched",
        "recovery_keys",
        "recovery_mode_email_last_sent",
        "fresh_site",
        "admin_email_lifespan",
        "wp_force_deactivated_plugins",
        "auto_core_update_notified",
        "blog_public",
        "woocommerce_custom_orders_table_enabled",
        "woocommerce_custom_orders_table_data_sync_enabled",
        "woocommerce_db_version",
        "woocommerce_version",
        "woocommerce_schema_version",
    ];
    const PREFIXES: &[&str] = &[
        "_transient_",
        "_site_transient_",
        "stackyard_",
        "action_scheduler_lock_",
        "wc_remote_inbox_",
        "_wc_session_",
    ];
    NAMES.contains(&name) || PREFIXES.iter().any(|p| name.starts_with(p))
}

/// Compares option checksums: the base, the staging now and the target now.
pub fn option_changes(
    base: &BTreeMap<String, String>,
    source: &BTreeMap<String, String>,
    target: &BTreeMap<String, String>,
) -> Vec<(String, Change, bool)> {
    let names: BTreeSet<&String> = base.keys().chain(source.keys()).collect();
    names
        .into_iter()
        .filter(|n| !option_kept_apart(n))
        .filter_map(|name| {
            let (b, s, t) = (base.get(name), source.get(name), target.get(name));
            if b == s || s == t {
                return None;
            }
            let change = match (b, s) {
                (None, _) => Change::Added,
                (_, None) => Change::Removed,
                _ => Change::Changed,
            };
            Some((name.clone(), change, b != t))
        })
        .collect()
}

/// The plugin's slug WP-CLI knows it by, from its file.
pub(super) fn plugin_slug(file: &str) -> String {
    match file.split_once('/') {
        Some((dir, _)) => dir.to_string(),
        None => file.trim_end_matches(".php").to_string(),
    }
}

/// Plugins and the theme switched on or off on the staging.
pub fn switch_changes(base: &Facts, source: &Facts, target: &Facts) -> Vec<Switch> {
    let set = |f: &Facts| -> BTreeSet<String> {
        f.active_plugins.iter().map(|p| plugin_slug(p)).collect()
    };
    let (b, s, t) = (set(base), set(source), set(target));
    let mut out: Vec<Switch> = b
        .symmetric_difference(&s)
        .filter_map(|slug| {
            let on = s.contains(slug);
            if t.contains(slug) == on {
                return None;
            }
            Some(Switch {
                id: format!("plugin:{slug}"),
                title: slug.clone(),
                on,
                conflict: t.contains(slug) != b.contains(slug),
            })
        })
        .collect();
    if source.stylesheet != base.stylesheet
        && source.stylesheet != target.stylesheet
        && !source.stylesheet.is_empty()
    {
        out.push(Switch {
            id: format!("theme:{}", source.stylesheet),
            title: source.stylesheet.clone(),
            on: true,
            conflict: target.stylesheet != base.stylesheet,
        });
    }
    out
}

struct Sides {
    source: Site,
    target: Site,
    source_wp: WpSite,
    target_wp: WpSite,
    base: snapshot::Snapshot,
}

fn sides(home: &Path, source: &str, target: &str) -> Result<Sides> {
    if source == target {
        return Err(fail("pick another copy to merge into"));
    }
    let main = staging::main_of(home, source);
    if staging::main_of(home, target) != main {
        return Err(fail(format!(
            "{target} is not a copy of the same site as {source}"
        )));
    }
    let base = snapshot::base(home, source).ok_or_else(|| {
        fail(format!(
            "{source} is not a staging, or was made before stagings could be merged"
        ))
    })?;
    let store = SiteStore::load_from(&home.join("sites.toml"))?;
    let find = |domain: &str| {
        store
            .list()
            .iter()
            .find(|s| s.domain() == domain)
            .cloned()
            .ok_or_else(|| Error::SiteNotFound(domain.to_string()))
    };
    let (source, target) = (find(source)?, find(target)?);
    Ok(Sides {
        source_wp: WpSite::open(&source)?,
        target_wp: WpSite::open(&target)?,
        source,
        target,
        base,
    })
}

/// The content folder relative to the site's folder, e.g. `wp-content`.
pub(super) fn content_rel(wp: &WpSite) -> String {
    wp.wp_content()
        .strip_prefix(&wp.site.path)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| "wp-content".into())
}

fn first_value(set: &crate::databases::ResultSet) -> Option<String> {
    set.rows.first().and_then(|r| r.first().cloned().flatten())
}

fn count(client: &Client, database: &str, sql: &str) -> u64 {
    client
        .rows(database, sql)
        .ok()
        .and_then(|s| first_value(&s))
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}

fn table_exists(client: &Client, database: &str, table: &str) -> bool {
    count(
        client,
        database,
        &format!(
            "SELECT COUNT(*) FROM information_schema.TABLES WHERE TABLE_SCHEMA = {} AND TABLE_NAME = {}",
            literal(database),
            literal(table)
        ),
    ) > 0
}

const ORDER_TYPES: &str =
    "'shop_order','shop_order_refund','shop_order_placehold','shop_subscription'";

fn store_of(client: &Client, wp: &WpSite) -> Result<Option<Store>> {
    let db = wp.database()?;
    let options = ident(&wp.table("options"));
    let value = |name: &str| {
        client
            .rows(
                &db,
                &format!(
                    "SELECT option_value FROM {options} WHERE option_name = {}",
                    literal(name)
                ),
            )
            .ok()
            .and_then(|s| first_value(&s))
    };
    if value("woocommerce_version").is_none() {
        return Ok(None);
    }
    let hpos = value("woocommerce_custom_orders_table_enabled").as_deref() == Some("yes")
        && table_exists(client, &db, &wp.table("wc_orders"));
    let orders = if hpos {
        count(
            client,
            &db,
            &format!(
                "SELECT COUNT(*) FROM {} WHERE type = 'shop_order'",
                ident(&wp.table("wc_orders"))
            ),
        )
    } else {
        count(
            client,
            &db,
            &format!(
                "SELECT COUNT(*) FROM {} WHERE post_type = 'shop_order'",
                ident(&wp.table("posts"))
            ),
        )
    };
    Ok(Some(Store { hpos, orders }))
}

/// Lists what changed on the staging `source` since it was made, against
/// `target`. The database server must be running.
pub fn changes(home: &Path, source: &str, target: &str) -> Result<ChangeList> {
    let sides = sides(home, source, target)?;
    let client = Client::open(home)?;
    let base_facts = sides.base.facts(home)?;
    let base_files = sides.base.manifest(home)?;
    let source_files = snapshot::manifest_with(&sides.source.path, &base_files)?;
    let target_files = snapshot::manifest_with(&sides.target.path, &base_files)?;
    let content = content_rel(&sides.source_wp);
    let files = file_changes(&base_files, &source_files, &target_files, &content);

    let source_db = sides.source_wp.database()?;
    let target_db = sides.target_wp.database()?;
    let source_facts = snapshot::read_facts(&client, &source_db, sides.source_wp.prefix())?;
    let target_facts = snapshot::read_facts(&client, &target_db, sides.target_wp.prefix())?;
    let switches = switch_changes(&base_facts, &source_facts, &target_facts);
    let options = option_changes(
        &base_facts.options,
        &source_facts.options,
        &target_facts.options,
    );
    let preview = |db: &str, wp: &WpSite| -> Result<BTreeMap<String, String>> {
        if options.is_empty() {
            return Ok(BTreeMap::new());
        }
        let names: Vec<String> = options.iter().map(|(n, _, _)| literal(n)).collect();
        Ok(client
            .rows(
                db,
                &format!(
                    "SELECT option_name, LEFT(option_value, 240) FROM {} WHERE option_name IN ({})",
                    ident(&wp.table("options")),
                    names.join(",")
                ),
            )?
            .rows
            .into_iter()
            .filter_map(|r| {
                Some((
                    r.first()?.clone()?,
                    r.get(1).cloned().flatten().unwrap_or_default(),
                ))
            })
            .collect())
    };
    let source_values = preview(&source_db, &sides.source_wp)?;
    let target_values = preview(&target_db, &sides.target_wp)?;
    let settings = options
        .into_iter()
        .map(|(name, change, conflict)| SettingChange {
            staging: source_values.get(&name).cloned(),
            target: target_values.get(&name).cloned(),
            name,
            change,
            conflict,
        })
        .collect();

    let target_new_posts = count(
        &client,
        &target_db,
        &format!(
            "SELECT COUNT(*) FROM {} WHERE ID > {} AND post_type NOT IN ({ORDER_TYPES},'revision','nav_menu_item') AND post_status <> 'auto-draft'",
            ident(&sides.target_wp.table("posts")),
            base_facts.max_post_id
        ),
    );
    let staging_new_users = count(
        &client,
        &source_db,
        &format!(
            "SELECT COUNT(*) FROM {} WHERE ID > {}",
            ident(&sides.source_wp.table("users")),
            base_facts.max_user_id
        ),
    );
    Ok(ChangeList {
        source: source.to_string(),
        target: target.to_string(),
        base_at: sides.base.created_at,
        files,
        switches,
        settings,
        store: store_of(&client, &sides.target_wp)?,
        target_new_posts,
        staging_new_users,
    })
}

/// Merges what `choice` picks from the staging `source` into `target`.
/// A snapshot of the target is taken first; its id is returned for undo.
pub fn merge(home: &Path, source: &str, target: &str, choice: &Choice) -> Result<Merged> {
    let sides = sides(home, source, target)?;
    let client = Client::open(home)?;
    let mut notes = Vec::new();
    let mut snap = snapshot::take(home, &sides.target_wp, &format!("Merge from {source}"))?;

    // Files first: plugins must be there before they are switched on.
    for rel in &choice.files {
        if rel.contains("..") || rel.starts_with('/') || rel.contains('\\') {
            return Err(fail(format!("{rel} is not a path inside the site")));
        }
        let from = sides.source.path.join(rel);
        let to = sides.target.path.join(rel);
        snap.save_file(home, &sides.target.path, rel)?;
        if from.is_file() {
            if let Some(parent) = to.parent() {
                std::fs::create_dir_all(parent).map_err(io(parent))?;
            }
            super::copy::copy_file(&from, &to).map_err(io(&to))?;
        } else if to.is_file() {
            std::fs::remove_file(&to).map_err(io(&to))?;
        }
    }

    let rules = replace::Rules::site_move(
        &super::home_url(home, &sides.source_wp),
        &super::home_url(home, &sides.target_wp),
    )
    .and_path(
        &sides.source.path.to_string_lossy(),
        &sides.target.path.to_string_lossy(),
    );
    let source_db = sides.source_wp.database()?;
    let target_db = sides.target_wp.database()?;
    let mut settings = 0;
    match choice.level {
        Level::Code => {}
        Level::Settings => {
            settings = copy_options(
                &client,
                &sides,
                &source_db,
                &target_db,
                &choice.settings,
                &rules,
            )?;
        }
        Level::Everything => {
            let base_facts = sides.base.facts(home)?;
            notes.extend(everything(
                home,
                &client,
                &sides,
                &source_db,
                &target_db,
                &base_facts,
                &rules,
            )?);
        }
    }

    // Switching plugins on runs their own setup, so it goes last, through
    // WordPress. After an Everything merge the staging's set is already in.
    let mut switches = 0;
    if choice.level != Level::Everything {
        for id in &choice.switches {
            let (kind, name) = match id.split_once(':') {
                Some(("plugin", name)) => (Kind::Plugin, name),
                Some(("theme", name)) => (Kind::Theme, name),
                _ => return Err(fail(format!("{id} is not a plugin or theme"))),
            };
            let on = match kind {
                Kind::Plugin => {
                    let facts =
                        snapshot::read_facts(&client, &source_db, sides.source_wp.prefix())?;
                    facts.active_plugins.iter().any(|p| plugin_slug(p) == name)
                }
                Kind::Theme => true,
            };
            let action = if on {
                ExtensionAction::Activate
            } else {
                ExtensionAction::Deactivate
            };
            match super::change_extension(home, &sides.target_wp, kind, name, action) {
                Ok(()) => switches += 1,
                Err(e) => notes.push(format!(
                    "{name} could not be switched {}: {e}",
                    if on { "on" } else { "off" }
                )),
            }
        }
    }
    let _ = super::muplugin::install(
        &sides.target_wp,
        &super::muplugin::Options::for_site(home, &sides.target_wp),
    );
    Ok(Merged {
        snapshot: snap.id,
        files: choice.files.len(),
        switches,
        settings,
        notes,
    })
}

/// Copies the picked options from the staging, with its address changed
/// to the target's; an option removed on the staging is removed.
fn copy_options(
    client: &Client,
    sides: &Sides,
    source_db: &str,
    target_db: &str,
    names: &[String],
    rules: &replace::Rules,
) -> Result<usize> {
    if names.is_empty() {
        return Ok(0);
    }
    let picked: Vec<String> = names
        .iter()
        .filter(|n| !option_kept_apart(n))
        .map(|n| literal(n))
        .collect();
    let rows = client.rows(
        source_db,
        &format!(
            "SELECT option_name, option_value, autoload FROM {} WHERE option_name IN ({})",
            ident(&sides.source_wp.table("options")),
            picked.join(",")
        ),
    )?;
    let table = ident(&sides.target_wp.table("options"));
    let mut sql = String::from("START TRANSACTION;\n");
    let mut found = BTreeSet::new();
    for row in &rows.rows {
        let (Some(Some(name)), value) = (
            row.first(),
            row.get(1).cloned().flatten().unwrap_or_default(),
        ) else {
            continue;
        };
        let autoload = row
            .get(2)
            .cloned()
            .flatten()
            .unwrap_or_else(|| "yes".into());
        let value = replace::replace_value(value.as_bytes(), rules)
            .map(|v| String::from_utf8_lossy(&v).into_owned())
            .unwrap_or(value);
        sql.push_str(&format!(
            "INSERT INTO {table} (option_name, option_value, autoload) VALUES ({}, {}, {}) \
             ON DUPLICATE KEY UPDATE option_value = VALUES(option_value), autoload = VALUES(autoload);\n",
            literal(name),
            literal(&value),
            literal(&autoload)
        ));
        found.insert(name.clone());
    }
    for name in names
        .iter()
        .filter(|n| !option_kept_apart(n) && !found.contains(*n))
    {
        sql.push_str(&format!(
            "DELETE FROM {table} WHERE option_name = {};\n",
            literal(name)
        ));
    }
    sql.push_str("COMMIT;\n");
    client.execute(target_db, &sql)?;
    Ok(names.len())
}

/// Tables the target keeps in an Everything merge: users and the store's
/// orders, customers and payment data.
pub(super) const PROTECTED: &[&str] = &[
    "users",
    "usermeta",
    "wc_orders",
    "wc_orders_meta",
    "wc_order_addresses",
    "wc_order_operational_data",
    "woocommerce_order_items",
    "woocommerce_order_itemmeta",
    "wc_order_stats",
    "wc_order_product_lookup",
    "wc_order_tax_lookup",
    "wc_order_coupon_lookup",
    "wc_customer_lookup",
    "woocommerce_downloadable_product_permissions",
    "wc_download_log",
    "woocommerce_payment_tokens",
    "woocommerce_payment_tokenmeta",
];

fn ids_of(client: &Client, database: &str, sql: &str) -> Result<Vec<u64>> {
    Ok(client
        .rows(database, sql)?
        .rows
        .iter()
        .filter_map(|r| r.first().cloned().flatten()?.parse().ok())
        .collect())
}

fn id_list(ids: &[u64]) -> String {
    ids.iter().map(u64::to_string).collect::<Vec<_>>().join(",")
}

/// Replaces the target's database with the staging's, keeping the target's
/// users, orders and customers. Built in a scratch database first, so the
/// target is only touched once it is ready.
fn everything(
    home: &Path,
    client: &Client,
    sides: &Sides,
    source_db: &str,
    target_db: &str,
    base: &Facts,
    rules: &replace::Rules,
) -> Result<Vec<String>> {
    if sides.source_wp.prefix() != sides.target_wp.prefix() {
        return Err(fail(
            "the two copies use different table prefixes; merge at the Code or Settings level",
        ));
    }
    everything_between(
        home,
        client,
        sides.target_wp.prefix(),
        source_db,
        target_db,
        base.max_user_id,
        rules,
    )
}

/// The work of an Everything merge between two databases on this
/// computer with the same table prefix: `target_db` ends up with
/// `source_db`'s content and its own users, orders and customers.
pub(super) fn everything_between(
    home: &Path,
    client: &Client,
    prefix: &str,
    source_db: &str,
    target_db: &str,
    base_max_user_id: u64,
    rules: &replace::Rules,
) -> Result<Vec<String>> {
    let mut notes = Vec::new();
    let prefix = prefix.to_string();
    let t = |name: &str| ident(&format!("{prefix}{name}"));
    let scratch = format!("sy_merge_{}_{}", super::env::now(), std::process::id());
    let work = home.join("tmp").join(&scratch);
    std::fs::create_dir_all(&work).map_err(io(&work))?;
    let result = (|| -> Result<()> {
        // The staging's database, with its address changed to the target's.
        let dump = work.join("source.sql.gz");
        client.dump_to(source_db, &[], &dump)?;
        let rewritten = work.join("rewritten.sql.gz");
        let input = flate2::read::GzDecoder::new(std::fs::File::open(&dump).map_err(io(&dump))?);
        let output = flate2::write::GzEncoder::new(
            std::fs::File::create(&rewritten).map_err(io(&rewritten))?,
            flate2::Compression::fast(),
        );
        replace::rewrite(input, output, rules).map_err(io(&rewritten))?;
        client.load(&scratch, &rewritten)?;

        // The staging's own orders go; the target's come in with their numbers.
        let target_orders = ids_of(
            client,
            target_db,
            &format!(
                "SELECT ID FROM {} WHERE post_type IN ({ORDER_TYPES})",
                t("posts")
            ),
        )?;
        let mut sql = String::from("SET FOREIGN_KEY_CHECKS = 0;\n");
        sql.push_str(&format!(
            "DELETE c, m FROM {} c LEFT JOIN {} m ON m.comment_id = c.comment_ID \
             WHERE c.comment_post_ID IN (SELECT ID FROM {} WHERE post_type IN ({ORDER_TYPES}));\n",
            t("comments"),
            t("commentmeta"),
            t("posts")
        ));
        sql.push_str(&format!(
            "DELETE p, m FROM {} p LEFT JOIN {} m ON m.post_id = p.ID WHERE p.post_type IN ({ORDER_TYPES});\n",
            t("posts"),
            t("postmeta")
        ));
        client.execute(&scratch, &sql)?;

        // Staging posts made since the base that took numbers the target's
        // new orders also took get new numbers.
        let clashing: Vec<u64> = if target_orders.is_empty() {
            Vec::new()
        } else {
            ids_of(
                client,
                &scratch,
                &format!(
                    "SELECT ID FROM {} WHERE ID IN ({})",
                    t("posts"),
                    id_list(&target_orders)
                ),
            )?
        };
        if !clashing.is_empty() {
            let top = count(
                client,
                &scratch,
                &format!("SELECT COALESCE(MAX(ID), 0) FROM {}", t("posts")),
            )
            .max(target_orders.iter().copied().max().unwrap_or(0));
            let map: BTreeMap<u64, u64> = clashing
                .iter()
                .enumerate()
                .map(|(i, id)| (*id, top + 1 + i as u64))
                .collect();
            posts::renumber(client, &scratch, &prefix, &map)?;
            notes.push(format!(
                "{} posts made on the staging had the same numbers as orders the target took meanwhile; they got new numbers.",
                map.len()
            ));
        }

        let mut sql = String::from("SET FOREIGN_KEY_CHECKS = 0;\n");
        for name in PROTECTED {
            let table = format!("{prefix}{name}");
            if !table_exists(client, target_db, &table) {
                continue;
            }
            sql.push_str(&format!(
                "DROP TABLE IF EXISTS {s}.{x};\nCREATE TABLE {s}.{x} LIKE {d}.{x};\nINSERT INTO {s}.{x} SELECT * FROM {d}.{x};\n",
                s = ident(&scratch),
                d = ident(target_db),
                x = ident(&table)
            ));
        }
        if !target_orders.is_empty() {
            let ids = id_list(&target_orders);
            sql.push_str(&format!(
                "INSERT INTO {s}.{p} SELECT * FROM {d}.{p} WHERE ID IN ({ids});\n\
                 INSERT INTO {s}.{m} (post_id, meta_key, meta_value) SELECT post_id, meta_key, meta_value FROM {d}.{m} WHERE post_id IN ({ids});\n",
                s = ident(&scratch),
                d = ident(target_db),
                p = t("posts"),
                m = t("postmeta"),
            ));
        }
        client.execute(&scratch, &sql)?;

        // Order notes are comments: they come over one by one so their new
        // numbers don't clash with the staging's comments.
        if !target_orders.is_empty() {
            let notes_ids = ids_of(
                client,
                target_db,
                &format!(
                    "SELECT comment_ID FROM {} WHERE comment_post_ID IN ({})",
                    t("comments"),
                    id_list(&target_orders)
                ),
            )?;
            let columns: Vec<String> = client
                .rows(
                    target_db,
                    &format!(
                        "SELECT COLUMN_NAME FROM information_schema.COLUMNS WHERE TABLE_SCHEMA = {} AND TABLE_NAME = {} AND COLUMN_NAME <> 'comment_ID' ORDER BY ORDINAL_POSITION",
                        literal(target_db),
                        literal(&format!("{prefix}comments"))
                    ),
                )?
                .rows
                .iter()
                .filter_map(|r| r.first().cloned().flatten())
                .map(|c| ident(&c))
                .collect();
            let cols = columns.join(",");
            let mut sql = String::new();
            for id in notes_ids {
                sql.push_str(&format!(
                    "INSERT INTO {s}.{c} ({cols}) SELECT {cols} FROM {d}.{c} WHERE comment_ID = {id};\n\
                     INSERT INTO {s}.{cm} (comment_id, meta_key, meta_value) SELECT LAST_INSERT_ID(), meta_key, meta_value FROM {d}.{cm} WHERE comment_id = {id};\n",
                    s = ident(&scratch),
                    d = ident(target_db),
                    c = t("comments"),
                    cm = t("commentmeta"),
                ));
            }
            if !sql.is_empty() {
                client.execute(&scratch, &sql)?;
            }
        }

        // Posts by users that only the staging had now belong to the
        // target's first administrator.
        let admin = client
            .rows(
                &scratch,
                &format!(
                    "SELECT user_id FROM {} WHERE meta_key = {} AND meta_value LIKE '%\"administrator\"%' ORDER BY user_id LIMIT 1",
                    t("usermeta"),
                    literal(&format!("{prefix}capabilities"))
                ),
            )
            .ok()
            .and_then(|s| first_value(&s));
        if let Some(admin) = admin {
            client.execute(
                &scratch,
                &format!(
                    "UPDATE {p} SET post_author = {admin} WHERE post_author NOT IN (SELECT ID FROM {u});",
                    p = t("posts"),
                    u = t("users"),
                ),
            )?;
        }
        if count(
            client,
            source_db,
            &format!(
                "SELECT COUNT(*) FROM {} WHERE ID > {}",
                t("users"),
                base_max_user_id
            ),
        ) > 0
        {
            notes.push(
                "Users added on the staging were not merged; the target keeps its own users."
                    .into(),
            );
        }

        // Swap: the target's tables are replaced by the scratch ones.
        let tables: Vec<String> = client
            .rows(
                &scratch,
                &format!(
                    "SELECT TABLE_NAME FROM information_schema.TABLES WHERE TABLE_SCHEMA = {} AND TABLE_TYPE = 'BASE TABLE'",
                    literal(&scratch)
                ),
            )?
            .rows
            .iter()
            .filter_map(|r| r.first().cloned().flatten())
            .collect();
        let mut sql = String::from("SET FOREIGN_KEY_CHECKS = 0;\n");
        for table in &tables {
            sql.push_str(&format!(
                "DROP TABLE IF EXISTS {d}.{x};\nRENAME TABLE {s}.{x} TO {d}.{x};\n",
                s = ident(&scratch),
                d = ident(target_db),
                x = ident(table)
            ));
        }
        client.execute(target_db, &sql)?;
        Ok(())
    })();
    let _ = client.drop(&scratch);
    let _ = std::fs::remove_dir_all(&work);
    result?;
    Ok(notes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(size: u64, hash: Option<&str>) -> FileEntry {
        FileEntry {
            size,
            modified: 0,
            hash: hash.map(str::to_string),
        }
    }

    #[test]
    fn groups_files() {
        let c = "wp-content";
        assert_eq!(
            group_of("wp-content/plugins/woocommerce/woocommerce.php", c),
            "plugin:woocommerce"
        );
        assert_eq!(group_of("wp-content/plugins/hello.php", c), "plugin:hello");
        assert_eq!(
            group_of("wp-content/themes/storefront/style.css", c),
            "theme:storefront"
        );
        assert_eq!(group_of("wp-content/uploads/2026/10/a.jpg", c), "media");
        assert_eq!(group_of("wp-content/mu-plugins/x.php", c), "mu-plugins");
        assert_eq!(group_of("wp-includes/version.php", c), "core");
        assert_eq!(group_of("wp-login.php", c), "core");
        assert_eq!(group_of(".htaccess", c), "other");
        assert_eq!(group_of("app/plugins/acf/acf.php", "app"), "plugin:acf");
    }

    #[test]
    fn finds_changes_and_conflicts() {
        let mut base = Manifest::new();
        base.insert("wp-content/themes/a/style.css".into(), entry(1, Some("a")));
        base.insert("wp-content/themes/a/footer.php".into(), entry(1, Some("f")));
        base.insert("wp-content/plugins/old/old.php".into(), entry(1, Some("o")));
        base.insert("wp-config.php".into(), entry(1, Some("c")));
        let mut source = base.clone();
        source.insert("wp-content/themes/a/style.css".into(), entry(2, Some("a2")));
        source.insert(
            "wp-content/themes/a/footer.php".into(),
            entry(2, Some("f2")),
        );
        source.remove("wp-content/plugins/old/old.php");
        source.insert("wp-content/plugins/new/new.php".into(), entry(3, Some("n")));
        source.insert("wp-config.php".into(), entry(9, Some("c2")));
        source.insert("wp-content/uploads/x.jpg".into(), entry(5, None));
        let mut target = base.clone();
        target.insert(
            "wp-content/themes/a/footer.php".into(),
            entry(4, Some("f3")),
        );
        // Media fetched on demand: the same file, another date.
        target.insert(
            "wp-content/uploads/x.jpg".into(),
            FileEntry {
                modified: 7,
                ..entry(5, None)
            },
        );
        let list = file_changes(&base, &source, &target, "wp-content");
        let by: BTreeMap<&str, &FileChange> = list.iter().map(|c| (c.path.as_str(), c)).collect();
        assert_eq!(list.len(), 4, "{list:#?}");
        assert_eq!(by["wp-content/themes/a/style.css"].change, Change::Changed);
        assert!(!by["wp-content/themes/a/style.css"].conflict);
        assert!(by["wp-content/themes/a/footer.php"].conflict);
        assert_eq!(by["wp-content/plugins/old/old.php"].change, Change::Removed);
        assert_eq!(by["wp-content/plugins/new/new.php"].group, "plugin:new");
        assert!(!by.contains_key("wp-config.php"));
        assert!(!by.contains_key("wp-content/uploads/x.jpg"));
    }

    #[test]
    fn finds_setting_and_switch_changes() {
        let map = |pairs: &[(&str, &str)]| -> BTreeMap<String, String> {
            pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect()
        };
        let base = map(&[
            ("blogname", "1"),
            ("woocommerce_currency", "1"),
            ("siteurl", "1"),
            ("gone", "1"),
        ]);
        let source = map(&[
            ("blogname", "2"),
            ("woocommerce_currency", "2"),
            ("siteurl", "2"),
            ("new", "1"),
        ]);
        let target = map(&[
            ("blogname", "3"),
            ("woocommerce_currency", "2"),
            ("siteurl", "3"),
            ("gone", "1"),
        ]);
        let list = option_changes(&base, &source, &target);
        assert_eq!(
            list,
            vec![
                ("blogname".to_string(), Change::Changed, true),
                ("gone".to_string(), Change::Removed, false),
                ("new".to_string(), Change::Added, false),
            ]
        );

        let facts = |plugins: &[&str], theme: &str| Facts {
            active_plugins: plugins.iter().map(|p| p.to_string()).collect(),
            stylesheet: theme.into(),
            ..Facts::default()
        };
        let base = facts(&["woocommerce/woocommerce.php", "hello.php"], "storefront");
        let source = facts(
            &[
                "woocommerce/woocommerce.php",
                "query-monitor/query-monitor.php",
            ],
            "astra",
        );
        let target = facts(&["woocommerce/woocommerce.php", "hello.php"], "storefront");
        let switches = switch_changes(&base, &source, &target);
        let ids: Vec<(&str, bool)> = switches.iter().map(|s| (s.id.as_str(), s.on)).collect();
        assert_eq!(
            ids,
            vec![
                ("plugin:hello", false),
                ("plugin:query-monitor", true),
                ("theme:astra", true)
            ]
        );
        assert!(switches.iter().all(|s| !s.conflict));
    }

    /// A 1×1 PNG, for a featured image.
    const PIXEL: &[u8] = &[
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f,
        0x15, 0xc4, 0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0xf8,
        0xcf, 0xc0, 0xf0, 0x1f, 0x00, 0x05, 0x00, 0x01, 0xff, 0x89, 0x99, 0x3d, 0x1d, 0x00, 0x00,
        0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    ];

    #[test]
    #[ignore = "needs a WordPress site added in Stackyard, named in STACKYARD_TEST_STAGING"]
    fn merging_a_real_staging() {
        use super::super::cli::Load;
        use super::super::staging::{self, MadeFrom, NewStaging};
        use crate::remote::Sessions;

        let Ok(parent) = std::env::var("STACKYARD_TEST_STAGING") else {
            return;
        };
        let home = crate::paths::home();
        let store = SiteStore::load_from(&home.join("sites.toml")).unwrap();
        let main = store
            .list()
            .iter()
            .find(|s| s.domain() == parent)
            .cloned()
            .unwrap();
        let main_wp = WpSite::open(&main).unwrap();
        let run = |wp: &WpSite, load: Load, args: &[&str]| {
            wp.wp(&home, load, args).unwrap().trim().to_string()
        };
        let description = run(&main_wp, Load::Core, &["option", "get", "blogdescription"]);
        let main_home = run(&main_wp, Load::Core, &["option", "get", "home"]);

        let site = staging::create(
            &home,
            &Sessions::default(),
            &NewStaging {
                parent: parent.clone(),
                name: "mrg".into(),
                from: MadeFrom::Site {
                    domain: parent.clone(),
                },
                copy_media: true,
            },
            &mut |_| true,
        )
        .unwrap();
        let domain = site.domain();
        let wp = WpSite::open(&site).unwrap();

        // On the staging: a post with a featured image and a new category,
        // a changed setting, and a new plugin switched on.
        let image = site.path.join("stackyard-pixel.png");
        std::fs::write(&image, PIXEL).unwrap();
        let image_arg = image.to_string_lossy().into_owned();
        let attachment = run(
            &wp,
            Load::Core,
            &["media", "import", &image_arg, "--porcelain"],
        );
        std::fs::remove_file(&image).unwrap();
        let post = run(
            &wp,
            Load::Core,
            &[
                "post",
                "create",
                "--post_title=Moved by Stackyard",
                "--post_name=moved-by-stackyard",
                "--post_status=publish",
                "--porcelain",
            ],
        );
        run(
            &wp,
            Load::Core,
            &["post", "meta", "set", &post, "_thumbnail_id", &attachment],
        );
        run(
            &wp,
            Load::Core,
            &["term", "create", "category", "Moving", "--slug=moving"],
        );
        run(
            &wp,
            Load::Core,
            &["post", "term", "set", &post, "category", "moving"],
        );
        run(
            &wp,
            Load::Core,
            &["option", "update", "blogdescription", "From the staging"],
        );
        let plugin_dir = wp.wp_content().join("plugins/merge-check");
        std::fs::create_dir_all(&plugin_dir).unwrap();
        std::fs::write(
            plugin_dir.join("merge-check.php"),
            "<?php\n/*\nPlugin Name: Merge check\n*/\n",
        )
        .unwrap();
        run(&wp, Load::Core, &["plugin", "activate", "merge-check"]);

        // On the main site meanwhile: an order.
        let order = run(
            &main_wp,
            Load::Everything,
            &["eval", "echo wc_create_order()->get_id();"],
        );

        // The change list.
        let list = changes(&home, &domain, &parent).unwrap();
        let plugin_file = "wp-content/plugins/merge-check/merge-check.php";
        assert!(
            list.files.iter().any(|f| f.path == plugin_file),
            "{:?}",
            list.files
        );
        assert!(list.settings.iter().any(|c| c.name == "blogdescription"));
        assert!(list
            .switches
            .iter()
            .any(|s| s.id == "plugin:merge-check" && s.on));
        assert!(list.store.as_ref().is_some_and(|s| s.orders >= 1));

        // Moving the one post: its image and category come along.
        let post_id: u64 = post.parse().unwrap();
        let choice = posts::MoveChoice {
            ids: vec![post_id],
            ..Default::default()
        };
        let planned = posts::plan(&home, &domain, &parent, &choice).unwrap();
        assert!(planned
            .posts
            .iter()
            .any(|p| p.post_type == "attachment" && p.action == posts::Action::Add));
        assert!(planned.counts.new_terms >= 1);
        let moved = posts::move_posts(&home, &domain, &parent, &choice).unwrap();
        let check = "$p = get_page_by_path('moved-by-stackyard', OBJECT, 'post'); \
             if (!$p) { echo 'missing'; return; } \
             $t = get_post_thumbnail_id($p); \
             echo get_post_meta($t, '_wp_attached_file', true), '|', \
             implode(',', wp_get_post_categories($p->ID, ['fields' => 'slugs']));";
        let found = run(&main_wp, Load::Core, &["eval", check]);
        let (file, categories) = found.split_once('|').expect(&found);
        assert!(file.ends_with("stackyard-pixel.png"), "{found}");
        assert!(main_wp.wp_content().join("uploads").join(file).is_file());
        assert_eq!(categories, "moving");
        // And undone.
        snapshot::restore(&home, &main_wp, &moved.snapshot).unwrap();
        assert_eq!(run(&main_wp, Load::Core, &["eval", check]), "missing");

        // Settings level: the file, the setting and the switch.
        let merged = merge(
            &home,
            &domain,
            &parent,
            &Choice {
                level: Level::Settings,
                files: vec![plugin_file.into()],
                switches: vec!["plugin:merge-check".into()],
                settings: vec!["blogdescription".into()],
            },
        )
        .unwrap();
        assert_eq!(merged.switches, 1, "{:?}", merged.notes);
        assert_eq!(
            run(&main_wp, Load::Core, &["option", "get", "blogdescription"]),
            "From the staging"
        );
        assert!(run(
            &main_wp,
            Load::Core,
            &["plugin", "is-active", "merge-check"]
        )
        .is_empty());
        snapshot::restore(&home, &main_wp, &merged.snapshot).unwrap();
        assert_eq!(
            run(&main_wp, Load::Core, &["option", "get", "blogdescription"]),
            description
        );
        assert!(!main.path.join(plugin_file).exists());

        // Everything: the staging's content, with the main site's order kept.
        let merged = merge(
            &home,
            &domain,
            &parent,
            &Choice {
                level: Level::Everything,
                files: vec![plugin_file.into()],
                ..Default::default()
            },
        )
        .unwrap();
        assert_ne!(run(&main_wp, Load::Core, &["eval", check]), "missing");
        let kept = format!("echo wc_get_order({order}) ? 'kept' : 'lost';");
        assert_eq!(run(&main_wp, Load::Everything, &["eval", &kept]), "kept");
        assert_eq!(
            run(&main_wp, Load::Core, &["option", "get", "home"]),
            main_home
        );
        snapshot::restore(&home, &main_wp, &merged.snapshot).unwrap();
        assert_eq!(run(&main_wp, Load::Core, &["eval", check]), "missing");

        let t = staging::destroy(&home, &domain).unwrap();
        staging::delete(&home, &t.id).unwrap();
    }
}
