//! `.stackyard` files: a project pins its runtime versions with one, e.g.
//!
//! ```toml
//! php = "8.3"
//! node = "22"
//! ```
//!
//! The nearest file that names a runtime wins, so a monorepo can pin Node at
//! the top and PHP in one sub-project.

use std::path::{Path, PathBuf};

use crate::catalog::{self, RuntimeKind};
use crate::settings::write_toml;
use crate::{Error, Result};

pub const PIN_FILE: &str = ".stackyard";

/// A version pinned for a project, and the file that pins it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pin {
    pub version: String,
    pub file: PathBuf,
}

/// Looks for `kind` in `.stackyard` files from `dir` up to the root.
pub fn pinned(dir: &Path, kind: RuntimeKind) -> Result<Option<Pin>> {
    for folder in dir.ancestors() {
        let file = folder.join(PIN_FILE);
        let Some(table) = read(&file)? else {
            continue;
        };
        let Some(value) = table.get(kind.id()) else {
            continue;
        };
        let version = match value {
            toml::Value::String(s) => s.trim().to_string(),
            // `node = 22` or `php = 8.3` are easy slips; accept them.
            toml::Value::Integer(n) => n.to_string(),
            toml::Value::Float(f) => format!("{f:?}"),
            _ => return Err(bad_pin(&file, kind, &value.to_string())),
        };
        if !catalog::runtime(kind).offers(&version) {
            return Err(bad_pin(&file, kind, &version));
        }
        return Ok(Some(Pin { version, file }));
    }
    Ok(None)
}

/// Pins `kind` to `version` in `dir/.stackyard`, keeping anything else in it.
pub fn pin(dir: &Path, kind: RuntimeKind, version: &str) -> Result<PathBuf> {
    let family = catalog::runtime(kind);
    if !family.offers(version) {
        return Err(Error::UnknownVersion {
            name: family.name.into(),
            version: version.into(),
        });
    }
    let file = dir.join(PIN_FILE);
    let mut table = read(&file)?.unwrap_or_default();
    table.insert(kind.id().into(), toml::Value::String(version.into()));
    write_toml(&file, &table)?;
    Ok(file)
}

fn read(file: &Path) -> Result<Option<toml::Table>> {
    match std::fs::read_to_string(file) {
        Ok(text) => text.parse().map(Some).map_err(|source| Error::Parse {
            path: file.to_owned(),
            source,
        }),
        // A folder named `.stackyard`, or one we may not read, is not a pin file.
        Err(_) => Ok(None),
    }
}

fn bad_pin(file: &Path, kind: RuntimeKind, value: &str) -> Error {
    let family = catalog::runtime(kind);
    Error::BadPin {
        file: file.to_owned(),
        message: format!(
            "{} {value} is not offered; use one of {}",
            family.name,
            family.versions.join(", ")
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearest_file_naming_the_runtime_wins() {
        let root = crate::testutil::tempdir();
        let app = root.join("apps/api");
        std::fs::create_dir_all(&app).unwrap();
        std::fs::write(root.join(PIN_FILE), "node = \"22\"\nphp = \"8.2\"\n").unwrap();
        std::fs::write(app.join(PIN_FILE), "php = \"8.4\"\n").unwrap();

        let php = pinned(&app, RuntimeKind::Php).unwrap().unwrap();
        assert_eq!(php.version, "8.4");
        assert_eq!(php.file, app.join(PIN_FILE));
        let node = pinned(&app, RuntimeKind::Node).unwrap().unwrap();
        assert_eq!(node.version, "22");
        assert_eq!(node.file, root.join(PIN_FILE));
        assert_eq!(
            pinned(&root, RuntimeKind::Php).unwrap().unwrap().version,
            "8.2"
        );
    }

    #[test]
    fn rejects_versions_stackyard_does_not_offer() {
        let dir = crate::testutil::tempdir();
        std::fs::write(dir.join(PIN_FILE), "php = \"5.6\"\n").unwrap();
        let err = pinned(&dir, RuntimeKind::Php).unwrap_err().to_string();
        assert!(err.contains("PHP 5.6 is not offered"), "{err}");

        std::fs::write(dir.join(PIN_FILE), "node = 22\n").unwrap();
        assert_eq!(
            pinned(&dir, RuntimeKind::Node).unwrap().unwrap().version,
            "22"
        );
    }

    #[test]
    fn pinning_keeps_other_entries() {
        let dir = crate::testutil::tempdir();
        std::fs::write(dir.join(PIN_FILE), "node = \"22\"\n# note\n").unwrap();
        pin(&dir, RuntimeKind::Php, "8.3").unwrap();
        assert_eq!(
            pinned(&dir, RuntimeKind::Node).unwrap().unwrap().version,
            "22"
        );
        assert_eq!(
            pinned(&dir, RuntimeKind::Php).unwrap().unwrap().version,
            "8.3"
        );
        assert!(pin(&dir, RuntimeKind::Php, "9.9").is_err());
    }
}
