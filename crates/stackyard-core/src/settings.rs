//! User settings, stored as TOML.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::catalog::RuntimeKind;
use crate::{domain, modules, paths, Error, Result};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Domain ending for new sites, e.g. `.stack`.
    pub tld: String,
    /// Modules the user switched on or off; anything missing uses its default.
    pub modules: BTreeMap<String, bool>,
    /// Default version per runtime, e.g. `php = "8.4"`, used by sites and
    /// the shell when a project does not pin its own.
    pub runtimes: BTreeMap<String, String>,
    /// Parked folders: every project folder inside one becomes a site.
    pub parked: Vec<PathBuf>,
    pub mcp: McpSettings,
}

/// What AI tools may do through the MCP server.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct McpSettings {
    /// Action tools the user turned on, e.g. `start_service`; every other
    /// action tool is off. Tools that only read are always on.
    pub allow: BTreeSet<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            tld: domain::DEFAULT_TLD.to_string(),
            modules: BTreeMap::new(),
            runtimes: BTreeMap::new(),
            parked: Vec::new(),
            mcp: McpSettings::default(),
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
            Some(def) if def.available_here() => {
                self.modules.get(id).copied().unwrap_or(def.on_by_default)
            }
            _ => false,
        }
    }

    /// Fails with a clear message when a module is switched off.
    pub fn require_module(&self, id: &str) -> Result<()> {
        if self.module_enabled(id) {
            return Ok(());
        }
        let def = modules::find(id).ok_or_else(|| Error::UnknownModule(id.to_string()))?;
        Err(unavailable(def).unwrap_or_else(|| Error::ModuleOff {
            name: def.name.to_string(),
        }))
    }

    pub fn set_module(&mut self, id: &str, on: bool) -> Result<()> {
        let def = modules::find(id).ok_or_else(|| Error::UnknownModule(id.to_string()))?;
        if let Some(err) = unavailable(def).filter(|_| on) {
            return Err(err);
        }
        if on == def.on_by_default {
            self.modules.remove(id);
        } else {
            self.modules.insert(id.to_string(), on);
        }
        Ok(())
    }
}

impl Settings {
    pub fn default_runtime(&self, kind: RuntimeKind) -> Option<&str> {
        self.runtimes.get(kind.id()).map(String::as_str)
    }

    /// Sets or clears the default version. Callers check it is installed.
    pub fn set_default_runtime(&mut self, kind: RuntimeKind, version: Option<&str>) {
        match version {
            Some(v) => self.runtimes.insert(kind.id().to_string(), v.to_string()),
            None => self.runtimes.remove(kind.id()),
        };
    }
}

impl Settings {
    /// Parks a folder (`~` is expanded). Parking it twice changes nothing.
    pub fn park(&mut self, folder: &Path) -> Result<PathBuf> {
        let folder = full_path(folder);
        if !folder.is_dir() {
            return Err(Error::NotAFolder(folder));
        }
        if !self.parked.contains(&folder) {
            self.parked.push(folder.clone());
        }
        Ok(folder)
    }

    pub fn unpark(&mut self, folder: &Path) -> Result<()> {
        let folder = full_path(folder);
        let before = self.parked.len();
        self.parked.retain(|p| *p != folder);
        if self.parked.len() == before {
            return Err(Error::NotParked(folder));
        }
        Ok(())
    }
}

/// Why a module cannot be switched on here, if it cannot.
fn unavailable(def: &modules::ModuleDef) -> Option<Error> {
    if !def.available {
        Some(Error::ModuleUnavailable(def.name.to_string()))
    } else if !def.available_here() {
        Some(Error::NotOnThisSystem {
            name: def.name.to_string(),
            system: crate::platform::Platform::current().os.to_string(),
        })
    } else {
        None
    }
}

/// Reads a TOML file, returning `None` when it does not exist yet.
/// `~/Code` or `Code` (from the current folder) as a full path.
fn full_path(folder: &Path) -> PathBuf {
    let folder = paths::expand_home(folder);
    std::path::absolute(&folder).unwrap_or(folder)
}

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
        assert!(matches!(
            settings.require_module("php"),
            Err(Error::ModuleOff { .. })
        ));
        assert!(settings.require_module("mailpit").is_ok());

        settings.set_module("php", true).unwrap();
        assert!(
            !settings.modules.contains_key("php"),
            "defaults are not stored"
        );
        assert!(settings.set_module("redis", true).is_err());
        assert!(settings.set_module("nope", false).is_err());
    }

    #[test]
    fn default_runtimes() {
        let mut settings = Settings::default();
        assert_eq!(settings.default_runtime(RuntimeKind::Php), None);
        settings.set_default_runtime(RuntimeKind::Php, Some("8.4"));
        assert_eq!(settings.default_runtime(RuntimeKind::Php), Some("8.4"));
        let text = toml::to_string(&settings).unwrap();
        assert!(text.contains("php = \"8.4\""), "{text}");
        settings.set_default_runtime(RuntimeKind::Php, None);
        assert_eq!(settings.default_runtime(RuntimeKind::Php), None);
    }
}
