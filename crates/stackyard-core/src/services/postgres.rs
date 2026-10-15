//! PostgreSQL. There is no official download for every system, so a CI job
//! (`.github/workflows/postgres-builds.yml`) builds it for macOS and Linux
//! from the official source, checked against postgresql.org's SHA-256, and
//! records the official Windows binaries (from EDB, which the PostgreSQL
//! project links to). It publishes a manifest with each file's SHA-256,
//! which the app checks every download against.
//!
//! The server listens on 127.0.0.1 only, without a Unix socket, so every
//! client connects the same way on every system. The `postgres` user has
//! no password, like MySQL's `root`.

use super::{slash, Context};
use crate::catalog::ServiceDef;
use crate::download::Http;
use crate::install::Release;
use crate::platform::{Os, Platform};
use crate::supervisor::ProcessSpec;
use crate::{manifest, Result};

const MANIFEST: &str =
    "https://github.com/ljamov/Stackyard/releases/download/postgres-builds/manifest.json";

/// The user the database is created with.
pub const USER: &str = "postgres";

pub(super) fn resolve(http: &Http, def: &ServiceDef, platform: Platform) -> Result<Release> {
    let url = std::env::var("STACKYARD_POSTGRES_MANIFEST").unwrap_or_else(|_| MANIFEST.into());
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
    let datadir = ctx.data.join("db");
    if !datadir.join("PG_VERSION").is_file() {
        let exe = if ctx.os == Os::Windows { ".exe" } else { "" };
        let initdb = ctx.install.join(format!("bin/initdb{exe}"));
        let args = [
            format!("--pgdata={}", slash(&datadir)),
            format!("--username={USER}"),
            // Anyone on this computer may connect, as with MySQL's root
            // without a password; the server only listens on 127.0.0.1.
            "--auth=trust".to_string(),
            "--encoding=UTF8".to_string(),
            // The C locale sorts the same on every system and needs no
            // locale data installed.
            "--locale=C".to_string(),
            "--no-instructions".to_string(),
        ];
        super::mysql::initialize(ctx, &initdb, &args, &datadir)?;
    }
    let port = ctx.def.ports[0];
    Ok(ctx.base_spec(vec![
        "-D".into(),
        slash(&datadir),
        "-p".into(),
        port.to_string(),
        "-c".into(),
        "listen_addresses=127.0.0.1".into(),
        "-c".into(),
        "unix_socket_directories=".into(),
        // Logs go to the supervisor through stderr.
        "-c".into(),
        "logging_collector=off".into(),
    ]))
}
