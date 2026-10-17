//! Java from Eclipse Temurin, the OpenJDK builds of the Eclipse Adoptium
//! project. Its API names the newest build of each Java release for each
//! platform, with the archive's SHA-256 checksum. (jdk.java.net only keeps
//! builds of the newest release, not of the long-term ones projects use.)

use serde::Deserialize;

use super::{Arch, Os, Platform, Release};
use crate::download::{is_sha256, Http};
use crate::{Error, Result};

/// Adoptium's names for a platform: (os, architecture).
fn naming(p: Platform) -> (&'static str, &'static str) {
    let os = match p.os {
        Os::Linux => "linux",
        Os::Macos => "mac",
        Os::Windows => "windows",
    };
    let arch = match p.arch {
        Arch::X64 => "x64",
        Arch::Arm64 => "aarch64",
    };
    (os, arch)
}

#[derive(Deserialize)]
struct Asset {
    binary: Binary,
    version: Version,
}

#[derive(Deserialize)]
struct Binary {
    package: Package,
}

#[derive(Deserialize)]
struct Package {
    name: String,
    link: String,
    checksum: String,
}

#[derive(Deserialize)]
struct Version {
    openjdk_version: String,
}

pub(super) fn resolve(
    http: &Http,
    api: &str,
    feature: &str,
    platform: Platform,
) -> Result<Release> {
    let (os, arch) = naming(platform);
    let url = format!(
        "{api}/assets/latest/{feature}/hotspot?architecture={arch}&image_type=jdk&os={os}&vendor=eclipse"
    );
    pick(&http.get_text(&url)?).ok_or_else(|| Error::NoBuild {
        name: "Java".into(),
        version: feature.into(),
        platform: platform.to_string(),
    })
}

/// The JDK archive in an answer from `/assets/latest`.
pub(super) fn pick(json: &str) -> Option<Release> {
    let assets: Vec<Asset> = serde_json::from_str(json).ok()?;
    assets.into_iter().find_map(|asset| {
        let package = asset.binary.package;
        let archive = package.name.ends_with(".tar.gz") || package.name.ends_with(".zip");
        let safe = !package.name.contains(['/', '\\'])
            && (package.link.starts_with("https://")
                || package.link.starts_with("http://127.0.0.1"));
        (archive && safe && is_sha256(&package.checksum)).then(|| Release {
            version: asset.version.openjdk_version,
            url: package.link,
            file_name: package.name,
            checksum: package.checksum.to_ascii_lowercase(),
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "da68394f9193b7f6b89d0c76861a4034ae10efee7fd55a7255d8118c2acf70d7";

    #[test]
    fn reads_the_adoptium_answer() {
        let json = format!(
            r#"[{{
            "binary": {{
                "architecture": "x64", "image_type": "jdk", "os": "linux",
                "installer": {{"name": "x.msi", "link": "https://example.test/x.msi", "checksum": "{A}"}},
                "package": {{
                    "checksum": "{A}",
                    "link": "https://github.com/adoptium/temurin21-binaries/releases/download/jdk-21.0.9%2B10/OpenJDK21U-jdk_x64_linux_hotspot_21.0.9_10.tar.gz",
                    "name": "OpenJDK21U-jdk_x64_linux_hotspot_21.0.9_10.tar.gz",
                    "size": 1
                }}
            }},
            "release_name": "jdk-21.0.9+10",
            "vendor": "eclipse",
            "version": {{"major": 21, "minor": 0, "security": 9, "build": 10, "openjdk_version": "21.0.9+10", "semver": "21.0.9+10.0.LTS"}}
        }}]"#
        );
        let release = pick(&json).unwrap();
        assert_eq!(release.version, "21.0.9+10");
        assert_eq!(
            release.file_name,
            "OpenJDK21U-jdk_x64_linux_hotspot_21.0.9_10.tar.gz"
        );
        assert!(release.url.ends_with(&release.file_name));
        assert_eq!(release.checksum, A);
        assert!(pick("[]").is_none());
        assert!(pick("nope").is_none());
    }

    /// Talks to api.adoptium.net: `cargo test -p stackyard-core -- --ignored live_`.
    #[test]
    #[ignore]
    fn live_resolves_every_java_version_everywhere() {
        let sources = super::super::Sources::default();
        for version in crate::catalog::runtime(crate::catalog::RuntimeKind::Java).versions {
            for os in [Os::Linux, Os::Macos, Os::Windows] {
                for arch in [Arch::X64, Arch::Arm64] {
                    let platform = Platform { os, arch };
                    match resolve(&Http::new(), &sources.adoptium_api, version, platform) {
                        Ok(release) => assert!(release.version.starts_with(version), "{release:?}"),
                        // Temurin builds for Windows on ARM start at Java 21.
                        Err(Error::NoBuild { .. }) if os == Os::Windows && arch == Arch::Arm64 => {}
                        Err(e) => panic!("Java {version} on {platform}: {e}"),
                    }
                }
            }
        }
    }
}
