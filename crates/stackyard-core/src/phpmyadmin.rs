//! phpMyAdmin, the database manager, served as a local site
//! (`phpmyadmin.stack`) by the web server with one of the installed PHP
//! versions, logged in to MySQL or MariaDB as root.
//!
//! The official release is downloaded from phpmyadmin.net and checked
//! against the SHA-256 published next to it. Stackyard writes the
//! `config.inc.php` itself; phpMyAdmin's own files are not changed.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::catalog::RuntimeKind;
use crate::download::{self, Http};
use crate::install::{self, Release, Step};
use crate::runtimes::RuntimeManager;
use crate::settings::Settings;
use crate::sites::{Site, SiteRuntime};
use crate::{Error, Result};

pub const MODULE: &str = "phpmyadmin";
/// The site name: `phpmyadmin.stack`.
pub const SITE: &str = "phpmyadmin";
const VERSION_URL: &str = "https://www.phpmyadmin.net/home_page/version.json";
const FILES: &str = "https://files.phpmyadmin.net/phpMyAdmin";
/// The newest PHP line phpMyAdmin is known to run on without warnings;
/// newer installed versions are used only when no older one is installed.
const NEWEST_PHP: (u32, u32) = (8, 4);
/// A file every release has, to catch a wrong or empty download.
const MARKER: &str = "index.php";

pub fn dir(home: &Path) -> PathBuf {
    home.join("tools").join("phpmyadmin")
}

/// The installed release, e.g. `5.2.2`.
pub fn installed(home: &Path) -> Option<String> {
    install::read_receipt(&dir(home)).map(|r| r.release)
}

#[derive(Debug, serde::Deserialize)]
struct VersionInfo {
    version: String,
}

/// The newest release and its official download.
pub fn resolve(http: &Http) -> Result<Release> {
    let info: VersionInfo =
        serde_json::from_str(&http.get_text(VERSION_URL)?).map_err(|e| Error::Download {
            url: VERSION_URL.into(),
            message: format!("unexpected answer: {e}"),
        })?;
    let version = info.version.trim();
    if version.is_empty() || !version.bytes().all(|b| b.is_ascii_digit() || b == b'.') {
        return Err(Error::Download {
            url: VERSION_URL.into(),
            message: format!("unexpected version {version:?}"),
        });
    }
    let file_name = format!("phpMyAdmin-{version}-all-languages.zip");
    let url = format!("{FILES}/{version}/{file_name}");
    let sums_url = format!("{url}.sha256");
    let checksum =
        download::find_checksum(&http.get_text(&sums_url)?, &file_name).ok_or_else(|| {
            Error::Download {
                url: sums_url,
                message: format!("no checksum for {file_name}"),
            }
        })?;
    Ok(Release {
        version: version.to_string(),
        url,
        file_name,
        checksum,
    })
}

pub fn install(home: &Path, progress: &mut dyn FnMut(Step)) -> Result<String> {
    let http = Http::new();
    progress(Step::Resolving);
    let release = resolve(&http)?;
    install::install(
        &http,
        &home.join("downloads"),
        &release,
        &dir(home),
        MARKER,
        progress,
    )?;
    write_config(home)?;
    Ok(release.version)
}

pub fn uninstall(home: &Path) -> Result<()> {
    install::remove(&dir(home))
}

/// The site the web server adds while the module is on and phpMyAdmin is
/// installed, or `None`.
pub fn site(home: &Path, settings: &Settings, runtimes: &RuntimeManager) -> Option<Site> {
    if !settings.module_enabled(MODULE) || installed(home).is_none() {
        return None;
    }
    // Written on every start, so it follows Stackyard's folder if it moves.
    if let Err(e) = write_config(home) {
        eprintln!("could not write phpMyAdmin's settings: {e}");
    }
    Some(Site {
        name: SITE.into(),
        tld: settings.tld.clone(),
        path: dir(home),
        runtime: SiteRuntime::Php {
            version: php_version(runtimes).unwrap_or_else(|| "8.4".into()),
        },
        https: true,
        dev_server: false,
    })
}

/// The newest installed PHP that phpMyAdmin runs on cleanly, else the
/// oldest installed one.
fn php_version(runtimes: &RuntimeManager) -> Option<String> {
    let parse = |v: &str| -> (u32, u32) {
        let (a, b) = v.split_once('.').unwrap_or((v, "0"));
        (a.parse().unwrap_or(0), b.parse().unwrap_or(0))
    };
    let mut versions: Vec<String> = runtimes
        .installed()
        .into_iter()
        .filter(|i| i.kind == RuntimeKind::Php)
        .map(|i| i.version)
        .collect();
    versions.sort_by_key(|v| parse(v));
    versions
        .iter()
        .rev()
        .find(|v| parse(v) <= NEWEST_PHP)
        .or(versions.first())
        .cloned()
}

/// Logs in as root on the local MySQL or MariaDB.
fn config_php(secret: &str, temp: &Path, port: u16) -> String {
    let temp = temp.display().to_string().replace('\\', "/");
    format!(
        "<?php\n\
         // Written by Stackyard. Changes are replaced; put your own in config.user.inc.php.\n\
         $cfg['blowfish_secret'] = '{secret}';\n\
         $cfg['TempDir'] = '{temp}';\n\
         $cfg['Servers'][1]['verbose'] = 'Local MySQL or MariaDB';\n\
         $cfg['Servers'][1]['host'] = '127.0.0.1';\n\
         $cfg['Servers'][1]['port'] = '{port}';\n\
         $cfg['Servers'][1]['auth_type'] = 'config';\n\
         $cfg['Servers'][1]['user'] = 'root';\n\
         $cfg['Servers'][1]['password'] = '';\n\
         $cfg['Servers'][1]['AllowNoPassword'] = true;\n\
         $cfg['UploadDir'] = '';\n\
         $cfg['SaveDir'] = '';\n\
         $cfg['VersionCheck'] = false;\n\
         $cfg['SendErrorReports'] = 'never';\n\
         $cfg['ShowDatabasesNavigationAsTree'] = true;\n\
         if (is_file(__DIR__ . '/config.user.inc.php')) {{\n\
         \x20   include __DIR__ . '/config.user.inc.php';\n\
         }}\n",
        temp = temp.replace('\'', "\\'"),
    )
}

/// The port of the installed MySQL, else MariaDB (3306 unless another
/// program had it).
fn database_port(home: &Path) -> u16 {
    let services = crate::services::ServiceManager::new(home);
    ["mysql", "mariadb"]
        .into_iter()
        .filter(|id| services.get(id).is_some())
        .find_map(crate::catalog::service)
        .map(|def| crate::services::port(home, def))
        .unwrap_or(3306)
}

/// Points phpMyAdmin at the database server that just started, when
/// phpMyAdmin is installed. It reads its settings on every page.
pub fn use_database(home: &Path, port: u16) {
    if installed(home).is_some() {
        if let Err(e) = write_config_for(home, port) {
            eprintln!("could not write phpMyAdmin's settings: {e}");
        }
    }
}

fn write_config(home: &Path) -> Result<()> {
    write_config_for(home, database_port(home))
}

/// Writes `config.inc.php`, with a secret kept in Stackyard's data folder
/// so logins survive reinstalls.
fn write_config_for(home: &Path, port: u16) -> Result<()> {
    let data = home.join("data").join("phpmyadmin");
    let temp = data.join("tmp");
    let io = |path: &Path| {
        let path = path.to_owned();
        move |source| Error::Io { path, source }
    };
    std::fs::create_dir_all(&temp).map_err(io(&temp))?;
    let secret_file = data.join("secret");
    let secret = match std::fs::read_to_string(&secret_file) {
        Ok(s) if s.trim().len() == 32 => s.trim().to_string(),
        _ => {
            // 16 random bytes as 32 hex characters: the 32-byte key phpMyAdmin wants.
            let mut bytes = [0u8; 16];
            getrandom::fill(&mut bytes).map_err(|e| Error::Io {
                path: secret_file.clone(),
                source: std::io::Error::other(e.to_string()),
            })?;
            let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
            std::fs::write(&secret_file, &hex).map_err(io(&secret_file))?;
            hex
        }
    };
    crate::services::write_config(
        &dir(home).join("config.inc.php"),
        &config_php(&secret, &temp, port),
    )
}

#[derive(Debug, Clone, Serialize)]
pub struct Status {
    /// The installed release, if any.
    pub installed: Option<String>,
    pub enabled: bool,
    /// Where it opens once the web server runs.
    pub url: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_logs_in_as_root_and_quotes_paths() {
        let text = config_php(
            &"ab".repeat(16),
            Path::new("C:\\stack'yard\\data\\phpmyadmin\\tmp"),
            3307,
        );
        assert!(text.contains("['port'] = '3307';"));
        assert!(text.contains("$cfg['Servers'][1]['auth_type'] = 'config';"));
        assert!(text.contains("'user'] = 'root'"));
        assert!(
            text.contains("C:/stack\\'yard/data/phpmyadmin/tmp"),
            "{text}"
        );
        assert!(text.contains(&format!("'blowfish_secret'] = '{}';", "ab".repeat(16))));
    }

    #[test]
    fn writes_config_with_a_lasting_secret() {
        let home = crate::testutil::tempdir();
        std::fs::create_dir_all(dir(&home)).unwrap();
        write_config(&home).unwrap();
        let secret = std::fs::read_to_string(home.join("data/phpmyadmin/secret")).unwrap();
        write_config(&home).unwrap();
        let again = std::fs::read_to_string(home.join("data/phpmyadmin/secret")).unwrap();
        assert_eq!(secret, again);
        let config = std::fs::read_to_string(dir(&home).join("config.inc.php")).unwrap();
        assert!(config.contains(&secret));
        let _ = std::fs::remove_dir_all(&home);
    }

    /// Looks up the real release: `cargo test -p stackyard-core -- --ignored live_`.
    #[test]
    #[ignore]
    fn live_resolves_phpmyadmin() {
        let release = resolve(&Http::new()).unwrap();
        assert!(release.url.ends_with("-all-languages.zip"), "{release:?}");
        assert!(download::is_sha256(&release.checksum), "{release:?}");
    }
}
