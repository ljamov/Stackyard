//! NATS, a message queue: one small program, with JetStream switched on so
//! queued messages are kept on disk and survive a restart. Downloads come
//! from the project's official GitHub releases (Apache 2.0).

use super::{slash, Context};
use crate::download::Http;
use crate::install::Release;
use crate::platform::{Arch, Os, Platform};
use crate::supervisor::ProcessSpec;
use crate::Result;

const REPO: &str = "nats-io/nats-server";

/// Release files carry the version: `nats-server-v2.12.1-linux-amd64.tar.gz`.
pub(super) fn asset_names(tag: &str, p: Platform) -> Vec<String> {
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
        .map(|ext| format!("nats-server-{tag}-{os}-{arch}.{ext}"))
        .collect()
}

pub(super) fn resolve(http: &Http, platform: Platform) -> Result<Release> {
    let latest = super::github::latest(http, REPO)?;
    let names = asset_names(&latest.tag_name, platform);
    super::github::release(&latest, "NATS", &names, platform)
}

pub(super) fn spec(ctx: &Context) -> Result<ProcessSpec> {
    let [client, monitor] = [ctx.ports[0], ctx.ports[1]];
    Ok(ctx.base_spec(vec![
        "--addr".into(),
        "127.0.0.1".into(),
        "--port".into(),
        client.to_string(),
        "--http_port".into(),
        monitor.to_string(),
        "--jetstream".into(),
        "--store_dir".into(),
        slash(&ctx.data),
        "--server_name".into(),
        "stackyard".into(),
    ]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_assets_like_the_releases() {
        let p = |os, arch| Platform { os, arch };
        assert_eq!(
            asset_names("v2.12.1", p(Os::Linux, Arch::X64))[0],
            "nats-server-v2.12.1-linux-amd64.tar.gz"
        );
        assert_eq!(
            asset_names("v2.12.1", p(Os::Windows, Arch::Arm64))[0],
            "nats-server-v2.12.1-windows-arm64.zip"
        );
    }
}
