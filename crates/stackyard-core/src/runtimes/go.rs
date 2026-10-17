//! Go from the official downloads at go.dev, which lists every release with
//! its files and their SHA-256 checksums as JSON.

use serde::Deserialize;

use super::{Arch, Os, Platform, Release};
use crate::download::{is_sha256, Http};
use crate::{Error, Result};

/// go.dev's names for a platform: (os, arch).
fn naming(p: Platform) -> (&'static str, &'static str) {
    let os = match p.os {
        Os::Linux => "linux",
        Os::Macos => "darwin",
        Os::Windows => "windows",
    };
    let arch = match p.arch {
        Arch::X64 => "amd64",
        Arch::Arm64 => "arm64",
    };
    (os, arch)
}

#[derive(Deserialize)]
struct GoRelease {
    version: String,
    stable: bool,
    files: Vec<GoFile>,
}

#[derive(Deserialize)]
struct GoFile {
    filename: String,
    os: String,
    arch: String,
    sha256: String,
    kind: String,
}

pub(super) fn resolve(http: &Http, dl: &str, minor: &str, platform: Platform) -> Result<Release> {
    let url = format!("{dl}/?mode=json&include=all");
    let json = http.get_text(&url)?;
    let (version, file_name, checksum) =
        pick(&json, minor, platform).ok_or_else(|| Error::NoBuild {
            name: "Go".into(),
            version: minor.into(),
            platform: platform.to_string(),
        })?;
    Ok(Release {
        version,
        url: format!("{dl}/{file_name}"),
        file_name,
        checksum,
    })
}

/// The newest stable `go1.23.x` archive for `platform`, as (version, file
/// name, checksum).
pub(super) fn pick(
    json: &str,
    minor: &str,
    platform: Platform,
) -> Option<(String, String, String)> {
    let (os, arch) = naming(platform);
    let releases: Vec<GoRelease> = serde_json::from_str(json).ok()?;
    releases
        .into_iter()
        .filter(|r| r.stable)
        .filter_map(|r| {
            let parts = parse(&r.version)?;
            if format!("{}.{}", parts.0, parts.1) != minor {
                return None;
            }
            let file = r.files.into_iter().find(|f| {
                f.os == os
                    && f.arch == arch
                    && f.kind == "archive"
                    && is_sha256(&f.sha256)
                    && !f.filename.contains(['/', '\\'])
            })?;
            let version = r.version.trim_start_matches("go").to_string();
            Some((
                parts,
                (version, file.filename, file.sha256.to_ascii_lowercase()),
            ))
        })
        .max_by_key(|(parts, _)| *parts)
        .map(|(_, found)| found)
}

/// `go1.23.4` → (1, 23, 4); `go1.20` → (1, 20, 0). `go1.24rc1` → `None`.
fn parse(version: &str) -> Option<(u32, u32, u32)> {
    let mut parts = version
        .strip_prefix("go")?
        .split('.')
        .map(|p| p.parse().ok());
    let major = parts.next()??;
    let minor = parts.next()??;
    let patch = match parts.next() {
        Some(p) => p?,
        None => 0,
    };
    parts.next().is_none().then_some((major, minor, patch))
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "da68394f9193b7f6b89d0c76861a4034ae10efee7fd55a7255d8118c2acf70d7";
    const B: &str = "14ae3250d4447c8ccfc4c45a70d90adfbcd61e728d85f0be56a7ddf8f9c8aace";

    fn json() -> String {
        format!(
            r#"[
            {{"version":"go1.27rc1","stable":false,"files":[
                {{"filename":"go1.27rc1.linux-amd64.tar.gz","os":"linux","arch":"amd64","version":"go1.27rc1","sha256":"{B}","size":1,"kind":"archive"}}]}},
            {{"version":"go1.26.10","stable":true,"files":[
                {{"filename":"go1.26.10.src.tar.gz","os":"","arch":"","version":"go1.26.10","sha256":"{B}","size":1,"kind":"source"}},
                {{"filename":"go1.26.10.windows-amd64.msi","os":"windows","arch":"amd64","version":"go1.26.10","sha256":"{B}","size":1,"kind":"installer"}},
                {{"filename":"go1.26.10.windows-amd64.zip","os":"windows","arch":"amd64","version":"go1.26.10","sha256":"{A}","size":1,"kind":"archive"}},
                {{"filename":"go1.26.10.linux-amd64.tar.gz","os":"linux","arch":"amd64","version":"go1.26.10","sha256":"{A}","size":1,"kind":"archive"}}]}},
            {{"version":"go1.26.9","stable":true,"files":[
                {{"filename":"go1.26.9.linux-amd64.tar.gz","os":"linux","arch":"amd64","version":"go1.26.9","sha256":"{B}","size":1,"kind":"archive"}}]}},
            {{"version":"go1.2","stable":true,"files":[
                {{"filename":"go1.2.linux-amd64.tar.gz","os":"linux","arch":"amd64","version":"go1.2","sha256":"{B}","size":1,"kind":"archive"}}]}}
        ]"#
        )
    }

    #[test]
    fn picks_the_newest_stable_archive() {
        let linux = Platform {
            os: Os::Linux,
            arch: Arch::X64,
        };
        let windows = Platform {
            os: Os::Windows,
            arch: Arch::X64,
        };
        let found = pick(&json(), "1.26", linux).unwrap();
        assert_eq!(
            found,
            (
                "1.26.10".into(),
                "go1.26.10.linux-amd64.tar.gz".into(),
                A.into()
            )
        );
        assert_eq!(
            pick(&json(), "1.26", windows).unwrap().1,
            "go1.26.10.windows-amd64.zip"
        );
        assert_eq!(pick(&json(), "1.27", linux), None);
        assert_eq!(pick(&json(), "1.2", linux).unwrap().0, "1.2");
        let mac = Platform {
            os: Os::Macos,
            arch: Arch::Arm64,
        };
        assert_eq!(pick(&json(), "1.26", mac), None);
    }

    /// Talks to go.dev: `cargo test -p stackyard-core -- --ignored live_`.
    #[test]
    #[ignore]
    fn live_resolves_every_go_version_everywhere() {
        let sources = super::super::Sources::default();
        for version in crate::catalog::runtime(crate::catalog::RuntimeKind::Go).versions {
            for os in [Os::Linux, Os::Macos, Os::Windows] {
                for arch in [Arch::X64, Arch::Arm64] {
                    let platform = Platform { os, arch };
                    let release = resolve(&Http::new(), &sources.go_dl, version, platform)
                        .unwrap_or_else(|e| panic!("Go {version} on {platform}: {e}"));
                    assert!(release.version.starts_with(version), "{release:?}");
                }
            }
        }
    }
}
