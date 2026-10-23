//! Working out which files differ between a site's folder and its folder on
//! a server, then copying them one way.
//!
//! Which files count as changed is up to [`Compare`]. By default a file is
//! copied when its size differs, or when the side being copied from is newer
//! (by more than two seconds, since FTP servers round times). Transfers keep
//! modification times on both sides, so a file that was just copied is seen
//! as the same next time.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::{is_relative_inside, join, mtime_of, Remote};
use crate::{Error, Result};

/// More files than any site should have; stops a wrong folder (say `/`)
/// from being walked for ever.
const MAX_FILES: usize = 200_000;
/// Seconds two modification times may differ by and still count as the same.
const SLACK: i64 = 2;
/// Failures in a row after which the server is taken to be gone.
const MAX_FAILURES_IN_A_ROW: usize = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    /// From the site's folder to the server.
    Upload,
    /// From the server to the site's folder.
    Download,
}

/// Which files a transfer copies, like the comparison settings of an FTP
/// program. Files only on the side copied from are copied in every mode.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Compare {
    /// Different size, or newer on the side copied from.
    #[default]
    Changed,
    /// Newer on the side copied from; size is not looked at.
    Newer,
    /// Different size or a different date, whichever side is newer.
    Different,
    /// Different size; dates are not looked at.
    Size,
    /// Only files missing on the other side; nothing is overwritten.
    Missing,
    /// Every file, even those that look the same.
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Action {
    Upload,
    Download,
    DeleteRemote,
    DeleteLocal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Reason {
    /// Only on the side copied from.
    New,
    /// On both sides, but different.
    Changed,
    /// Only on the side copied to, and deleting was asked for.
    Extra,
    /// Picked by hand.
    Picked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlannedFile {
    /// Relative to the connection's folders, `/`-separated.
    pub path: String,
    pub action: Action,
    pub reason: Reason,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Plan {
    pub direction: Direction,
    pub files: Vec<PlannedFile>,
    /// Bytes to copy.
    pub bytes: u64,
    /// Files that are the same on both sides.
    pub unchanged: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "step", rename_all = "lowercase")]
pub enum Progress {
    /// Looking through folders; `found` files so far.
    Scanning { folder: String, found: usize },
    /// Copying file `index` (from 0) of `total`; `done` of its `size` bytes so far.
    Transfer {
        index: usize,
        total: usize,
        path: String,
        done: u64,
        size: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Failure {
    pub path: String,
    pub error: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
pub struct Outcome {
    /// Files copied or deleted.
    pub done: usize,
    pub bytes: u64,
    pub failed: Vec<Failure>,
    /// Stopped by hand, or after too many failures in a row.
    pub stopped: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FileInfo {
    pub size: u64,
    pub modified: Option<i64>,
}

pub(crate) struct Sides<'a> {
    pub local: &'a Path,
    pub remote: &'a str,
    pub skip: &'a [String],
}

fn stopped() -> Error {
    Error::Remote("stopped".into())
}

pub(crate) fn plan(
    remote: &mut dyn Remote,
    sides: &Sides,
    direction: Direction,
    compare: Compare,
    delete: bool,
    only: Option<&[String]>,
    progress: &mut dyn FnMut(Progress) -> bool,
) -> Result<Plan> {
    let mut files = Vec::new();
    let mut unchanged = 0;
    if let Some(picked) = only {
        for rel in picked {
            let rel = rel.trim_matches('/');
            if !is_relative_inside(rel) {
                return Err(Error::Remote(format!(
                    "{rel:?} is outside this connection's folder"
                )));
            }
            let found = match direction {
                Direction::Upload => walk_local(sides.local, rel, sides.skip, progress)?,
                Direction::Download => {
                    walk_remote(remote, sides.remote, rel, sides.skip, progress)?
                }
            };
            let action = match direction {
                Direction::Upload => Action::Upload,
                Direction::Download => Action::Download,
            };
            files.extend(found.into_iter().map(|(path, info)| PlannedFile {
                path,
                action,
                reason: Reason::Picked,
                size: info.size,
            }));
        }
    } else {
        let local = walk_local(sides.local, "", sides.skip, progress)?;
        let server = walk_remote(remote, sides.remote, "", sides.skip, progress)?;
        let (from, to, copy, remove) = match direction {
            Direction::Upload => (&local, &server, Action::Upload, Action::DeleteRemote),
            Direction::Download => (&server, &local, Action::Download, Action::DeleteLocal),
        };
        for (path, source) in from {
            let reason = match to.get(path) {
                None => Some(Reason::New),
                Some(target) if differs(source, target, compare) => Some(Reason::Changed),
                Some(_) => None,
            };
            match reason {
                Some(reason) => files.push(PlannedFile {
                    path: path.clone(),
                    action: copy,
                    reason,
                    size: source.size,
                }),
                None => unchanged += 1,
            }
        }
        if delete {
            files.extend(to.iter().filter(|(path, _)| !from.contains_key(*path)).map(
                |(path, info)| PlannedFile {
                    path: path.clone(),
                    action: remove,
                    reason: Reason::Extra,
                    size: info.size,
                },
            ));
        }
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    files.dedup_by(|a, b| a.path == b.path);
    let bytes = files
        .iter()
        .filter(|f| matches!(f.action, Action::Upload | Action::Download))
        .map(|f| f.size)
        .sum();
    Ok(Plan {
        direction,
        files,
        bytes,
        unchanged,
    })
}

/// Whether the source file should replace the target.
fn differs(source: &FileInfo, target: &FileInfo, compare: Compare) -> bool {
    let sized = source.size != target.size;
    let (newer, dated) = match (source.modified, target.modified) {
        (Some(s), Some(t)) => (s > t + SLACK, (s - t).abs() > SLACK),
        _ => (false, false),
    };
    match compare {
        Compare::Changed => sized || newer,
        Compare::Newer => newer,
        Compare::Different => sized || dated,
        Compare::Size => sized,
        Compare::Missing => false,
        Compare::All => true,
    }
}

/// Whether a relative path is left out by the skip list. A pattern without
/// `/` matches a file or folder name anywhere (`*` and `?` work); one with
/// `/` matches that path from the top of the folder.
pub(crate) fn skipped(rel: &str, skip: &[String]) -> bool {
    skip.iter().any(|pattern| {
        if pattern.contains('/') {
            rel == pattern || rel.starts_with(&format!("{pattern}/"))
        } else {
            rel.split('/').any(|name| wildcard(pattern, name))
        }
    })
}

fn wildcard(pattern: &str, name: &str) -> bool {
    let (p, n): (Vec<char>, Vec<char>) = (pattern.chars().collect(), name.chars().collect());
    let (mut pi, mut ni) = (0, 0);
    let (mut star, mut mark) = (None, 0);
    while ni < n.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == n[ni]) {
            pi += 1;
            ni += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ni;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ni = mark;
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|c| *c == '*')
}

fn rel_join(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        name.to_string()
    } else {
        format!("{dir}/{name}")
    }
}

/// Files under `root/start`, by path relative to `root`.
fn walk_local(
    root: &Path,
    start: &str,
    skip: &[String],
    progress: &mut dyn FnMut(Progress) -> bool,
) -> Result<BTreeMap<String, FileInfo>> {
    let mut found = BTreeMap::new();
    let first = root.join(start);
    let meta = match std::fs::metadata(&first) {
        Ok(meta) => meta,
        Err(_) if start.is_empty() => return Ok(found),
        Err(source) => {
            return Err(Error::Io {
                path: first,
                source,
            })
        }
    };
    if meta.is_file() {
        found.insert(
            start.to_string(),
            FileInfo {
                size: meta.len(),
                modified: mtime_of(&meta),
            },
        );
        return Ok(found);
    }
    let mut folders = vec![start.to_string()];
    while let Some(rel) = folders.pop() {
        if !progress(Progress::Scanning {
            folder: rel.clone(),
            found: found.len(),
        }) {
            return Err(stopped());
        }
        let dir = root.join(&rel);
        let entries = std::fs::read_dir(&dir).map_err(super::io_error(&dir))?;
        for entry in entries.flatten() {
            let Ok(name) = entry.file_name().into_string() else {
                continue;
            };
            let child = rel_join(&rel, &name);
            if skipped(&child, skip) || name.ends_with(".part") {
                continue;
            }
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            // Follow links to files, never to folders, which could loop.
            let meta = if kind.is_symlink() {
                match std::fs::metadata(entry.path()) {
                    Ok(m) if m.is_file() => m,
                    _ => continue,
                }
            } else {
                match entry.metadata() {
                    Ok(m) => m,
                    Err(_) => continue,
                }
            };
            if meta.is_dir() {
                folders.push(child);
            } else if meta.is_file() {
                found.insert(
                    child,
                    FileInfo {
                        size: meta.len(),
                        modified: mtime_of(&meta),
                    },
                );
            }
        }
        if found.len() > MAX_FILES {
            return Err(Error::Remote(format!(
                "{} has more than {MAX_FILES} files; pick a smaller folder",
                root.display()
            )));
        }
    }
    Ok(found)
}

/// Files under `root/start` on the server, by path relative to `root`.
pub(crate) fn walk_remote(
    remote: &mut dyn Remote,
    root: &str,
    start: &str,
    skip: &[String],
    progress: &mut dyn FnMut(Progress) -> bool,
) -> Result<BTreeMap<String, FileInfo>> {
    let mut found = BTreeMap::new();
    if !start.is_empty() {
        // A single file: find it in its folder's listing.
        let (parent, name) = start.rsplit_once('/').unwrap_or(("", start));
        let listing = remote.list(&join(root, parent))?;
        match listing.iter().find(|e| e.name == name) {
            Some(entry) if !entry.dir => {
                found.insert(
                    start.to_string(),
                    FileInfo {
                        size: entry.size,
                        modified: entry.modified,
                    },
                );
                return Ok(found);
            }
            Some(_) => {}
            None => return Err(Error::Remote(format!("{start} is not on the server"))),
        }
    }
    let mut folders = vec![start.to_string()];
    while let Some(rel) = folders.pop() {
        if !progress(Progress::Scanning {
            folder: rel.clone(),
            found: found.len(),
        }) {
            return Err(stopped());
        }
        let entries = match remote.list(&join(root, &rel)) {
            Ok(entries) => entries,
            // Nothing uploaded yet: the folder is made on the first upload.
            Err(_) if rel.is_empty() => return Ok(found),
            Err(e) => return Err(e),
        };
        for entry in entries {
            if entry.name == "." || entry.name == ".." || entry.name.contains('/') {
                continue;
            }
            let child = rel_join(&rel, &entry.name);
            if skipped(&child, skip) {
                continue;
            }
            if entry.dir {
                folders.push(child);
            } else {
                found.insert(
                    child,
                    FileInfo {
                        size: entry.size,
                        modified: entry.modified,
                    },
                );
            }
        }
        if found.len() > MAX_FILES {
            return Err(Error::Remote(format!(
                "{root} has more than {MAX_FILES} files; pick a smaller folder"
            )));
        }
    }
    Ok(found)
}

pub(crate) fn run(
    remote: &mut dyn Remote,
    local_root: &Path,
    remote_root: &str,
    files: &[PlannedFile],
    progress: &mut dyn FnMut(Progress) -> bool,
) -> Result<Outcome> {
    let mut outcome = Outcome::default();
    let mut made: HashSet<String> = HashSet::new();
    let mut in_a_row = 0;
    if files.iter().any(|f| f.action == Action::Upload) {
        make_remote_dirs(remote, remote_root, "", &mut made);
    }
    for (index, file) in files.iter().enumerate() {
        let path = file.path.trim_matches('/');
        if path.is_empty() || !is_relative_inside(path) {
            return Err(Error::Remote(format!(
                "{:?} is outside this connection's folder",
                file.path
            )));
        }
        let mut stop = false;
        let mut report = |done: u64| {
            let go = progress(Progress::Transfer {
                index,
                total: files.len(),
                path: path.to_string(),
                done,
                size: file.size,
            });
            stop |= !go;
            go
        };
        if !report(0) {
            outcome.stopped = true;
            break;
        }
        let local = local_root.join(path);
        let target = join(remote_root, path);
        let result = match file.action {
            Action::Upload => {
                let parent = path.rsplit_once('/').map_or("", |(p, _)| p);
                make_remote_dirs(remote, remote_root, parent, &mut made);
                remote.upload(&local, &target, &mut report)
            }
            Action::Download => {
                let made_parent = match local.parent() {
                    Some(parent) => {
                        std::fs::create_dir_all(parent).map_err(super::io_error(parent))
                    }
                    None => Ok(()),
                };
                made_parent.and_then(|_| remote.download(&target, &local, &mut report))
            }
            Action::DeleteRemote => remote.remove_file(&target),
            Action::DeleteLocal => std::fs::remove_file(&local).map_err(super::io_error(&local)),
        };
        if stop {
            outcome.stopped = true;
            break;
        }
        match result {
            Ok(()) => {
                in_a_row = 0;
                outcome.done += 1;
                if matches!(file.action, Action::Upload | Action::Download) {
                    outcome.bytes += file.size;
                }
            }
            Err(e) => {
                in_a_row += 1;
                outcome.failed.push(Failure {
                    path: path.to_string(),
                    error: e.to_string(),
                });
                if in_a_row >= MAX_FAILURES_IN_A_ROW {
                    outcome.stopped = true;
                    break;
                }
            }
        }
    }
    Ok(outcome)
}

/// Makes `root/rel` and each folder above it, below `root`, on the server.
/// Failures are left for the upload to report.
fn make_remote_dirs(remote: &mut dyn Remote, root: &str, rel: &str, made: &mut HashSet<String>) {
    if made.insert(String::new()) {
        // The connection's own folder, and the folders above it.
        let mut path = String::new();
        for part in root.split('/').filter(|p| !p.is_empty()) {
            path = format!("{path}/{part}");
            let _ = remote.make_dir(&path);
        }
    }
    let mut so_far = String::new();
    for part in rel.split('/').filter(|p| !p.is_empty()) {
        so_far = rel_join(&so_far, part);
        if made.insert(so_far.clone()) {
            let _ = remote.make_dir(&join(root, &so_far));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::{Entry, OnBytes};
    use std::collections::HashMap;
    use std::path::PathBuf;

    /// A server kept in memory: path → (contents, modified).
    #[derive(Default)]
    struct Memory {
        files: HashMap<String, (Vec<u8>, i64)>,
        dirs: HashSet<String>,
    }

    impl Remote for Memory {
        fn list(&mut self, dir: &str) -> Result<Vec<Entry>> {
            if dir != "/" && !self.dirs.contains(dir) {
                return Err(Error::Remote(format!("{dir}: no such folder")));
            }
            let prefix = if dir == "/" {
                "/".to_string()
            } else {
                format!("{dir}/")
            };
            let mut entries = Vec::new();
            for (path, (data, at)) in &self.files {
                if let Some(name) = path.strip_prefix(&prefix).filter(|n| !n.contains('/')) {
                    entries.push(Entry {
                        name: name.into(),
                        dir: false,
                        size: data.len() as u64,
                        modified: Some(*at),
                    });
                }
            }
            for path in &self.dirs {
                if let Some(name) = path.strip_prefix(&prefix).filter(|n| !n.contains('/')) {
                    entries.push(Entry {
                        name: name.into(),
                        dir: true,
                        size: 0,
                        modified: None,
                    });
                }
            }
            Ok(entries)
        }

        fn download(&mut self, remote: &str, local: &Path, on_bytes: OnBytes) -> Result<()> {
            let (data, at) = self.files.get(remote).cloned().unwrap();
            std::fs::write(local, &data).unwrap();
            super::super::set_mtime(
                &std::fs::File::options().write(true).open(local).unwrap(),
                at,
            );
            on_bytes(data.len() as u64);
            Ok(())
        }

        fn upload(&mut self, local: &Path, remote: &str, on_bytes: OnBytes) -> Result<()> {
            let parent = remote.rsplit_once('/').unwrap().0;
            if !parent.is_empty() && !self.dirs.contains(parent) {
                return Err(Error::Remote(format!("{parent}: no such folder")));
            }
            let data = std::fs::read(local).unwrap();
            let at = mtime_of(&std::fs::metadata(local).unwrap()).unwrap();
            on_bytes(data.len() as u64);
            self.files.insert(remote.into(), (data, at));
            Ok(())
        }

        fn make_dir(&mut self, dir: &str) -> Result<()> {
            self.dirs.insert(dir.into());
            Ok(())
        }

        fn remove_file(&mut self, path: &str) -> Result<()> {
            self.files.remove(path);
            Ok(())
        }

        fn start_dir(&mut self) -> Result<String> {
            Ok("/".into())
        }
    }

    fn write(root: &Path, rel: &str, data: &str) -> PathBuf {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, data).unwrap();
        path
    }

    fn sides<'a>(local: &'a Path, skip: &'a [String]) -> Sides<'a> {
        Sides {
            local,
            remote: "/srv/site",
            skip,
        }
    }

    fn go(_: Progress) -> bool {
        true
    }

    fn paths(plan: &Plan) -> Vec<(&str, Action, Reason)> {
        plan.files
            .iter()
            .map(|f| (f.path.as_str(), f.action, f.reason))
            .collect()
    }

    #[test]
    fn uploads_then_sees_nothing_changed() {
        let local = crate::testutil::tempdir();
        write(&local, "index.php", "<?php echo 1;");
        write(&local, "css/app.css", "body{}");
        write(&local, ".git/HEAD", "ref");
        write(&local, "node_modules/x/i.js", "x");
        let skip = super::super::default_skip();
        let mut server = Memory::default();

        let plan = plan(
            &mut server,
            &sides(&local, &skip),
            Direction::Upload,
            Compare::Changed,
            false,
            None,
            &mut go,
        )
        .unwrap();
        assert_eq!(
            paths(&plan),
            [
                ("css/app.css", Action::Upload, Reason::New),
                ("index.php", Action::Upload, Reason::New)
            ]
        );
        assert_eq!(plan.bytes, 19);
        let outcome = run(&mut server, &local, "/srv/site", &plan.files, &mut go).unwrap();
        assert_eq!(outcome.done, 2);
        assert!(outcome.failed.is_empty(), "{:?}", outcome.failed);
        assert!(server.files.contains_key("/srv/site/css/app.css"));

        let again = super::plan(
            &mut server,
            &sides(&local, &skip),
            Direction::Upload,
            Compare::Changed,
            false,
            None,
            &mut go,
        )
        .unwrap();
        assert!(again.files.is_empty());
        assert_eq!(again.unchanged, 2);

        write(&local, "index.php", "<?php echo 22;");
        let changed = super::plan(
            &mut server,
            &sides(&local, &skip),
            Direction::Upload,
            Compare::Changed,
            false,
            None,
            &mut go,
        )
        .unwrap();
        assert_eq!(
            paths(&changed),
            [("index.php", Action::Upload, Reason::Changed)]
        );
    }

    #[test]
    fn downloads_and_deletes_extras_only_when_asked() {
        let local = crate::testutil::tempdir();
        write(&local, "old.txt", "gone on the server");
        let mut server = Memory::default();
        server.dirs.extend([
            "/srv".to_string(),
            "/srv/site".into(),
            "/srv/site/img".into(),
        ]);
        server
            .files
            .insert("/srv/site/img/a.png".into(), (vec![1, 2, 3], 1_700_000_000));
        let skip = Vec::new();

        let plan = plan(
            &mut server,
            &sides(&local, &skip),
            Direction::Download,
            Compare::Changed,
            false,
            None,
            &mut go,
        )
        .unwrap();
        assert_eq!(paths(&plan), [("img/a.png", Action::Download, Reason::New)]);
        let with_delete = super::plan(
            &mut server,
            &sides(&local, &skip),
            Direction::Download,
            Compare::Changed,
            true,
            None,
            &mut go,
        )
        .unwrap();
        assert_eq!(
            paths(&with_delete),
            [
                ("img/a.png", Action::Download, Reason::New),
                ("old.txt", Action::DeleteLocal, Reason::Extra)
            ]
        );
        let outcome = run(
            &mut server,
            &local,
            "/srv/site",
            &with_delete.files,
            &mut go,
        )
        .unwrap();
        assert_eq!(outcome.done, 2);
        assert_eq!(std::fs::read(local.join("img/a.png")).unwrap(), [1, 2, 3]);
        assert!(!local.join("old.txt").exists());
        let meta = std::fs::metadata(local.join("img/a.png")).unwrap();
        assert_eq!(mtime_of(&meta), Some(1_700_000_000));
    }

    #[test]
    fn picked_paths_go_even_when_unchanged() {
        let local = crate::testutil::tempdir();
        write(&local, "a/one.txt", "1");
        write(&local, "a/two.txt", "2");
        write(&local, "b.txt", "b");
        let mut server = Memory::default();
        let only = vec!["a".to_string()];
        let plan = plan(
            &mut server,
            &sides(&local, &[]),
            Direction::Upload,
            Compare::Changed,
            true,
            Some(&only),
            &mut go,
        )
        .unwrap();
        assert_eq!(
            paths(&plan),
            [
                ("a/one.txt", Action::Upload, Reason::Picked),
                ("a/two.txt", Action::Upload, Reason::Picked)
            ]
        );
        let outside = vec!["../x".to_string()];
        assert!(super::plan(
            &mut server,
            &sides(&local, &[]),
            Direction::Upload,
            Compare::Changed,
            false,
            Some(&outside),
            &mut go
        )
        .is_err());
    }

    #[test]
    fn refuses_paths_outside_the_folder() {
        let local = crate::testutil::tempdir();
        let mut server = Memory::default();
        let bad = [PlannedFile {
            path: "../../etc/passwd".into(),
            action: Action::Download,
            reason: Reason::Picked,
            size: 1,
        }];
        assert!(run(&mut server, &local, "/srv/site", &bad, &mut go).is_err());
    }

    #[test]
    fn skip_patterns() {
        let skip: Vec<String> = ["*.log", "storage/cache", ".env"]
            .map(String::from)
            .to_vec();
        assert!(skipped("storage/logs/laravel.log", &skip));
        assert!(skipped("storage/cache/x/y", &skip));
        assert!(skipped(".env", &skip));
        assert!(!skipped(".env.example", &skip));
        assert!(!skipped("storage/cachet", &skip));
        assert!(!skipped("app/Log.php", &skip));
        assert!(wildcard("a?c*", "abcdef"));
        assert!(!wildcard("a?c", "abcd"));
    }

    #[test]
    fn a_newer_target_is_left_alone() {
        let a = FileInfo {
            size: 3,
            modified: Some(100),
        };
        assert!(!differs(
            &a,
            &FileInfo {
                size: 3,
                modified: Some(200)
            },
            Compare::Changed,
        ));
        assert!(!differs(
            &a,
            &FileInfo {
                size: 3,
                modified: Some(99)
            },
            Compare::Changed,
        ));
        assert!(differs(
            &FileInfo {
                size: 3,
                modified: Some(110)
            },
            &a,
            Compare::Changed
        ));
        assert!(differs(
            &a,
            &FileInfo {
                size: 4,
                modified: Some(100)
            },
            Compare::Changed,
        ));
        assert!(!differs(
            &a,
            &FileInfo {
                size: 3,
                modified: None
            },
            Compare::Changed,
        ));
    }

    #[test]
    fn each_compare_mode_picks_its_files() {
        let info = |size, modified| FileInfo {
            size,
            modified: Some(modified),
        };
        let same = (info(3, 100), info(3, 100));
        let bigger = (info(4, 100), info(3, 100));
        let newer = (info(3, 200), info(3, 100));
        let older = (info(3, 100), info(3, 200));
        let check = |compare, expect: [bool; 4]| {
            for ((s, t), want) in [same, bigger, newer, older].iter().zip(expect) {
                assert_eq!(differs(s, t, compare), want, "{compare:?} {s:?} {t:?}");
            }
        };
        check(Compare::Changed, [false, true, true, false]);
        check(Compare::Newer, [false, false, true, false]);
        check(Compare::Different, [false, true, true, true]);
        check(Compare::Size, [false, true, false, false]);
        check(Compare::Missing, [false, false, false, false]);
        check(Compare::All, [true, true, true, true]);
    }
}
