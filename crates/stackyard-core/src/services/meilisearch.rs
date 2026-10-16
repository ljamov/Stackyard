//! Meilisearch, a search engine apps send documents to and query over HTTP.
//! Downloads come from the project's official GitHub releases, where each
//! platform's build is the program itself rather than an archive.

use super::{slash, Context};
use crate::download::Http;
use crate::install::Release;
use crate::platform::{Arch, Os, Platform};
use crate::supervisor::ProcessSpec;
use crate::{Error, Result};

const REPO: &str = "meilisearch/meilisearch";

/// The release file for a platform, or `None` where none is published.
pub(super) fn asset_name(p: Platform) -> Option<&'static str> {
    Some(match (p.os, p.arch) {
        (Os::Linux, Arch::X64) => "meilisearch-linux-amd64",
        (Os::Linux, Arch::Arm64) => "meilisearch-linux-aarch64",
        (Os::Macos, Arch::X64) => "meilisearch-macos-amd64",
        (Os::Macos, Arch::Arm64) => "meilisearch-macos-apple-silicon",
        (Os::Windows, Arch::X64) => "meilisearch-windows-amd64.exe",
        (Os::Windows, Arch::Arm64) => return None,
    })
}

/// Checked against the SHA-256 GitHub records for each uploaded file.
pub(super) fn resolve(http: &Http, platform: Platform) -> Result<Release> {
    let latest = super::github::latest(http, REPO)?;
    let no_build = || Error::NoBuild {
        name: "Meilisearch".into(),
        version: latest.version().into(),
        platform: platform.to_string(),
    };
    let file_name = asset_name(platform).ok_or_else(no_build)?;
    let asset = latest.asset(file_name).ok_or_else(no_build)?;
    let checksum = asset.sha256().ok_or_else(|| Error::Download {
        url: asset.browser_download_url.clone(),
        message: "GitHub lists no checksum for this file".into(),
    })?;
    Ok(Release {
        version: latest.version().to_string(),
        url: asset.browser_download_url.clone(),
        file_name: file_name.to_string(),
        checksum,
    })
}

pub(super) fn spec(ctx: &Context) -> Result<ProcessSpec> {
    let port = ctx.ports[0];
    let mut spec = ctx.base_spec(vec![
        "--http-addr".into(),
        format!("127.0.0.1:{port}"),
        "--db-path".into(),
        slash(&ctx.data.join("data.ms")),
        "--dump-dir".into(),
        slash(&ctx.data.join("dumps")),
        "--snapshot-dir".into(),
        slash(&ctx.data.join("snapshots")),
        // No master key: development mode, the right fit on one's own computer.
        "--env".into(),
        "development".into(),
        "--no-analytics".into(),
    ]);
    // Lets a newer release open indexes an older one wrote, so updating
    // keeps them. Set as a variable so a release that no longer knows it
    // still starts.
    spec.env
        .push(("MEILI_EXPERIMENTAL_DUMPLESS_UPGRADE".into(), "true".into()));
    Ok(spec)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_assets_like_the_releases() {
        let p = |os, arch| Platform { os, arch };
        assert_eq!(
            asset_name(p(Os::Linux, Arch::Arm64)),
            Some("meilisearch-linux-aarch64")
        );
        assert_eq!(
            asset_name(p(Os::Macos, Arch::Arm64)),
            Some("meilisearch-macos-apple-silicon")
        );
        assert_eq!(
            asset_name(p(Os::Windows, Arch::X64)),
            Some("meilisearch-windows-amd64.exe")
        );
        assert_eq!(asset_name(p(Os::Windows, Arch::Arm64)), None);
    }
}
