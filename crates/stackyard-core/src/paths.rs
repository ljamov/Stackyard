//! Where Stackyard keeps its files.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Root folder for Stackyard's config, runtimes, services, data and logs.
///
/// `STACKYARD_HOME` overrides it (used by tests and portable installs).
/// Otherwise it is the folder chosen in first-run setup; else the OS data
/// folder when an older version already keeps its files there; else the
/// default: `C:\stackyard` on Windows, the OS data folder elsewhere (e.g.
/// `~/Library/Application Support/Stackyard`).
pub fn home() -> PathBuf {
    if let Some(dir) = std::env::var_os("STACKYARD_HOME") {
        return PathBuf::from(dir);
    }
    if let Some(location) = Location::load() {
        // Until the move has happened, the files are still in the old place.
        return location.move_from.unwrap_or(location.home);
    }
    let legacy = legacy_home();
    if legacy.join("settings.toml").is_file() {
        return legacy;
    }
    default_home()
}

/// Where Stackyard 0.1 kept everything: the OS data folder.
pub fn legacy_home() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Stackyard")
}

/// Where a fresh install keeps its files unless the user picks another folder.
pub fn default_home() -> PathBuf {
    if cfg!(windows) {
        let drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
        PathBuf::from(format!("{drive}\\stackyard"))
    } else {
        legacy_home()
    }
}

/// The folder chosen in first-run setup, kept outside that folder so it
/// can be found again. Absent until setup has run.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Location {
    pub home: PathBuf,
    /// The folder to move everything from on the next start, when setup
    /// picked a new place for an existing installation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub move_from: Option<PathBuf>,
    /// What went wrong moving, shown in setup; Stackyard stays where it was.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub move_error: Option<String>,
    /// What setup picked, applied once any move is done.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub choices: Option<crate::setup::Choices>,
    /// Set once setup has finished.
    #[serde(default)]
    pub done: bool,
}

impl Location {
    pub fn file() -> Option<PathBuf> {
        dirs::config_local_dir().map(|d| d.join("dev.stackyard.app").join("location.toml"))
    }

    pub fn load() -> Option<Self> {
        crate::settings::read_toml(&Self::file()?).ok().flatten()
    }

    pub fn save(&self) -> crate::Result<()> {
        let file = Self::file().ok_or_else(|| crate::Error::Io {
            path: PathBuf::from("location.toml"),
            source: std::io::Error::other("could not find the user's settings folder"),
        })?;
        crate::settings::write_toml(&file, self)
    }
}

pub fn settings_file() -> PathBuf {
    home().join("settings.toml")
}

pub fn sites_file() -> PathBuf {
    home().join("sites.toml")
}

pub fn logs_dir() -> PathBuf {
    home().join("logs")
}

/// `~/Code/app` → `/home/me/Code/app`; other paths are returned as they are.
pub fn expand_home(path: &Path) -> PathBuf {
    let Ok(rest) = path.strip_prefix("~") else {
        return path.to_owned();
    };
    match dirs::home_dir() {
        Some(home) => home.join(rest),
        None => path.to_owned(),
    }
}
