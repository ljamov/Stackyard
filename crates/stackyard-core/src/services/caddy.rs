//! Caddy, the web server. Downloads come from the project's official
//! GitHub releases, verified against their published SHA-512 checksums.

use super::{slash, write_config, Context};
use crate::catalog::ServiceDef;
use crate::download::{find_checksum, Http};
use crate::install::Release;
use crate::platform::{Arch, Os, Platform};
use crate::supervisor::ProcessSpec;
use crate::{Error, Result};

const REPO: &str = "caddyserver/caddy";

pub(super) fn asset_name(version: &str, p: Platform) -> String {
    let os = match p.os {
        Os::Linux => "linux",
        Os::Macos => "mac",
        Os::Windows => "windows",
    };
    let arch = match p.arch {
        Arch::X64 => "amd64",
        Arch::Arm64 => "arm64",
    };
    let ext = if p.os == Os::Windows { "zip" } else { "tar.gz" };
    format!("caddy_{version}_{os}_{arch}.{ext}")
}

pub(super) fn resolve(http: &Http, def: &ServiceDef, platform: Platform) -> Result<Release> {
    let latest = super::github::latest(http, REPO)?;
    let version = latest.version().to_string();
    if !version.starts_with(&format!("{}.", def.version)) {
        return Err(Error::Download {
            url: format!("https://github.com/{REPO}/releases"),
            message: format!(
                "the newest Caddy is {version}, outside the {}.x line Stackyard supports",
                def.version
            ),
        });
    }
    let file_name = asset_name(&version, platform);
    let asset = latest.asset(&file_name).ok_or_else(|| Error::NoBuild {
        name: def.name.into(),
        version: version.clone(),
        platform: platform.to_string(),
    })?;
    // Caddy's own SHA-512 list, published with every release.
    let sums_name = format!("caddy_{version}_checksums.txt");
    let sums = latest.asset(&sums_name).ok_or_else(|| Error::Download {
        url: asset.browser_download_url.clone(),
        message: format!("the release has no {sums_name}"),
    })?;
    let checksum = find_checksum(&http.get_text(&sums.browser_download_url)?, &file_name)
        .ok_or_else(|| Error::Download {
            url: sums.browser_download_url.clone(),
            message: format!("no checksum listed for {file_name}"),
        })?;
    Ok(Release {
        version,
        url: asset.browser_download_url.clone(),
        file_name,
        checksum,
    })
}

/// Until sites arrive, Caddy answers on its port with a short page, which
/// shows the web server works end to end.
pub(super) fn caddyfile(ctx: &Context) -> String {
    let port = ctx.def.ports[0];
    let storage = slash(&ctx.data);
    format!(
        "# Written by Stackyard before each start; edits here are replaced.\n\
         {{\n\
         \tadmin off\n\
         \tpersist_config off\n\
         \tauto_https off\n\
         \tstorage file_system \"{storage}\"\n\
         }}\n\
         \n\
         http://127.0.0.1:{port}, http://localhost:{port} {{\n\
         \tbind 127.0.0.1\n\
         \trespond \"Stackyard: Caddy is running.\" 200\n\
         }}\n"
    )
}

pub(super) fn spec(ctx: &Context) -> Result<ProcessSpec> {
    let config = ctx.config.join("Caddyfile");
    write_config(&config, &caddyfile(ctx))?;
    let mut spec = ctx.base_spec(vec![
        "run".into(),
        "--config".into(),
        slash(&config),
        "--adapter".into(),
        "caddyfile".into(),
    ]);
    // Keep Caddy's own files (certificates, autosave) inside Stackyard's folder.
    for var in ["XDG_DATA_HOME", "XDG_CONFIG_HOME"] {
        spec.env.push((var.into(), slash(&ctx.data)));
    }
    Ok(spec)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_assets_like_the_releases() {
        let p = |os, arch| Platform { os, arch };
        assert_eq!(
            asset_name("2.10.2", p(Os::Linux, Arch::X64)),
            "caddy_2.10.2_linux_amd64.tar.gz"
        );
        assert_eq!(
            asset_name("2.10.2", p(Os::Macos, Arch::Arm64)),
            "caddy_2.10.2_mac_arm64.tar.gz"
        );
        assert_eq!(
            asset_name("2.10.2", p(Os::Windows, Arch::X64)),
            "caddy_2.10.2_windows_amd64.zip"
        );
    }
}
