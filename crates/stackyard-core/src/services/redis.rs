//! Redis, for caches and queues. Redis publishes source only, so a CI job
//! (`.github/workflows/redis-builds.yml`) builds it for macOS and Linux from
//! the official source, checked against the SHA-256 Redis publishes, and
//! publishes a manifest with each archive's SHA-256. Redis has no Windows
//! release, so Stackyard doesn't offer it there.

use super::{slash, Context};
use crate::catalog::ServiceDef;
use crate::download::Http;
use crate::install::Release;
use crate::platform::Platform;
use crate::supervisor::ProcessSpec;
use crate::{manifest, Result};

const MANIFEST: &str =
    "https://github.com/ljamov/Stackyard/releases/download/redis-builds/manifest.json";

pub(super) fn resolve(http: &Http, def: &ServiceDef, platform: Platform) -> Result<Release> {
    let url = std::env::var("STACKYARD_REDIS_MANIFEST").unwrap_or_else(|_| MANIFEST.into());
    manifest::release(
        &http.get_text(&url)?,
        def.version,
        &manifest::platform_name(platform),
    )
    .ok_or_else(|| crate::Error::NoBuild {
        name: def.name.into(),
        version: def.version.into(),
        platform: platform.to_string(),
    })
}

pub(super) fn spec(ctx: &Context) -> Result<ProcessSpec> {
    let port = ctx.def.ports[0];
    Ok(ctx.base_spec(vec![
        "--bind".into(),
        "127.0.0.1".into(),
        "--port".into(),
        port.to_string(),
        "--dir".into(),
        slash(&ctx.data),
        // Saved to disk now and then, so queued jobs survive a restart.
        "--save".into(),
        "300 1 60 1000".into(),
        "--appendonly".into(),
        "no".into(),
        "--daemonize".into(),
        "no".into(),
        // Logs go to the supervisor through stdout.
        "--logfile".into(),
        "".into(),
    ]))
}
