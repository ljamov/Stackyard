//! MySQL Community Server, from Oracle's official builds. Also holds the
//! start-up logic MariaDB shares, since both take the same server options.
//!
//! Oracle publishes only MD5 sums and GnuPG signatures, so a CI job
//! (`.github/workflows/mysql-manifest.yml`) downloads each official file,
//! checks its signature against Oracle's release key, and publishes a
//! manifest with the file's SHA-256 and its official download address.
//! The app downloads from Oracle and checks that SHA-256.

use std::path::Path;
use std::process::Command;

use super::{slash, Context};
use crate::catalog::ServiceDef;
use crate::download::Http;
use crate::install::Release;
use crate::platform::{Os, Platform};
use crate::supervisor::ProcessSpec;
use crate::{manifest, Error, Result};

const MANIFEST: &str =
    "https://github.com/ljamov/stackyard-php/releases/download/mysql/manifest.json";

pub(super) fn resolve(http: &Http, def: &ServiceDef, platform: Platform) -> Result<Release> {
    let url = std::env::var("STACKYARD_MYSQL_MANIFEST").unwrap_or_else(|_| MANIFEST.into());
    manifest::release(
        &http.get_text(&url)?,
        def.version,
        &manifest::platform_name(platform),
    )
    .ok_or_else(|| Error::NoBuild {
        name: def.name.into(),
        version: def.version.into(),
        platform: platform.to_string(),
    })
}

/// Where the database files live, inside the service's data folder.
pub(super) fn datadir(ctx: &Context) -> std::path::PathBuf {
    ctx.data.join("db")
}

pub(super) fn spec(ctx: &Context) -> Result<ProcessSpec> {
    let datadir = datadir(ctx);
    if !datadir.join("mysql").is_dir() {
        let mut args = vec![
            "--no-defaults".to_string(),
            "--initialize-insecure".to_string(),
            format!("--basedir={}", slash(ctx.install)),
            format!("--datadir={}", slash(&datadir)),
        ];
        if ctx.os == Os::Windows {
            args.push("--console".into());
        }
        initialize(ctx, &ctx.program(), &args, &datadir)?;
    }
    let mut args = server_args(ctx, &datadir);
    // X Protocol is rarely used locally; off saves a port and memory.
    args.push("--mysqlx=OFF".into());
    Ok(ctx.base_spec(args))
}

/// Options both MySQL and MariaDB servers take: their own files only
/// (`--no-defaults` ignores any my.cnf on the system), local connections
/// only, logs to the console so the supervisor captures them.
pub(super) fn server_args(ctx: &Context, datadir: &Path) -> Vec<String> {
    let mut args = vec![
        "--no-defaults".to_string(),
        format!("--basedir={}", slash(ctx.install)),
        format!("--datadir={}", slash(datadir)),
        format!("--port={}", ctx.def.ports[0]),
        "--bind-address=127.0.0.1".to_string(),
    ];
    if ctx.os == Os::Windows {
        args.push("--console".into());
    } else {
        args.push(format!("--socket={}", slash(&ctx.data.join("mysql.sock"))));
    }
    args
}

/// Creates a fresh database folder (root user, no password, local only).
/// On failure the half-made folder is removed so the next start retries.
pub(super) fn initialize(
    ctx: &Context,
    program: &Path,
    args: &[String],
    datadir: &Path,
) -> Result<()> {
    let fail = |message: String| {
        let _ = std::fs::remove_dir_all(datadir);
        Error::ServiceSetup {
            name: ctx.def.name.into(),
            message,
        }
    };
    let output = Command::new(program)
        .args(args)
        .current_dir(&ctx.data)
        .output()
        .map_err(|e| fail(format!("could not run {}: {e}", program.display())))?;
    if output.status.success() {
        return Ok(());
    }
    let mut text = String::from_utf8_lossy(&output.stderr).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stdout));
    let last: Vec<&str> = text.lines().rev().take(5).collect();
    let summary: Vec<&str> = last.into_iter().rev().collect();
    Err(fail(format!(
        "creating the database failed ({}): {}",
        output.status,
        summary.join(" / ")
    )))
}
