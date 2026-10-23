//! Reads a project's latest release from the GitHub API. Unauthenticated
//! use allows 60 calls an hour per address, plenty for installs. Setting
//! `STACKYARD_GITHUB_TOKEN` raises that limit (CI uses it); the token is only
//! ever sent to api.github.com.

use serde::Deserialize;

use crate::download::{is_checksum, Http};
use crate::{Error, Result};

const API: &str = "https://api.github.com/repos";

#[derive(Debug, Deserialize)]
pub(crate) struct Latest {
    pub tag_name: String,
    pub assets: Vec<Asset>,
    #[serde(default)]
    pub draft: bool,
    #[serde(default)]
    pub prerelease: bool,
}

#[derive(Debug, Deserialize)]
pub(crate) struct Asset {
    pub name: String,
    pub browser_download_url: String,
    /// `sha256:<hex>`, computed by GitHub when the file was uploaded.
    #[serde(default)]
    pub digest: Option<String>,
}

impl Latest {
    /// Version without the leading `v`, e.g. `1.31.5`.
    pub fn version(&self) -> &str {
        self.tag_name.trim_start_matches('v')
    }

    pub fn asset(&self, name: &str) -> Option<&Asset> {
        self.assets.iter().find(|a| a.name == name)
    }
}

impl Asset {
    /// The SHA-256 GitHub recorded for this file, if any.
    pub fn sha256(&self) -> Option<String> {
        let hex = self.digest.as_deref()?.strip_prefix("sha256:")?;
        (hex.len() == 64 && is_checksum(hex)).then(|| hex.to_ascii_lowercase())
    }
}

/// The first of `names` the release has, checked against the SHA-256
/// GitHub records for each uploaded file.
pub(crate) fn release(
    latest: &Latest,
    name: &str,
    names: &[String],
    platform: crate::platform::Platform,
) -> Result<crate::install::Release> {
    let asset = names
        .iter()
        .find_map(|n| latest.asset(n))
        .ok_or_else(|| Error::NoBuild {
            name: name.into(),
            version: latest.version().into(),
            platform: platform.to_string(),
        })?;
    let checksum = asset.sha256().ok_or_else(|| Error::Download {
        url: asset.browser_download_url.clone(),
        message: "GitHub lists no checksum for this file".into(),
    })?;
    Ok(crate::install::Release {
        version: latest.version().to_string(),
        url: asset.browser_download_url.clone(),
        file_name: asset.name.clone(),
        checksum,
    })
}

/// Like [`release`] on the latest release, but when that one has no file
/// for this platform (a project still uploading its builds, or one that
/// skipped a platform), the newest of the last few releases that has one.
pub(crate) fn newest_with(
    http: &Http,
    repo: &str,
    name: &str,
    names: &[String],
    platform: crate::platform::Platform,
) -> Result<crate::install::Release> {
    let latest = latest(http, repo)?;
    match release(&latest, name, names, platform) {
        Err(Error::NoBuild { .. }) => {}
        other => return other,
    }
    let json = get(http, &format!("{repo}/releases?per_page=10"))?;
    let list: Vec<Latest> = serde_json::from_str(&json).map_err(|e| Error::Download {
        url: format!("{API}/{repo}/releases"),
        message: format!("unexpected answer: {e}"),
    })?;
    list.into_iter()
        .filter(|r| !r.draft && !r.prerelease && check_tag(r, repo).is_ok())
        .find(|r| names.iter().any(|n| r.asset(n).is_some()))
        .map_or_else(
            || release(&latest, name, names, platform),
            |r| release(&r, name, names, platform),
        )
}

/// `repo` is `owner/name`, e.g. `axllent/mailpit`.
pub(crate) fn latest(http: &Http, repo: &str) -> Result<Latest> {
    parse(&get(http, &format!("{repo}/releases/latest"))?, repo)
}

/// `path` follows the repos API base, e.g. `owner/name/releases/latest`.
fn get(http: &Http, path: &str) -> Result<String> {
    let base = std::env::var("STACKYARD_GITHUB_API").unwrap_or_else(|_| API.into());
    let token = std::env::var("STACKYARD_GITHUB_TOKEN")
        .ok()
        .filter(|t| !t.is_empty() && base == API);
    http.get_text_as(&format!("{base}/{path}"), token.as_deref())
}

fn parse(json: &str, repo: &str) -> Result<Latest> {
    let latest: Latest = serde_json::from_str(json).map_err(|e| Error::Download {
        url: format!("{API}/{repo}/releases/latest"),
        message: format!("unexpected answer: {e}"),
    })?;
    check_tag(&latest, repo)?;
    Ok(latest)
}

fn check_tag(latest: &Latest, repo: &str) -> Result<()> {
    let tag_ok = !latest.tag_name.is_empty()
        && latest
            .tag_name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b));
    if !tag_ok {
        return Err(Error::Download {
            url: format!("{API}/{repo}/releases/latest"),
            message: format!("unexpected release tag {:?}", latest.tag_name),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHA: &str = "5924ace84f38c83c29f941a644a2d7be6c11f339eb4a9a12ab526414897ceed1";

    #[test]
    fn reads_tag_assets_and_digests() {
        let json = format!(
            r#"{{"tag_name": "v1.31.5", "name": "v1.31.5", "assets": [
                {{"name": "mailpit-linux-amd64.tar.gz", "browser_download_url": "https://github.com/axllent/mailpit/releases/download/v1.31.5/mailpit-linux-amd64.tar.gz", "digest": "sha256:{SHA}"}},
                {{"name": "old.tar.gz", "browser_download_url": "https://x/old.tar.gz"}}
            ]}}"#
        );
        let latest = parse(&json, "axllent/mailpit").unwrap();
        assert_eq!(latest.version(), "1.31.5");
        let asset = latest.asset("mailpit-linux-amd64.tar.gz").unwrap();
        assert_eq!(asset.sha256().as_deref(), Some(SHA));
        assert_eq!(latest.asset("old.tar.gz").unwrap().sha256(), None);
        assert!(latest.asset("nope").is_none());

        let evil = r#"{"tag_name": "../../x", "assets": []}"#;
        assert!(parse(evil, "a/b").is_err());
        assert!(parse("not json", "a/b").is_err());
    }
}
