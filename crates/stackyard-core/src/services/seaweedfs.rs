//! SeaweedFS, S3-compatible file storage for apps that keep uploads in a
//! bucket. It stands in for MinIO, whose free edition stopped publishing
//! builds in 2025. Downloads come from the project's official GitHub
//! releases (Apache 2.0); each archive holds the single `weed` program.
//!
//! It runs as `weed mini`: one process with the S3 endpoint, a page to
//! manage buckets and keys, and the storage parts behind them, all on
//! 127.0.0.1. Apps sign in with [`ACCESS_KEY`] and [`SECRET_KEY`].

use super::{slash, Context};
use crate::download::Http;
use crate::install::Release;
use crate::platform::{Arch, Os, Platform};
use crate::supervisor::ProcessSpec;
use crate::Result;

const REPO: &str = "seaweedfs/seaweedfs";

/// The key apps use, like an AWS access key ID.
pub const ACCESS_KEY: &str = "stackyard";
/// The secret that goes with it. Only reachable from this computer.
pub const SECRET_KEY: &str = "stackyard-secret";

/// The release files for a platform, the preferred one first.
pub(super) fn asset_names(p: Platform) -> Vec<String> {
    let os = match p.os {
        Os::Linux => "linux",
        Os::Macos => "darwin",
        Os::Windows => "windows",
    };
    let arch = match p.arch {
        Arch::X64 => "amd64",
        Arch::Arm64 => "arm64",
    };
    let exts: &[&str] = if p.os == Os::Windows {
        &["zip", "tar.gz"]
    } else {
        &["tar.gz", "zip"]
    };
    exts.iter()
        .map(|ext| format!("{os}_{arch}.{ext}"))
        .collect()
}

pub(super) fn resolve(http: &Http, platform: Platform) -> Result<Release> {
    let latest = super::github::latest(http, REPO)?;
    super::github::release(&latest, "SeaweedFS", &asset_names(platform), platform)
}

pub(super) fn spec(ctx: &Context) -> Result<ProcessSpec> {
    let [s3, admin, master, volume, filer] = [
        ctx.ports[0],
        ctx.ports[1],
        ctx.ports[2],
        ctx.ports[3],
        ctx.ports[4],
    ];
    let mut spec = ctx.base_spec(vec![
        "mini".into(),
        format!("-dir={}", slash(&ctx.data)),
        "-ip=127.0.0.1".into(),
        "-ip.bind=127.0.0.1".into(),
        format!("-s3.port={s3}"),
        format!("-admin.port={admin}"),
        format!("-master.port={master}"),
        format!("-volume.port={volume}"),
        format!("-filer.port={filer}"),
        // Parts a local app doesn't need, each on its own port.
        "-webdav=false".into(),
        "-s3.port.iceberg=0".into(),
        "-master.telemetry=false".into(),
    ]);
    // The first start makes this user; after that every request must be
    // signed with it, as with S3 itself.
    spec.env
        .push(("AWS_ACCESS_KEY_ID".into(), ACCESS_KEY.into()));
    spec.env
        .push(("AWS_SECRET_ACCESS_KEY".into(), SECRET_KEY.into()));
    Ok(spec)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_assets_like_the_releases() {
        let p = |os, arch| Platform { os, arch };
        assert_eq!(
            asset_names(p(Os::Linux, Arch::X64))[0],
            "linux_amd64.tar.gz"
        );
        assert_eq!(
            asset_names(p(Os::Macos, Arch::Arm64))[0],
            "darwin_arm64.tar.gz"
        );
        assert_eq!(
            asset_names(p(Os::Windows, Arch::X64))[0],
            "windows_amd64.zip"
        );
    }
}
