//! Where Stackyard keeps its files.

use std::path::{Path, PathBuf};

/// Root folder for Stackyard's config, downloads and logs.
///
/// `STACKYARD_HOME` overrides it (used by tests and portable installs);
/// otherwise it is the OS data folder, e.g. `~/Library/Application Support/Stackyard`.
pub fn home() -> PathBuf {
    if let Some(dir) = std::env::var_os("STACKYARD_HOME") {
        return PathBuf::from(dir);
    }
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Stackyard")
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
