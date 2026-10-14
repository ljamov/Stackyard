//! Reads a project's latest release from the GitHub API. Unauthenticated
//! use allows 60 calls an hour per address, plenty for installs. Setting
//! `STACKYARD_GITHUB_TOKEN` raises that limit (CI uses it); the token is only
//! ever sent to api.github.com.

use serde::Deserialize;

use crate::download::{is_checksum, Http};
use crate::{Error, Result};

const API: &str = "https://api.github.com/repos";

#[derive(Debug, Deserialize)]
pub(super) struct Latest {
    pub tag_name: String,
    pub assets: Vec<Asset>,
}

#[derive(Debug, Deserialize)]
pub(super) struct Asset {
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

/// `repo` is `owner/name`, e.g. `axllent/mailpit`.
pub(super) fn latest(http: &Http, repo: &str) -> Result<Latest> {
    let base = std::env::var("STACKYARD_GITHUB_API").unwrap_or_else(|_| API.into());
    let token = std::env::var("STACKYARD_GITHUB_TOKEN")
        .ok()
        .filter(|t| !t.is_empty() && base == API);
    let url = format!("{base}/{repo}/releases/latest");
    parse(&http.get_text_as(&url, token.as_deref())?, repo)
}

fn parse(json: &str, repo: &str) -> Result<Latest> {
    let latest: Latest = serde_json::from_str(json).map_err(|e| Error::Download {
        url: format!("{API}/{repo}/releases/latest"),
        message: format!("unexpected answer: {e}"),
    })?;
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
    Ok(latest)
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
