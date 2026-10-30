//! Starters: a new site, app or project that begins from a template
//! instead of an empty folder.
//!
//! - WordPress comes from wordpress.org, checked against the SHA-1 published
//!   next to it (the only checksum WordPress publishes). Stackyard makes its
//!   database and writes `wp-config.php`; the browser shows WordPress's own
//!   install page next.
//! - WordPress with WooCommerce adds the WooCommerce plugin from
//!   wordpress.org, each file checked against the SHA-256 list wordpress.org
//!   publishes for that version, and a must-use plugin that switches it on
//!   once WordPress's install page is done.
//! - Laravel is made by Composer (`composer create-project`), with SQLite
//!   as Laravel's default database.
//! - Astro, React, Vue, Svelte and Next.js are made by their own `create-*`
//!   tools, and Stackyard runs their dev server while the web server runs.
//! - A static site, a Node.js server and Python scripts are a few files
//!   Stackyard writes itself.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::Serialize;

use crate::catalog::RuntimeKind;
use crate::download::Http;
use crate::install::Step;
use crate::projects::Category;
use crate::runtimes::RuntimeManager;
use crate::settings::Settings;
use crate::shell::{self, Context, Tool};
use crate::sites::SiteRuntime;
use crate::{archive, ports, web, Error, Result};

const WORDPRESS_VERSIONS: &str = "https://api.wordpress.org/core/version-check/1.7/";
const WORDPRESS_FILES: &str = "https://wordpress.org";
const WOOCOMMERCE_INFO: &str = "https://api.wordpress.org/plugins/info/1.2/?action=plugin_information&request[slug]=woocommerce&request[fields][sections]=0";
const PLUGIN_CHECKSUMS: &str = "https://downloads.wordpress.org/plugin-checksums";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Starter {
    WordPress,
    WooCommerce,
    Laravel,
    Astro,
    React,
    Vue,
    Svelte,
    NextJs,
    Static,
    NodeServer,
    PythonScripts,
}

#[derive(Debug, Clone, Serialize)]
pub struct StarterInfo {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    /// The list it goes in: `site`, `app` or `project`.
    pub category: &'static str,
}

impl Starter {
    pub const ALL: [Starter; 11] = [
        Starter::WordPress,
        Starter::WooCommerce,
        Starter::Laravel,
        Starter::Astro,
        Starter::React,
        Starter::Vue,
        Starter::Svelte,
        Starter::NextJs,
        Starter::Static,
        Starter::NodeServer,
        Starter::PythonScripts,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Starter::WordPress => "wordpress",
            Starter::WooCommerce => "woocommerce",
            Starter::Laravel => "laravel",
            Starter::Astro => "astro",
            Starter::React => "react",
            Starter::Vue => "vue",
            Starter::Svelte => "svelte",
            Starter::NextJs => "nextjs",
            Starter::Static => "static",
            Starter::NodeServer => "node-server",
            Starter::PythonScripts => "python-scripts",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Starter::WordPress => "WordPress",
            Starter::WooCommerce => "WordPress + WooCommerce",
            Starter::Laravel => "Laravel",
            Starter::Astro => "Astro",
            Starter::React => "React",
            Starter::Vue => "Vue",
            Starter::Svelte => "Svelte",
            Starter::NextJs => "Next.js",
            Starter::Static => "Static HTML",
            Starter::NodeServer => "Node.js server",
            Starter::PythonScripts => "Python scripts",
        }
    }

    pub fn from_id(id: &str) -> Option<Starter> {
        Self::ALL.into_iter().find(|s| s.id() == id)
    }

    /// The runtime the project runs on; `None` for plain files.
    pub fn runtime(self) -> Option<RuntimeKind> {
        match self {
            Starter::WordPress | Starter::WooCommerce | Starter::Laravel => Some(RuntimeKind::Php),
            Starter::Astro
            | Starter::React
            | Starter::Vue
            | Starter::Svelte
            | Starter::NextJs
            | Starter::NodeServer => Some(RuntimeKind::Node),
            Starter::Static | Starter::PythonScripts => None,
        }
    }

    /// The list a new one goes in.
    pub fn category(self) -> Category {
        match self {
            Starter::NodeServer => Category::App,
            Starter::PythonScripts => Category::Project,
            _ => Category::Site,
        }
    }

    pub fn info(self) -> StarterInfo {
        StarterInfo {
            id: self.id(),
            name: self.name(),
            category: self.category().id(),
            description: match self {
                Starter::WordPress => {
                    "The latest WordPress, with its database made in MySQL or MariaDB"
                }
                Starter::WooCommerce => {
                    "The latest WordPress with the WooCommerce shop plugin, switched on after WordPress's install page"
                }
                Starter::Laravel => "A new Laravel app, made with Composer",
                Starter::Astro => "A new Astro site; Stackyard runs its dev server",
                Starter::React => "A React app with Vite and TypeScript; Stackyard runs its dev server",
                Starter::Vue => "A Vue app with Vite and TypeScript; Stackyard runs its dev server",
                Starter::Svelte => "A Svelte app with Vite and TypeScript; Stackyard runs its dev server",
                Starter::NextJs => "A Next.js app with TypeScript; Stackyard runs its dev server",
                Starter::Static => "An index.html, a stylesheet and a script, served as they are",
                Starter::NodeServer => {
                    "A small Node.js server with no packages; Stackyard runs it and restarts it on changes"
                }
                Starter::PythonScripts => "A main.py to start from; run it in the terminal",
            },
        }
    }

    /// Whether the project needs MySQL or MariaDB running while it is made.
    pub fn needs_database(self) -> bool {
        matches!(self, Starter::WordPress | Starter::WooCommerce)
    }

    /// The port its dev server usually takes, for the Node starters.
    fn dev_port(self) -> u16 {
        match self {
            Starter::Astro => 4321,
            Starter::NextJs | Starter::NodeServer => 3000,
            _ => 5173,
        }
    }
}

pub fn list() -> Vec<StarterInfo> {
    Starter::ALL.into_iter().map(Starter::info).collect()
}

/// What the new site runs with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Made {
    pub runtime: SiteRuntime,
    pub dev_server: bool,
    /// The database made for it, e.g. `blog` for WordPress.
    pub database: Option<String>,
}

/// Makes the project in `folder` (missing or empty) for the site `name`.
/// The runtime is installed first if it is not, and MySQL or MariaDB must
/// already be running for WordPress.
pub fn create(
    home: &Path,
    starter: Starter,
    name: &str,
    folder: &Path,
    progress: &mut dyn FnMut(String),
) -> Result<Made> {
    let fail = |message: String| Error::Starter {
        name: starter.name().into(),
        message,
    };
    if folder
        .read_dir()
        .map(|mut entries| entries.next().is_some())
        .unwrap_or(false)
    {
        return Err(fail(format!(
            "{} already has files in it; pick a new or empty folder",
            folder.display()
        )));
    }
    let version = match starter.runtime() {
        Some(kind) => ensure_runtime(home, kind, progress)?,
        None => String::new(),
    };
    let node = |version: String| Made {
        runtime: SiteRuntime::Node {
            version,
            port: free_dev_port(home, starter.dev_port()),
        },
        dev_server: true,
        database: None,
    };
    let made = match starter {
        Starter::WordPress | Starter::WooCommerce => {
            let database = database_name(name);
            wordpress(home, folder, &database, progress)?;
            if starter == Starter::WooCommerce {
                woocommerce(home, folder, progress)?;
            }
            Made {
                runtime: SiteRuntime::Php { version },
                dev_server: false,
                database: Some(database),
            }
        }
        Starter::Laravel => {
            laravel(home, folder, progress)?;
            Made {
                runtime: SiteRuntime::Php { version },
                dev_server: false,
                database: None,
            }
        }
        Starter::Astro => {
            astro(home, folder, progress)?;
            node(version)
        }
        Starter::React | Starter::Vue | Starter::Svelte => {
            vite(home, starter, folder, progress)?;
            node(version)
        }
        Starter::NextJs => {
            next(home, folder, progress)?;
            node(version)
        }
        Starter::NodeServer => {
            write_files(folder, &node_server_files(name))?;
            node(version)
        }
        Starter::Static | Starter::PythonScripts => {
            let files = match starter {
                Starter::Static => static_files(name),
                _ => python_files(name),
            };
            write_files(folder, &files)?;
            Made {
                runtime: SiteRuntime::Static,
                dev_server: false,
                database: None,
            }
        }
    };
    Ok(made)
}

/// The PHP version new sites get, installed now when it is not yet.
pub fn php_version(home: &Path) -> Result<String> {
    ensure_runtime(home, RuntimeKind::Php, &mut |_| {})
}

/// The version new sites get, installed now when it is not yet.
fn ensure_runtime(
    home: &Path,
    kind: RuntimeKind,
    progress: &mut dyn FnMut(String),
) -> Result<String> {
    let settings = Settings::load_from(&home.join("settings.toml"))?;
    settings.require_module(kind.id())?;
    let runtimes = RuntimeManager::new(home);
    let version = web::default_version(&settings, &runtimes, kind);
    if runtimes.get(kind, &version).is_none() {
        let name = crate::catalog::runtime(kind).name;
        runtimes.install(kind, &version, &mut |step| {
            progress(step_text(&format!("{name} {version}"), &step))
        })?;
    }
    Ok(version)
}

fn step_text(what: &str, step: &Step) -> String {
    match step {
        Step::Resolving => format!("Finding {what}…"),
        Step::Downloading {
            done,
            total: Some(total),
        } if *total > 0 => format!("Downloading {what}… {}%", done * 100 / total),
        Step::Downloading { .. } => format!("Downloading {what}…"),
        Step::Unpacking => format!("Unpacking {what}…"),
    }
}

/// `my-blog` becomes the database `my_blog`.
pub fn database_name(site: &str) -> String {
    site.replace('-', "_")
}

#[derive(serde::Deserialize)]
struct VersionCheck {
    offers: Vec<Offer>,
}

#[derive(serde::Deserialize)]
struct Offer {
    version: String,
}

/// The newest WordPress version, e.g. `6.8.3`.
fn wordpress_version(http: &Http) -> Result<String> {
    let bad = |message: String| Error::Download {
        url: WORDPRESS_VERSIONS.into(),
        message,
    };
    let check: VersionCheck = serde_json::from_str(&http.get_text(WORDPRESS_VERSIONS)?)
        .map_err(|e| bad(format!("unexpected answer: {e}")))?;
    let version = check
        .offers
        .into_iter()
        .map(|o| o.version)
        .next()
        .ok_or_else(|| bad("no version offered".into()))?;
    if version.is_empty() || !version.bytes().all(|b| b.is_ascii_digit() || b == b'.') {
        return Err(bad(format!("unexpected version {version:?}")));
    }
    Ok(version)
}

fn wordpress(
    home: &Path,
    folder: &Path,
    database: &str,
    progress: &mut dyn FnMut(String),
) -> Result<()> {
    let http = Http::new();
    progress("Finding the latest WordPress…".into());
    let version = wordpress_version(&http)?;
    let file_name = format!("wordpress-{version}.zip");
    let url = format!("{WORDPRESS_FILES}/{file_name}");
    let sums_url = format!("{url}.sha1");
    let checksum = http
        .get_text(&sums_url)?
        .split_whitespace()
        .next()
        .filter(|s| s.len() == 40 && s.bytes().all(|b| b.is_ascii_hexdigit()))
        .map(str::to_ascii_lowercase)
        .ok_or_else(|| Error::Download {
            url: sums_url.clone(),
            message: "no SHA-1 checksum in it".into(),
        })?;
    let downloads = home.join("downloads");
    let zip = downloads.join(&file_name);
    let what = format!("WordPress {version}");
    http.download(&url, &zip, &checksum, &mut |done, total| {
        progress(step_text(&what, &Step::Downloading { done, total }))
    })?;
    progress(format!("Unpacking {what}…"));
    let temp = downloads.join(format!("wordpress-{version}-unpacked"));
    let _ = std::fs::remove_dir_all(&temp);
    let root = archive::extract(&zip, &temp)?;
    move_contents(&root, folder)?;
    let _ = std::fs::remove_dir_all(&temp);

    progress(format!("Making the database {database}…"));
    let client = crate::databases::Client::open(home)?;
    // A server that just started takes a few seconds to take connections.
    let mut tries = 0;
    let existing = loop {
        match client.databases() {
            Ok(list) => break list,
            Err(_) if tries < 30 => {
                tries += 1;
                std::thread::sleep(std::time::Duration::from_secs(1));
            }
            Err(e) => return Err(e),
        }
    };
    let exists = existing.iter().any(|d| d.name == database);
    if !exists {
        client.create(database)?;
    }
    let server = client.server().id;
    let port =
        crate::catalog::service(&server).map_or(3306, |def| crate::services::port(home, def));
    let config = folder.join("wp-config.php");
    std::fs::write(&config, wp_config(database, port, &salts()?)).map_err(|source| Error::Io {
        path: config,
        source,
    })
}

/// Moves what is inside `from` into `to`.
fn move_contents(from: &Path, to: &Path) -> Result<()> {
    let io = |path: &Path| {
        let path = path.to_owned();
        move |source| Error::Io { path, source }
    };
    std::fs::create_dir_all(to).map_err(io(to))?;
    for entry in std::fs::read_dir(from).map_err(io(from))? {
        let entry = entry.map_err(io(from))?;
        let target = to.join(entry.file_name());
        std::fs::rename(entry.path(), &target).map_err(io(&target))?;
    }
    Ok(())
}

/// The eight keys and salts WordPress signs cookies with: 64 random
/// printable characters each.
fn salts() -> Result<Vec<String>> {
    const CHARS: &[u8] =
        b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789!#$%&()*+,-./:;<=>?@[]^_{|}~";
    let mut bytes = [0u8; 8 * 64];
    getrandom::fill(&mut bytes).map_err(|e| Error::Starter {
        name: "WordPress".into(),
        message: format!("no randomness: {e}"),
    })?;
    Ok(bytes
        .chunks(64)
        .map(|chunk| {
            chunk
                .iter()
                .map(|b| CHARS[*b as usize % CHARS.len()] as char)
                .collect()
        })
        .collect())
}

const SALT_NAMES: [&str; 8] = [
    "AUTH_KEY",
    "SECURE_AUTH_KEY",
    "LOGGED_IN_KEY",
    "NONCE_KEY",
    "AUTH_SALT",
    "SECURE_AUTH_SALT",
    "LOGGED_IN_SALT",
    "NONCE_SALT",
];

/// Local settings: root with no password on the local MySQL or MariaDB,
/// errors logged to `wp-content/debug.log`.
fn wp_config(database: &str, port: u16, salts: &[String]) -> String {
    let mut out = String::from(
        "<?php\n\
         // Written by Stackyard for local development.\n\
         \n",
    );
    out.push_str(&format!("define( 'DB_NAME', '{database}' );\n"));
    out.push_str("define( 'DB_USER', 'root' );\n");
    out.push_str("define( 'DB_PASSWORD', '' );\n");
    out.push_str(&format!("define( 'DB_HOST', '127.0.0.1:{port}' );\n"));
    out.push_str("define( 'DB_CHARSET', 'utf8mb4' );\n");
    out.push_str("define( 'DB_COLLATE', '' );\n\n");
    for (name, salt) in SALT_NAMES.iter().zip(salts) {
        out.push_str(&format!("define( '{name}', '{salt}' );\n"));
    }
    out.push_str(
        "\n$table_prefix = 'wp_';\n\
         \n\
         define( 'WP_DEBUG', true );\n\
         define( 'WP_DEBUG_LOG', true );\n\
         define( 'WP_DEBUG_DISPLAY', false );\n\
         define( 'WP_ENVIRONMENT_TYPE', 'local' );\n\
         \n\
         if ( ! defined( 'ABSPATH' ) ) {\n\
         \x20   define( 'ABSPATH', __DIR__ . '/' );\n\
         }\n\
         \n\
         require_once ABSPATH . 'wp-settings.php';\n",
    );
    out
}

fn laravel(home: &Path, folder: &Path, progress: &mut dyn FnMut(String)) -> Result<()> {
    if !shell::composer_phar(home).is_file() {
        shell::install_composer(home, &mut |step| progress(step_text("Composer", &step)))?;
    }
    progress("Making the Laravel app with Composer (this can take a few minutes)…".into());
    let (parent, dir_name) = split(folder)?;
    run(
        home,
        Starter::Laravel,
        Tool::Composer,
        &parent,
        [
            "create-project",
            "laravel/laravel",
            &dir_name,
            "--no-interaction",
            "--prefer-dist",
            "--no-progress",
        ],
        &[("COMPOSER_NO_INTERACTION", "1")],
    )
}

fn astro(home: &Path, folder: &Path, progress: &mut dyn FnMut(String)) -> Result<()> {
    progress(
        "Making the Astro site and installing its packages (this can take a few minutes)…".into(),
    );
    let (parent, dir_name) = split(folder)?;
    run(
        home,
        Starter::Astro,
        Tool::Npx,
        &parent,
        [
            "--yes",
            "create-astro@latest",
            &dir_name,
            "--template",
            "minimal",
            "--install",
            "--no-git",
            "--skip-houston",
            "--yes",
        ],
        &[("ASTRO_TELEMETRY_DISABLED", "1")],
    )
}

/// The newest WooCommerce, unpacked into `wp-content/plugins` with every
/// file checked against wordpress.org's list for that version, and a
/// must-use plugin that switches it on once WordPress is installed.
fn woocommerce(home: &Path, wp: &Path, progress: &mut dyn FnMut(String)) -> Result<()> {
    #[derive(serde::Deserialize)]
    struct Info {
        version: String,
        download_link: String,
    }
    let fail = |message: String| Error::Starter {
        name: Starter::WooCommerce.name().into(),
        message,
    };
    let http = Http::new();
    progress("Finding the latest WooCommerce…".into());
    let info: Info = serde_json::from_str(&http.get_text(WOOCOMMERCE_INFO)?)
        .map_err(|e| fail(format!("unexpected answer from wordpress.org: {e}")))?;
    if !info
        .version
        .bytes()
        .all(|b| b.is_ascii_digit() || b == b'.')
        || !info
            .download_link
            .starts_with("https://downloads.wordpress.org/plugin/")
    {
        return Err(fail(format!(
            "unexpected version {:?} or link {:?}",
            info.version, info.download_link
        )));
    }
    let sums_url = format!("{PLUGIN_CHECKSUMS}/woocommerce/{}.json", info.version);
    let sums: serde_json::Value = serde_json::from_str(&http.get_text(&sums_url)?)
        .map_err(|e| fail(format!("unexpected checksum list: {e}")))?;
    let what = format!("WooCommerce {}", info.version);
    let zip = home
        .join("downloads")
        .join(format!("woocommerce-{}.zip", info.version));
    http.download_unverified(&info.download_link, &zip, &mut |done, total| {
        progress(step_text(&what, &Step::Downloading { done, total }))
    })?;
    progress(format!("Checking {what}…"));
    let temp = home
        .join("downloads")
        .join(format!("woocommerce-{}-unpacked", info.version));
    let _ = std::fs::remove_dir_all(&temp);
    let unpacked = archive::extract(&zip, &temp).and_then(|root| {
        check_plugin_files(&root, &sums).map_err(fail)?;
        Ok(root)
    });
    let root = match unpacked {
        Ok(root) => root,
        Err(e) => {
            // A bad download is not kept to be used again.
            let _ = std::fs::remove_file(&zip);
            let _ = std::fs::remove_dir_all(&temp);
            return Err(e);
        }
    };
    move_contents(&root, &wp.join("wp-content/plugins/woocommerce"))?;
    let _ = std::fs::remove_dir_all(&temp);
    write_files(
        wp,
        &[(
            "wp-content/mu-plugins/stackyard-switch-on-woocommerce.php".into(),
            WOOCOMMERCE_SWITCH_ON.into(),
        )],
    )
}

/// Switches WooCommerce on at the first page after WordPress's install,
/// then removes itself.
const WOOCOMMERCE_SWITCH_ON: &str = "<?php
// Written by Stackyard: switches WooCommerce on once WordPress is installed, then removes itself.
add_action( 'wp_loaded', function () {
	if ( wp_installing() || ! is_blog_installed() ) {
		return;
	}
	require_once ABSPATH . 'wp-admin/includes/plugin.php';
	if ( ! is_wp_error( activate_plugin( 'woocommerce/woocommerce.php' ) ) ) {
		@unlink( __FILE__ );
	}
} );
";

/// Every file under `root` must be in wordpress.org's list (`files`:
/// path → `sha256`, a hash or a list of them) with a matching SHA-256.
fn check_plugin_files(root: &Path, sums: &serde_json::Value) -> std::result::Result<(), String> {
    let files = sums
        .get("files")
        .and_then(|f| f.as_object())
        .ok_or("the checksum list has no files")?;
    let mut checked = 0;
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            if entry.file_type().map_err(|e| e.to_string())?.is_dir() {
                stack.push(path);
                continue;
            }
            let relative = path
                .strip_prefix(root)
                .map_err(|e| e.to_string())?
                .to_string_lossy()
                .replace('\\', "/");
            let sum = crate::download::sha256_file(&path).map_err(|e| e.to_string())?;
            let listed = files.get(&relative).and_then(|f| f.get("sha256"));
            let matches = match listed {
                Some(serde_json::Value::String(s)) => s.eq_ignore_ascii_case(&sum),
                Some(serde_json::Value::Array(list)) => list
                    .iter()
                    .any(|s| s.as_str().is_some_and(|s| s.eq_ignore_ascii_case(&sum))),
                _ => false,
            };
            if !matches {
                return Err(format!(
                    "{relative} does not match wordpress.org's checksum; nothing was installed"
                ));
            }
            checked += 1;
        }
    }
    if checked == 0 {
        return Err("the download was empty".into());
    }
    Ok(())
}

/// React, Vue or Svelte with Vite's own `create-vite`, then its packages.
fn vite(
    home: &Path,
    starter: Starter,
    folder: &Path,
    progress: &mut dyn FnMut(String),
) -> Result<()> {
    progress(format!(
        "Making the {} app and installing its packages (this can take a few minutes)…",
        starter.name()
    ));
    let template = match starter {
        Starter::Vue => "vue-ts",
        Starter::Svelte => "svelte-ts",
        _ => "react-ts",
    };
    let (parent, dir_name) = split(folder)?;
    run(
        home,
        starter,
        Tool::Npx,
        &parent,
        [
            "--yes",
            "create-vite@latest",
            &dir_name,
            "--template",
            template,
            "--no-interactive",
            "--no-immediate",
        ],
        &[],
    )?;
    run(
        home,
        starter,
        Tool::Npm,
        folder,
        ["install", "--no-audit", "--no-fund"],
        &[],
    )
}

fn next(home: &Path, folder: &Path, progress: &mut dyn FnMut(String)) -> Result<()> {
    progress(
        "Making the Next.js app and installing its packages (this can take a few minutes)…".into(),
    );
    let (parent, dir_name) = split(folder)?;
    run(
        home,
        Starter::NextJs,
        Tool::Npx,
        &parent,
        [
            "--yes",
            "create-next-app@latest",
            &dir_name,
            "--yes",
            "--use-npm",
            "--disable-git",
        ],
        &[("NEXT_TELEMETRY_DISABLED", "1")],
    )
}

/// Writes `files` (path inside `folder`, text) into `folder`.
fn write_files(folder: &Path, files: &[(String, String)]) -> Result<()> {
    for (name, text) in files {
        let path = folder.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| Error::Io {
                path: parent.to_owned(),
                source,
            })?;
        }
        std::fs::write(&path, text).map_err(|source| Error::Io { path, source })?;
    }
    Ok(())
}

/// What goes in HTML: `<`, `>`, `&` and `"` written out.
fn html_text(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn static_files(name: &str) -> Vec<(String, String)> {
    let title = html_text(name);
    vec![
        (
            "index.html".into(),
            format!(
                "<!doctype html>\n<html lang=\"en\">\n<head>\n  <meta charset=\"utf-8\">\n  <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n  <title>{title}</title>\n  <link rel=\"stylesheet\" href=\"style.css\">\n</head>\n<body>\n  <main>\n    <h1>{title}</h1>\n    <p>Edit index.html to start.</p>\n  </main>\n  <script src=\"script.js\"></script>\n</body>\n</html>\n"
            ),
        ),
        (
            "style.css".into(),
            "body {\n  margin: 0 auto;\n  max-width: 48rem;\n  padding: 2rem;\n  font-family: system-ui, sans-serif;\n}\n".into(),
        ),
        ("script.js".into(), "// Your script.\n".into()),
    ]
}

/// A server with no packages. Stackyard's dev server runs `npm run dev --
/// --port N`, so it takes the port from `--port`, else `PORT`.
fn node_server_files(name: &str) -> Vec<(String, String)> {
    vec![
        (
            "package.json".into(),
            format!(
                "{{\n  \"name\": \"{name}\",\n  \"version\": \"0.1.0\",\n  \"private\": true,\n  \"type\": \"module\",\n  \"scripts\": {{\n    \"dev\": \"node --watch server.js\",\n    \"start\": \"node server.js\"\n  }}\n}}\n"
            ),
        ),
        (
            "server.js".into(),
            "import { createServer } from \"node:http\";\n\nconst flag = process.argv.indexOf(\"--port\");\nconst port = Number(flag > 0 ? process.argv[flag + 1] : process.env.PORT) || 3000;\n\ncreateServer((req, res) => {\n  res.writeHead(200, { \"content-type\": \"text/html; charset=utf-8\" });\n  res.end(\"<h1>Hello from Node.js</h1><p>Edit server.js to start.</p>\");\n}).listen(port, \"127.0.0.1\", () => console.log(`Listening on http://127.0.0.1:${port}`));\n".into(),
        ),
    ]
}

fn python_files(name: &str) -> Vec<(String, String)> {
    vec![
        (
            "main.py".into(),
            "def main():\n    print(\"Hello from Python\")\n\n\nif __name__ == \"__main__\":\n    main()\n".into(),
        ),
        (
            "README.md".into(),
            format!("# {name}\n\nRun it in Stackyard's terminal:\n\n```\npython main.py\n```\n"),
        ),
    ]
}

/// The folder's parent (created) and its own name, since project makers
/// create the folder themselves.
fn split(folder: &Path) -> Result<(PathBuf, String)> {
    let parent = folder
        .parent()
        .ok_or_else(|| Error::NotAFolder(folder.to_owned()))?
        .to_path_buf();
    std::fs::create_dir_all(&parent).map_err(|source| Error::Io {
        path: parent.clone(),
        source,
    })?;
    let name = folder
        .file_name()
        .ok_or_else(|| Error::NotAFolder(folder.to_owned()))?
        .to_string_lossy()
        .into_owned();
    Ok((parent, name))
}

/// Runs a Stackyard terminal command (`composer`, `npx`) and keeps its
/// output in `logs/starter.log`; a failure ends with its last lines.
fn run<const N: usize>(
    home: &Path,
    starter: Starter,
    tool: Tool,
    dir: &Path,
    args: [&str; N],
    env: &[(&str, &str)],
) -> Result<()> {
    let fail = |message: String| Error::Starter {
        name: starter.name().into(),
        message,
    };
    let context = Context::load(home)?;
    let launch = context.launch(tool, dir, args.iter().map(OsString::from).collect())?;
    let mut command = Command::new(&launch.program);
    command
        .args(&launch.args)
        .current_dir(dir)
        .env("PATH", launch.path_var())
        .envs(launch.env.iter().map(|(k, v)| (k, v)))
        .envs(env.iter().copied())
        .stdin(Stdio::null());
    crate::platform::hide_console(&mut command);
    let output = command
        .output()
        .map_err(|e| fail(format!("could not run {}: {e}", tool.name())))?;
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    let log = home.join("logs").join("starter.log");
    let _ = std::fs::create_dir_all(home.join("logs"));
    let _ = std::fs::write(&log, &text);
    if output.status.success() {
        return Ok(());
    }
    let last: Vec<&str> = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .rev()
        .take(4)
        .collect();
    let last: Vec<&str> = last.into_iter().rev().collect();
    Err(fail(format!(
        "{} failed ({}): {}. The full output is in {}",
        tool.name(),
        output.status,
        last.join(" / "),
        log.display()
    )))
}

/// A port for a new dev server: `usual` unless another site or program
/// has it.
fn free_dev_port(home: &Path, usual: u16) -> u16 {
    let taken: Vec<u16> = crate::sites::SiteStore::load_from(&home.join("sites.toml"))
        .map(|store| {
            store
                .list()
                .iter()
                .filter_map(|s| s.runtime.port())
                .collect()
        })
        .unwrap_or_default();
    (usual..usual + 100)
        .find(|p| !taken.contains(p) && ports::is_free(*p))
        .unwrap_or(usual)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starters_round_trip_by_id() {
        for starter in Starter::ALL {
            assert_eq!(Starter::from_id(starter.id()), Some(starter));
        }
        assert_eq!(Starter::from_id("drupal"), None);
        assert_eq!(list().len(), Starter::ALL.len());
        assert_eq!(Starter::PythonScripts.category(), Category::Project);
        assert_eq!(Starter::NodeServer.category(), Category::App);
        assert_eq!(Starter::React.category(), Category::Site);
    }

    #[test]
    fn wp_config_points_at_the_local_database() {
        let salts = salts().unwrap();
        assert_eq!(salts.len(), 8);
        assert!(salts.iter().all(|s| s.len() == 64 && !s.contains('\'')));
        let config = wp_config("my_blog", 3307, &salts);
        assert!(
            config.contains("define( 'DB_NAME', 'my_blog' );"),
            "{config}"
        );
        assert!(config.contains("'DB_HOST', '127.0.0.1:3307'"), "{config}");
        assert!(config.contains(&format!("'NONCE_SALT', '{}'", salts[7])));
        assert!(config
            .trim_end()
            .ends_with("require_once ABSPATH . 'wp-settings.php';"));
        assert_eq!(database_name("my-blog"), "my_blog");
    }

    #[test]
    fn refuses_a_folder_with_files() {
        let home = crate::testutil::tempdir();
        let folder = home.join("www").join("blog");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("index.php"), "<?php").unwrap();
        let err = create(&home, Starter::Astro, "blog", &folder, &mut |_| {}).unwrap_err();
        assert!(err.to_string().contains("already has files"), "{err}");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn dev_ports_skip_other_node_sites() {
        let home = crate::testutil::tempdir();
        let mut store = crate::sites::SiteStore::load_from(&home.join("sites.toml")).unwrap();
        store
            .add(crate::sites::Site {
                name: "docs".into(),
                tld: ".stack".into(),
                path: home.join("docs"),
                runtime: SiteRuntime::Node {
                    version: "22".into(),
                    port: 4321,
                },
                https: true,
                dev_server: true,
                settings: Default::default(),
            })
            .unwrap();
        store.save().unwrap();
        assert_ne!(free_dev_port(&home, 4321), 4321);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn plain_starters_write_their_files() {
        let home = crate::testutil::tempdir();
        let site = home.join("www").join("page");
        let made = create(&home, Starter::Static, "page", &site, &mut |_| {}).unwrap();
        assert_eq!(made.runtime, SiteRuntime::Static);
        let index = std::fs::read_to_string(site.join("index.html")).unwrap();
        assert!(index.contains("<title>page</title>"), "{index}");
        let scripts = home.join("www").join("tools");
        create(
            &home,
            Starter::PythonScripts,
            "tools",
            &scripts,
            &mut |_| {},
        )
        .unwrap();
        assert!(scripts.join("main.py").is_file());
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn plugin_files_must_match_the_published_list() {
        let home = crate::testutil::tempdir();
        let root = home.join("woocommerce");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("woocommerce.php"), "<?php").unwrap();
        std::fs::write(root.join("src/a.php"), "a").unwrap();
        let sum = |p: &str| crate::download::sha256_file(&root.join(p)).unwrap();
        let good = serde_json::json!({"files": {
            "woocommerce.php": {"md5": "x", "sha256": sum("woocommerce.php")},
            "src/a.php": {"md5": "x", "sha256": ["0", sum("src/a.php")]},
        }});
        check_plugin_files(&root, &good).unwrap();
        std::fs::write(root.join("src/a.php"), "changed").unwrap();
        assert!(check_plugin_files(&root, &good).is_err());
        std::fs::write(root.join("src/a.php"), "a").unwrap();
        std::fs::write(root.join("extra.php"), "").unwrap();
        assert!(check_plugin_files(&root, &good).is_err(), "not in the list");
        let _ = std::fs::remove_dir_all(&home);
    }

    /// Needs the network: checks wordpress.org still answers as expected.
    #[test]
    #[ignore]
    fn live_wordpress_version_and_checksum() {
        let http = Http::new();
        let version = wordpress_version(&http).unwrap();
        let sums = http
            .get_text(&format!("{WORDPRESS_FILES}/wordpress-{version}.zip.sha1"))
            .unwrap();
        let sum = sums.split_whitespace().next().unwrap();
        assert_eq!(sum.len(), 40, "{sums}");
    }
}
