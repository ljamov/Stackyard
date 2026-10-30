//! Unpacks downloaded `.tar.gz`, `.tar.xz` and `.zip` files.
//!
//! Entries that would land outside the target folder (absolute paths, `..`)
//! are refused, so a hostile archive cannot overwrite files elsewhere.

use std::fs::File;
use std::path::{Path, PathBuf};

use crate::{Error, Result};

/// Whether a download is an archive this module can unpack, rather than a
/// program shipped on its own.
pub fn is_archive(file_name: &str) -> bool {
    let name = file_name.to_ascii_lowercase();
    [".tar.gz", ".tgz", ".tar.xz", ".zip"]
        .iter()
        .any(|ext| name.ends_with(ext))
}

/// Unpacks `archive` into `dest` (created if missing) and returns the folder
/// that holds the contents: `dest` itself, or its only subfolder when the
/// archive wraps everything in one top-level folder like `node-v22.1.0-linux-x64/`.
pub fn extract(archive: &Path, dest: &Path) -> Result<PathBuf> {
    extract_with(archive, dest, &mut |_, _| true, None)
}

/// How a site backup is unpacked: some folders are left out, and an entry
/// that can't be written is noted in `failed` and the rest goes on, where
/// a download for a program fails as a whole.
#[derive(Default)]
pub struct Lenient<'a> {
    /// Folders left out wherever they are, e.g. `wp-content/upgrade`.
    pub leave_out: &'a [&'a str],
    /// Each entry that could not be written, with why.
    pub failed: Vec<String>,
}

/// [`extract`], telling `on` how many bytes of the archive file are read
/// and how big it is. Returning `false` stops it with [`STOPPED`].
pub fn extract_with(
    archive: &Path,
    dest: &Path,
    on: &mut dyn FnMut(u64, u64) -> bool,
    lenient: Option<&mut Lenient>,
) -> Result<PathBuf> {
    let fail = |message: String| Error::Archive {
        path: archive.to_owned(),
        message,
    };
    std::fs::create_dir_all(dest).map_err(|e| fail(e.to_string()))?;
    let name = archive
        .file_name()
        .map(|n| n.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    let kind = [".tar.gz", ".tgz", ".tar.xz", ".tar", ".zip"]
        .into_iter()
        .find(|ext| name.ends_with(ext))
        .ok_or_else(|| fail("unsupported archive type".to_string()))?;
    let file = File::open(archive).map_err(|e| fail(e.to_string()))?;
    let total = file.metadata().map(|m| m.len()).unwrap_or(0);
    let file = Counted {
        inner: file,
        at: 0,
        total,
        on,
    };
    match kind {
        ".tar.gz" | ".tgz" => extract_tar(flate2::read::GzDecoder::new(file), dest, lenient),
        ".tar.xz" => extract_tar(liblzma::read::XzDecoder::new(file), dest, lenient),
        ".tar" => extract_tar(file, dest, lenient),
        _ => extract_zip(file, dest, lenient),
    }
    .map_err(fail)?;
    content_root(dest).map_err(|e| fail(e.to_string()))
}

/// What an unpacking stopped by its progress callback fails with.
pub const STOPPED: &str = "stopped";

/// A file that says how far into it reading has got.
struct Counted<'a, R> {
    inner: R,
    at: u64,
    total: u64,
    on: &'a mut dyn FnMut(u64, u64) -> bool,
}

impl<R: std::io::Read> std::io::Read for Counted<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.at += n as u64;
        if !(self.on)(self.at.min(self.total), self.total) {
            return Err(std::io::Error::other(STOPPED));
        }
        Ok(n)
    }
}

impl<R: std::io::Seek> std::io::Seek for Counted<'_, R> {
    fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
        self.at = self.inner.seek(pos)?;
        Ok(self.at)
    }
}

/// An error with what caused it: the tar crate's own says only which
/// entry failed, the cause (access denied, path too long…) is under it.
fn with_causes(e: &dyn std::error::Error) -> String {
    let mut text = e.to_string();
    let mut cause = e.source();
    while let Some(c) = cause {
        let more = c.to_string();
        if !text.contains(&more) {
            text = format!("{text}: {more}");
        }
        cause = c.source();
    }
    text
}

/// Whether `path` is inside one of the folders left out.
fn left_out(path: &Path, leave_out: &[&str]) -> bool {
    let path = format!("/{}/", path.to_string_lossy().replace('\\', "/"));
    leave_out
        .iter()
        .any(|folder| path.contains(&format!("/{}/", folder.trim_matches('/'))))
}

/// Notes an entry that failed in a lenient unpacking, or fails the whole.
fn entry_failed(
    lenient: &mut Option<&mut Lenient>,
    path: &Path,
    why: String,
) -> std::result::Result<(), String> {
    // Stopped on purpose is not an entry going wrong.
    let stopped = why == STOPPED || why.ends_with(&format!(": {STOPPED}"));
    match lenient {
        Some(lenient) if !stopped => {
            lenient.failed.push(format!("{}: {why}", path.display()));
            Ok(())
        }
        _ => Err(why),
    }
}

fn extract_tar(
    stream: impl std::io::Read,
    dest: &Path,
    mut lenient: Option<&mut Lenient>,
) -> std::result::Result<(), String> {
    let mut tar = tar::Archive::new(std::io::BufReader::new(stream));
    // A backup's modes come from the web server's system and mean nothing
    // here; read-only ones would only get in the way of the copy.
    tar.set_preserve_permissions(lenient.is_none());
    tar.set_preserve_mtime(true);
    tar.set_overwrite(true);
    for entry in tar.entries().map_err(|e| with_causes(&e))? {
        let mut entry = entry.map_err(|e| with_causes(&e))?;
        let path = entry.path().map_err(|e| with_causes(&e))?.into_owned();
        if !is_safe_relative(&path) {
            return Err(format!("refusing unsafe path {}", path.display()));
        }
        if lenient
            .as_ref()
            .is_some_and(|l| left_out(&path, l.leave_out))
        {
            continue;
        }
        // `unpack_in` also refuses to write through links that point outside `dest`.
        if let Err(e) = entry.unpack_in(dest) {
            entry_failed(&mut lenient, &path, with_causes(&e))?;
        }
    }
    Ok(())
}

fn extract_zip(
    file: impl std::io::Read + std::io::Seek,
    dest: &Path,
    mut lenient: Option<&mut Lenient>,
) -> std::result::Result<(), String> {
    let mut zip = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| e.to_string())?;
        let Some(relative) = entry.enclosed_name().filter(|p| is_safe_relative(p)) else {
            return Err(format!("refusing unsafe path {}", entry.name()));
        };
        if entry.is_symlink() {
            return Err(format!("refusing link {}", entry.name()));
        }
        if lenient
            .as_ref()
            .is_some_and(|l| left_out(&relative, l.leave_out))
        {
            continue;
        }
        let target = dest.join(&relative);
        let written = (|| -> std::io::Result<()> {
            if entry.is_dir() {
                return std::fs::create_dir_all(&target);
            }
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut out = File::create(&target)?;
            std::io::copy(&mut entry, &mut out)?;
            #[cfg(unix)]
            if let Some(mode) = entry.unix_mode() {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&target, std::fs::Permissions::from_mode(mode & 0o777))?;
            }
            Ok(())
        })();
        if let Err(e) = written {
            entry_failed(&mut lenient, &relative, e.to_string())?;
        }
    }
    Ok(())
}

/// Only plain relative paths: no root, no drive letter, no `..`.
fn is_safe_relative(path: &Path) -> bool {
    use std::path::Component;
    path.components()
        .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
}

fn content_root(dest: &Path) -> std::io::Result<PathBuf> {
    let mut entries = std::fs::read_dir(dest)?.collect::<std::io::Result<Vec<_>>>()?;
    if entries.len() == 1 && entries[0].file_type()?.is_dir() {
        return Ok(entries.remove(0).path());
    }
    Ok(dest.to_owned())
}

#[cfg(test)]
pub(crate) mod fixtures {
    use std::io::Write;
    use std::path::Path;

    /// Writes a `.tar.gz` holding `files` (path, contents, mode).
    pub fn tar_gz(path: &Path, files: &[(&str, &[u8], u32)]) {
        let gz = flate2::write::GzEncoder::new(
            std::fs::File::create(path).unwrap(),
            flate2::Compression::fast(),
        );
        let mut tar = tar::Builder::new(gz);
        for (name, data, mode) in files {
            let mut header = tar::Header::new_gnu();
            header.set_size(data.len() as u64);
            header.set_mode(*mode);
            header.set_cksum();
            tar.append_data(&mut header, name, *data).unwrap();
        }
        tar.into_inner().unwrap().finish().unwrap();
    }

    /// Writes a `.zip` holding `files` (path, contents).
    pub fn zip(path: &Path, files: &[(&str, &[u8])]) {
        let mut zip = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
        for (name, data) in files {
            zip.start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(data).unwrap();
        }
        zip.finish().unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unpacks_tar_gz_and_finds_the_top_folder() {
        let dir = crate::testutil::tempdir();
        let archive = dir.join("node.tar.gz");
        fixtures::tar_gz(
            &archive,
            &[
                ("node-v1/bin/node", b"#!/bin/sh\n", 0o755),
                ("node-v1/LICENSE", b"MIT", 0o644),
            ],
        );
        let root = extract(&archive, &dir.join("out")).unwrap();
        assert_eq!(root, dir.join("out").join("node-v1"));
        assert_eq!(std::fs::read(root.join("LICENSE")).unwrap(), b"MIT");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(root.join("bin/node"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o111, 0o111, "executable bit kept");
        }
    }

    #[test]
    fn a_lenient_unpacking_leaves_folders_out_and_notes_what_failed() {
        let dir = crate::testutil::tempdir();
        let archive = dir.join("site.tar.gz");
        fixtures::tar_gz(
            &archive,
            &[
                ("site/index.php", b"<?php", 0o644),
                ("site/wp-content/upgrade/x/a.php", b"<?php", 0o644),
                ("site/blocked/b.txt", b"b", 0o644),
                ("site/after.txt", b"after", 0o644),
            ],
        );
        let dest = dir.join("out");
        // A file where a folder of the backup should go: that entry fails.
        std::fs::create_dir_all(dest.join("site")).unwrap();
        std::fs::write(dest.join("site/blocked"), b"in the way").unwrap();
        let mut seen = 0;
        let mut lenient = Lenient {
            leave_out: &["wp-content/upgrade"],
            failed: Vec::new(),
        };
        extract_with(
            &archive,
            &dest,
            &mut |done, total| {
                seen = done;
                assert!(done <= total);
                true
            },
            Some(&mut lenient),
        )
        .unwrap();
        assert_eq!(seen, std::fs::metadata(&archive).unwrap().len());
        assert!(dest.join("site/index.php").is_file());
        assert!(
            dest.join("site/after.txt").is_file(),
            "goes on after a failed entry"
        );
        assert!(!dest.join("site/wp-content/upgrade").exists());
        assert_eq!(lenient.failed.len(), 1, "{:?}", lenient.failed);
        assert!(lenient.failed[0].contains("blocked"));

        // Without it, the same archive fails, saying why.
        assert!(extract(&archive, &dest).is_err());
        // Stopping stops, also when lenient.
        let mut lenient = Lenient::default();
        let stopped = extract_with(
            &archive,
            &dir.join("out2"),
            &mut |_, _| false,
            Some(&mut lenient),
        );
        assert!(stopped.unwrap_err().to_string().contains(STOPPED));
    }

    #[test]
    fn unpacks_tar_xz() {
        let dir = crate::testutil::tempdir();
        let tar_path = dir.join("plain.tar");
        let mut tar = tar::Builder::new(std::fs::File::create(&tar_path).unwrap());
        let mut header = tar::Header::new_gnu();
        header.set_size(2);
        header.set_mode(0o755);
        header.set_cksum();
        tar.append_data(&mut header, "mysql-8.4/bin/mysqld", &b"hi"[..])
            .unwrap();
        tar.into_inner().unwrap();
        let archive = dir.join("mysql.tar.xz");
        let mut xz = liblzma::write::XzEncoder::new(std::fs::File::create(&archive).unwrap(), 1);
        std::io::copy(&mut std::fs::File::open(&tar_path).unwrap(), &mut xz).unwrap();
        xz.finish().unwrap();

        let root = extract(&archive, &dir.join("out")).unwrap();
        assert_eq!(std::fs::read(root.join("bin/mysqld")).unwrap(), b"hi");
    }

    #[test]
    fn unpacks_flat_zip_into_dest() {
        let dir = crate::testutil::tempdir();
        let archive = dir.join("php.zip");
        fixtures::zip(&archive, &[("php.exe", b"MZ"), ("ext/php_curl.dll", b"MZ")]);
        let root = extract(&archive, &dir.join("out")).unwrap();
        assert_eq!(root, dir.join("out"));
        assert!(root.join("ext/php_curl.dll").is_file());
    }

    #[test]
    fn refuses_paths_that_escape() {
        let dir = crate::testutil::tempdir();
        let archive = dir.join("evil.zip");
        fixtures::zip(&archive, &[("../evil.txt", b"x")]);
        assert!(extract(&archive, &dir.join("out")).is_err());
        assert!(!dir.join("evil.txt").exists());

        assert!(!is_safe_relative(Path::new("/etc/passwd")));
        assert!(!is_safe_relative(Path::new("a/../../b")));
        assert!(is_safe_relative(Path::new("./a/b")));
    }

    #[test]
    fn refuses_unknown_formats() {
        let dir = crate::testutil::tempdir();
        let archive = dir.join("thing.rar");
        std::fs::write(&archive, b"x").unwrap();
        assert!(extract(&archive, &dir.join("out")).is_err());
    }
}
