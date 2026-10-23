//! Copying a site's files quickly. Where the disk can share blocks between
//! files (APFS on macOS, Btrfs and XFS on Linux) a copy takes no time and no
//! space until one side changes; elsewhere it is a normal copy. Dates are
//! kept, so later comparisons by size and date still match.

use std::io;
use std::path::Path;

/// Copies one file, replacing `to`, and returns its size.
pub fn copy_file(from: &Path, to: &Path) -> io::Result<u64> {
    let meta = std::fs::metadata(from)?;
    if to.exists() {
        std::fs::remove_file(to)?;
    }
    if !reflink(from, to) {
        std::fs::copy(from, to)?;
    }
    if let Ok(modified) = meta.modified() {
        if let Ok(file) = std::fs::File::options().write(true).open(to) {
            let _ = file.set_modified(modified);
        }
    }
    Ok(meta.len())
}

#[cfg(target_os = "macos")]
fn reflink(from: &Path, to: &Path) -> bool {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let (Ok(src), Ok(dst)) = (
        CString::new(from.as_os_str().as_bytes()),
        CString::new(to.as_os_str().as_bytes()),
    ) else {
        return false;
    };
    // SAFETY: both are valid NUL-terminated paths.
    unsafe { libc::clonefile(src.as_ptr(), dst.as_ptr(), 0) == 0 }
}

#[cfg(target_os = "linux")]
fn reflink(from: &Path, to: &Path) -> bool {
    use std::os::fd::AsRawFd;
    // FICLONE: share the source's blocks with the new file.
    const FICLONE: libc::c_ulong = 0x4004_9409;
    let Ok(src) = std::fs::File::open(from) else {
        return false;
    };
    let Ok(dst) = std::fs::File::create(to) else {
        return false;
    };
    // SAFETY: both descriptors are open for the duration of the call.
    let done = unsafe { libc::ioctl(dst.as_raw_fd(), FICLONE as _, src.as_raw_fd()) } == 0;
    if !done {
        drop(dst);
        let _ = std::fs::remove_file(to);
    }
    done
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn reflink(_from: &Path, _to: &Path) -> bool {
    false
}

/// Folders a copy of a site leaves out: caches the site makes again.
pub const SKIPPED: &[&str] = &["wp-content/cache", "wp-content/et-cache", "node_modules"];

/// Copies a site's folder to `to`, leaving out [`SKIPPED`] folders and
/// the `also_skipped` ones (paths relative to `from`). `progress` gets the bytes copied so far after each
/// file. Returns the files and bytes copied.
pub fn copy_tree(
    from: &Path,
    to: &Path,
    also_skipped: &[&str],
    progress: &mut dyn FnMut(u64),
) -> io::Result<(u64, u64)> {
    std::fs::create_dir_all(to)?;
    let (mut files, mut bytes) = (0u64, 0u64);
    let mut folders = vec![String::new()];
    while let Some(rel) = folders.pop() {
        let dir = from.join(&rel);
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            let path = if rel.is_empty() {
                name
            } else {
                format!("{rel}/{name}")
            };
            let meta = std::fs::symlink_metadata(entry.path())?;
            if meta.is_dir() {
                if !SKIPPED.contains(&path.as_str()) && !also_skipped.contains(&path.as_str()) {
                    std::fs::create_dir_all(to.join(&path))?;
                    folders.push(path);
                }
            } else if meta.is_file() {
                bytes += copy_file(&entry.path(), &to.join(&path))?;
                files += 1;
                progress(bytes);
            } else if meta.file_type().is_symlink() {
                copy_link(&entry.path(), &to.join(&path));
            }
        }
    }
    Ok((files, bytes))
}

#[cfg(unix)]
fn copy_link(from: &Path, to: &Path) {
    if let Ok(target) = std::fs::read_link(from) {
        let _ = std::os::unix::fs::symlink(target, to);
    }
}

#[cfg(not(unix))]
fn copy_link(from: &Path, to: &Path) {
    // Links on Windows need extra rights; copy what they point at instead.
    if from.is_file() {
        let _ = copy_file(from, to);
    }
}

/// The total size of the files [`copy_tree`] would copy.
pub fn tree_size(from: &Path, also_skipped: &[&str]) -> u64 {
    let mut total = 0;
    let mut folders = vec![String::new()];
    while let Some(rel) = folders.pop() {
        let Ok(entries) = std::fs::read_dir(from.join(&rel)) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let path = if rel.is_empty() {
                name
            } else {
                format!("{rel}/{name}")
            };
            let Ok(meta) = std::fs::symlink_metadata(entry.path()) else {
                continue;
            };
            if meta.is_dir() {
                if !SKIPPED.contains(&path.as_str()) && !also_skipped.contains(&path.as_str()) {
                    folders.push(path);
                }
            } else if meta.is_file() {
                total += meta.len();
            }
        }
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copies_a_tree_keeping_dates_and_leaving_out_caches() {
        let from = crate::testutil::tempdir();
        std::fs::create_dir_all(from.join("wp-content/cache")).unwrap();
        std::fs::create_dir_all(from.join("wp-content/themes/a")).unwrap();
        std::fs::write(from.join("wp-content/cache/x.html"), "cache").unwrap();
        std::fs::write(from.join("wp-content/themes/a/style.css"), "body{}").unwrap();
        std::fs::write(from.join("index.php"), "<?php").unwrap();
        let old = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
        std::fs::File::options()
            .write(true)
            .open(from.join("index.php"))
            .unwrap()
            .set_modified(old)
            .unwrap();
        assert_eq!(tree_size(&from, &[]), 11);
        assert_eq!(tree_size(&from, &["wp-content/themes"]), 5);
        let to = crate::testutil::tempdir().join("copy");
        let mut seen = 0;
        let (files, bytes) = copy_tree(&from, &to, &[], &mut |b| seen = b).unwrap();
        assert_eq!((files, bytes, seen), (2, 11, 11));
        assert!(!to.join("wp-content/cache").exists());
        assert_eq!(
            std::fs::read_to_string(to.join("wp-content/themes/a/style.css")).unwrap(),
            "body{}"
        );
        let modified = std::fs::metadata(to.join("index.php"))
            .unwrap()
            .modified()
            .unwrap();
        assert_eq!(modified, old);
        // Copying over an existing file replaces it.
        std::fs::write(from.join("index.php"), "<?php echo 1;").unwrap();
        copy_file(&from.join("index.php"), &to.join("index.php")).unwrap();
        assert_eq!(
            std::fs::read_to_string(to.join("index.php")).unwrap(),
            "<?php echo 1;"
        );
    }
}
