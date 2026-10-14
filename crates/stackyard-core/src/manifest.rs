//! Manifests published by Stackyard's own CI jobs (PHP builds for macOS and
//! Linux, checked MySQL downloads): a list of files with their SHA-256.
//!
//! ```json
//! {"schema": 1, "builds": [{"major": "8.4", "version": "8.4.26",
//!   "platform": "linux-x86_64", "file": "php-8.4.26-linux-x86_64.tar.gz",
//!   "url": "https://...", "sha256": "..."}]}
//! ```

use serde::Deserialize;

use crate::download::is_sha256;
use crate::install::Release;
use crate::platform::{Arch, Os, Platform};

/// Platform names used in manifests: `linux-x86_64`, `macos-aarch64`, ...
pub(crate) fn platform_name(p: Platform) -> String {
    let os = match p.os {
        Os::Linux => "linux",
        Os::Macos => "macos",
        Os::Windows => "windows",
    };
    let arch = match p.arch {
        Arch::X64 => "x86_64",
        Arch::Arm64 => "aarch64",
    };
    format!("{os}-{arch}")
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
pub(crate) fn release(json: &str, major: &str, platform: &str) -> Option<Release> {
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
            checksum: b.sha256.to_ascii_lowercase(),
        })
}

fn version_key(v: &str) -> Vec<u32> {
    v.split('.').map(|p| p.parse().unwrap_or(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_build_platforms() {
        let p = Platform {
            os: Os::Macos,
            arch: Arch::Arm64,
        };
        assert_eq!(platform_name(p), "macos-aarch64");
    }

    const SHA_A: &str = "da68394f9193b7f6b89d0c76861a4034ae10efee7fd55a7255d8118c2acf70d7";
    const SHA_B: &str = "14ae3250d4447c8ccfc4c45a70d90adfbcd61e728d85f0be56a7ddf8f9c8aace";

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
        let found = release(&json, "8.4", "linux-x86_64").unwrap();
        assert_eq!(found.version, "8.4.10", "8.4.10 is newer than 8.4.9");
        assert_eq!(found.checksum, SHA_A);
        assert!(
            release(&json, "8.3", "linux-x86_64").is_none(),
            "unsafe file name"
        );
        assert!(release(&json, "7.4", "linux-x86_64").is_none());
    }
}
