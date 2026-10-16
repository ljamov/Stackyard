//! First-run setup: where Stackyard keeps its files, where new sites go,
//! and what to install. Choosing a new folder for an existing installation
//! moves it there on the next start, before anything opens a file in it.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::catalog::{self, RuntimeKind};
use crate::modules::{self, ModuleGroup};
use crate::paths::{self, Location};
use crate::settings::{self, Settings};
use crate::{Error, Result};

/// What the user picked; kept in the location file until setup finishes,
/// so it survives the restart a move needs.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Choices {
    pub sites_folder: PathBuf,
    /// Module ids to switch on and install: services, `php`, `node`.
    pub install: Vec<String>,
}

/// Something setup offers to install.
#[derive(Debug, Clone, Serialize)]
pub struct Offer {
    pub id: &'static str,
    pub name: &'static str,
    pub group: ModuleGroup,
    pub description: &'static str,
    /// Runtimes: the version setup installs.
    pub version: Option<&'static str>,
    /// Ticked to begin with.
    pub recommended: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Status {
    /// Setup has not finished yet.
    pub needed: bool,
    /// Where Stackyard keeps its files now.
    pub home: PathBuf,
    /// Suggested folders.
    pub default_home: PathBuf,
    pub default_sites_folder: PathBuf,
    /// An earlier installation's files, which move along if another folder is chosen.
    pub existing: Option<PathBuf>,
    /// Picked before a restart, waiting to be applied.
    pub choices: Option<Choices>,
    pub move_error: Option<String>,
    pub offers: Vec<Offer>,
}

/// The Node.js line setup installs: the current long-term support one.
const NODE_LTS: &str = "24";
const RECOMMENDED: &[&str] = &["caddy", "mysql", "phpmyadmin", "mailpit", "php", "node"];

pub fn status() -> Status {
    let location = Location::load();
    let home = paths::home();
    let settings = Settings::load_from(&home.join("settings.toml")).unwrap_or_default();
    let needed =
        std::env::var_os("STACKYARD_HOME").is_none() && !location.as_ref().is_some_and(|l| l.done);
    let default_home = match &location {
        Some(l) => l.home.clone(),
        None => paths::default_home(),
    };
    Status {
        needed,
        existing: has_installation(&home).then(|| home.clone()),
        default_sites_folder: match &location {
            Some(Location {
                choices: Some(c), ..
            }) => c.sites_folder.clone(),
            _ if settings.sites_folder.is_some() => settings.sites_folder(&home),
            _ => settings::default_sites_folder(&default_home),
        },
        default_home,
        home,
        choices: location.as_ref().and_then(|l| l.choices.clone()),
        move_error: location.and_then(|l| l.move_error),
        offers: offers(),
    }
}

fn offers() -> Vec<Offer> {
    let runtime = |kind: RuntimeKind| {
        let versions = catalog::runtime(kind).versions;
        match kind {
            RuntimeKind::Node if versions.contains(&NODE_LTS) => Some(NODE_LTS),
            _ => versions.first().copied(),
        }
    };
    modules::MODULES
        .iter()
        .filter(|m| m.available_here())
        .filter_map(|m| {
            let version = match m.group {
                ModuleGroup::Service if catalog::service(m.id).is_some() => None,
                // Listed with the services: it is what you open to manage them.
                ModuleGroup::Feature if m.id == crate::phpmyadmin::MODULE => None,
                ModuleGroup::Runtime => Some(runtime(RuntimeKind::from_id(m.id)?)?),
                _ => return None,
            };
            Some(Offer {
                id: m.id,
                name: m.name,
                group: match m.group {
                    ModuleGroup::Feature => ModuleGroup::Service,
                    group => group,
                },
                description: m.description,
                version,
                recommended: RECOMMENDED.contains(&m.id),
            })
        })
        .collect()
}

/// Saves the user's picks. Returns `true` when Stackyard has to restart to
/// use another folder (and move its files there first); otherwise call
/// [`apply`] next.
pub fn choose(home: &Path, choices: Choices) -> Result<bool> {
    if !home.is_absolute() || home.parent().is_none() {
        return Err(Error::Setup(format!(
            "pick a folder of its own for Stackyard, not {}",
            home.display()
        )));
    }
    if !choices.sites_folder.is_absolute() {
        return Err(Error::Setup(format!(
            "pick a full path for the sites folder, not {}",
            choices.sites_folder.display()
        )));
    }
    let current = paths::home();
    let restart = !same_path(&current, home);
    let moving = !same_path(&current, home) && has_files(&current);
    if moving && home.starts_with(&current) {
        return Err(Error::Setup(format!(
            "{} is inside Stackyard's current folder; pick a folder outside it",
            home.display()
        )));
    }
    if !moving {
        std::fs::create_dir_all(home).map_err(|source| Error::Io {
            path: home.to_owned(),
            source,
        })?;
    }
    Location {
        home: home.to_owned(),
        move_from: moving.then_some(current),
        move_error: None,
        choices: Some(choices),
        done: false,
    }
    .save()?;
    Ok(restart)
}

/// Turns the picked modules on (and the other services and runtimes off),
/// creates the sites folder and parks it, so every folder in it is a site.
pub fn apply(choices: &Choices) -> Result<()> {
    let home = paths::home();
    let folder = &choices.sites_folder;
    std::fs::create_dir_all(folder).map_err(|source| Error::Io {
        path: folder.clone(),
        source,
    })?;
    let mut settings = Settings::load()?;
    settings.sites_folder =
        (*folder != settings::default_sites_folder(&home)).then(|| folder.clone());
    settings.park(folder)?;
    for offer in offers() {
        settings.set_module(offer.id, choices.install.iter().any(|id| id == offer.id))?;
    }
    settings.save()
}

/// Marks setup as finished so it is not shown again.
pub fn finish() -> Result<()> {
    let mut location = Location::load().unwrap_or_else(|| Location {
        home: paths::home(),
        ..Location::default()
    });
    location.done = true;
    location.choices = None;
    location.move_error = None;
    location.save()
}

/// Run first thing on start: moves the files when setup chose a new folder.
/// Returns the old and new folder after a move, so the caller can point
/// the terminal commands at the new place. When the move fails, Stackyard
/// stays in the old folder and setup shows why.
pub fn move_if_pending() -> Option<(PathBuf, PathBuf)> {
    if std::env::var_os("STACKYARD_HOME").is_some() {
        return None;
    }
    let mut location = Location::load()?;
    let from = location.move_from.take()?;
    let to = location.home.clone();
    let moved = move_when_free(&from, &to);
    if let Err(e) = &moved {
        location.home = from.clone();
        location.move_error = Some(format!(
            "Could not move Stackyard's files from {} to {}: {e}. They stay where they are. \
             Restart your computer, then click Next and Install to try again.",
            from.display(),
            to.display()
        ));
    }
    if location.save().is_err() || moved.is_err() {
        return None;
    }
    fix_config_paths(&to, &from);
    Some((from, to))
}

/// Windows can't move a file another program has open. Stackyard's own
/// programs (a database server still running, a terminal command an AI tool
/// keeps open) are stopped first, and the Stackyard that restarted to get
/// here may still be closing, so a file in use is tried again for a while.
fn move_when_free(from: &Path, to: &Path) -> std::io::Result<()> {
    stop_programs_in(from);
    let mut waits = [250, 500, 1000, 2000, 3000, 4000, 5000].into_iter();
    loop {
        match move_folder(from, to) {
            Err(e) if in_use(&e) => match waits.next() {
                Some(ms) => std::thread::sleep(std::time::Duration::from_millis(ms)),
                None => return Err(e),
            },
            result => return result,
        }
    }
}

/// Sharing and lock violations, and the access error Windows gives for a
/// folder that is open in another program.
fn in_use(e: &std::io::Error) -> bool {
    let code = match e.get_ref().and_then(|inner| inner.downcast_ref::<OnFile>()) {
        Some(on_file) => on_file.source.raw_os_error(),
        None => e.raw_os_error(),
    };
    matches!(code, Some(5 | 32 | 33))
}

/// Stops programs running from inside `folder`: they are ones Stackyard
/// installed there, and they hold their own files open.
#[cfg(windows)]
fn stop_programs_in(folder: &Path) {
    let script = "$f = $env:STACKYARD_MOVE_FROM.TrimEnd('\\') + '\\'; \
        Get-Process | Where-Object { $_.Id -ne [int]$env:STACKYARD_PID -and $_.Path -and \
        $_.Path.StartsWith($f, [StringComparison]::OrdinalIgnoreCase) } | \
        Stop-Process -Force -ErrorAction SilentlyContinue";
    let _ = crate::system::powershell_command(script)
        .env("STACKYARD_MOVE_FROM", folder)
        .env("STACKYARD_PID", std::process::id().to_string())
        .status();
}

#[cfg(not(windows))]
fn stop_programs_in(_folder: &Path) {}

fn move_folder(from: &Path, to: &Path) -> std::io::Result<()> {
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // An empty folder can be replaced; one with files in it is merged into.
    if to.is_dir() && std::fs::read_dir(to)?.next().is_none() {
        std::fs::remove_dir(to)?;
    }
    if !to.exists() && std::fs::rename(from, to).is_ok() {
        return Ok(());
    }
    // Another drive, or the folder already has files: move entry by entry,
    // and put back what was moved if one of them can't go.
    std::fs::create_dir_all(to)?;
    let mut moved = Vec::new();
    let result = (|| {
        for entry in std::fs::read_dir(from)? {
            let entry = entry?;
            let target = to.join(entry.file_name());
            if target.exists() {
                return Err(std::io::Error::other(format!(
                    "{} already exists",
                    target.display()
                )));
            }
            if std::fs::rename(entry.path(), &target).is_err() {
                if let Err(e) = copy_all(&entry.path(), &target) {
                    let _ = remove_all(&target);
                    return Err(e);
                }
                if let Err(e) = remove_all(&entry.path()) {
                    let _ = remove_all(&target);
                    return Err(naming(e, &entry.path()));
                }
            }
            moved.push(entry.file_name());
        }
        Ok(())
    })();
    if result.is_err() {
        for name in moved {
            if std::fs::rename(to.join(&name), from.join(&name)).is_err() {
                let _ = copy_all(&to.join(&name), &from.join(&name))
                    .and_then(|()| remove_all(&to.join(&name)));
            }
        }
        return result;
    }
    let _ = std::fs::remove_dir(from);
    Ok(())
}

/// An error about one file, which keeps the original so a file in use can
/// still be told apart.
#[derive(Debug)]
struct OnFile {
    source: std::io::Error,
    path: PathBuf,
}

impl std::fmt::Display for OnFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.source, self.path.display())
    }
}

impl std::error::Error for OnFile {}

fn naming(source: std::io::Error, path: &Path) -> std::io::Error {
    std::io::Error::new(
        source.kind(),
        OnFile {
            source,
            path: path.to_owned(),
        },
    )
}

fn copy_all(from: &Path, to: &Path) -> std::io::Result<()> {
    if std::fs::symlink_metadata(from)?.is_dir() {
        std::fs::create_dir_all(to)?;
        for entry in std::fs::read_dir(from)? {
            let entry = entry?;
            copy_all(&entry.path(), &to.join(entry.file_name()))?;
        }
        Ok(())
    } else {
        std::fs::copy(from, to)
            .map(|_| ())
            .map_err(|e| naming(e, from))
    }
}

fn remove_all(path: &Path) -> std::io::Result<()> {
    if std::fs::symlink_metadata(path)?.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    }
}

/// Generated settings files written once (php.ini) name the old folder.
fn fix_config_paths(home: &Path, old: &Path) {
    let Ok(versions) = std::fs::read_dir(home.join("config").join("php")) else {
        return;
    };
    let forms = |p: &Path| {
        let plain = p.display().to_string();
        let slashed = plain.replace('\\', "/");
        [plain, slashed]
    };
    let (old_forms, new_forms) = (forms(old), forms(home));
    for version in versions.flatten() {
        let ini = version.path().join("php.ini");
        let Ok(text) = std::fs::read_to_string(&ini) else {
            continue;
        };
        let mut fixed = text.clone();
        for (o, n) in old_forms.iter().zip(&new_forms) {
            fixed = fixed.replace(o.as_str(), n);
        }
        if fixed != text {
            let _ = std::fs::write(&ini, fixed);
        }
    }
}

/// Settings, runtimes or services from an earlier run.
fn has_installation(home: &Path) -> bool {
    ["settings.toml", "sites.toml", "runtimes", "services"]
        .iter()
        .any(|name| home.join(name).exists())
}

fn has_files(dir: &Path) -> bool {
    std::fs::read_dir(dir).is_ok_and(|mut d| d.next().is_some())
}

fn same_path(a: &Path, b: &Path) -> bool {
    let norm = |p: &Path| {
        let s = p.display().to_string().replace('\\', "/");
        let s = s.trim_end_matches('/').to_string();
        if cfg!(windows) {
            s.to_lowercase()
        } else {
            s
        }
    };
    norm(a) == norm(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A program left running from the old folder used to stop the move
    /// with "the process cannot access the file" (os error 32).
    #[cfg(windows)]
    #[test]
    fn moves_a_folder_a_program_is_running_from() {
        let base = crate::testutil::tempdir();
        let from = base.join("old");
        let to = base.join("new");
        std::fs::create_dir_all(from.join("bin")).unwrap();
        let program = from.join("bin").join("ping.exe");
        std::fs::copy(r"C:\Windows\System32\PING.EXE", &program).unwrap();
        let mut running = std::process::Command::new(&program)
            .args(["-n", "60", "127.0.0.1"])
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(500));
        move_when_free(&from, &to).unwrap();
        assert!(to.join("bin").join("ping.exe").is_file());
        assert!(!from.exists());
        assert!(running.try_wait().unwrap().is_some(), "still running");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn a_file_in_use_is_tried_again_and_named() {
        let busy = naming(
            std::io::Error::from_raw_os_error(32),
            Path::new("old/logs/caddy.log"),
        );
        assert!(in_use(&busy));
        assert!(busy.to_string().contains("caddy.log"), "{busy}");
        assert!(in_use(&std::io::Error::from_raw_os_error(32)));
        let missing = naming(
            std::io::Error::from(std::io::ErrorKind::NotFound),
            Path::new("x"),
        );
        assert!(!in_use(&missing));
    }

    #[test]
    fn moves_a_folder_and_fixes_php_ini() {
        let base = crate::testutil::tempdir();
        let from = base.join("old");
        let to = base.join("new").join("stackyard");
        std::fs::create_dir_all(from.join("config/php/8.4")).unwrap();
        std::fs::write(
            from.join("config/php/8.4/php.ini"),
            format!(
                "extension_dir = \"{}/runtimes/php/8.4/ext\"\n",
                from.display()
            ),
        )
        .unwrap();
        std::fs::write(from.join("settings.toml"), "tld = \".stack\"\n").unwrap();

        move_folder(&from, &to).unwrap();
        fix_config_paths(&to, &from);
        assert!(!from.exists());
        assert!(to.join("settings.toml").is_file());
        let ini = std::fs::read_to_string(to.join("config/php/8.4/php.ini")).unwrap();
        assert!(ini.contains(&to.display().to_string()), "{ini}");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn merges_into_a_folder_with_other_files_but_never_overwrites() {
        let base = crate::testutil::tempdir();
        let from = base.join("old");
        let to = base.join("new");
        std::fs::create_dir_all(from.join("logs")).unwrap();
        std::fs::create_dir_all(to.join("www")).unwrap();
        move_folder(&from, &to).unwrap();
        assert!(to.join("logs").is_dir() && to.join("www").is_dir());

        std::fs::create_dir_all(from.join("logs")).unwrap();
        assert!(move_folder(&from, &to).is_err());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn offers_services_and_runtimes_only() {
        let offers = offers();
        assert!(offers.iter().any(|o| o.id == "caddy" && o.recommended));
        let php = offers.iter().find(|o| o.id == "php").unwrap();
        assert!(php.version.is_some());
        assert!(!offers.iter().any(|o| o.id == "logs" || o.id == "python"));
    }

    #[test]
    fn paths_compare_loosely() {
        assert!(same_path(Path::new("/a/b/"), Path::new("/a/b")));
        assert!(!same_path(Path::new("/a/b"), Path::new("/a/c")));
    }
}
