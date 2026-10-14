//! The shim files in `<home>/bin`: one per [`Tool`], each leading back to the
//! `stackyard` program. Symbolic links on macOS and Linux; on Windows, hard
//! links named `php.exe` and so on (copies if the folder is on another drive).

use std::path::{Path, PathBuf};

use super::Tool;
use crate::{Error, Result};

pub fn bin_dir(home: &Path) -> PathBuf {
    home.join("bin")
}

pub fn shim_path(home: &Path, tool: Tool) -> PathBuf {
    bin_dir(home).join(format!("{}{}", tool.name(), std::env::consts::EXE_SUFFIX))
}

/// Creates (or refreshes) every shim so it runs `exe`.
pub fn install(home: &Path, exe: &Path) -> Result<Vec<PathBuf>> {
    let dir = bin_dir(home);
    std::fs::create_dir_all(&dir).map_err(|source| Error::Io {
        path: dir.clone(),
        source,
    })?;
    Tool::ALL
        .into_iter()
        .map(|tool| {
            let shim = shim_path(home, tool);
            let io = |source| Error::Io {
                path: shim.clone(),
                source,
            };
            remove_file(&shim).map_err(io)?;
            link(exe, &shim).map_err(io)?;
            Ok(shim)
        })
        .collect()
}

/// Removes the shims; other files in `<home>/bin` are left alone.
pub fn remove(home: &Path) -> Result<()> {
    for tool in Tool::ALL {
        let shim = shim_path(home, tool);
        remove_file(&shim).map_err(|source| Error::Io { path: shim, source })?;
    }
    let _ = std::fs::remove_dir(bin_dir(home)); // only if empty
    Ok(())
}

/// Whether every shim exists.
pub fn installed(home: &Path) -> bool {
    Tool::ALL
        .into_iter()
        .all(|tool| shim_path(home, tool).symlink_metadata().is_ok())
}

fn remove_file(path: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

#[cfg(unix)]
fn link(exe: &Path, shim: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(exe, shim)
}

#[cfg(windows)]
fn link(exe: &Path, shim: &Path) -> std::io::Result<()> {
    std::fs::hard_link(exe, shim).or_else(|_| std::fs::copy(exe, shim).map(|_| ()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installs_refreshes_and_removes() {
        let home = crate::testutil::tempdir();
        let exe = home.join(format!("stackyard{}", std::env::consts::EXE_SUFFIX));
        std::fs::write(&exe, "program").unwrap();
        assert!(!installed(&home));

        let shims = install(&home, &exe).unwrap();
        assert_eq!(shims.len(), Tool::ALL.len());
        assert!(installed(&home));
        let php = shim_path(&home, Tool::Php);
        assert_eq!(std::fs::read_to_string(&php).unwrap(), "program");

        // Installing again replaces the old shims instead of failing.
        install(&home, &exe).unwrap();

        std::fs::write(bin_dir(&home).join("mine"), "").unwrap();
        remove(&home).unwrap();
        assert!(!installed(&home));
        assert!(!php.exists());
        assert!(bin_dir(&home).join("mine").exists(), "other files stay");
        assert!(exe.exists());
    }
}
