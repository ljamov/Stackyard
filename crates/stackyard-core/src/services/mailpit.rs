//! Mailpit catches mail sent by local apps and shows it in a web page.
//! Downloads come from the project's official GitHub releases.

use super::{slash, Context};
use crate::download::Http;
use crate::install::Release;
use crate::platform::{Arch, Os, Platform};
use crate::supervisor::ProcessSpec;
use crate::{Error, Result};

const REPO: &str = "axllent/mailpit";

pub(super) fn asset_name(p: Platform) -> String {
    let os = match p.os {
        Os::Linux => "linux",
        Os::Macos => "darwin",
        Os::Windows => "windows",
    };
    let arch = match p.arch {
        Arch::X64 => "amd64",
        Arch::Arm64 => "arm64",
    };
    let ext = if p.os == Os::Windows { "zip" } else { "tar.gz" };
    format!("mailpit-{os}-{arch}.{ext}")
}

/// Mailpit publishes no checksum file, so the SHA-256 GitHub records for
/// each uploaded file is what the download is checked against.
pub(super) fn resolve(http: &Http, platform: Platform) -> Result<Release> {
    let latest = super::github::latest(http, REPO)?;
    let file_name = asset_name(platform);
    let asset = latest.asset(&file_name).ok_or_else(|| Error::NoBuild {
        name: "Mailpit".into(),
        version: latest.version().into(),
        platform: platform.to_string(),
    })?;
    let checksum = asset.sha256().ok_or_else(|| Error::Download {
        url: asset.browser_download_url.clone(),
        message: "GitHub lists no checksum for this file".into(),
    })?;
    Ok(Release {
        version: latest.version().to_string(),
        url: asset.browser_download_url.clone(),
        file_name,
        checksum,
    })
}

pub(super) fn spec(ctx: &Context) -> Result<ProcessSpec> {
    let [smtp, web] = [ctx.ports[0], ctx.ports[1]];
    Ok(ctx.base_spec(vec![
        "--smtp".into(),
        format!("127.0.0.1:{smtp}"),
        "--listen".into(),
        format!("127.0.0.1:{web}"),
        "--database".into(),
        slash(&ctx.data.join("mailpit.db")),
    ]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_assets_like_the_releases() {
        let p = |os, arch| Platform { os, arch };
        assert_eq!(
            asset_name(p(Os::Linux, Arch::X64)),
            "mailpit-linux-amd64.tar.gz"
        );
        assert_eq!(
            asset_name(p(Os::Macos, Arch::Arm64)),
            "mailpit-darwin-arm64.tar.gz"
        );
        assert_eq!(
            asset_name(p(Os::Windows, Arch::X64)),
            "mailpit-windows-amd64.zip"
        );
    }
}
