//! `.stackyard` files: a project pins its runtime versions with one, e.g.
//!
//! ```toml
//! php = "8.3"
//! node = "22"
//! ```
//!
//! The nearest file that names a runtime wins, so a monorepo can pin Node at
//! the top and PHP in one sub-project.
//!
//! The version files other tools use count too (`.nvmrc`, `.node-version`,
//! `.php-version`, `.tool-versions` and so on), so a project that already
//! has one needs nothing new. In the same folder `.stackyard` wins, and a
//! version Stackyard does not offer (`lts/*`, `18`) in one of those files is
//! skipped rather than an error: the file is written for another tool.

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

/// Looks for `kind` in `.stackyard` and other version files from `dir` up
/// to the root.
pub fn pinned(dir: &Path, kind: RuntimeKind) -> Result<Option<Pin>> {
    for folder in dir.ancestors() {
        if let Some(pin) = pinned_here(folder, kind)? {
            return Ok(Some(pin));
        }
    }
    Ok(None)
}

/// The version `folder` itself pins, not looking further up.
fn pinned_here(folder: &Path, kind: RuntimeKind) -> Result<Option<Pin>> {
    if let Some(pin) = stackyard_pin(folder, kind)? {
        return Ok(Some(pin));
    }
    for name in version_files(kind) {
        let file = folder.join(name);
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        let written = if *name == ".tool-versions" {
            tool_versions_entry(&text, kind)
        } else {
            text.lines()
                .map(str::trim)
                .find(|l| !l.is_empty() && !l.starts_with('#'))
        };
        if let Some(version) = written.and_then(|w| offered_line(kind, w)) {
            return Ok(Some(Pin { version, file }));
        }
    }
    Ok(None)
}

/// Version files other tools use for `kind`, most specific first.
fn version_files(kind: RuntimeKind) -> &'static [&'static str] {
    match kind {
        RuntimeKind::Php => &[".php-version", ".tool-versions"],
        RuntimeKind::Node => &[".nvmrc", ".node-version", ".tool-versions"],
        RuntimeKind::Python => &[".python-version", ".tool-versions"],
        RuntimeKind::Go => &[".go-version", ".tool-versions"],
        RuntimeKind::Java => &[".java-version", ".sdkmanrc", ".tool-versions"],
    }
}

/// `nodejs 22.11.0` in asdf's and mise's `.tool-versions` → `22.11.0`.
fn tool_versions_entry(text: &str, kind: RuntimeKind) -> Option<&str> {
    let names: &[&str] = match kind {
        RuntimeKind::Php => &["php"],
        RuntimeKind::Node => &["nodejs", "node"],
        RuntimeKind::Python => &["python"],
        RuntimeKind::Go => &["golang", "go"],
        RuntimeKind::Java => &["java"],
    };
    text.lines().find_map(|line| {
        let line = line.split('#').next()?;
        let mut words = line.split_whitespace();
        let name = words.next()?;
        names.contains(&name).then(|| words.next()).flatten()
    })
}

/// The release line Stackyard offers that `written` falls in:
/// `v22.11.0` → `22`, `8.3.12` → `8.3`, `java=21.0.2-tem` → `21`.
fn offered_line(kind: RuntimeKind, written: &str) -> Option<String> {
    let written = written.rsplit('=').next()?.trim();
    let written = written.strip_prefix('v').unwrap_or(written);
    catalog::runtime(kind)
        .versions
        .iter()
        .find(|line| {
            written == **line
                || written
                    .strip_prefix(**line)
                    .is_some_and(|rest| rest.starts_with(['.', '-', '+']))
        })
        .map(|line| line.to_string())
}

fn stackyard_pin(folder: &Path, kind: RuntimeKind) -> Result<Option<Pin>> {
    let file = folder.join(PIN_FILE);
    let Some(table) = read(&file)? else {
        return Ok(None);
    };
    let Some(value) = table.get(kind.id()) else {
        return Ok(None);
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
    Ok(Some(Pin { version, file }))
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

/// Removes `kind` from `dir/.stackyard`, and the file once nothing is left
/// in it. Version files of other tools are left alone.
pub fn unpin(dir: &Path, kind: RuntimeKind) -> Result<()> {
    let file = dir.join(PIN_FILE);
    let Some(mut table) = read(&file)? else {
        return Ok(());
    };
    if table.remove(kind.id()).is_none() {
        return Ok(());
    }
    if table.is_empty() {
        std::fs::remove_file(&file).map_err(|source| Error::Io { path: file, source })
    } else {
        write_toml(&file, &table)
    }
}

fn read(file: &Path) -> Result<Option<toml::Table>> {
    match std::fs::read_to_string(file) {
        Ok(text) => text.parse().map(Some).map_err(|source| Error::Parse {
            path: file.to_owned(),
            source: Box::new(source),
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
    fn reads_the_version_files_of_other_tools() {
        let root = crate::testutil::tempdir();
        let app = root.join("app");
        std::fs::create_dir_all(&app).unwrap();
        let node = |dir: &Path| pinned(dir, RuntimeKind::Node).unwrap();

        std::fs::write(
            root.join(".tool-versions"),
            "# tools\nnodejs 20.18.1\nphp 8.2.27\n",
        )
        .unwrap();
        assert_eq!(node(&app).unwrap().version, "20");
        let php = pinned(&app, RuntimeKind::Php).unwrap().unwrap();
        assert_eq!(
            (php.version.as_str(), php.file),
            ("8.2", root.join(".tool-versions"))
        );

        std::fs::write(app.join(".nvmrc"), "v22.11.0\n").unwrap();
        assert_eq!(node(&app).unwrap().version, "22");
        assert_eq!(node(&app).unwrap().file, app.join(".nvmrc"));

        // Not a version Stackyard offers: skipped, the next one up counts.
        std::fs::write(app.join(".nvmrc"), "lts/*\n").unwrap();
        assert_eq!(node(&app).unwrap().version, "20");
        std::fs::write(app.join(".nvmrc"), "2\n").unwrap();
        assert_eq!(node(&app).unwrap().version, "20", "2 is not 22");

        // `.stackyard` wins in the same folder.
        std::fs::write(app.join(".nvmrc"), "22\n").unwrap();
        pin(&app, RuntimeKind::Node, "24").unwrap();
        assert_eq!(node(&app).unwrap().version, "24");

        std::fs::write(app.join(".php-version"), "8.4\n").unwrap();
        assert_eq!(
            pinned(&app, RuntimeKind::Php).unwrap().unwrap().version,
            "8.4"
        );
        std::fs::write(app.join(".sdkmanrc"), "java=21.0.2-tem\n").unwrap();
        assert_eq!(
            pinned(&app, RuntimeKind::Java).unwrap().unwrap().version,
            "21"
        );
    }

    #[test]
    fn unpinning_removes_the_entry_then_the_file() {
        let dir = crate::testutil::tempdir();
        pin(&dir, RuntimeKind::Php, "8.3").unwrap();
        pin(&dir, RuntimeKind::Node, "22").unwrap();
        unpin(&dir, RuntimeKind::Php).unwrap();
        assert!(pinned(&dir, RuntimeKind::Php).unwrap().is_none());
        assert!(pinned(&dir, RuntimeKind::Node).unwrap().is_some());
        unpin(&dir, RuntimeKind::Node).unwrap();
        assert!(!dir.join(PIN_FILE).exists());
        unpin(&dir, RuntimeKind::Node).unwrap();
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
