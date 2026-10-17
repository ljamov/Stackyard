//! MongoDB Community Server, a document database. Downloads come from
//! MongoDB's official download site: the list at `downloads.mongodb.org`
//! names every archive with its SHA-256. Stackyard downloads it there for
//! the user and never hands it on (the server is under the SSPL).
//!
//! Linux builds are made per distribution, so the one for this system is
//! picked from `/etc/os-release`.

use serde::Deserialize;

use super::{slash, Context};
use crate::catalog::ServiceDef;
use crate::download::{is_checksum, Http};
use crate::install::Release;
use crate::platform::{Arch, Os, Platform};
use crate::supervisor::ProcessSpec;
use crate::{Error, Result};

const LIST: &str = "https://downloads.mongodb.org/current.json";
/// Downloads are only taken from MongoDB's own server.
const HOST: &str = "https://fastdl.mongodb.org/";

#[derive(Debug, Deserialize)]
struct List {
    versions: Vec<Version>,
}

#[derive(Debug, Deserialize)]
struct Version {
    version: String,
    #[serde(default)]
    production_release: bool,
    #[serde(default)]
    downloads: Vec<Download>,
}

#[derive(Debug, Deserialize)]
struct Download {
    target: String,
    arch: String,
    edition: String,
    archive: Option<Archive>,
}

#[derive(Debug, Deserialize)]
struct Archive {
    url: String,
    sha256: Option<String>,
}

pub(super) fn resolve(http: &Http, def: &ServiceDef, platform: Platform) -> Result<Release> {
    let url = std::env::var("STACKYARD_MONGODB_LIST").unwrap_or_else(|_| LIST.into());
    let os_release = if platform.os == Os::Linux {
        std::fs::read_to_string("/etc/os-release").unwrap_or_default()
    } else {
        String::new()
    };
    pick(&http.get_text(&url)?, def, platform, &os_release)
}

fn pick(json: &str, def: &ServiceDef, platform: Platform, os_release: &str) -> Result<Release> {
    let list: List = serde_json::from_str(json).map_err(|e| Error::Download {
        url: LIST.into(),
        message: format!("unexpected answer: {e}"),
    })?;
    let no_build = || Error::NoBuild {
        name: def.name.into(),
        version: def.version.into(),
        platform: platform.to_string(),
    };
    let prefix = format!("{}.", def.version);
    let version = list
        .versions
        .iter()
        .filter(|v| v.production_release && v.version.starts_with(&prefix))
        .max_by_key(|v| numbers(&v.version))
        .ok_or_else(no_build)?;
    let arch = match platform.arch {
        Arch::X64 => "x86_64",
        Arch::Arm64 if platform.os == Os::Macos => "arm64",
        Arch::Arm64 => "aarch64",
    };
    // `base` on Windows and macOS, `targeted` for a Linux distribution;
    // never `enterprise`, which needs a paid licence.
    let builds: Vec<&Download> = version
        .downloads
        .iter()
        .filter(|d| d.arch == arch && (d.edition == "base" || d.edition == "targeted"))
        .filter(|d| d.archive.is_some())
        .collect();
    let target = match platform.os {
        Os::Windows => Some("windows".to_string()),
        Os::Macos => Some("macos".to_string()),
        Os::Linux => {
            let targets: Vec<&str> = builds.iter().map(|d| d.target.as_str()).collect();
            linux_target(os_release, &targets)
        }
    }
    .ok_or_else(no_build)?;
    let build = builds
        .iter()
        .find(|d| d.target == target)
        .ok_or_else(no_build)?;
    let archive = build.archive.as_ref().expect("filtered above");
    let checksum = archive
        .sha256
        .as_deref()
        .filter(|s| s.len() == 64 && is_checksum(s))
        .ok_or_else(|| Error::Download {
            url: archive.url.clone(),
            message: "MongoDB lists no checksum for this file".into(),
        })?;
    let file_name = archive
        .url
        .strip_prefix(HOST)
        .and_then(|rest| rest.rsplit('/').next())
        .filter(|n| {
            !n.is_empty()
                && n.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        })
        .ok_or_else(|| Error::Download {
            url: archive.url.clone(),
            message: "not a MongoDB download".into(),
        })?;
    Ok(Release {
        version: version.version.clone(),
        url: archive.url.clone(),
        file_name: file_name.to_string(),
        checksum: checksum.to_ascii_lowercase(),
    })
}

fn numbers(version: &str) -> Vec<u32> {
    version.split('.').map(|n| n.parse().unwrap_or(0)).collect()
}

/// The build for this Linux: the one made for this distribution and
/// release, else the newest one for an older release of it, else (for
/// distributions MongoDB doesn't build for) the oldest Ubuntu build, which
/// asks the least of the system.
fn linux_target(os_release: &str, targets: &[&str]) -> Option<String> {
    let field = |key: &str| {
        os_release.lines().find_map(|line| {
            let value = line.strip_prefix(key)?.strip_prefix('=')?;
            Some(value.trim().trim_matches('"').to_ascii_lowercase())
        })
    };
    let id = field("ID").unwrap_or_default();
    let like = field("ID_LIKE").unwrap_or_default();
    let version = field("VERSION_ID").unwrap_or_default();
    let major: u32 = version
        .split('.')
        .next()
        .and_then(|m| m.parse().ok())
        .unwrap_or(0);
    let is = |name: &str| id == name || like.split_whitespace().any(|l| l == name);

    // Each target with the release it was made for, as a sortable number.
    let family = |prefix: &str, release: fn(&str) -> Option<u32>| -> Vec<(u32, String)> {
        let mut found: Vec<(u32, String)> = targets
            .iter()
            .filter_map(|t| Some((release(t.strip_prefix(prefix)?)?, t.to_string())))
            .collect();
        found.sort();
        found
    };
    let newest_up_to = |found: Vec<(u32, String)>, mine: u32| {
        found
            .into_iter()
            .rev()
            .find(|(r, _)| *r <= mine)
            .map(|(_, t)| t)
    };
    let number = |s: &str| s.parse::<u32>().ok();
    // `rhel8`, `rhel93`, `rhel10`: the major release first.
    let rhel = |s: &str| match s {
        s if s.starts_with("10") => Some(1000 + s[2..].parse::<u32>().unwrap_or(0)),
        s if !s.is_empty() => {
            let major = s[..1].parse::<u32>().ok()?;
            Some(major * 100 + s[1..].parse::<u32>().unwrap_or(0))
        }
        _ => None,
    };
    let ubuntu = family("ubuntu", number);

    let picked = if id == "ubuntu" {
        // `24.04` → 2404
        let mine = version.replace('.', "").parse().unwrap_or(0);
        newest_up_to(ubuntu.clone(), mine)
    } else if id == "debian" {
        newest_up_to(family("debian", number), major)
    } else if id == "amzn" {
        newest_up_to(family("amazon", number), major)
    } else if is("suse") || id.starts_with("opensuse") || id == "sles" {
        newest_up_to(family("suse", number), major)
    } else if is("rhel") || is("fedora") || is("centos") {
        // Fedora is newer than any RHEL, so it takes the newest build.
        let mine = if id == "fedora" {
            u32::MAX
        } else {
            major * 100 + 99
        };
        newest_up_to(family("rhel", rhel), mine)
    } else if is("ubuntu") {
        // Linux Mint, Pop!_OS and friends say which Ubuntu they follow.
        let mine = match field("UBUNTU_CODENAME").as_deref() {
            Some("jammy") => 2204,
            Some("noble") => 2404,
            _ => u32::MAX,
        };
        newest_up_to(ubuntu.clone(), mine)
    } else if is("debian") {
        newest_up_to(family("debian", number), u32::MAX)
    } else {
        None
    };
    picked.or_else(|| ubuntu.into_iter().next().map(|(_, t)| t))
}

pub(super) fn spec(ctx: &Context) -> Result<ProcessSpec> {
    let db = ctx.data.join("db");
    std::fs::create_dir_all(&db).map_err(|source| Error::Io {
        path: db.clone(),
        source,
    })?;
    Ok(ctx.base_spec(vec![
        "--dbpath".into(),
        slash(&db),
        "--bind_ip".into(),
        "127.0.0.1".into(),
        "--port".into(),
        ctx.ports[0].to_string(),
    ]))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TARGETS: &[&str] = &[
        "ubuntu2204",
        "ubuntu2404",
        "ubuntu2604",
        "debian12",
        "debian13",
        "rhel8",
        "rhel93",
        "rhel10",
        "amazon2023",
        "suse15",
    ];

    fn target(os_release: &str) -> Option<String> {
        linux_target(os_release, TARGETS)
    }

    #[test]
    fn picks_the_build_for_this_linux() {
        assert_eq!(
            target("ID=ubuntu\nVERSION_ID=\"24.04\"\n").as_deref(),
            Some("ubuntu2404")
        );
        // A release in between takes the one before it.
        assert_eq!(
            target("ID=ubuntu\nVERSION_ID=\"25.10\"\n").as_deref(),
            Some("ubuntu2404")
        );
        assert_eq!(
            target("ID=debian\nVERSION_ID=\"12\"\n").as_deref(),
            Some("debian12")
        );
        assert_eq!(
            target("ID=\"rocky\"\nID_LIKE=\"rhel centos fedora\"\nVERSION_ID=\"9.4\"\n").as_deref(),
            Some("rhel93")
        );
        assert_eq!(
            target("ID=almalinux\nID_LIKE=\"rhel centos fedora\"\nVERSION_ID=10.0\n").as_deref(),
            Some("rhel10")
        );
        assert_eq!(
            target("ID=fedora\nVERSION_ID=42\n").as_deref(),
            Some("rhel10")
        );
        assert_eq!(
            target("ID=linuxmint\nID_LIKE=\"ubuntu debian\"\nVERSION_ID=\"22\"\nUBUNTU_CODENAME=noble\n").as_deref(),
            Some("ubuntu2404")
        );
        assert_eq!(
            target("ID=amzn\nVERSION_ID=\"2023\"\n").as_deref(),
            Some("amazon2023")
        );
        // Anything else gets the Ubuntu build that asks the least.
        assert_eq!(target("ID=arch\n").as_deref(), Some("ubuntu2204"));
        assert_eq!(linux_target("ID=arch\n", &["debian12"]), None);
    }

    #[test]
    fn reads_the_download_list() {
        let sha = "a".repeat(64);
        let json = format!(
            r#"{{"versions": [
              {{"version": "9.0.4", "production_release": true, "downloads": [
                {{"target": "ubuntu2404", "arch": "x86_64", "edition": "enterprise",
                  "archive": {{"url": "https://fastdl.mongodb.org/linux/mongodb-linux-x86_64-enterprise-ubuntu2404-9.0.4.tgz", "sha256": "{sha}"}}}},
                {{"target": "ubuntu2404", "arch": "x86_64", "edition": "targeted",
                  "archive": {{"url": "https://fastdl.mongodb.org/linux/mongodb-linux-x86_64-ubuntu2404-9.0.4.tgz", "sha256": "{sha}"}}}},
                {{"target": "windows", "arch": "x86_64", "edition": "base",
                  "archive": {{"url": "https://fastdl.mongodb.org/windows/mongodb-windows-x86_64-9.0.4.zip", "sha256": "{sha}"}}}},
                {{"target": "macos", "arch": "arm64", "edition": "base",
                  "archive": {{"url": "https://evil.example/mongodb-macos-arm64-9.0.4.tgz", "sha256": "{sha}"}}}}
              ]}},
              {{"version": "9.0.10-rc0", "production_release": false, "downloads": []}},
              {{"version": "8.3.13", "production_release": true, "downloads": []}}
            ]}}"#
        );
        let def = crate::catalog::service("mongodb").unwrap();
        let p = |os, arch| Platform { os, arch };
        let linux = pick(
            &json,
            def,
            p(Os::Linux, Arch::X64),
            "ID=ubuntu\nVERSION_ID=24.04\n",
        )
        .unwrap();
        assert_eq!(linux.version, "9.0.4");
        assert_eq!(linux.file_name, "mongodb-linux-x86_64-ubuntu2404-9.0.4.tgz");
        let windows = pick(&json, def, p(Os::Windows, Arch::X64), "").unwrap();
        assert_eq!(windows.file_name, "mongodb-windows-x86_64-9.0.4.zip");
        assert!(pick(&json, def, p(Os::Macos, Arch::Arm64), "").is_err());
        assert!(matches!(
            pick(&json, def, p(Os::Windows, Arch::Arm64), ""),
            Err(Error::NoBuild { .. })
        ));
    }
}
