//! User settings, stored as TOML.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::{domain, modules, paths, Error, Result};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Domain ending for new sites, e.g. `.stack`.
    pub tld: String,
    /// Modules the user switched on or off; anything missing uses its default.
    pub modules: BTreeMap<String, bool>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            tld: domain::DEFAULT_TLD.to_string(),
            modules: BTreeMap::new(),
        }
    }
}

impl Settings {
    pub fn load() -> Result<Self> {
        Self::load_from(&paths::settings_file())
    }

    pub fn save(&self) -> Result<()> {
        self.save_to(&paths::settings_file())
    }

    pub fn load_from(path: &Path) -> Result<Self> {
        read_toml(path).map(Option::unwrap_or_default)
    }

    pub fn save_to(&self, path: &Path) -> Result<()> {
        write_toml(path, self)
    }

    /// Validates and stores a new domain ending.
    pub fn set_tld(&mut self, input: &str) -> Result<()> {
        self.tld = domain::normalize_tld(input)?;
        Ok(())
    }

    /// Whether a module is on. Unknown and not-yet-available modules are off.
    pub fn module_enabled(&self, id: &str) -> bool {
        match modules::find(id) {
            Some(def) if def.available => {
                self.modules.get(id).copied().unwrap_or(def.on_by_default)
            }
            _ => false,
        }
    }

    pub fn set_module(&mut self, id: &str, on: bool) -> Result<()> {
        let def = modules::find(id).ok_or_else(|| Error::UnknownModule(id.to_string()))?;
        if on && !def.available {
            return Err(Error::ModuleUnavailable(def.name.to_string()));
        }
        if on == def.on_by_default {
            self.modules.remove(id);
        } else {
            self.modules.insert(id.to_string(), on);
        }
        Ok(())
    }
}

/// Reads a TOML file, returning `None` when it does not exist yet.
pub(crate) fn read_toml<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<Option<T>> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(Error::Io {
                path: path.to_owned(),
                source,
            })
        }
    };
    toml::from_str(&text)
        .map(Some)
        .map_err(|source| Error::Parse {
            path: path.to_owned(),
            source,
        })
}

pub(crate) fn write_toml<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let text = toml::to_string_pretty(value)?;
    let io = |source| Error::Io {
        path: path.to_owned(),
        source,
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(io)?;
    }
    // Write a sibling file, then rename over the old one, so a crash
    // mid-write can never leave a half-written config behind.
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    std::fs::write(&tmp, text).map_err(io)?;
    std::fs::rename(&tmp, path).map_err(io)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_stack() {
        assert_eq!(Settings::default().tld, ".stack");
    }

    #[test]
    fn round_trips_through_disk() {
        let dir = crate::testutil::tempdir();
        let path = dir.join("settings.toml");
        assert_eq!(Settings::load_from(&path).unwrap(), Settings::default());

        let mut settings = Settings::default();
        settings.set_tld("Test").unwrap();
        settings.save_to(&path).unwrap();
        assert_eq!(Settings::load_from(&path).unwrap().tld, ".test");
        assert!(!dir.join("settings.toml.tmp").exists());
    }

    #[test]
    fn modules_default_and_override() {
        let mut settings = Settings::default();
        assert!(settings.module_enabled("php"));
        assert!(!settings.module_enabled("mailpit"));
        assert!(!settings.module_enabled("redis"), "not available yet");
        assert!(!settings.module_enabled("nope"));

        settings.set_module("php", false).unwrap();
        settings.set_module("mailpit", true).unwrap();
        assert!(!settings.module_enabled("php"));
        assert!(settings.module_enabled("mailpit"));

        settings.set_module("php", true).unwrap();
        assert!(
            !settings.modules.contains_key("php"),
            "defaults are not stored"
        );
        assert!(settings.set_module("redis", true).is_err());
        assert!(settings.set_module("nope", false).is_err());
    }
}
