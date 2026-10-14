//! Node.js from the official builds at nodejs.org.

use serde::Deserialize;

use super::{Arch, Os, Platform, Release};
use crate::download::{find_checksum, Http};
use crate::{Error, Result};

/// Names nodejs.org uses for a platform: (file name part, index.json key, archive type).
fn naming(p: Platform) -> (&'static str, &'static str, &'static str) {
    match (p.os, p.arch) {
        (Os::Linux, Arch::X64) => ("linux-x64", "linux-x64", "tar.gz"),
        (Os::Linux, Arch::Arm64) => ("linux-arm64", "linux-arm64", "tar.gz"),
        (Os::Macos, Arch::X64) => ("darwin-x64", "osx-x64-tar", "tar.gz"),
        (Os::Macos, Arch::Arm64) => ("darwin-arm64", "osx-arm64-tar", "tar.gz"),
        (Os::Windows, Arch::X64) => ("win-x64", "win-x64-zip", "zip"),
        (Os::Windows, Arch::Arm64) => ("win-arm64", "win-arm64-zip", "zip"),
    }
}

#[derive(Deserialize)]
struct IndexEntry {
    version: String,
    files: Vec<String>,
}

/// Newest `vMAJOR.x.y` in nodejs.org's `index.json` that has a build for `index_key`.
pub(super) fn latest_version(index_json: &str, major: &str, index_key: &str) -> Option<String> {
    let entries: Vec<IndexEntry> = serde_json::from_str(index_json).ok()?;
    entries
        .into_iter()
        .filter(|e| e.files.iter().any(|f| f == index_key))
        .filter_map(|e| {
            let parts = parse_version(&e.version)?;
            (parts.0.to_string() == major).then_some((parts, e.version))
        })
        .max()
        .map(|(_, version)| version)
}

fn parse_version(v: &str) -> Option<(u32, u32, u32)> {
    let mut parts = v.strip_prefix('v')?.split('.').map(|p| p.parse().ok());
    Some((parts.next()??, parts.next()??, parts.next()??))
}

pub(super) fn resolve(http: &Http, dist: &str, major: &str, platform: Platform) -> Result<Release> {
    let (file_part, index_key, ext) = naming(platform);
    let index_url = format!("{dist}/index.json");
    let index = http.get_text(&index_url)?;
    let version = latest_version(&index, major, index_key).ok_or_else(|| Error::NoBuild {
        name: "Node.js".into(),
        version: major.into(),
        platform: platform.to_string(),
    })?;

    let file_name = format!("node-{version}-{file_part}.{ext}");
    let sums_url = format!("{dist}/{version}/SHASUMS256.txt");
    let checksum =
        find_checksum(&http.get_text(&sums_url)?, &file_name).ok_or_else(|| Error::Download {
            url: sums_url,
            message: format!("no checksum listed for {file_name}"),
        })?;
    Ok(Release {
        version: version.trim_start_matches('v').to_string(),
        url: format!("{dist}/{version}/{file_name}"),
        file_name,
        checksum,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const INDEX: &str = r#"[
        {"version":"v24.2.0","files":["linux-x64","osx-arm64-tar","win-x64-zip"],"lts":false},
        {"version":"v22.10.1","files":["linux-x64","osx-arm64-tar"],"lts":"Jod"},
        {"version":"v22.9.0","files":["linux-x64","osx-arm64-tar","win-x64-zip"],"lts":"Jod"},
        {"version":"v22.10.0","files":["linux-x64","osx-arm64-tar","win-x64-zip"],"lts":"Jod"},
        {"version":"v2.0.0","files":["linux-x64"],"lts":false}
    ]"#;

    #[test]
    fn picks_the_newest_build_for_the_platform() {
        assert_eq!(
            latest_version(INDEX, "22", "linux-x64").as_deref(),
            Some("v22.10.1")
        );
        // v22.10.1 has no Windows build, so the newest one that does wins.
        assert_eq!(
            latest_version(INDEX, "22", "win-x64-zip").as_deref(),
            Some("v22.10.0")
        );
        assert_eq!(
            latest_version(INDEX, "2", "linux-x64").as_deref(),
            Some("v2.0.0")
        );
        assert_eq!(latest_version(INDEX, "20", "linux-x64"), None);
        assert_eq!(latest_version("not json", "22", "linux-x64"), None);
    }

    #[test]
    fn names_files_like_nodejs_org() {
        let mac = Platform {
            os: Os::Macos,
            arch: Arch::Arm64,
        };
        assert_eq!(naming(mac), ("darwin-arm64", "osx-arm64-tar", "tar.gz"));
    }

    /// Talks to nodejs.org: `cargo test -p stackyard-core -- --ignored live_`.
    #[test]
    #[ignore]
    fn live_resolves_node_22() {
        let release = resolve(
            &Http::new(),
            super::super::NODE_DIST,
            "22",
            Platform::current(),
        )
        .unwrap();
        assert!(release.version.starts_with("22."), "{release:?}");
        assert!(release.url.ends_with(&release.file_name));
    }
}
