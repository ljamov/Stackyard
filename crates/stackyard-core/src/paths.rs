//! Where Stackyard keeps its files.

use std::path::PathBuf;

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
