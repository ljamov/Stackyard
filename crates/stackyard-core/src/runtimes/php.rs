//! PHP downloads.
//!
//! - Windows: the official builds from the PHP project (non-thread-safe x64,
//!   the variant meant for FastCGI and php-fpm style setups).
//! - macOS and Linux: self-contained builds made from the official PHP source
//!   by Stackyard's own build job (`.github/workflows/php-builds.yml`), listed
//!   with checksums in a `manifest.json` published next to the files.

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
            crate::manifest::release(&manifest, major, &crate::manifest::platform_name(platform))
                .ok_or_else(no_build)
        }
    }
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
        checksum: sha256.to_ascii_lowercase(),
    })
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
        assert_eq!(release.checksum, SHA_A);
        assert_eq!(
            windows_release(&json, "7.4", base).unwrap().version,
            "7.4.33"
        );
        assert!(windows_release(&json, "8.3", base).is_none());
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
