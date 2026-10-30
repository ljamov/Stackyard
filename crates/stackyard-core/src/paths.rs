//! Where Stackyard keeps its files.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Root folder for Stackyard's config, runtimes, services, data and logs.
///
/// `STACKYARD_HOME` overrides it (used by tests and portable installs).
/// Otherwise it is the folder chosen in first-run setup; else the OS data
/// folder when an older version already keeps its files there; else the
/// default: `C:\stackyard` on Windows, the OS data folder elsewhere (e.g.
/// `~/Library/Application Support/Stackyard`).
pub fn home() -> PathBuf {
    if let Some(dir) = std::env::var_os("STACKYARD_HOME") {
        return PathBuf::from(dir);
    }
    if let Some(location) = Location::load() {
        // Until the move has happened, the files are still in the old place.
        return location.move_from.unwrap_or(location.home);
    }
    let legacy = legacy_home();
    if legacy.join("settings.toml").is_file() {
        return legacy;
    }
    default_home()
}

/// Where Stackyard 0.1 kept everything: the OS data folder.
pub fn legacy_home() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Stackyard")
}

/// Where a fresh install keeps its files unless the user picks another folder.
pub fn default_home() -> PathBuf {
    if cfg!(windows) {
        let drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
        PathBuf::from(format!("{drive}\\stackyard"))
    } else {
        legacy_home()
    }
}

/// The folder chosen in first-run setup, kept outside that folder so it
/// can be found again. Absent until setup has run.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Location {
    pub home: PathBuf,
    /// The folder to move everything from on the next start, when setup
    /// picked a new place for an existing installation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub move_from: Option<PathBuf>,
    /// What went wrong moving, shown in setup; Stackyard stays where it was.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub move_error: Option<String>,
    /// What setup picked, applied once any move is done.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub choices: Option<crate::setup::Choices>,
    /// Set once setup has finished.
    #[serde(default)]
    pub done: bool,
}

impl Location {
    pub fn file() -> Option<PathBuf> {
        dirs::config_local_dir().map(|d| d.join("dev.stackyard.app").join("location.toml"))
    }

    pub fn load() -> Option<Self> {
        crate::settings::read_toml(&Self::file()?).ok().flatten()
    }

    pub fn save(&self) -> crate::Result<()> {
        let file = Self::file().ok_or_else(|| crate::Error::Io {
            path: PathBuf::from("location.toml"),
            source: std::io::Error::other("could not find the user's settings folder"),
        })?;
        crate::settings::write_toml(&file, self)
    }
}

pub fn settings_file() -> PathBuf {
    home().join("settings.toml")
}

pub fn sites_file() -> PathBuf {
    home().join("sites.toml")
}

pub fn logs_dir() -> PathBuf {
    home().join("logs")
}

/// `~/Code/app` → `/home/me/Code/app`; other paths are returned as they are.
pub fn expand_home(path: &Path) -> PathBuf {
    let Ok(rest) = path.strip_prefix("~") else {
        return path.to_owned();
    };
    match dirs::home_dir() {
        Some(home) => home.join(rest),
        None => path.to_owned(),
    }
}

/// Bytes free for this user on the drive that holds `path` (or the nearest
/// folder above it that exists); `None` when the system can't say.
pub fn free_space(path: &Path) -> Option<u64> {
    let existing = path.ancestors().find(|p| p.exists())?;
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
        let wide: Vec<u16> = existing
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let mut free = 0u64;
        // SAFETY: `wide` is a NUL-terminated path; the other outputs may be null.
        let ok = unsafe {
            GetDiskFreeSpaceExW(
                wide.as_ptr(),
                &mut free,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        (ok != 0).then_some(free)
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let c = std::ffi::CString::new(existing.as_os_str().as_bytes()).ok()?;
        // SAFETY: `c` is a NUL-terminated path and `stat` is written by statvfs.
        let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
        let ok = unsafe { libc::statvfs(c.as_ptr(), &mut stat) } == 0;
        #[allow(clippy::unnecessary_cast)]
        ok.then(|| stat.f_bavail as u64 * stat.f_frsize as u64)
    }
    #[cfg(not(any(windows, unix)))]
    {
        let _ = existing;
        None
    }
}
