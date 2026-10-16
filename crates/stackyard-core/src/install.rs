//! Installs a downloaded archive into its own folder. Shared by runtimes
//! (`runtimes/php/8.4/`) and services (`services/mysql/8.4/`).
//!
//! The archive is verified before it is unpacked, the new folder is swapped
//! in only once it is complete, and a receipt inside it records exactly what
//! was installed and where it came from.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use serde::{Deserialize, Serialize};

use crate::download::{self, Http};
use crate::settings;
use crate::{archive, Error, Result};

pub(crate) const RECEIPT: &str = ".stackyard-install.toml";

/// One downloadable release: an exact version and its archive.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Release {
    pub version: String,
    pub url: String,
    pub file_name: String,
    /// SHA-256 or SHA-512 of the archive, as hex.
    pub checksum: String,
}

/// What an install is doing right now, for progress bars.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(tag = "step", rename_all = "lowercase")]
pub enum Step {
    Resolving,
    Downloading { done: u64, total: Option<u64> },
    Unpacking,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Receipt {
    pub release: String,
    pub url: String,
    #[serde(alias = "sha256")]
    pub checksum: String,
}

pub fn read_receipt(dir: &Path) -> Option<Receipt> {
    settings::read_toml(&dir.join(RECEIPT)).ok()?
}

/// Downloads `release` into `downloads`, verifies it, and unpacks it to
/// `dest`, replacing what was there. `program` is a path inside the archive
/// that must exist (e.g. `bin/php`), to catch a wrong or empty archive.
pub(crate) fn install(
    http: &Http,
    downloads: &Path,
    release: &Release,
    dest: &Path,
    program: &str,
    progress: &mut dyn FnMut(Step),
) -> Result<()> {
    let archive_path = downloads.join(&release.file_name);
    let reusable = archive_path.is_file()
        && download::file_checksum(&archive_path, &release.checksum)
            .is_ok_and(|sum| sum.eq_ignore_ascii_case(&release.checksum));
    if !reusable {
        http.download(
            &release.url,
            &archive_path,
            &release.checksum,
            &mut |done, total| progress(Step::Downloading { done, total }),
        )?;
    }

    progress(Step::Unpacking);
    let name = dest
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let staging = dest.with_file_name(unique(".staging", &name));
    let result = unpack_into_place(release, &archive_path, &staging, dest, program);
    let _ = std::fs::remove_dir_all(&staging);
    // Keep disk use down: the archive is only needed again for a reinstall.
    let _ = std::fs::remove_file(&archive_path);
    result
}

fn unpack_into_place(
    release: &Release,
    archive_path: &Path,
    staging: &Path,
    dest: &Path,
    program: &str,
) -> Result<()> {
    let content = if archive::is_archive(&release.file_name) {
        archive::extract(archive_path, staging)?
    } else {
        place_program(archive_path, staging, program)?
    };
    if !content.join(program).is_file() {
        return Err(Error::Archive {
            path: archive_path.to_owned(),
            message: format!("it does not contain {program}"),
        });
    }
    let receipt = Receipt {
        release: release.version.clone(),
        url: release.url.clone(),
        checksum: release.checksum.clone(),
    };
    settings::write_toml(&content.join(RECEIPT), &receipt)?;

    // Swap the new folder in; the old one (if any) is removed afterwards
    // so a failure part-way never leaves a half-installed folder.
    let name = dest
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let old = dest.with_file_name(unique(".old", &name));
    let had_old = dest.exists();
    if had_old {
        std::fs::rename(dest, &old).map_err(|source| in_use(dest, source))?;
    }
    if let Err(source) = std::fs::rename(&content, dest) {
        if had_old {
            let _ = std::fs::rename(&old, dest);
        }
        return Err(Error::Io {
            path: dest.to_owned(),
            source,
        });
    }
    if had_old {
        let _ = std::fs::remove_dir_all(&old);
    }
    Ok(())
}

/// A download that is the program itself, with no archive around it: it is
/// copied into the folder under its program name and made runnable.
fn place_program(file: &Path, staging: &Path, program: &str) -> Result<PathBuf> {
    let target = staging.join(program);
    let io = |source| Error::Io {
        path: target.clone(),
        source,
    };
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(io)?;
    }
    std::fs::copy(file, &target).map_err(io)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755)).map_err(io)?;
    }
    Ok(staging.to_owned())
}

/// Removes an installed folder. It is moved aside first so it disappears in
/// one step, even if deleting thousands of files is interrupted.
pub(crate) fn remove(dest: &Path) -> Result<()> {
    let name = dest
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let trash: PathBuf = dest.with_file_name(unique(".trash", &name));
    std::fs::rename(dest, &trash).map_err(|source| in_use(dest, source))?;
    std::fs::remove_dir_all(&trash).map_err(|source| Error::Io {
        path: trash,
        source,
    })
}

fn unique(prefix: &str, name: &str) -> String {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    format!(
        "{prefix}-{name}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}

fn in_use(path: &Path, source: std::io::Error) -> Error {
    Error::Io {
        path: path.to_owned(),
        source: std::io::Error::new(
            source.kind(),
            format!("{source} (is a site, service or terminal still using it?)"),
        ),
    }
}

/// Marks `dir` as an installed release, for tests elsewhere in the crate.
#[cfg(test)]
pub(crate) fn write_receipt_for_tests(dir: &Path, release: &str) {
    let receipt = Receipt {
        release: release.into(),
        url: "https://example.com/x.tar.gz".into(),
        checksum: "0".repeat(64),
    };
    std::fs::create_dir_all(dir).unwrap();
    settings::write_toml(&dir.join(RECEIPT), &receipt).unwrap();
}
