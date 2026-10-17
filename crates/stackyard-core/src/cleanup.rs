//! Disk space: how much Stackyard's data folder takes, and what in it can
//! go. Only Stackyard's own folder is looked at, never project folders.
//!
//! [`scan`] finds removable things and changes nothing; [`clean`] scans
//! again and removes the items asked for by id, so an id that has gone
//! stale in the meantime is reported rather than guessed at.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::Serialize;

use crate::catalog::{self, RuntimeKind};
use crate::runtimes::RuntimeManager;
use crate::services::ServiceManager;
use crate::settings::Settings;
use crate::shell::pins;
use crate::{web, Error, Result};

/// Downloads and half-finished installs touched more recently than this
/// may belong to an install that is still running, so they are left alone.
const RECENT: Duration = Duration::from_secs(10 * 60);

/// Folder names the installer uses while it swaps or removes a folder.
const LEFTOVER_PREFIXES: [&str; 3] = [".staging-", ".old-", ".trash-"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Part {
    pub name: &'static str,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Item {
    /// Stable for the same thing between scans, e.g. `runtime-php-8.2`.
    pub id: String,
    pub title: String,
    pub detail: String,
    pub size: u64,
    /// Nothing anyone would miss; picked unless the person unpicks it.
    pub recommended: bool,
    /// Deletes data that no download brings back.
    pub permanent: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Report {
    /// The data folder's main parts, largest first.
    pub usage: Vec<Part>,
    pub total: u64,
    pub items: Vec<Item>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Outcome {
    pub freed: u64,
    /// Titles of what was cleaned.
    pub cleaned: Vec<String>,
    pub problems: Vec<String>,
}

enum Action {
    Delete(Vec<PathBuf>),
    /// Log files that stay in place, emptied, so whatever writes to them
    /// carries on.
    Empty(Vec<PathBuf>),
    Runtime(RuntimeKind, String),
}

struct Found {
    item: Item,
    action: Action,
}

pub fn scan(home: &Path) -> Report {
    let found = find(home, SystemTime::now());
    let mut usage: Vec<Part> = [
        ("Runtimes", "runtimes"),
        ("Services", "services"),
        ("Service data", "data"),
        ("Backups", "backups"),
        ("Logs", "logs"),
        ("Downloads", "downloads"),
    ]
    .into_iter()
    .map(|(name, dir)| Part {
        name,
        size: size_of(&home.join(dir)),
    })
    .filter(|p| p.size > 0)
    .collect();
    usage.sort_by_key(|p| std::cmp::Reverse(p.size));
    Report {
        total: size_of(home),
        usage,
        items: found.into_iter().map(|f| f.item).collect(),
    }
}

/// Removes the items with these ids. Each item is done on its own, so one
/// that can't be removed (a file in use, say) doesn't stop the rest.
pub fn clean(home: &Path, ids: &[String]) -> Outcome {
    let mut found = find(home, SystemTime::now());
    let mut outcome = Outcome::default();
    for id in ids {
        let Some(at) = found.iter().position(|f| &f.item.id == id) else {
            outcome
                .problems
                .push(format!("{id}: nothing to clean up there any more"));
            continue;
        };
        let Found { item, action } = found.swap_remove(at);
        match apply(home, action) {
            Ok(()) => {
                outcome.freed += item.size;
                outcome.cleaned.push(item.title);
            }
            Err(e) => outcome.problems.push(format!("{}: {e}", item.title)),
        }
    }
    outcome
}

fn apply(home: &Path, action: Action) -> Result<()> {
    match action {
        Action::Delete(paths) => {
            for path in paths {
                remove(&path)?;
            }
            Ok(())
        }
        Action::Empty(paths) => {
            for path in paths {
                let file = std::fs::OpenOptions::new()
                    .write(true)
                    .open(&path)
                    .and_then(|f| f.set_len(0));
                if let Err(source) = file {
                    if source.kind() != std::io::ErrorKind::NotFound {
                        return Err(Error::Io { path, source });
                    }
                }
            }
            Ok(())
        }
        Action::Runtime(kind, version) => {
            let manager = RuntimeManager::new(home);
            manager.uninstall(kind, &version)?;
            let file = home.join("settings.toml");
            let mut settings = Settings::load_from(&file)?;
            if manager.fix_default(&mut settings, kind) {
                settings.save_to(&file)?;
            }
            Ok(())
        }
    }
}

fn remove(path: &Path) -> Result<()> {
    let result = if path.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    };
    match result {
        Err(source) if source.kind() != std::io::ErrorKind::NotFound => Err(Error::Io {
            path: path.to_owned(),
            source,
        }),
        _ => Ok(()),
    }
}

fn find(home: &Path, now: SystemTime) -> Vec<Found> {
    let mut found = Vec::new();
    let settled = |path: &Path| {
        modified(path).is_none_or(|at| now.duration_since(at).is_ok_and(|age| age >= RECENT))
    };

    let downloads: Vec<PathBuf> = entries(&home.join("downloads"))
        .into_iter()
        .filter(|p| settled(p))
        .collect();
    push(
        &mut found,
        "downloads",
        "Leftover downloads",
        "Archives from installs that stopped part-way. Installs download again when needed.",
        true,
        false,
        downloads,
    );

    let leftovers: Vec<PathBuf> = ["runtimes", "services"]
        .iter()
        .flat_map(|root| entries(&home.join(root)))
        .flat_map(|family| entries(&family))
        .filter(|p| {
            let name = file_name(p);
            LEFTOVER_PREFIXES.iter().any(|x| name.starts_with(x)) && settled(p)
        })
        .collect();
    push(
        &mut found,
        "leftovers",
        "Half-finished installs",
        "Folders left behind when an install, update or removal was interrupted.",
        true,
        false,
        leftovers,
    );

    let logs = entries(&home.join("logs"));
    let rotated: Vec<PathBuf> = logs
        .iter()
        .filter(|p| file_name(p).ends_with(".log.1"))
        .cloned()
        .collect();
    push(
        &mut found,
        "old-logs",
        "Previous logs",
        "Older halves of logs that grew past their size limit.",
        true,
        false,
        rotated,
    );
    let current: Vec<PathBuf> = logs
        .iter()
        .filter(|p| {
            let name = file_name(p);
            name.ends_with(".log") || name.ends_with(".jsonl")
        })
        .cloned()
        .collect();
    let size = current.iter().map(|p| size_of(p)).sum();
    if size > 0 {
        found.push(Found {
            item: Item {
                id: "logs".into(),
                title: "Current logs".into(),
                detail: "Empties every log, including the record of what AI tools did. \
                         Services keep running and write new lines as usual."
                    .into(),
                size,
                recommended: false,
                permanent: false,
            },
            action: Action::Empty(current),
        });
    }

    runtime_versions(home, &mut found);
    retired_services(home, &mut found);

    // A service's data folder stays when the service is removed, so that
    // reinstalling brings its databases back. Once nobody wants that, it can go.
    let services = ServiceManager::new(home);
    for def in catalog::SERVICES {
        let data = services.data_dir(def.id);
        if services.get(def.id).is_some() || !data.exists() {
            continue;
        }
        let size = size_of(&data);
        found.push(Found {
            item: Item {
                id: format!("data-{}", def.id),
                title: format!("{} data", def.name),
                detail: if def.id == "caddy" {
                    "The web server is no longer installed. This holds the certificate \
                     authority your computer trusts for HTTPS; a new one would need trusting \
                     again with `stackyard system setup`."
                        .into()
                } else {
                    format!(
                        "{} is no longer installed, but its data was kept in case it comes \
                         back. Deleting it can't be undone.",
                        def.name
                    )
                },
                size,
                recommended: false,
                permanent: true,
            },
            action: Action::Delete(vec![data]),
        });
    }
    found
}

/// The runtime versions something relies on: each kind's default, every
/// site's version, and versions pinned in a site's folder. `None` when the
/// settings or sites can't be read, so that nothing is called unused.
fn used_versions(home: &Path) -> Option<Vec<(RuntimeKind, String)>> {
    let settings = Settings::load_from(&home.join("settings.toml")).ok()?;
    let sites = web::sites(home, &settings, &RuntimeManager::new(home)).ok()?;
    let mut used = Vec::new();
    for kind in RuntimeKind::ALL {
        if let Some(version) = settings.default_runtime(kind) {
            used.push((kind, version.to_string()));
        }
        for site in &sites {
            if site.site.runtime.kind() == Some(kind) {
                if let Some(version) = site.site.runtime.version() {
                    used.push((kind, version.to_string()));
                }
            }
            if let Ok(Some(pin)) = pins::pinned(&site.site.path, kind) {
                used.push((kind, pin.version));
            }
        }
    }
    Some(used)
}

/// Installed runtime versions nothing relies on, and versions Stackyard no
/// longer offers that are still on disk from an older Stackyard.
fn runtime_versions(home: &Path, found: &mut Vec<Found>) {
    let Some(used) = used_versions(home) else {
        return;
    };
    let is_used =
        |kind: RuntimeKind, version: &str| used.iter().any(|(k, v)| *k == kind && v == version);
    let runtimes = RuntimeManager::new(home);
    for installed in runtimes.installed() {
        let (kind, version) = (installed.kind, installed.version.as_str());
        if is_used(kind, version) {
            continue;
        }
        found.push(Found {
            item: Item {
                id: format!("runtime-{}-{version}", kind.id()),
                title: format!("{} {version}", catalog::runtime(kind).name),
                detail: format!(
                    "Installed ({}), but no site uses it and it isn't the default. \
                     A project pinned to it outside your sites would need it installed again.",
                    installed.release
                ),
                size: size_of(&installed.path),
                recommended: false,
                permanent: false,
            },
            action: Action::Runtime(kind, version.to_string()),
        });
    }
    for family in catalog::RUNTIMES {
        for dir in entries(&home.join("runtimes").join(family.kind.id())) {
            let version = file_name(&dir);
            if version.starts_with('.')
                || family.versions.contains(&version.as_str())
                || is_used(family.kind, &version)
            {
                continue;
            }
            found.push(Found {
                item: Item {
                    id: format!("runtime-{}-{version}", family.kind.id()),
                    title: format!("{} {version}", family.name),
                    detail: "Stackyard no longer offers this version, so it gets no updates, \
                             and no site uses it."
                        .into(),
                    size: size_of(&dir),
                    recommended: true,
                    permanent: false,
                },
                action: Action::Delete(vec![dir]),
            });
        }
    }
}

/// Service release lines Stackyard has moved on from, still on disk.
fn retired_services(home: &Path, found: &mut Vec<Found>) {
    for def in catalog::SERVICES {
        for dir in entries(&home.join("services").join(def.id)) {
            let line = file_name(&dir);
            if line.starts_with('.') || line == def.version {
                continue;
            }
            found.push(Found {
                item: Item {
                    id: format!("service-{}-{line}", def.id),
                    title: format!("{} {line}", def.name),
                    detail: format!(
                        "Replaced by {} {}. Its data is kept.",
                        def.name, def.version
                    ),
                    size: size_of(&dir),
                    recommended: true,
                    permanent: false,
                },
                action: Action::Delete(vec![dir]),
            });
        }
    }
}

fn push(
    found: &mut Vec<Found>,
    id: &str,
    title: &str,
    detail: &str,
    recommended: bool,
    permanent: bool,
    paths: Vec<PathBuf>,
) {
    let size: u64 = paths.iter().map(|p| size_of(p)).sum();
    if paths.is_empty() {
        return;
    }
    found.push(Found {
        item: Item {
            id: id.into(),
            title: title.into(),
            detail: detail.into(),
            size,
            recommended,
            permanent,
        },
        action: Action::Delete(paths),
    });
}

fn entries(dir: &Path) -> Vec<PathBuf> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = read.flatten().map(|e| e.path()).collect();
    paths.sort();
    paths
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::symlink_metadata(path).ok()?.modified().ok()
}

/// Bytes in a file or folder. Links are counted as themselves, never
/// followed, so nothing outside the folder is counted.
pub fn size_of(path: &Path) -> u64 {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return 0;
    };
    if !meta.is_dir() {
        return meta.len();
    }
    let mut total = 0;
    let mut stack = vec![path.to_owned()];
    while let Some(dir) = stack.pop() {
        let Ok(read) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in read.flatten() {
            let Ok(meta) = entry.metadata() else { continue };
            if meta.is_dir() {
                stack.push(entry.path());
            } else {
                total += meta.len();
            }
        }
    }
    total
}

#[cfg(test)]
mod tests;
