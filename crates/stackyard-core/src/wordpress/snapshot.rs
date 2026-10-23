//! Snapshots of a WordPress copy, so a merge, push, pull or restore that
//! changed it can be undone.
//!
//! A snapshot holds the copy's whole database and the files a step is
//! about to change: [`Snapshot::save_file`] is called for each file before
//! it is overwritten or deleted, and [`Snapshot::note_added`] for each new
//! one. Undoing loads the database back, puts the saved files back and
//! removes the added ones, so files the step never touched are left alone.
//!
//! The *base* snapshot of a staging is different: taken when the staging
//! is made, it lists every file (size, date, and a checksum for everything
//! outside the media folder) and keeps the database as it was then. Merges
//! compare both sides against it to tell "changed here" from "changed there".
//!
//! Kept under `wordpress/snapshots/<domain>/<id>/` in Stackyard's home;
//! the last [`KEEP`] snapshots of each copy are kept, besides the base.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{env, WpSite};
use crate::databases::Client;
use crate::{Error, Result};

pub const KEEP: usize = 5;
pub const BASE: &str = "base";
const DATABASE: &str = "database.sql.gz";

fn io(path: &Path) -> impl FnOnce(std::io::Error) -> Error + '_ {
    move |source| Error::Io {
        path: path.to_owned(),
        source,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    pub id: String,
    pub domain: String,
    /// What it was taken before, e.g. "Merge from checkout.shop.stack".
    pub reason: String,
    pub created_at: u64,
    /// Files saved or added since it was taken, relative to the site's folder.
    #[serde(default)]
    pub saved: Vec<String>,
    #[serde(default)]
    pub added: Vec<String>,
    #[serde(default)]
    pub bytes: u64,
}

/// One file in a base snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileEntry {
    pub size: u64,
    pub modified: i64,
    /// SHA-256, for files outside the media folder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hash: Option<String>,
}

/// Every file of a copy, by path relative to its folder (`/`-separated).
pub type Manifest = BTreeMap<String, FileEntry>;

pub fn dir(home: &Path, domain: &str) -> PathBuf {
    home.join("wordpress").join("snapshots").join(domain)
}

fn now_id() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}{:03}", now.as_secs(), now.subsec_millis())
}

/// Folders a manifest leaves out: caches, made again by the site.
const NOT_LISTED: &[&str] = &[
    "wp-content/cache",
    "wp-content/et-cache",
    ".git",
    "node_modules",
];

/// Whether a path is media, compared by size and date only.
pub fn is_media(rel: &str) -> bool {
    rel.starts_with("wp-content/uploads/")
}

/// Lists every file under `root` with its size, date and (outside the
/// media folder) checksum.
pub fn manifest(root: &Path) -> Result<Manifest> {
    let mut out = Manifest::new();
    let mut folders = vec![String::new()];
    while let Some(rel) = folders.pop() {
        let dir = root.join(&rel);
        for entry in std::fs::read_dir(&dir).map_err(io(&dir))?.flatten() {
            let Ok(name) = entry.file_name().into_string() else {
                continue;
            };
            let path = if rel.is_empty() {
                name
            } else {
                format!("{rel}/{name}")
            };
            let Ok(meta) = std::fs::symlink_metadata(entry.path()) else {
                continue;
            };
            if meta.is_dir() {
                if !NOT_LISTED.contains(&path.as_str()) {
                    folders.push(path);
                }
            } else if meta.is_file() {
                out.insert(path.clone(), entry_of(&entry.path(), &path, &meta)?);
            }
        }
    }
    Ok(out)
}

pub fn entry_of(file: &Path, rel: &str, meta: &std::fs::Metadata) -> Result<FileEntry> {
    Ok(FileEntry {
        size: meta.len(),
        modified: crate::remote::mtime_of(meta).unwrap_or(0),
        hash: if is_media(rel) {
            None
        } else {
            Some(hash_file(file)?)
        },
    })
}

pub fn hash_file(file: &Path) -> Result<String> {
    let mut reader = std::fs::File::open(file).map_err(io(file))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 256 * 1024];
    loop {
        let n = reader.read(&mut buf).map_err(io(file))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

impl Snapshot {
    fn folder(&self, home: &Path) -> PathBuf {
        dir(home, &self.domain).join(&self.id)
    }

    fn write(&self, home: &Path) -> Result<()> {
        let file = self.folder(home).join("snapshot.json");
        let text = serde_json::to_vec_pretty(self)
            .map_err(|e| Error::WordPress(format!("could not save the snapshot: {e}")))?;
        std::fs::write(&file, text).map_err(io(&file))
    }

    /// Keeps a copy of a file before it is overwritten or deleted; a file
    /// saved once keeps its first copy.
    pub fn save_file(&mut self, home: &Path, root: &Path, rel: &str) -> Result<()> {
        if self.saved.iter().any(|s| s == rel) || self.added.iter().any(|s| s == rel) {
            return Ok(());
        }
        let from = root.join(rel);
        if !from.is_file() {
            return self.note_added(home, rel);
        }
        let to = self.folder(home).join("files").join(rel);
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent).map_err(io(parent))?;
        }
        self.bytes += super::copy::copy_file(&from, &to).map_err(io(&to))?;
        self.saved.push(rel.to_string());
        self.write(home)
    }

    /// Records a file the step adds, so undoing removes it.
    pub fn note_added(&mut self, home: &Path, rel: &str) -> Result<()> {
        if !self.added.iter().any(|s| s == rel) {
            self.added.push(rel.to_string());
            self.write(home)?;
        }
        Ok(())
    }

    pub fn manifest(&self, home: &Path) -> Result<Manifest> {
        let file = self.folder(home).join("files.json");
        let bytes = std::fs::read(&file).map_err(io(&file))?;
        serde_json::from_slice(&bytes)
            .map_err(|e| Error::WordPress(format!("{} is damaged: {e}", file.display())))
    }

    /// The database as it was, gzipped.
    pub fn database(&self, home: &Path) -> PathBuf {
        self.folder(home).join(DATABASE)
    }
}

/// Takes a snapshot of the copy's database; files are added by the step
/// with [`Snapshot::save_file`]. Older snapshots beyond [`KEEP`] are removed.
pub fn take(home: &Path, wp: &WpSite, reason: &str) -> Result<Snapshot> {
    take_as(home, wp, reason, &now_id(), false)
}

/// Takes a staging's base snapshot: its database and a list of every file.
pub fn take_base(home: &Path, wp: &WpSite) -> Result<Snapshot> {
    let _ = std::fs::remove_dir_all(dir(home, &wp.site.domain()).join(BASE));
    take_as(home, wp, "Made the staging", BASE, true)
}

fn take_as(home: &Path, wp: &WpSite, reason: &str, id: &str, list: bool) -> Result<Snapshot> {
    let snapshot = Snapshot {
        id: id.to_string(),
        domain: wp.site.domain(),
        reason: reason.to_string(),
        created_at: env::now(),
        saved: Vec::new(),
        added: Vec::new(),
        bytes: 0,
    };
    let folder = snapshot.folder(home);
    std::fs::create_dir_all(&folder).map_err(io(&folder))?;
    let result = (|| {
        let database = snapshot.database(home);
        Client::open(home)?.dump_to(&wp.database()?, &[], &database)?;
        let mut snapshot = snapshot.clone();
        snapshot.bytes = std::fs::metadata(&database).map(|m| m.len()).unwrap_or(0);
        if list {
            let files = manifest(&wp.site.path)?;
            let path = folder.join("files.json");
            let text = serde_json::to_vec(&files)
                .map_err(|e| Error::WordPress(format!("could not list the files: {e}")))?;
            std::fs::write(&path, text).map_err(io(&path))?;
        }
        snapshot.write(home)?;
        Ok(snapshot)
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&folder);
    } else if !list {
        prune(home, &wp.site.domain());
    }
    result
}

/// The copy's snapshots, newest first; the base snapshot is not among them.
pub fn list(home: &Path, domain: &str) -> Vec<Snapshot> {
    let mut all: Vec<Snapshot> = std::fs::read_dir(dir(home, domain))
        .map(|entries| {
            entries
                .flatten()
                .filter(|e| e.file_name() != BASE)
                .filter_map(|e| std::fs::read(e.path().join("snapshot.json")).ok())
                .filter_map(|b| serde_json::from_slice(&b).ok())
                .collect()
        })
        .unwrap_or_default();
    all.sort_by(|a, b| b.id.cmp(&a.id));
    all
}

pub fn get(home: &Path, domain: &str, id: &str) -> Result<Snapshot> {
    let file = dir(home, domain).join(id).join("snapshot.json");
    let bytes = std::fs::read(&file).map_err(|_| {
        Error::WordPress(format!(
            "no snapshot {id} of {domain}; it may have been removed"
        ))
    })?;
    serde_json::from_slice(&bytes)
        .map_err(|e| Error::WordPress(format!("{} is damaged: {e}", file.display())))
}

/// A staging's base snapshot, when it has one.
pub fn base(home: &Path, domain: &str) -> Option<Snapshot> {
    get(home, domain, BASE).ok()
}

fn prune(home: &Path, domain: &str) {
    for old in list(home, domain).into_iter().skip(KEEP) {
        let _ = std::fs::remove_dir_all(old.folder(home));
    }
}

pub fn remove(home: &Path, domain: &str, id: &str) -> Result<()> {
    if id == BASE || id.contains(['/', '\\', '.']) {
        return Err(Error::WordPress(format!("{id} can't be removed")));
    }
    let folder = dir(home, domain).join(id);
    std::fs::remove_dir_all(&folder).map_err(io(&folder))
}

/// Puts the copy back as it was when the snapshot was taken: the database,
/// the files the step changed or removed, and without the files it added.
/// A snapshot of the state before restoring is taken first, so restoring
/// can itself be undone.
pub fn restore(home: &Path, wp: &WpSite, id: &str) -> Result<Snapshot> {
    let snapshot = get(home, &wp.site.domain(), id)?;
    let before = take(
        home,
        wp,
        &format!(
            "Before restoring the snapshot of {}",
            time_text(snapshot.created_at)
        ),
    )?;
    let mut before = before;
    let root = &wp.site.path;
    for rel in &snapshot.added {
        before.save_file(home, root, rel)?;
        let path = root.join(rel);
        if path.is_file() {
            std::fs::remove_file(&path).map_err(io(&path))?;
        }
    }
    let saved = snapshot.folder(home).join("files");
    for rel in &snapshot.saved {
        before.save_file(home, root, rel)?;
        let to = root.join(rel);
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent).map_err(io(parent))?;
        }
        super::copy::copy_file(&saved.join(rel), &to).map_err(io(&to))?;
    }
    let client = Client::open(home)?;
    let database = wp.database()?;
    client.drop(&database)?;
    client.load(&database, &snapshot.database(home))?;
    Ok(before)
}

/// A date and time for a snapshot's description, in UTC.
fn time_text(at: u64) -> String {
    let days = at / 86_400;
    let secs = at % 86_400;
    // Days since 1970 to a calendar date (Howard Hinnant's method).
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year}-{month:02}-{day:02} {:02}:{:02} UTC",
        secs / 3600,
        secs % 3600 / 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_files_with_checksums_outside_media() {
        let root = crate::testutil::tempdir();
        std::fs::create_dir_all(root.join("wp-content/uploads/2026")).unwrap();
        std::fs::create_dir_all(root.join("wp-content/cache")).unwrap();
        std::fs::write(root.join("index.php"), "<?php").unwrap();
        std::fs::write(root.join("wp-content/uploads/2026/a.jpg"), "jpg").unwrap();
        std::fs::write(root.join("wp-content/cache/page.html"), "x").unwrap();
        let files = manifest(&root).unwrap();
        assert_eq!(files.len(), 2);
        assert!(files["index.php"].hash.is_some());
        assert_eq!(files["wp-content/uploads/2026/a.jpg"].hash, None);
        assert_eq!(files["wp-content/uploads/2026/a.jpg"].size, 3);
    }

    #[test]
    fn saves_files_once_and_notes_new_ones() {
        let home = crate::testutil::tempdir();
        let root = crate::testutil::tempdir();
        std::fs::write(root.join("a.php"), "old").unwrap();
        let mut snapshot = Snapshot {
            id: "1".into(),
            domain: "shop.stack".into(),
            reason: "test".into(),
            created_at: 0,
            saved: Vec::new(),
            added: Vec::new(),
            bytes: 0,
        };
        std::fs::create_dir_all(snapshot.folder(&home)).unwrap();
        snapshot.save_file(&home, &root, "a.php").unwrap();
        std::fs::write(root.join("a.php"), "new").unwrap();
        snapshot.save_file(&home, &root, "a.php").unwrap();
        snapshot.save_file(&home, &root, "b.php").unwrap();
        assert_eq!(snapshot.saved, vec!["a.php"]);
        assert_eq!(snapshot.added, vec!["b.php"]);
        let kept = snapshot.folder(&home).join("files/a.php");
        assert_eq!(std::fs::read_to_string(kept).unwrap(), "old");
        assert_eq!(get(&home, "shop.stack", "1").unwrap(), snapshot);
        assert_eq!(list(&home, "shop.stack").len(), 1);
        assert!(remove(&home, "shop.stack", BASE).is_err());
    }

    #[test]
    fn writes_dates() {
        assert_eq!(time_text(0), "1970-01-01 00:00 UTC");
        assert_eq!(time_text(1_792_704_017), "2026-10-22 21:20 UTC");
    }
}
