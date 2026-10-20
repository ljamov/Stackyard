//! Files the app's window keeps for people to edit by hand: the `[interface]`
//! section of `settings.toml` (theme, accent color, language) and
//! `keybindings.toml` (keyboard shortcuts).
//!
//! The core stores both as they are written and does not interpret them. The
//! window checks every value and reports what it cannot use, so a typo there
//! never stops Stackyard from starting.

use std::path::{Path, PathBuf};

use serde::Serialize;

pub use toml::Table;

use crate::settings::{read_toml, write_toml, Settings};
use crate::{paths, Result};

/// A file's values, or why they could not be read.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct InterfaceFile {
    pub path: PathBuf,
    /// Empty when the file does not exist yet, or when `error` is set.
    pub values: Table,
    /// What is wrong with the file and where, when it is not valid TOML.
    pub error: Option<String>,
}

pub fn keybindings_file() -> PathBuf {
    paths::home().join("keybindings.toml")
}

/// The `[interface]` section of `settings.toml`.
pub fn load_settings() -> InterfaceFile {
    load_settings_from(&paths::settings_file())
}

/// Replaces the `[interface]` section; every other setting is kept.
pub fn save_settings(values: Table) -> Result<()> {
    save_settings_to(&paths::settings_file(), values)
}

pub fn load_keybindings() -> InterfaceFile {
    load_table(&keybindings_file())
}

pub fn save_keybindings(values: &Table) -> Result<()> {
    write_toml(&keybindings_file(), values)
}

pub fn load_settings_from(path: &Path) -> InterfaceFile {
    match Settings::load_from(path) {
        Ok(settings) => InterfaceFile {
            path: path.to_owned(),
            values: settings.interface,
            error: None,
        },
        Err(e) => InterfaceFile {
            path: path.to_owned(),
            values: Table::new(),
            error: Some(e.to_string()),
        },
    }
}

pub fn save_settings_to(path: &Path, values: Table) -> Result<()> {
    let mut settings = Settings::load_from(path)?;
    settings.interface = values;
    settings.save_to(path)
}

pub fn load_table(path: &Path) -> InterfaceFile {
    match read_toml::<Table>(path) {
        Ok(values) => InterfaceFile {
            path: path.to_owned(),
            values: values.unwrap_or_default(),
            error: None,
        },
        Err(e) => InterfaceFile {
            path: path.to_owned(),
            values: Table::new(),
            error: Some(e.to_string()),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(text: &str) -> Table {
        toml::from_str(text).unwrap()
    }

    #[test]
    fn interface_section_is_kept_beside_other_settings() {
        let dir = crate::testutil::tempdir();
        let path = dir.join("settings.toml");
        let mut settings = Settings::default();
        settings.set_tld("test").unwrap();
        settings.save_to(&path).unwrap();

        assert_eq!(load_settings_from(&path).values, Table::new());
        save_settings_to(&path, table("theme = \"light\"\naccent = \"teal\"")).unwrap();

        let file = load_settings_from(&path);
        assert_eq!(file.error, None);
        assert_eq!(file.values, table("theme = \"light\"\naccent = \"teal\""));
        assert_eq!(Settings::load_from(&path).unwrap().tld, ".test");
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("[interface]"), "{text}");
    }

    #[test]
    fn an_empty_section_is_not_written() {
        let dir = crate::testutil::tempdir();
        let path = dir.join("settings.toml");
        save_settings_to(&path, Table::new()).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.contains("interface"), "{text}");
    }

    #[test]
    fn a_broken_file_is_reported_not_fatal() {
        let dir = crate::testutil::tempdir();
        let path = dir.join("keybindings.toml");
        std::fs::write(&path, "[keys\n").unwrap();
        let file = load_table(&path);
        assert_eq!(file.values, Table::new());
        assert!(file.error.is_some());
    }

    #[test]
    fn a_missing_file_is_empty() {
        let dir = crate::testutil::tempdir();
        let file = load_table(&dir.join("keybindings.toml"));
        assert_eq!(file.values, Table::new());
        assert_eq!(file.error, None);
    }

    #[test]
    fn keybindings_round_trip() {
        let dir = crate::testutil::tempdir();
        let path = dir.join("keybindings.toml");
        let keys = table("[keys]\n\"view.explorer\" = [\"ctrl+shift+e\"]\n\"tab.close\" = []");
        write_toml(&path, &keys).unwrap();
        assert_eq!(load_table(&path).values, keys);
    }
}
