//! PHP downloads.
//!
//! - Windows: the official builds from the PHP project (non-thread-safe x64,
//!   the variant meant for FastCGI and php-fpm style setups).
//! - macOS and Linux: self-contained builds made from the official PHP source
//!   by Stackyard's own build job (`.github/workflows/php-builds.yml`), listed
//!   with checksums in a `manifest.json` published next to the files.

use serde::Deserialize;

use super::{Os, Platform, Release};
use crate::download::{is_sha256, Http};
use crate::{Error, Result};

pub(super) fn resolve(
    http: &Http,
    sources: &super::Sources,
    major: &str,
    platform: Platform,
) -> Result<Release> {
    let no_build = || Error::NoBuild {
        name: "PHP".into(),
        version: major.into(),
        platform: platform.to_string(),
    };
    match platform.os {
        Os::Windows => {
            let base = sources.php_windows.trim_end_matches('/');
            let url = format!("{base}/releases.json");
            windows_release(&http.get_text(&url)?, major, base).ok_or_else(no_build)
        }
        Os::Macos | Os::Linux => {
            let manifest = http.get_text(&sources.php_manifest)?;
            manifest_release(&manifest, major, &build_platform(platform)).ok_or_else(no_build)
        }
    }
}

/// Platform names used by the build job: `linux-x86_64`, `macos-aarch64`, ...
pub fn build_platform(p: Platform) -> String {
    let os = match p.os {
        Os::Linux => "linux",
        Os::Macos => "macos",
        Os::Windows => "windows",
    };
    let arch = match p.arch {
        super::Arch::X64 => "x86_64",
        super::Arch::Arm64 => "aarch64",
    };
    format!("{os}-{arch}")
}

/// Reads the PHP project's Windows `releases.json`, which maps each
/// `MAJOR.MINOR` to its current version and one entry per build variant.
pub(super) fn windows_release(json: &str, major: &str, base: &str) -> Option<Release> {
    let all: serde_json::Value = serde_json::from_str(json).ok()?;
    let entry = all.get(major)?.as_object()?;
    let version = entry.get("version")?.as_str()?;
    let zip = entry
        .iter()
        .find(|(key, _)| key.starts_with("nts-") && key.ends_with("-x64"))?
        .1
        .get("zip")?;
    let file_name = zip.get("path")?.as_str()?;
    let sha256 = zip.get("sha256")?.as_str()?;
    if !is_sha256(sha256) || file_name.contains(['/', '\\']) {
        return None;
    }
    Some(Release {
        version: version.to_string(),
        url: format!("{base}/{file_name}"),
        file_name: file_name.to_string(),
        sha256: sha256.to_ascii_lowercase(),
    })
}

#[derive(Deserialize)]
struct Manifest {
    builds: Vec<Build>,
}

#[derive(Deserialize)]
struct Build {
    major: String,
    version: String,
    platform: String,
    file: String,
    url: String,
    sha256: String,
}

/// Newest build for `major` on `platform` in the build job's manifest.
pub(super) fn manifest_release(json: &str, major: &str, platform: &str) -> Option<Release> {
    let manifest: Manifest = serde_json::from_str(json).ok()?;
    manifest
        .builds
        .into_iter()
        .filter(|b| b.major == major && b.platform == platform && is_sha256(&b.sha256))
        .filter(|b| !b.file.contains(['/', '\\']))
        .max_by_key(|b| version_key(&b.version))
        .map(|b| Release {
            version: b.version,
            url: b.url,
            file_name: b.file,
            sha256: b.sha256.to_ascii_lowercase(),
        })
}

fn version_key(v: &str) -> Vec<u32> {
    v.split('.').map(|p| p.parse().unwrap_or(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHA_A: &str = "da68394f9193b7f6b89d0c76861a4034ae10efee7fd55a7255d8118c2acf70d7";
    const SHA_B: &str = "14ae3250d4447c8ccfc4c45a70d90adfbcd61e728d85f0be56a7ddf8f9c8aace";

    #[test]
    fn reads_windows_releases_json() {
        let json = format!(
            r#"{{
            "8.4": {{
                "version": "8.4.26",
                "ts-vs17-x64": {{"zip": {{"path": "php-8.4.26-Win32-vs17-x64.zip", "sha256": "{SHA_B}"}}}},
                "source": {{"path": "php-8.4.26-src.zip", "sha256": "{SHA_B}"}},
                "nts-vs17-x86": {{"zip": {{"path": "php-8.4.26-nts-Win32-vs17-x86.zip", "sha256": "{SHA_B}"}}}},
                "nts-vs17-x64": {{"zip": {{"path": "php-8.4.26-nts-Win32-vs17-x64.zip", "size": "33.48MB", "sha256": "{SHA_A}"}}}}
            }},
            "7.4": {{
                "version": "7.4.33",
                "nts-vc15-x64": {{"zip": {{"path": "php-7.4.33-nts-Win32-vc15-x64.zip", "sha256": "{SHA_B}"}}}}
            }}
        }}"#
        );
        let base = "https://example.test/releases";
        let release = windows_release(&json, "8.4", base).unwrap();
        assert_eq!(release.version, "8.4.26");
        assert_eq!(release.file_name, "php-8.4.26-nts-Win32-vs17-x64.zip");
        assert_eq!(
            release.url,
            format!("{base}/php-8.4.26-nts-Win32-vs17-x64.zip")
        );
        assert_eq!(release.sha256, SHA_A);
        assert_eq!(
            windows_release(&json, "7.4", base).unwrap().version,
            "7.4.33"
        );
        assert!(windows_release(&json, "8.3", base).is_none());
    }

    #[test]
    fn reads_the_build_manifest() {
        let json = format!(
            r#"{{"schema": 1, "builds": [
            {{"major": "8.4", "version": "8.4.9", "platform": "linux-x86_64", "file": "php-8.4.9-linux-x86_64.tar.gz", "url": "https://dl.test/php-8.4.9-linux-x86_64.tar.gz", "sha256": "{SHA_B}"}},
            {{"major": "8.4", "version": "8.4.10", "platform": "linux-x86_64", "file": "php-8.4.10-linux-x86_64.tar.gz", "url": "https://dl.test/php-8.4.10-linux-x86_64.tar.gz", "sha256": "{SHA_A}"}},
            {{"major": "8.4", "version": "8.4.11", "platform": "macos-aarch64", "file": "php-8.4.11-macos-aarch64.tar.gz", "url": "https://dl.test/x", "sha256": "{SHA_A}"}},
            {{"major": "8.3", "version": "8.3.1", "platform": "linux-x86_64", "file": "../evil.tar.gz", "url": "https://dl.test/y", "sha256": "{SHA_A}"}}
        ]}}"#
        );
        let release = manifest_release(&json, "8.4", "linux-x86_64").unwrap();
        assert_eq!(release.version, "8.4.10", "8.4.10 is newer than 8.4.9");
        assert_eq!(release.sha256, SHA_A);
        assert!(
            manifest_release(&json, "8.3", "linux-x86_64").is_none(),
            "unsafe file name"
        );
        assert!(manifest_release(&json, "7.4", "linux-x86_64").is_none());
    }

    #[test]
    fn names_build_platforms() {
        let p = Platform {
            os: Os::Macos,
            arch: super::super::Arch::Arm64,
        };
        assert_eq!(build_platform(p), "macos-aarch64");
    }

    /// Talks to php.net: `cargo test -p stackyard-core -- --ignored live_`.
    #[test]
    #[ignore]
    fn live_resolves_php_for_windows() {
        let sources = super::super::Sources::default();
        let windows = Platform {
            os: Os::Windows,
            arch: super::super::Arch::X64,
        };
        let release = resolve(&Http::new(), &sources, "8.4", windows).unwrap();
        assert!(release.version.starts_with("8.4."), "{release:?}");
    }
}
