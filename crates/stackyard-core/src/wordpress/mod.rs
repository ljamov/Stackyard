//! WordPress tools for local sites: an overview, plugins and themes, users
//! (with a one-time login link), debug settings and `debug.log`, search and
//! replace that understands serialized data, database export, import and
//! upkeep, and WooCommerce helpers.
//!
//! Plugins, themes, users and WooCommerce go through WP-CLI ([`cli`]),
//! downloaded on first use. Everything else reads files and the database
//! directly, so it works even when a plugin breaks WordPress.

pub mod cli;
pub mod config;
pub mod connect;
pub mod copy;
pub mod env;
pub mod helper;
pub mod live;
pub mod merge;
pub mod muplugin;
pub mod php;
pub mod posts;
pub mod replace;
pub mod snapshot;
pub mod staging;
pub mod sync;
pub mod woo;

use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::databases::{self, Client};
use crate::sites::Site;
use crate::{Error, Result};

pub use cli::Load;
use config::WpConfig;

fn fail(message: impl Into<String>) -> Error {
    Error::WordPress(message.into())
}

/// The folder with `wp-load.php`: the project itself, or `web/wp` in a
/// Bedrock project.
pub fn root_of(project: &Path) -> Option<PathBuf> {
    [project.to_path_buf(), project.join("web").join("wp")]
        .into_iter()
        .find(|dir| dir.join("wp-load.php").is_file())
}

/// Whether a project folder holds WordPress.
pub fn is_wordpress(project: &Path) -> bool {
    root_of(project).is_some()
}

/// Files a plain upload or download between a WordPress folder and its
/// server leaves alone, since each side needs its own: `wp-config.php`
/// with its database login, the live one Stackyard keeps beside it,
/// `.user.ini`, the object cache (its cache server is only on the live
/// side), Stackyard's own files, caches and maintenance mode.
pub struct KeptApart {
    /// WordPress's folder, relative to the project and ending in `/`; empty
    /// when they are the same.
    root: String,
    /// `wp-content`, relative to the project.
    content: String,
}

impl KeptApart {
    /// `None` when `project` holds no WordPress.
    pub fn of(project: &Path) -> Option<Self> {
        let root = root_of(project)?;
        let rel = root
            .strip_prefix(project)
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default();
        let (root, content) = if rel.is_empty() {
            (String::new(), "wp-content".to_string())
        } else if rel == "web/wp" {
            (format!("{rel}/"), "web/app".to_string())
        } else {
            (format!("{rel}/"), format!("{rel}/wp-content"))
        };
        Some(Self { root, content })
    }

    /// Whether `rel`, relative to the project with `/` between folders,
    /// is one of those files.
    pub fn keeps(&self, rel: &str) -> bool {
        let in_root = rel.strip_prefix(self.root.as_str()).unwrap_or(rel);
        merge::kept_apart(rel, &self.content)
            || merge::kept_apart(in_root, &self.content)
            || one_side_only(in_root, &self.content)
    }
}

/// Besides [`merge::kept_apart`], what neither a push nor a pull nor a file
/// transfer takes to the other side: maintenance mode, Stackyard's helper
/// files, and the live object cache, which a copy keeps switched off.
pub(crate) fn one_side_only(rel: &str, content: &str) -> bool {
    rel == ".maintenance"
        || (rel.starts_with("stackyard-") && !rel.contains('/'))
        || rel == format!("{content}/object-cache.php")
}

/// A local WordPress site, with what its `wp-config.php` says.
#[derive(Debug, Clone)]
pub struct WpSite {
    pub site: Site,
    /// The folder with `wp-load.php`.
    pub root: PathBuf,
    pub config_file: PathBuf,
    pub config: WpConfig,
}

impl WpSite {
    pub fn open(site: &Site) -> Result<Self> {
        let root = root_of(&site.path).ok_or_else(|| {
            fail(format!(
                "{} has no WordPress in it (no wp-load.php)",
                site.path.display()
            ))
        })?;
        // WordPress also looks one folder up, and Bedrock keeps it in web/.
        let config_file = [
            root.join("wp-config.php"),
            root.parent()
                .map(|p| p.join("wp-config.php"))
                .unwrap_or_default(),
        ]
        .into_iter()
        .find(|p| p.is_file())
        .ok_or_else(|| {
            fail(format!(
                "{} has no wp-config.php yet; finish WordPress's install page first",
                site.domain()
            ))
        })?;
        let text = read_text(&config_file)?;
        Ok(Self {
            site: site.clone(),
            root,
            config: WpConfig::read(&text),
            config_file,
        })
    }

    pub fn database(&self) -> Result<String> {
        let name = self.site.settings.database.as_ref().map(|d| d.name.clone());
        name.or_else(|| self.config.get("DB_NAME").map(str::to_string))
            .filter(|n| databases::check_name(n).is_ok())
            .ok_or_else(|| {
                fail("wp-config.php has no DB_NAME Stackyard can read; pick the database in the site's Settings")
            })
    }

    pub fn prefix(&self) -> &str {
        self.config.prefix()
    }

    /// A table's full name, e.g. `wp_options`.
    pub fn table(&self, name: &str) -> String {
        format!("{}{name}", self.prefix())
    }

    pub fn wp_content(&self) -> PathBuf {
        match self.config.get("WP_CONTENT_DIR") {
            Some(dir) if Path::new(dir).is_absolute() => PathBuf::from(dir),
            _ if self.root.ends_with("web/wp") => self
                .root
                .parent()
                .map_or_else(|| self.root.join("wp-content"), |web| web.join("app")),
            _ => self.root.join("wp-content"),
        }
    }

    /// The WordPress version, from `wp-includes/version.php`.
    pub fn version(&self) -> Option<String> {
        let text = std::fs::read_to_string(self.root.join("wp-includes/version.php")).ok()?;
        php_variable(&text, "$wp_version")
    }

    /// A plugin's version, from its main file's header.
    pub fn plugin_version(&self, slug: &str) -> Option<String> {
        let file = self
            .wp_content()
            .join("plugins")
            .join(slug)
            .join(format!("{slug}.php"));
        let mut head = String::new();
        std::fs::File::open(file)
            .ok()?
            .take(8192)
            .read_to_string(&mut head)
            .ok()?;
        header_field(&head, "Version")
    }

    /// Runs SQL in the site's database and returns the first result's rows.
    pub fn sql(&self, home: &Path, sql: &str) -> Result<Vec<Vec<Option<String>>>> {
        let database = self.database()?;
        let client = Client::open(home)?;
        let sets = client.query(Some(&database), sql)?;
        Ok(sets.into_iter().next().map(|s| s.rows).unwrap_or_default())
    }

    /// One option's raw value.
    pub fn option(&self, home: &Path, name: &str) -> Result<Option<String>> {
        let rows = self.sql(
            home,
            &format!(
                "SELECT option_value FROM {} WHERE option_name = {}",
                databases::ident(&self.table("options")),
                databases::literal(name)
            ),
        )?;
        Ok(rows
            .into_iter()
            .next()
            .and_then(|r| r.into_iter().next().flatten()))
    }

    /// Writes one option, adding it if missing.
    pub fn set_option(&self, home: &Path, name: &str, value: &[u8], autoload: bool) -> Result<()> {
        let value = String::from_utf8_lossy(value);
        self.sql(
            home,
            &format!(
                "INSERT INTO {} (option_name, option_value, autoload) VALUES ({}, {}, {}) \
                 ON DUPLICATE KEY UPDATE option_value = VALUES(option_value)",
                databases::ident(&self.table("options")),
                databases::literal(name),
                databases::literal(&value),
                databases::literal(if autoload { "yes" } else { "no" })
            ),
        )?;
        Ok(())
    }

    /// Runs WP-CLI for this site.
    pub fn wp(&self, home: &Path, load: Load, args: &[&str]) -> Result<String> {
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        Ok(cli::run(home, &self.root, load, &args)?.stdout)
    }

    fn wp_json<T: for<'de> Deserialize<'de>>(
        &self,
        home: &Path,
        load: Load,
        args: &[&str],
    ) -> Result<T> {
        let out = self.wp(home, load, args)?;
        // Plugins sometimes print notices before the JSON.
        let start = out.find(['[', '{']).unwrap_or(0);
        serde_json::from_str(&out[start..]).map_err(|e| {
            fail(format!(
                "WP-CLI answered in a way Stackyard doesn't understand: {e}"
            ))
        })
    }

    fn write_config(&self, text: &str) -> Result<()> {
        write_text(&self.config_file, text)
    }
}

fn read_text(path: &Path) -> Result<String> {
    std::fs::read_to_string(path).map_err(|source| Error::Io {
        path: path.to_owned(),
        source,
    })
}

fn write_text(path: &Path, text: &str) -> Result<()> {
    std::fs::write(path, text).map_err(|source| Error::Io {
        path: path.to_owned(),
        source,
    })
}

/// `$wp_version = '6.8.3';` → `6.8.3`.
pub(crate) fn php_variable(text: &str, name: &str) -> Option<String> {
    let at = text.find(&format!("{name} "))?;
    let rest = text[at + name.len()..].trim_start().strip_prefix('=')?;
    let end = rest.find(';')?;
    config::literal(&rest[..end])
}

/// ` * Version: 9.3.1` in a plugin's header comment.
fn header_field(head: &str, field: &str) -> Option<String> {
    head.lines()
        .find_map(|line| {
            let line = line.trim_start_matches([' ', '\t', '*', '#', '/']);
            let (key, value) = line.split_once(':')?;
            (key.trim().eq_ignore_ascii_case(field)).then(|| value.trim().to_string())
        })
        .filter(|v| !v.is_empty())
}

// ---------------------------------------------------------------------------
// Overview

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Overview {
    pub wordpress: Option<String>,
    /// The site's PHP version.
    pub php: Option<String>,
    pub database: Option<String>,
    pub prefix: String,
    /// e.g. `MariaDB 11.8`.
    pub database_server: Option<String>,
    pub debug: DebugSettings,
    /// How the live site is set, for a copy of one: read from the live
    /// `wp-config.php` kept beside this one. Push and pull never carry
    /// `wp-config.php`, so `debug` stays this copy's own.
    pub live_debug: Option<DebugSettings>,
    /// WooCommerce's version, when it is installed.
    pub woocommerce: Option<String>,
    pub multisite: bool,
    /// `WP_ENVIRONMENT_TYPE`, e.g. `local`.
    pub environment: Option<String>,
    /// Whether WP-CLI is downloaded yet.
    pub wp_cli: bool,
    /// The theme in use (its folder), when the database could be read.
    pub theme: Option<String>,
    /// Whether that theme is a block theme, edited in the site editor
    /// rather than the Customizer.
    pub block_theme: bool,
    pub warnings: Vec<String>,
}

pub fn overview(home: &Path, wp: &WpSite) -> Overview {
    let mut warnings = Vec::new();
    let database = wp.database().ok();
    if database.is_none() {
        warnings.push("wp-config.php names no database Stackyard can read.".into());
    }
    let host = wp.config.get("DB_HOST").unwrap_or("localhost");
    let server = Client::open(home).ok();
    if let Some(client) = &server {
        if let Some(def) = crate::catalog::service(&client.server().id) {
            let port = crate::services::port(home, def);
            let local = ["localhost", "127.0.0.1", "::1"]
                .iter()
                .any(|h| host == *h || host.starts_with(&format!("{h}:")));
            let port_named = host
                .rsplit_once(':')
                .and_then(|(_, p)| p.parse::<u16>().ok());
            if !local {
                warnings.push(format!("DB_HOST is {host}, not this computer, so the database tools here may show another database than the site uses."));
            } else if port_named.unwrap_or(3306) != port {
                warnings.push(format!(
                    "DB_HOST is {host}, but {} listens on port {port}. Set DB_HOST to 127.0.0.1:{port}.",
                    def.name
                ));
            }
        }
    }
    let debug = DebugSettings::of(&wp.config);
    if debug.display {
        warnings.push(
            "Errors are shown on the pages (WP_DEBUG_DISPLAY). Fine locally; never on a live site."
                .into(),
        );
    }
    let theme = wp
        .option(home, "stylesheet")
        .ok()
        .flatten()
        .filter(|t| !t.is_empty());
    Overview {
        wordpress: wp.version(),
        php: wp.site.runtime.version().map(str::to_string),
        database,
        prefix: wp.prefix().to_string(),
        database_server: server.map(|c| c.server().name),
        debug,
        live_debug: std::fs::read_to_string(live_config_file(wp))
            .ok()
            .map(|text| DebugSettings::of(&WpConfig::read(&text))),
        woocommerce: wp.plugin_version("woocommerce"),
        multisite: wp.config.flag("MULTISITE"),
        environment: wp.config.get("WP_ENVIRONMENT_TYPE").map(str::to_string),
        wp_cli: cli::installed(home),
        block_theme: theme.as_deref().is_some_and(|t| is_block_theme(wp, t)),
        theme,
        warnings,
    }
}

/// Disk use, in bytes.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct DiskUse {
    pub total: u64,
    pub uploads: u64,
    pub plugins: u64,
    pub themes: u64,
    /// Data and indexes; `None` when the database can't be reached.
    pub database: Option<u64>,
}

pub fn disk_use(home: &Path, wp: &WpSite) -> DiskUse {
    let content = wp.wp_content();
    let database = wp.database().ok().and_then(|db| {
        Client::open(home)
            .ok()?
            .databases()
            .ok()?
            .into_iter()
            .find(|d| d.name == db)
            .map(|d| d.size)
    });
    DiskUse {
        total: folder_size(&wp.site.path),
        uploads: folder_size(&content.join("uploads")),
        plugins: folder_size(&content.join("plugins")),
        themes: folder_size(&content.join("themes")),
        database,
    }
}

pub(crate) fn folder_size(dir: &Path) -> u64 {
    let mut total = 0;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                stack.push(entry.path());
            } else if kind.is_file() {
                total += entry.metadata().map_or(0, |m| m.len());
            }
        }
    }
    total
}

// ---------------------------------------------------------------------------
// Debug

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DebugSettings {
    /// `WP_DEBUG`
    pub debug: bool,
    /// `WP_DEBUG_LOG`: errors written to `debug.log`.
    pub log: bool,
    /// `WP_DEBUG_DISPLAY`: errors shown on the page.
    pub display: bool,
    /// `SCRIPT_DEBUG`: full, unminified scripts and styles.
    pub scripts: bool,
    /// `SAVEQUERIES`: every database query kept for the page.
    pub queries: bool,
}

impl DebugSettings {
    pub fn of(config: &WpConfig) -> Self {
        let debug = config.flag("WP_DEBUG");
        let log = config.flag("WP_DEBUG_LOG")
            || config
                .get("WP_DEBUG_LOG")
                .is_some_and(|v| v.contains('/') || v.contains('\\'));
        Self {
            debug,
            log,
            // WordPress shows errors whenever WP_DEBUG is on and this isn't false.
            display: debug && config.get("WP_DEBUG_DISPLAY") != Some("false"),
            scripts: config.flag("SCRIPT_DEBUG"),
            queries: config.flag("SAVEQUERIES"),
        }
    }
}

pub fn set_debug(wp: &WpSite, settings: DebugSettings) -> Result<()> {
    let mut text = read_text(&wp.config_file)?;
    let flag = |on: bool| if on { "true" } else { "false" };
    text = config::set_define(&text, "WP_DEBUG", flag(settings.debug));
    // A custom log path is kept while logging stays on.
    let custom_log = wp
        .config
        .get("WP_DEBUG_LOG")
        .filter(|v| !matches!(*v, "true" | "false" | "1" | "0"));
    if !(settings.log && custom_log.is_some()) {
        text = config::set_define(&text, "WP_DEBUG_LOG", flag(settings.log));
    }
    text = config::set_define(&text, "WP_DEBUG_DISPLAY", flag(settings.display));
    for (name, on) in [
        ("SCRIPT_DEBUG", settings.scripts),
        ("SAVEQUERIES", settings.queries),
    ] {
        text = if on {
            config::set_define(&text, name, "true")
        } else {
            config::remove_define(&text, name)
        };
    }
    wp.write_config(&text)
}

/// The live site's `wp-config.php`, as a copy of a live site keeps it
/// beside its own: saved when cloned, read again on each push, pull and
/// compare.
pub fn live_config_file(wp: &WpSite) -> PathBuf {
    wp.config_file.with_file_name("wp-config.live.php")
}

/// Where errors are logged: `WP_DEBUG_LOG`'s path, or `wp-content/debug.log`.
pub fn log_file(wp: &WpSite) -> PathBuf {
    match wp.config.get("WP_DEBUG_LOG") {
        Some(path) if path.contains('/') || path.contains('\\') => PathBuf::from(path),
        _ => wp.wp_content().join("debug.log"),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LogTail {
    pub path: PathBuf,
    /// The last part of the log.
    pub text: String,
    pub size: u64,
}

/// The end of `debug.log`, up to `max` bytes, starting at a whole line.
pub fn read_log(wp: &WpSite, max: u64) -> Result<LogTail> {
    let path = log_file(wp);
    let Ok(mut file) = std::fs::File::open(&path) else {
        return Ok(LogTail {
            path,
            text: String::new(),
            size: 0,
        });
    };
    let size = file.metadata().map_or(0, |m| m.len());
    let start = size.saturating_sub(max);
    let io = |source| Error::Io {
        path: path.clone(),
        source,
    };
    file.seek(SeekFrom::Start(start)).map_err(io)?;
    let mut bytes = Vec::new();
    BufReader::new(file).read_to_end(&mut bytes).map_err(io)?;
    let mut text = String::from_utf8_lossy(&bytes).into_owned();
    if start > 0 {
        if let Some(nl) = text.find('\n') {
            text.drain(..=nl);
        }
    }
    Ok(LogTail { path, text, size })
}

pub fn clear_log(wp: &WpSite) -> Result<()> {
    let path = log_file(wp);
    match std::fs::write(&path, "") {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(Error::Io { path, source: e }),
        _ => Ok(()),
    }
}

// ---------------------------------------------------------------------------
// Plugins and themes

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Extension {
    /// The folder name, e.g. `woocommerce`.
    #[serde(deserialize_with = "loose_string")]
    pub name: String,
    #[serde(default, deserialize_with = "loose_string")]
    pub title: String,
    /// `active`, `inactive`, `must-use`, `dropin`, `parent` …
    #[serde(deserialize_with = "loose_string")]
    pub status: String,
    #[serde(default, deserialize_with = "loose_string")]
    pub version: String,
    /// `available`, `none` or `version higher than expected`.
    #[serde(default, deserialize_with = "loose_update")]
    pub update: String,
    #[serde(default, deserialize_with = "loose_option")]
    pub update_version: Option<String>,
}

/// WP-CLI fills a field it has no value for with `false` (a drop-in has
/// no version, a plugin with `--skip-update-check` has no update), and a
/// version can come back as a number. Any of these reads as text here.
fn loose_value(value: serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(s) => Some(s),
        serde_json::Value::Number(n) => Some(n.to_string()),
        serde_json::Value::Bool(true) => Some("true".into()),
        _ => None,
    }
}

fn loose_string<'de, D: serde::Deserializer<'de>>(d: D) -> std::result::Result<String, D::Error> {
    Ok(loose_value(serde_json::Value::deserialize(d)?).unwrap_or_default())
}

fn loose_option<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> std::result::Result<Option<String>, D::Error> {
    Ok(loose_value(serde_json::Value::deserialize(d)?).filter(|s| !s.is_empty()))
}

/// `update` as `true`/`false` means available/none.
fn loose_update<'de, D: serde::Deserializer<'de>>(d: D) -> std::result::Result<String, D::Error> {
    Ok(match serde_json::Value::deserialize(d)? {
        serde_json::Value::Bool(true) => "available".into(),
        serde_json::Value::Bool(false) | serde_json::Value::Null => "none".into(),
        other => loose_value(other).unwrap_or_default(),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Plugin,
    Theme,
}

impl Kind {
    fn word(self) -> &'static str {
        match self {
            Kind::Plugin => "plugin",
            Kind::Theme => "theme",
        }
    }
}

/// The site's plugins or themes. `check_updates` asks wordpress.org for
/// new versions, which takes a few seconds; without it the updates shown
/// are the ones WordPress itself last found (it checks twice a day).
pub fn extensions(
    home: &Path,
    wp: &WpSite,
    kind: Kind,
    check_updates: bool,
) -> Result<Vec<Extension>> {
    let mut args = vec![
        kind.word(),
        "list",
        "--format=json",
        "--fields=name,title,status,version,update,update_version",
    ];
    if !check_updates {
        args.push("--skip-update-check");
    }
    let mut list: Vec<Extension> = wp.wp_json(home, Load::Core, &args)?;
    list.sort_by_key(|e| e.title.to_lowercase());
    Ok(list)
}

/// A theme's own picture (`screenshot.png` and the like), which every
/// theme on wordpress.org ships.
pub fn theme_screenshot(wp: &WpSite, theme: &str) -> Option<PathBuf> {
    if theme.is_empty() || theme.contains(['/', '\\']) || theme.starts_with('.') {
        return None;
    }
    let dir = wp.wp_content().join("themes").join(theme);
    ["png", "jpg", "jpeg", "webp"]
        .iter()
        .map(|ext| dir.join(format!("screenshot.{ext}")))
        .find(|p| p.is_file())
}

/// A block theme has `templates/index.html` (WordPress's own test), in
/// itself or in its parent.
pub fn is_block_theme(wp: &WpSite, theme: &str) -> bool {
    if theme.is_empty() || theme.contains(['/', '\\']) || theme.starts_with('.') {
        return false;
    }
    let dir = wp.wp_content().join("themes").join(theme);
    if dir.join("templates").join("index.html").is_file()
        || dir.join("block-templates").join("index.html").is_file()
    {
        return true;
    }
    let style = std::fs::read_to_string(dir.join("style.css")).unwrap_or_default();
    let head: String = style.chars().take(8192).collect();
    header_field(&head, "Template")
        .filter(|parent| parent != theme)
        .is_some_and(|parent| {
            let parent = wp.wp_content().join("themes").join(parent);
            parent.join("templates").join("index.html").is_file()
                || parent.join("block-templates").join("index.html").is_file()
        })
}

/// The picture of the theme the site uses, read from the database.
pub fn active_theme_screenshot(home: &Path, wp: &WpSite) -> Option<PathBuf> {
    let theme = wp.option(home, "stylesheet").ok().flatten()?;
    theme_screenshot(wp, &theme)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExtensionAction {
    /// Switch on (a theme becomes the site's theme).
    Activate,
    Deactivate,
    Update,
    Delete,
}

/// Switches a plugin or theme on or off, updates or deletes it. `name` `*`
/// updates every one that has an update.
pub fn change_extension(
    home: &Path,
    wp: &WpSite,
    kind: Kind,
    name: &str,
    action: ExtensionAction,
) -> Result<()> {
    let safe = name == "*"
        || (!name.is_empty()
            && name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b)));
    if !safe {
        return Err(fail(format!("{name:?} is not a {} name", kind.word())));
    }
    let verb = match action {
        ExtensionAction::Activate => "activate",
        ExtensionAction::Deactivate if kind == Kind::Theme => {
            return Err(fail("a theme is switched off by switching another one on"))
        }
        ExtensionAction::Deactivate => "deactivate",
        ExtensionAction::Update => "update",
        ExtensionAction::Delete => "delete",
    };
    let mut args = vec![kind.word(), verb];
    if name == "*" {
        if action != ExtensionAction::Update {
            return Err(fail("only updates can be done for all at once"));
        }
        args.push("--all");
    } else {
        args.push(name);
    }
    // Plugins load for (de)activation so their own hooks run, as in wp-admin.
    let load = match (kind, action) {
        (Kind::Plugin, ExtensionAction::Activate | ExtensionAction::Deactivate) => Load::Everything,
        _ => Load::Core,
    };
    wp.wp(home, load, &args).map(|_| ())
}

/// Installs a plugin or theme from wordpress.org by its slug.
pub fn install_extension(
    home: &Path,
    wp: &WpSite,
    kind: Kind,
    slug: &str,
    activate: bool,
) -> Result<()> {
    if slug.is_empty()
        || !slug
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    {
        return Err(fail(
            "use the slug from the wordpress.org address, e.g. query-monitor",
        ));
    }
    let mut args = vec![kind.word(), "install", slug];
    if activate {
        args.push("--activate");
    }
    wp.wp(home, Load::Core, &args).map(|_| ())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CoreUpdate {
    pub current: Option<String>,
    /// The newest version offered, when it is newer.
    pub available: Option<String>,
}

pub fn core_update(home: &Path, wp: &WpSite) -> Result<CoreUpdate> {
    #[derive(Deserialize)]
    struct Offer {
        version: String,
    }
    let offers: Vec<Offer> = wp
        .wp_json(home, Load::Core, &["core", "check-update", "--format=json"])
        .or_else(|e| match e {
            // An up-to-date site prints a success line, not JSON.
            Error::WordPress(m) if m.contains("understand") => Ok(Vec::new()),
            e => Err(e),
        })?;
    Ok(CoreUpdate {
        current: wp.version(),
        available: offers.into_iter().next().map(|o| o.version),
    })
}

pub fn update_core(home: &Path, wp: &WpSite) -> Result<()> {
    wp.wp(home, Load::Core, &["core", "update"])?;
    wp.wp(home, Load::Core, &["core", "update-db"]).map(|_| ())
}

// ---------------------------------------------------------------------------
// Users

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct User {
    #[serde(rename = "ID")]
    pub id: u64,
    pub user_login: String,
    #[serde(default)]
    pub user_email: String,
    #[serde(default)]
    pub display_name: String,
    /// Comma-separated, e.g. `administrator`.
    #[serde(default)]
    pub roles: String,
}

pub fn users(home: &Path, wp: &WpSite) -> Result<Vec<User>> {
    wp.wp_json(
        home,
        Load::Core,
        &[
            "user",
            "list",
            "--format=json",
            "--fields=ID,user_login,user_email,display_name,roles",
        ],
    )
}

/// Adds an administrator; returns the new user's ID.
pub fn add_admin(
    home: &Path,
    wp: &WpSite,
    login: &str,
    email: &str,
    password: &str,
) -> Result<u64> {
    if login.trim().is_empty() || email.trim().is_empty() || password.len() < 8 {
        return Err(fail(
            "enter a user name, an e-mail address and a password of at least 8 characters",
        ));
    }
    let pass = format!("--user_pass={password}");
    let out = wp.wp(
        home,
        Load::Core,
        &[
            "user",
            "create",
            login.trim(),
            email.trim(),
            "--role=administrator",
            &pass,
            "--porcelain",
        ],
    )?;
    out.trim()
        .lines()
        .last()
        .and_then(|l| l.trim().parse().ok())
        .ok_or_else(|| fail(format!("WP-CLI didn't say the new user's ID: {out}")))
}

pub fn set_password(home: &Path, wp: &WpSite, user: u64, password: &str) -> Result<()> {
    if password.len() < 8 {
        return Err(fail("use a password of at least 8 characters"));
    }
    let id = user.to_string();
    let pass = format!("--user_pass={password}");
    wp.wp(
        home,
        Load::Core,
        &["user", "update", &id, &pass, "--skip-email"],
    )
    .map(|_| ())
}

/// A link that logs in as `user` once, within five minutes, through
/// Stackyard's must-use plugin.
pub fn login_link(home: &Path, wp: &WpSite, user: u64) -> Result<String> {
    muplugin::install(wp, &muplugin::Options::for_site(home, wp))?;
    let mut bytes = [0u8; 24];
    getrandom::fill(&mut bytes).map_err(|e| fail(format!("no randomness: {e}")))?;
    let token: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    let digest = {
        use sha2::Digest;
        let hash = sha2::Sha256::digest(token.as_bytes());
        hash.iter().map(|b| format!("{b:02x}")).collect::<String>()
    };
    let expires = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
        + 300;
    let value = php::Value::Array(vec![
        (php::Value::str("hash"), php::Value::str(&digest)),
        (php::Value::str("user"), php::Value::Int(user as i64)),
        (php::Value::str("expires"), php::Value::Int(expires as i64)),
    ]);
    wp.set_option(home, muplugin::LOGIN_OPTION, &value.serialize(), false)?;
    Ok(format!("{}/?stackyard-login={token}", home_url(home, wp)))
}

/// The site's address as WordPress has it (`home`), else the local one.
pub fn home_url(home: &Path, wp: &WpSite) -> String {
    wp.config
        .get("WP_HOME")
        .map(str::to_string)
        .or_else(|| wp.option(home, "home").ok().flatten())
        .unwrap_or_else(|| wp.site.url())
        .trim_end_matches('/')
        .to_string()
}

// ---------------------------------------------------------------------------
// Search and replace

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Replaced {
    pub report: replace::Report,
    /// The backup taken before, in the Databases screen's backups; `None`
    /// for a preview.
    pub backup: Option<String>,
}

/// Replaces `from` with `to` everywhere in the site's database, keeping
/// serialized data intact. `addresses` treats them as site addresses
/// (`http://`, `www.` and the other forms change too). With `dry_run`
/// nothing changes and the counts say what would.
pub fn search_replace(
    home: &Path,
    wp: &WpSite,
    from: &str,
    to: &str,
    addresses: bool,
    dry_run: bool,
) -> Result<Replaced> {
    if from.is_empty() {
        return Err(fail("enter the text to find"));
    }
    let rules = if addresses {
        replace::Rules::site_move(from, to)
    } else {
        replace::Rules::text(from, to)
    };
    let database = wp.database()?;
    let client = Client::open(home)?;
    let work = temp_dir(home, "replace")?;
    let dump = work.join("before.sql");
    let result = (|| {
        client.dump_to(&database, &[], &dump)?;
        let input = std::fs::File::open(&dump).map_err(|source| Error::Io {
            path: dump.clone(),
            source,
        })?;
        if dry_run {
            let report = replace::preview(input, &rules).map_err(|source| Error::Io {
                path: dump.clone(),
                source,
            })?;
            return Ok(Replaced {
                report,
                backup: None,
            });
        }
        let after = work.join("after.sql");
        let output = std::fs::File::create(&after).map_err(|source| Error::Io {
            path: after.clone(),
            source,
        })?;
        let report = replace::rewrite(input, output, &rules).map_err(|source| Error::Io {
            path: after.clone(),
            source,
        })?;
        if report.replaced == 0 {
            return Ok(Replaced {
                report,
                backup: None,
            });
        }
        let backup = client.backup(&database)?;
        client.load(&database, &after)?;
        Ok(Replaced {
            report,
            backup: Some(backup.file),
        })
    })();
    let _ = std::fs::remove_dir_all(&work);
    result
}

/// A new empty folder under `tmp/` in Stackyard's data folder.
pub(crate) fn temp_dir(home: &Path, what: &str) -> Result<PathBuf> {
    let mut bytes = [0u8; 4];
    let _ = getrandom::fill(&mut bytes);
    let dir = home.join("tmp").join(format!(
        "wp-{what}-{}",
        bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()
    ));
    std::fs::create_dir_all(&dir).map_err(|source| Error::Io {
        path: dir.clone(),
        source,
    })?;
    Ok(dir)
}

// ---------------------------------------------------------------------------
// Database

/// Saves the site's database to `dest` (`.sql`, or `.sql.gz` compressed).
pub fn export_database(home: &Path, wp: &WpSite, dest: &Path) -> Result<()> {
    Client::open(home)?.dump_to(&wp.database()?, &[], dest)
}

/// Loads a `.sql` or `.sql.gz` file into the site's database, after a backup.
/// Returns the backup's file name.
pub fn import_database(home: &Path, wp: &WpSite, file: &Path) -> Result<String> {
    let database = wp.database()?;
    let client = Client::open(home)?;
    let exists = client.databases()?.iter().any(|d| d.name == database);
    let backup = if exists {
        client.backup(&database)?.file
    } else {
        String::new()
    };
    client.load(&database, file)?;
    Ok(backup)
}

/// Checks a table prefix the way WordPress does: letters, digits and
/// underscores only.
pub fn check_prefix(prefix: &str) -> Result<()> {
    if prefix.is_empty() {
        return Err(fail("The table prefix can't be empty"));
    }
    if prefix.len() > 20 {
        return Err(fail("Keep the table prefix to 20 characters or fewer"));
    }
    if !prefix
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'_')
    {
        return Err(fail(
            "The table prefix can only have letters, digits and underscores",
        ));
    }
    Ok(())
}

/// The blog part of a prefixed table, `` for `wp_options` and `2_` for
/// `wp_2_options` on a multisite.
fn blog_part(rest: &str) -> Option<&str> {
    let mid = rest.strip_suffix("options")?;
    let digits = mid.strip_suffix('_').unwrap_or(mid);
    (mid.is_empty() || (!digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())))
        .then_some(mid)
}

/// Gives the site's tables a new prefix, e.g. `wp_` to `shop_`: renames
/// every table, the `user_roles` option of each blog and the user
/// settings WordPress keys by prefix, then `$table_prefix` in
/// wp-config.php. A backup is taken first; returns its file name.
pub fn change_prefix(home: &Path, wp: &WpSite, prefix: &str) -> Result<String> {
    check_prefix(prefix)?;
    let old = wp.prefix().to_string();
    if prefix == old {
        return Err(fail(format!("The tables already start with {old}")));
    }
    let database = wp.database()?;
    let client = Client::open(home)?;
    let all: Vec<String> = client
        .tables(&database)?
        .into_iter()
        .map(|t| t.name)
        .collect();
    let ours: Vec<&String> = all.iter().filter(|t| t.starts_with(&old)).collect();
    if !ours.iter().any(|t| t.as_str() == wp.table("options")) {
        return Err(fail(format!(
            "No table {} in {database}; is this the right database?",
            wp.table("options")
        )));
    }
    // Another WordPress in the same database whose prefix starts with
    // ours (wp_ and wp_shop_) would have its tables renamed too.
    if let Some(other) = ours
        .iter()
        .find(|t| t.ends_with("options") && blog_part(&t[old.len()..]).is_none())
    {
        return Err(fail(format!(
            "{other} looks like another WordPress in {database}; change the prefix in phpMyAdmin instead"
        )));
    }
    let renames: Vec<(String, String)> = ours
        .iter()
        .map(|t| ((*t).clone(), format!("{prefix}{}", &t[old.len()..])))
        .collect();
    if let Some((_, taken)) = renames.iter().find(|(_, to)| all.contains(to)) {
        return Err(fail(format!(
            "{database} already has a table {taken}; pick another prefix"
        )));
    }

    let backup = client.backup(&database)?.file;
    let pairs: Vec<String> = renames
        .iter()
        .map(|(from, to)| format!("{} TO {}", databases::ident(from), databases::ident(to)))
        .collect();
    let mut sql = format!("RENAME TABLE {};", pairs.join(", "));
    for (_, to) in &renames {
        let Some(mid) = blog_part(&to[prefix.len()..]) else {
            continue;
        };
        sql.push_str(&format!(
            " UPDATE {} SET option_name = {} WHERE option_name = {};",
            databases::ident(to),
            databases::literal(&format!("{prefix}{mid}user_roles")),
            databases::literal(&format!("{old}{mid}user_roles")),
        ));
    }
    // update_user_option() keys user settings by blog prefix:
    // wp_capabilities, wp_user_level, wp_2_capabilities …
    sql.push_str(&format!(
        " UPDATE {usermeta} SET meta_key = CONCAT({new}, SUBSTRING(meta_key, {from})) \
         WHERE LEFT(meta_key, {len}) = BINARY {old};",
        usermeta = databases::ident(&format!("{prefix}usermeta")),
        new = databases::literal(prefix),
        from = old.len() + 1,
        len = old.len(),
        old = databases::literal(&old),
    ));
    client.query(Some(&database), &sql).map_err(|e| {
        fail(format!(
            "Changing the prefix stopped part way ({e}). The database as it was is backed up as {backup} (Databases screen)"
        ))
    })?;
    let text = read_text(&wp.config_file)?;
    wp.write_config(&config::set_table_prefix(&text, prefix))
        .map_err(|e| {
            fail(format!(
                "The tables now start with {prefix}, but wp-config.php couldn't be saved ({e}); set $table_prefix = '{prefix}'; in it by hand"
            ))
        })?;
    Ok(backup)
}

/// Optimizes every table of the site; returns how many there were.
pub fn optimize_database(home: &Path, wp: &WpSite) -> Result<usize> {
    let database = wp.database()?;
    let client = Client::open(home)?;
    let tables: Vec<String> = client
        .tables(&database)?
        .into_iter()
        .filter(|t| t.kind == "BASE TABLE")
        .map(|t| databases::ident(&t.name))
        .collect();
    if tables.is_empty() {
        return Ok(0);
    }
    client.query(
        Some(&database),
        &format!("OPTIMIZE TABLE {}", tables.join(", ")),
    )?;
    Ok(tables.len())
}

/// Removes expired and leftover transients (WordPress's cached values);
/// returns how many rows went.
pub fn clear_transients(home: &Path, wp: &WpSite) -> Result<u64> {
    let options = databases::ident(&wp.table("options"));
    let rows = wp.sql(
        home,
        &format!(
            "DELETE FROM {options} WHERE option_name LIKE '\\_transient\\_%' \
             OR option_name LIKE '\\_site\\_transient\\_%'; SELECT ROW_COUNT()"
        ),
    )?;
    Ok(rows
        .first()
        .and_then(|r| r.first().cloned().flatten())
        .and_then(|n| n.parse().ok())
        .unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extension_list_takes_false_and_numbers() {
        let json = r#"[
            {"name":"akismet","title":"Akismet","status":"active","version":"5.3","update":"available","update_version":"5.4"},
            {"name":"advanced-cache.php","title":false,"status":"dropin","version":false,"update":false,"update_version":false},
            {"name":"hello","title":"Hello","status":"inactive","version":1.7,"update":"none","update_version":null}
        ]"#;
        let list: Vec<Extension> = serde_json::from_str(json).unwrap();
        assert_eq!(list[0].update_version.as_deref(), Some("5.4"));
        assert_eq!(list[1].title, "");
        assert_eq!(list[1].version, "");
        assert_eq!(list[1].update, "none");
        assert_eq!(list[1].update_version, None);
        assert_eq!(list[2].version, "1.7");
    }

    #[test]
    fn file_transfers_keep_each_sides_own_files() {
        let dir = crate::testutil::tempdir();
        assert!(KeptApart::of(&dir).is_none());
        fake_wordpress(&dir);
        let kept = KeptApart::of(&dir).unwrap();
        for rel in [
            "wp-config.php",
            "wp-config.live.php",
            ".user.ini",
            ".maintenance",
            "stackyard-helper-1a2b.php",
            "wp-content/object-cache.php",
            "wp-content/object-cache.php.stackyard-off",
            "wp-content/mu-plugins/stackyard.php",
            "wp-content/cache/page/index.html",
        ] {
            assert!(kept.keeps(rel), "{rel}");
        }
        for rel in [
            "index.php",
            ".htaccess",
            "wp-content/advanced-cache.php",
            "wp-content/plugins/akismet/akismet.php",
            "wp-content/plugins/stackyard-x/stackyard-x.php",
            "wp-content/themes/twentyfive/wp-config.php",
        ] {
            assert!(!kept.keeps(rel), "{rel}");
        }

        let bedrock = crate::testutil::tempdir();
        fake_wordpress(&bedrock.join("web/wp"));
        let kept = KeptApart::of(&bedrock).unwrap();
        assert!(kept.keeps("web/wp/wp-config.php"));
        assert!(kept.keeps("web/app/object-cache.php"));
        assert!(!kept.keeps("web/app/plugins/a/a.php"));
    }

    pub(crate) fn fake_wordpress(dir: &Path) {
        std::fs::create_dir_all(dir.join("wp-includes")).unwrap();
        std::fs::create_dir_all(dir.join("wp-content/plugins/woocommerce")).unwrap();
        std::fs::write(dir.join("wp-load.php"), "<?php").unwrap();
        std::fs::write(
            dir.join("wp-includes/version.php"),
            "<?php\n$wp_version = '6.8.3';\n$wp_db_version = 58975;\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("wp-content/plugins/woocommerce/woocommerce.php"),
            "<?php\n/**\n * Plugin Name: WooCommerce\n * Version: 10.2.1\n */\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("wp-config.php"),
            "<?php\ndefine( 'DB_NAME', 'shop' );\ndefine( 'DB_HOST', '127.0.0.1:3306' );\n$table_prefix = 'wp_';\ndefine( 'WP_DEBUG', true );\ndefine( 'WP_DEBUG_LOG', true );\ndefine( 'WP_DEBUG_DISPLAY', false );\n/* That's all, stop editing! */\nrequire_once ABSPATH . 'wp-settings.php';\n",
        )
        .unwrap();
    }

    pub(crate) fn site_at(dir: &Path) -> Site {
        Site {
            name: "shop".into(),
            tld: ".stack".into(),
            path: dir.to_owned(),
            runtime: crate::sites::SiteRuntime::Php {
                version: "8.3".into(),
            },
            https: true,
            dev_server: false,
            settings: Default::default(),
        }
    }

    #[test]
    fn reads_a_site_without_running_it() {
        let dir = crate::testutil::tempdir();
        fake_wordpress(&dir);
        let wp = WpSite::open(&site_at(&dir)).unwrap();
        assert_eq!(wp.version().as_deref(), Some("6.8.3"));
        assert_eq!(wp.plugin_version("woocommerce").as_deref(), Some("10.2.1"));
        assert_eq!(wp.database().unwrap(), "shop");
        assert_eq!(wp.table("options"), "wp_options");
        let debug = DebugSettings::of(&wp.config);
        assert!(debug.debug && debug.log && !debug.display);
        assert!(is_wordpress(&dir));
        assert!(!is_wordpress(&dir.join("wp-content")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn debug_settings_go_into_wp_config() {
        let dir = crate::testutil::tempdir();
        fake_wordpress(&dir);
        let wp = WpSite::open(&site_at(&dir)).unwrap();
        set_debug(
            &wp,
            DebugSettings {
                debug: true,
                log: true,
                display: true,
                scripts: true,
                queries: false,
            },
        )
        .unwrap();
        let wp = WpSite::open(&site_at(&dir)).unwrap();
        let debug = DebugSettings::of(&wp.config);
        assert!(debug.display && debug.scripts && !debug.queries);
        std::fs::write(
            dir.join("wp-content/debug.log"),
            "first\nsecond line\nthird line\n",
        )
        .unwrap();
        let tail = read_log(&wp, 16).unwrap();
        assert_eq!(tail.text, "third line\n");
        clear_log(&wp).unwrap();
        assert_eq!(read_log(&wp, 100).unwrap().size, 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn prefix_rules() {
        assert!(check_prefix("shop_").is_ok());
        assert!(check_prefix("").is_err());
        assert!(check_prefix("wp-").is_err());
        assert!(check_prefix("x'; DROP").is_err());
        assert_eq!(blog_part("options"), Some(""));
        assert_eq!(blog_part("12_options"), Some("12_"));
        assert_eq!(blog_part("shop_options"), None);
        assert_eq!(blog_part("posts"), None);
    }

    #[test]
    fn plugin_headers() {
        assert_eq!(
            header_field("/*\n * Plugin Name: X\n * Version:   2.1.0\n */", "Version").as_deref(),
            Some("2.1.0")
        );
        assert_eq!(
            php_variable("$wp_version = '6.9-RC1';", "$wp_version").as_deref(),
            Some("6.9-RC1")
        );
    }
}

/// Runs against a real WordPress site, given as `STACKYARD_TEST_WP=<folder>`,
/// with MySQL or MariaDB running in `STACKYARD_HOME`:
/// `cargo test -p stackyard-core -- --ignored wordpress_tools_on_a_real_site`.
#[cfg(test)]
mod site_tests {
    use super::*;

    #[test]
    #[ignore = "needs a WordPress site in STACKYARD_TEST_WP"]
    fn wordpress_tools_on_a_real_site() {
        let Ok(folder) = std::env::var("STACKYARD_TEST_WP") else {
            return;
        };
        let home = crate::paths::home();
        let site = tests::site_at(Path::new(&folder));
        let wp = WpSite::open(&site).unwrap();
        // A site fresh from the starter: finish WordPress's install, and add
        // WooCommerce with a product, as someone would in wp-admin.
        if wp.wp(&home, Load::Core, &["core", "is-installed"]).is_err() {
            wp.wp(
                &home,
                Load::Core,
                &[
                    "core",
                    "install",
                    "--url=https://shop.stack",
                    "--title=Shop",
                    "--admin_user=admin",
                    "--admin_password=password-123",
                    "--admin_email=admin@example.test",
                    "--skip-email",
                ],
            )
            .unwrap();
            wp.wp(
                &home,
                Load::Core,
                &["plugin", "install", "woocommerce", "--activate"],
            )
            .unwrap();
            wp.wp(
                &home,
                Load::Everything,
                &[
                    "wc",
                    "product",
                    "create",
                    "--name=Shirt",
                    "--regular_price=10",
                    "--user=1",
                ],
            )
            .unwrap();
        }

        let info = overview(&home, &wp);
        assert!(info.wordpress.is_some(), "{info:?}");
        assert!(info.warnings.is_empty(), "{info:?}");
        let disk = disk_use(&home, &wp);
        assert!(
            disk.total > 1_000_000 && disk.database.unwrap() > 0,
            "{disk:?}"
        );

        let plugins = extensions(&home, &wp, Kind::Plugin, true).unwrap();
        assert!(plugins.iter().any(|p| p.name == "hello"), "{plugins:?}");
        change_extension(&home, &wp, Kind::Plugin, "hello", ExtensionAction::Activate).unwrap();
        let plugins = extensions(&home, &wp, Kind::Plugin, true).unwrap();
        assert_eq!(
            plugins.iter().find(|p| p.name == "hello").unwrap().status,
            "active"
        );
        change_extension(
            &home,
            &wp,
            Kind::Plugin,
            "hello",
            ExtensionAction::Deactivate,
        )
        .unwrap();
        let themes = extensions(&home, &wp, Kind::Theme, true).unwrap();
        assert!(themes.iter().any(|t| t.status == "active"), "{themes:?}");

        let before = users(&home, &wp).unwrap();
        let id = add_admin(
            &home,
            &wp,
            &format!("t{}", before.len()),
            &format!("t{}@example.test", before.len()),
            "password-123",
        )
        .unwrap();
        set_password(&home, &wp, id, "another-pass-1").unwrap();
        assert_eq!(users(&home, &wp).unwrap().len(), before.len() + 1);
        let link = login_link(&home, &wp, id).unwrap();
        assert!(link.contains("?stackyard-login="), "{link}");
        assert!(muplugin::path(&wp).is_file());

        let site_url = home_url(&home, &wp);
        let preview =
            search_replace(&home, &wp, &site_url, "https://renamed.stack", true, true).unwrap();
        assert!(preview.report.replaced >= 2, "{preview:?}");
        assert!(preview.backup.is_none());
        let done =
            search_replace(&home, &wp, &site_url, "https://renamed.stack", true, false).unwrap();
        assert_eq!(done.report.replaced, preview.report.replaced);
        assert_eq!(
            wp.option(&home, "siteurl").unwrap().as_deref(),
            Some("https://renamed.stack")
        );
        search_replace(&home, &wp, "https://renamed.stack", &site_url, true, false).unwrap();
        assert_eq!(home_url(&home, &wp), site_url);

        let dump = crate::testutil::tempdir().join("export.sql.gz");
        export_database(&home, &wp, &dump).unwrap();
        assert!(std::fs::metadata(&dump).unwrap().len() > 1000);
        import_database(&home, &wp, &dump).unwrap();
        assert!(optimize_database(&home, &wp).unwrap() > 5);
        clear_transients(&home, &wp).unwrap();

        let first = wp.prefix().to_string();
        assert!(change_prefix(&home, &wp, &first).is_err());
        assert!(change_prefix(&home, &wp, "bad-prefix").is_err());
        change_prefix(&home, &wp, "sy_").unwrap();
        let moved = WpSite::open(&site).unwrap();
        assert_eq!(moved.prefix(), "sy_");
        assert!(moved.option(&home, "sy_user_roles").unwrap().is_some());
        assert!(users(&home, &moved)
            .unwrap()
            .iter()
            .any(|u| u.roles.contains("administrator")));
        change_prefix(&home, &moved, &first).unwrap();
        let wp = WpSite::open(&site).unwrap();
        assert_eq!(wp.prefix(), first);
        assert!(wp
            .option(&home, &format!("{first}user_roles"))
            .unwrap()
            .is_some());

        if wp.plugin_version("woocommerce").is_some() {
            let status = woo::status(&home, &wp).unwrap();
            assert!(status.products > 0, "{status:?}");
            let orders = woo::create_orders(&home, &wp, 2).unwrap();
            assert_eq!(orders.len(), 2);
            let customers = woo::create_customers(&home, &wp, 1).unwrap();
            assert_eq!(customers.len(), 1);
            let after = woo::status(&home, &wp).unwrap();
            assert_eq!(after.orders, status.orders + 2, "{after:?}");
            woo::run_due_actions(&home, &wp).unwrap();
            let gateways = woo::gateways(&home, &wp).unwrap();
            if let Some(g) = gateways.iter().find(|g| g.test_mode.is_some()) {
                woo::set_test_mode(&home, &wp, &g.id, true).unwrap();
                let again = woo::gateways(&home, &wp).unwrap();
                assert_eq!(
                    again.iter().find(|x| x.id == g.id).unwrap().test_mode,
                    Some(true)
                );
            }
        }
    }
}
