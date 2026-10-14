//! The shim files in `<home>/bin`: one per [`Tool`], each leading back to a
//! copy of the `stackyard` program kept in the same folder, so they keep
//! working when the program that made them moves (an AppImage, an update).
//! Symbolic links on macOS and Linux; on Windows, hard links named `php.exe`
//! and so on (copies if the folder is on another drive). The copy also puts
//! `stackyard` itself on the PATH.

use std::path::{Path, PathBuf};

use super::Tool;
use crate::{Error, Result};

pub fn bin_dir(home: &Path) -> PathBuf {
    home.join("bin")
}

pub fn shim_path(home: &Path, tool: Tool) -> PathBuf {
    bin_dir(home).join(format!("{}{}", tool.name(), std::env::consts::EXE_SUFFIX))
}

/// The copy of `stackyard` the shims lead to.
pub fn program_path(home: &Path) -> PathBuf {
    bin_dir(home).join(format!("stackyard{}", std::env::consts::EXE_SUFFIX))
}

/// Copies `exe` into `<home>/bin` and creates (or refreshes) every shim.
pub fn install(home: &Path, exe: &Path) -> Result<Vec<PathBuf>> {
    let dir = bin_dir(home);
    let program = program_path(home);
    let io = |path: &Path| {
        let path = path.to_owned();
        move |source| Error::Io { path, source }
    };
    std::fs::create_dir_all(&dir).map_err(io(&dir))?;
    if !same_file(exe, &program) {
        let tmp = dir.join(".stackyard.new");
        std::fs::copy(exe, &tmp).map_err(io(exe))?;
        std::fs::rename(&tmp, &program).map_err(|source| {
            let _ = std::fs::remove_file(&tmp);
            Error::Io {
                path: program.clone(),
                source: std::io::Error::new(
                    source.kind(),
                    format!("{source} (is a terminal command still running?)"),
                ),
            }
        })?;
    }
    Tool::ALL
        .into_iter()
        .map(|tool| {
            let shim = shim_path(home, tool);
            remove_file(&shim).map_err(io(&shim))?;
            link(&program, &shim).map_err(io(&shim))?;
            Ok(shim)
        })
        .collect()
}

/// Whether the shims run an older `stackyard` than `exe`, e.g. after an update.
pub fn outdated(home: &Path, exe: &Path) -> bool {
    let modified = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    let size = |p: &Path| std::fs::metadata(p).map(|m| m.len()).ok();
    installed(home)
        && !same_file(exe, &program_path(home))
        && (size(exe) != size(&program_path(home)) || modified(exe) > modified(&program_path(home)))
}

/// Removes the shims and the copy; other files in `<home>/bin` are left alone.
pub fn remove(home: &Path) -> Result<()> {
    let program = program_path(home);
    let files = Tool::ALL.into_iter().map(|tool| shim_path(home, tool));
    for file in files.chain([program]) {
        remove_file(&file).map_err(|source| Error::Io { path: file, source })?;
    }
    let _ = std::fs::remove_dir(bin_dir(home)); // only if empty
    Ok(())
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
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
        assert_eq!(
            std::fs::read_to_string(program_path(&home)).unwrap(),
            "program"
        );
        assert!(!outdated(&home, &exe));

        // A newer program (an update) is noticed and copied on the next install.
        std::fs::write(&exe, "program v2").unwrap();
        assert!(outdated(&home, &exe));
        install(&home, &exe).unwrap();
        assert_eq!(std::fs::read_to_string(&php).unwrap(), "program v2");
        assert!(!outdated(&home, &exe));

        // Installing from the copy itself works too.
        install(&home, &program_path(&home)).unwrap();

        std::fs::write(bin_dir(&home).join("mine"), "").unwrap();
        remove(&home).unwrap();
        assert!(!installed(&home));
        assert!(!php.exists());
        assert!(!program_path(&home).exists());
        assert!(bin_dir(&home).join("mine").exists(), "other files stay");
        assert!(exe.exists());
    }
}
