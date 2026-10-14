//! MariaDB, from the MariaDB Foundation's official builds (Windows and
//! Linux; there is no official macOS build).

use std::collections::BTreeMap;

use serde::Deserialize;

use super::mysql::{datadir, initialize, server_args};
use super::{slash, Context};
use crate::catalog::ServiceDef;
use crate::download::{is_sha256, Http};
use crate::install::Release;
use crate::platform::{Arch, Os, Platform};
use crate::supervisor::ProcessSpec;
use crate::{Error, Result};

const API: &str = "https://downloads.mariadb.org/rest-api/mariadb";

#[derive(Deserialize)]
struct Latest {
    releases: BTreeMap<String, ReleaseData>,
}

#[derive(Deserialize)]
struct ReleaseData {
    files: Vec<FileData>,
}

#[derive(Deserialize)]
struct FileData {
    file_name: String,
    os: Option<String>,
    cpu: Option<String>,
    package_type: Option<String>,
    file_download_url: Option<String>,
    checksum: Option<Checksums>,
}

#[derive(Deserialize)]
struct Checksums {
    sha256sum: Option<String>,
}

/// The newest release in `def.version` (e.g. 11.8) from the MariaDB
/// Foundation's download API, with its published SHA-256.
pub(super) fn resolve(http: &Http, def: &ServiceDef, platform: Platform) -> Result<Release> {
    let url = format!("{API}/{}/latest/", def.version);
    pick(&http.get_text(&url)?, platform).ok_or_else(|| Error::NoBuild {
        name: def.name.into(),
        version: def.version.into(),
        platform: platform.to_string(),
    })
}

fn pick(json: &str, platform: Platform) -> Option<Release> {
    // Official binary archives exist for x86_64 Linux and Windows only.
    let (os, package) = match (platform.os, platform.arch) {
        (Os::Linux, Arch::X64) => ("Linux", "gzipped tar file"),
        (Os::Windows, Arch::X64 | Arch::Arm64) => ("Windows", "ZIP file"),
        _ => return None,
    };
    let latest: Latest = serde_json::from_str(json).ok()?;
    let (version, data) = latest.releases.into_iter().next_back()?;
    let file = data.files.into_iter().find(|f| {
        f.os.as_deref() == Some(os)
            && f.cpu.as_deref() == Some("x86_64")
            && f.package_type.as_deref() == Some(package)
            && !f.file_name.contains("debug")
            && !f.file_name.contains(['/', '\\'])
    })?;
    let checksum = file.checksum?.sha256sum.filter(|c| is_sha256(c))?;
    let url = file.file_download_url?.replacen("http://", "https://", 1);
    url.starts_with("https://").then(|| Release {
        version,
        url,
        file_name: file.file_name,
        checksum: checksum.to_ascii_lowercase(),
    })
}

pub(super) fn spec(ctx: &Context) -> Result<ProcessSpec> {
    let datadir = datadir(ctx);
    if !datadir.join("mysql").is_dir() {
        let (program, args) = if ctx.os == Os::Windows {
            (
                ctx.install.join("bin/mariadb-install-db.exe"),
                vec![format!("--datadir={}", slash(&datadir))],
            )
        } else {
            (
                ctx.install.join("scripts/mariadb-install-db"),
                vec![
                    "--no-defaults".to_string(),
                    format!("--basedir={}", slash(ctx.install)),
                    format!("--datadir={}", slash(&datadir)),
                    "--auth-root-authentication-method=normal".to_string(),
                    "--skip-test-db".to_string(),
                ],
            )
        };
        initialize(ctx, &program, &args, &datadir)?;
    }
    Ok(ctx.base_spec(server_args(ctx, &datadir)))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHA: &str = "32f8871a2aa38b36aa418b7f4abc6fc0c6ef1ba5a45798cacb2afa52c8a6113b";

    fn sample() -> String {
        format!(
            r#"{{"releases": {{"11.8.9": {{"release_id": "11.8.9", "files": [
                {{"file_name": "repo/", "os": null, "cpu": null, "package_type": null, "file_download_url": null, "checksum": null}},
                {{"file_name": "mariadb-11.8.9.tar.gz", "os": "Source", "cpu": null, "package_type": "gzipped tar file", "file_download_url": "http://x/src", "checksum": {{"sha256sum": "{SHA}"}}}},
                {{"file_name": "mariadb-11.8.9-linux-systemd-x86_64.tar.gz", "os": "Linux", "cpu": "x86_64", "package_type": "gzipped tar file",
                  "file_download_url": "http://downloads.mariadb.org/rest-api/mariadb/11.8.9/mariadb-11.8.9-linux-systemd-x86_64.tar.gz",
                  "checksum": {{"md5sum": "x", "sha256sum": "{SHA}"}}}},
                {{"file_name": "mariadb-11.8.9-winx64-debugsymbols.zip", "os": "Windows", "cpu": "x86_64", "package_type": "ZIP file", "file_download_url": "http://x/dbg", "checksum": {{"sha256sum": "{SHA}"}}}},
                {{"file_name": "mariadb-11.8.9-winx64.zip", "os": "Windows", "cpu": "x86_64", "package_type": "ZIP file", "file_download_url": "http://x/win.zip", "checksum": {{"sha256sum": "{SHA}"}}}}
            ]}}}}}}"#
        )
    }

    #[test]
    fn picks_the_archive_for_each_system() {
        let p = |os, arch| Platform { os, arch };
        let linux = pick(&sample(), p(Os::Linux, Arch::X64)).unwrap();
        assert_eq!(linux.version, "11.8.9");
        assert_eq!(
            linux.file_name,
            "mariadb-11.8.9-linux-systemd-x86_64.tar.gz"
        );
        assert!(linux.url.starts_with("https://downloads.mariadb.org/"));
        assert_eq!(linux.checksum, SHA);
        let windows = pick(&sample(), p(Os::Windows, Arch::X64)).unwrap();
        assert_eq!(windows.file_name, "mariadb-11.8.9-winx64.zip");
        assert!(pick(&sample(), p(Os::Linux, Arch::Arm64)).is_none());
        assert!(pick(&sample(), p(Os::Macos, Arch::Arm64)).is_none());
    }
}
