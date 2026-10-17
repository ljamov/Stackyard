//! Python from the portable CPython builds of the python-build-standalone
//! project (the builds `uv` and other Python tools install). python.org only
//! offers installers for macOS and Windows and source code for Linux, none of
//! which unpack into a folder of their own.
//!
//! The project's `latest-release.json` names its newest release; that
//! release's `SHA256SUMS` lists every file in it with its checksum, which is
//! enough to find the newest `3.x.y` for a platform.

use serde::Deserialize;

use super::{Arch, Os, Platform, Release};
use crate::download::{is_sha256, Http};
use crate::{Error, Result};

/// The build's name for a platform.
fn triple(p: Platform) -> &'static str {
    match (p.os, p.arch) {
        (Os::Linux, Arch::X64) => "x86_64-unknown-linux-gnu",
        (Os::Linux, Arch::Arm64) => "aarch64-unknown-linux-gnu",
        (Os::Macos, Arch::X64) => "x86_64-apple-darwin",
        (Os::Macos, Arch::Arm64) => "aarch64-apple-darwin",
        (Os::Windows, Arch::X64) => "x86_64-pc-windows-msvc",
        (Os::Windows, Arch::Arm64) => "aarch64-pc-windows-msvc",
    }
}

#[derive(Deserialize)]
struct Latest {
    tag: String,
    asset_url_prefix: String,
}

pub(super) fn resolve(
    http: &Http,
    latest_url: &str,
    minor: &str,
    platform: Platform,
) -> Result<Release> {
    let latest: Latest =
        serde_json::from_str(&http.get_text(latest_url)?).map_err(|e| Error::Download {
            url: latest_url.into(),
            message: format!("unexpected answer: {e}"),
        })?;
    let base = latest.asset_url_prefix.trim_end_matches('/');
    if !base.starts_with("https://") && !base.starts_with("http://127.0.0.1") {
        return Err(Error::Download {
            url: latest_url.into(),
            message: format!("unexpected download address {base}"),
        });
    }
    let sums = http.get_text(&format!("{base}/SHA256SUMS"))?;
    let (version, file_name, checksum) = pick(&sums, minor, &latest.tag, triple(platform))
        .ok_or_else(|| Error::NoBuild {
            name: "Python".into(),
            version: minor.into(),
            platform: platform.to_string(),
        })?;
    Ok(Release {
        version,
        url: format!("{base}/{}", file_name.replace('+', "%2B")),
        file_name,
        checksum,
    })
}

/// The newest `cpython-3.13.x+<tag>-<triple>-install_only.tar.gz` listed in
/// `SHA256SUMS`, as (version, file name, checksum). Pre-releases such as
/// `3.15.0rc1` are skipped.
pub(super) fn pick(
    sums: &str,
    minor: &str,
    tag: &str,
    triple: &str,
) -> Option<(String, String, String)> {
    let tail = format!("+{tag}-{triple}-install_only.tar.gz");
    sums.lines()
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            let (sum, name) = (words.next()?, words.next()?);
            let version = name.strip_prefix("cpython-")?.strip_suffix(&tail)?;
            let parts = parse(version)?;
            (format!("{}.{}", parts.0, parts.1) == minor && is_sha256(sum)).then(|| {
                (
                    parts,
                    (
                        version.to_string(),
                        name.to_string(),
                        sum.to_ascii_lowercase(),
                    ),
                )
            })
        })
        .max_by_key(|(parts, _)| *parts)
        .map(|(_, found)| found)
}

fn parse(version: &str) -> Option<(u32, u32, u32)> {
    let mut parts = version.split('.').map(|p| p.parse().ok());
    let found = (parts.next()??, parts.next()??, parts.next()??);
    parts.next().is_none().then_some(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "da68394f9193b7f6b89d0c76861a4034ae10efee7fd55a7255d8118c2acf70d7";
    const B: &str = "14ae3250d4447c8ccfc4c45a70d90adfbcd61e728d85f0be56a7ddf8f9c8aace";

    #[test]
    fn picks_the_newest_install_only_build() {
        let sums = format!(
            "{B}  cpython-3.13.1+20261013-x86_64-unknown-linux-gnu-install_only.tar.gz\n\
             {A}  cpython-3.13.9+20261013-x86_64-unknown-linux-gnu-install_only.tar.gz\n\
             {B}  cpython-3.13.9+20261013-x86_64-unknown-linux-gnu-install_only_stripped.tar.gz\n\
             {B}  cpython-3.13.9+20261013-x86_64_v3-unknown-linux-gnu-install_only.tar.gz\n\
             {B}  cpython-3.13.9+20261013-x86_64-unknown-linux-gnu-pgo+lto-full.tar.zst\n\
             {B}  cpython-3.13.10+20261013-aarch64-apple-darwin-install_only.tar.gz\n\
             {B}  cpython-3.15.0rc2+20261013-x86_64-unknown-linux-gnu-install_only.tar.gz\n\
             {B}  cpython-3.1.99+20261013-x86_64-unknown-linux-gnu-install_only.tar.gz\n"
        );
        let linux = "x86_64-unknown-linux-gnu";
        let (version, name, sum) = pick(&sums, "3.13", "20261013", linux).unwrap();
        assert_eq!(version, "3.13.9");
        assert_eq!(
            name,
            "cpython-3.13.9+20261013-x86_64-unknown-linux-gnu-install_only.tar.gz"
        );
        assert_eq!(sum, A);
        assert_eq!(
            pick(&sums, "3.13", "20261013", "aarch64-apple-darwin")
                .unwrap()
                .0,
            "3.13.10"
        );
        assert_eq!(pick(&sums, "3.15", "20261013", linux), None);
        assert_eq!(pick(&sums, "3.13", "20250101", linux), None);
        assert_eq!(pick(&sums, "3.1", "20261013", linux).unwrap().0, "3.1.99");
    }

    #[test]
    fn resolves_through_latest_release_json() {
        use crate::download::testserver;
        let name = "cpython-3.12.4+20261013-x86_64-unknown-linux-gnu-install_only.tar.gz";
        let sums = format!("{A}  {name}\n");
        let base = testserver::serve(sums.into_bytes());
        let latest = format!(r#"{{"version":1,"tag":"20261013","asset_url_prefix":"{base}"}}"#);
        let latest_url = testserver::serve(latest.into_bytes());
        let linux = Platform {
            os: Os::Linux,
            arch: Arch::X64,
        };
        let release = resolve(&Http::new(), &latest_url, "3.12", linux).unwrap();
        assert_eq!(release.version, "3.12.4");
        assert_eq!(release.file_name, name);
        assert!(release
            .url
            .ends_with("cpython-3.12.4%2B20261013-x86_64-unknown-linux-gnu-install_only.tar.gz"));
        assert_eq!(release.checksum, A);
    }

    /// Talks to GitHub: `cargo test -p stackyard-core -- --ignored live_`.
    #[test]
    #[ignore]
    fn live_resolves_every_python_version_everywhere() {
        let sources = super::super::Sources::default();
        for version in crate::catalog::runtime(crate::catalog::RuntimeKind::Python).versions {
            for os in [Os::Linux, Os::Macos, Os::Windows] {
                for arch in [Arch::X64, Arch::Arm64] {
                    let platform = Platform { os, arch };
                    match resolve(&Http::new(), &sources.python_latest, version, platform) {
                        Ok(release) => assert!(release.version.starts_with(&format!("{version}."))),
                        // Builds for Windows on ARM start at newer versions.
                        Err(Error::NoBuild { .. }) if os == Os::Windows && arch == Arch::Arm64 => {}
                        Err(e) => panic!("Python {version} on {platform}: {e}"),
                    }
                }
            }
        }
    }
}
