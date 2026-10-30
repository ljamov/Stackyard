//! Starters: a new site that begins as a WordPress, Laravel or Astro project
//! instead of an empty folder.
//!
//! - WordPress comes from wordpress.org, checked against the SHA-1 published
//!   next to it (the only checksum WordPress publishes). Stackyard makes its
//!   database and writes `wp-config.php`; the browser shows WordPress's own
//!   install page next.
//! - Laravel is made by Composer (`composer create-project`), with SQLite
//!   as Laravel's default database.
//! - Astro is made by its own `create-astro`, and Stackyard runs its dev
//!   server while the web server runs.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::Serialize;

use crate::catalog::RuntimeKind;
use crate::download::Http;
use crate::install::Step;
use crate::runtimes::RuntimeManager;
use crate::settings::Settings;
use crate::shell::{self, Context, Tool};
use crate::sites::SiteRuntime;
use crate::{archive, ports, web, Error, Result};

const WORDPRESS_VERSIONS: &str = "https://api.wordpress.org/core/version-check/1.7/";
const WORDPRESS_FILES: &str = "https://wordpress.org";
/// Astro's dev server port; the next free one is used when it is taken.
const ASTRO_PORT: u16 = 4321;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Starter {
    WordPress,
    Laravel,
    Astro,
}

#[derive(Debug, Clone, Serialize)]
pub struct StarterInfo {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
}

impl Starter {
    pub const ALL: [Starter; 3] = [Starter::WordPress, Starter::Laravel, Starter::Astro];

    pub fn id(self) -> &'static str {
        match self {
            Starter::WordPress => "wordpress",
            Starter::Laravel => "laravel",
            Starter::Astro => "astro",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Starter::WordPress => "WordPress",
            Starter::Laravel => "Laravel",
            Starter::Astro => "Astro",
        }
    }

    pub fn from_id(id: &str) -> Option<Starter> {
        Self::ALL.into_iter().find(|s| s.id() == id)
    }

    /// The runtime the project runs on.
    pub fn runtime(self) -> RuntimeKind {
        match self {
            Starter::WordPress | Starter::Laravel => RuntimeKind::Php,
            Starter::Astro => RuntimeKind::Node,
        }
    }

    pub fn info(self) -> StarterInfo {
        StarterInfo {
            id: self.id(),
            name: self.name(),
            description: match self {
                Starter::WordPress => {
                    "The latest WordPress, with its database made in MySQL or MariaDB"
                }
                Starter::Laravel => "A new Laravel app, made with Composer",
                Starter::Astro => "A new Astro site; Stackyard runs its dev server",
            },
        }
    }

    /// Whether the project needs MySQL or MariaDB running while it is made.
    pub fn needs_database(self) -> bool {
        self == Starter::WordPress
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
    let version = ensure_runtime(home, starter.runtime(), progress)?;
    let made = match starter {
        Starter::WordPress => {
            let database = database_name(name);
            wordpress(home, folder, &database, progress)?;
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
            Made {
                runtime: SiteRuntime::Node {
                    version,
                    port: free_dev_port(home),
                },
                dev_server: true,
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

/// A port for a new dev server: Astro's usual one unless another site or
/// program has it.
fn free_dev_port(home: &Path) -> u16 {
    let taken: Vec<u16> = crate::sites::SiteStore::load_from(&home.join("sites.toml"))
        .map(|store| {
            store
                .list()
                .iter()
                .filter_map(|s| s.runtime.port())
                .collect()
        })
        .unwrap_or_default();
    (ASTRO_PORT..ASTRO_PORT + 100)
        .find(|p| !taken.contains(p) && ports::is_free(*p))
        .unwrap_or(ASTRO_PORT)
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
        assert_eq!(list().len(), 3);
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
                    port: ASTRO_PORT,
                },
                https: true,
                dev_server: true,
                settings: Default::default(),
            })
            .unwrap();
        store.save().unwrap();
        assert_ne!(free_dev_port(&home), ASTRO_PORT);
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
