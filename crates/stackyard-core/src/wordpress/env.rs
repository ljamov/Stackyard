//! Which environment a WordPress copy is, and where its live site is.
//! Kept per site in `wordpress/<domain>.toml` under Stackyard's home, so it
//! survives the site's own files being replaced.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::settings::{read_toml, write_toml};
use crate::Result;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// The main development copy.
    #[default]
    Local,
    Staging,
}

/// What a clone does with `wp-content/uploads`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Media {
    /// Every file is downloaded.
    #[default]
    Download,
    /// Nothing is downloaded up front; a file a page asks for is fetched
    /// from the live site the first time and kept.
    FromLive,
}

/// How a live server was reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Route {
    /// SSH: files over SFTP, the database with `mysqldump` on the server.
    Ssh,
    /// SFTP or FTP for files, the database through the PHP helper.
    Helper,
    /// Backup files picked on this computer.
    Files,
}

/// The live site a copy came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Live {
    /// The site's address, e.g. `https://example.com`.
    pub url: String,
    /// The saved connection to its server; none when cloned from backup files.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connection: Option<String>,
    /// WordPress's folder on the server.
    #[serde(default)]
    pub path: String,
    pub route: Route,
    #[serde(default)]
    pub media: Media,
    /// When the copy was cloned or last pulled, in seconds since 1970.
    pub synced_at: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Environment {
    #[serde(default)]
    pub kind: Kind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub live: Option<Live>,
    /// The copy's mail, indexing, outside calls and payments are made safe
    /// (see the must-use plugin).
    #[serde(default)]
    pub safe: bool,
    /// A staging's main local site, e.g. `shop.stack` for `checkout.shop.stack`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    /// What a staging was made from: a site's domain, or `live`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub made_from: Option<String>,
    /// When the staging was made, in seconds since 1970.
    #[serde(default)]
    pub created_at: u64,
    /// The uploads folder a staging made without its own media copies
    /// missing media from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shared_media: Option<PathBuf>,
}

pub fn file(home: &Path, domain: &str) -> PathBuf {
    home.join("wordpress").join(format!("{domain}.toml"))
}

/// The site's environment; `None` for a WordPress site Stackyard did not clone.
pub fn load(home: &Path, domain: &str) -> Result<Option<Environment>> {
    read_toml(&file(home, domain))
}

pub fn save(home: &Path, domain: &str, env: &Environment) -> Result<()> {
    write_toml(&file(home, domain), env)
}

/// Forgets a site's environment, e.g. when the site is removed.
pub fn forget(home: &Path, domain: &str) {
    let _ = std::fs::remove_file(file(home, domain));
}

/// Keeps a site's environment when its domain changes.
pub fn rename(home: &Path, old: &str, new: &str) {
    let _ = std::fs::rename(file(home, old), file(home, new));
}

pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saves_and_loads() {
        let home = crate::testutil::tempdir();
        assert_eq!(load(&home, "shop.stack").unwrap(), None);
        let env = Environment {
            kind: Kind::Local,
            live: Some(Live {
                url: "https://shop.example".into(),
                connection: Some("abcd2345".into()),
                path: "/var/www/shop".into(),
                route: Route::Ssh,
                media: Media::FromLive,
                synced_at: 1_800_000_000,
            }),
            safe: true,
            ..Environment::default()
        };
        save(&home, "shop.stack", &env).unwrap();
        assert_eq!(load(&home, "shop.stack").unwrap(), Some(env));
        forget(&home, "shop.stack");
        assert_eq!(load(&home, "shop.stack").unwrap(), None);
    }
}
