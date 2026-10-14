//! User settings, stored as TOML.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::{domain, paths, Error, Result};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Domain ending for new sites, e.g. `.stack`.
    pub tld: String,
    /// Expose the MCP server to AI coding tools (step 4).
    pub mcp_enabled: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            tld: domain::DEFAULT_TLD.to_string(),
            mcp_enabled: false,
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
    std::fs::write(path, text).map_err(io)
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
    }
}
