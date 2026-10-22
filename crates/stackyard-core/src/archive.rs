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
    let fail = |message: String| Error::Archive {
        path: archive.to_owned(),
        message,
    };
    std::fs::create_dir_all(dest).map_err(|e| fail(e.to_string()))?;
    let name = archive
        .file_name()
        .map(|n| n.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
        let file = File::open(archive).map_err(|e| fail(e.to_string()))?;
        extract_tar(flate2::read::GzDecoder::new(file), dest).map_err(fail)?;
    } else if name.ends_with(".tar") {
        let file = File::open(archive).map_err(|e| fail(e.to_string()))?;
        extract_tar(file, dest).map_err(fail)?;
    } else if name.ends_with(".tar.xz") {
        let file = File::open(archive).map_err(|e| fail(e.to_string()))?;
        extract_tar(liblzma::read::XzDecoder::new(file), dest).map_err(fail)?;
    } else if name.ends_with(".zip") {
        extract_zip(archive, dest).map_err(fail)?;
    } else {
        return Err(fail("unsupported archive type".to_string()));
    }
    content_root(dest).map_err(|e| fail(e.to_string()))
}

fn extract_tar(stream: impl std::io::Read, dest: &Path) -> std::result::Result<(), String> {
    let mut tar = tar::Archive::new(std::io::BufReader::new(stream));
    tar.set_preserve_permissions(true);
    tar.set_preserve_mtime(true);
    tar.set_overwrite(true);
    for entry in tar.entries().map_err(|e| e.to_string())? {
        let mut entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path().map_err(|e| e.to_string())?.into_owned();
        if !is_safe_relative(&path) {
            return Err(format!("refusing unsafe path {}", path.display()));
        }
        // `unpack_in` also refuses to write through links that point outside `dest`.
        entry.unpack_in(dest).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn extract_zip(archive: &Path, dest: &Path) -> std::result::Result<(), String> {
    let file = File::open(archive).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| e.to_string())?;
        let Some(relative) = entry.enclosed_name().filter(|p| is_safe_relative(p)) else {
            return Err(format!("refusing unsafe path {}", entry.name()));
        };
        if entry.is_symlink() {
            return Err(format!("refusing link {}", entry.name()));
        }
        let target = dest.join(relative);
        if entry.is_dir() {
            std::fs::create_dir_all(&target).map_err(|e| e.to_string())?;
            continue;
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut out = File::create(&target).map_err(|e| e.to_string())?;
        std::io::copy(&mut entry, &mut out).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        if let Some(mode) = entry.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&target, std::fs::Permissions::from_mode(mode & 0o777))
                .map_err(|e| e.to_string())?;
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
