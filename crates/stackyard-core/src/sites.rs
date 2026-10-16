//! Local sites such as `https://myapp.stack`, stored in `sites.toml`.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::catalog::RuntimeKind;
use crate::settings::{read_toml, write_toml};
use crate::{domain, paths, Error, Result};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase", tag = "kind")]
pub enum SiteRuntime {
    /// Served through PHP-FPM (or php-cgi on Windows).
    Php { version: String },
    /// Proxied to the app's own dev server.
    Node { version: String, port: u16 },
    /// Plain files from the project folder.
    Static,
}

impl SiteRuntime {
    pub fn kind(&self) -> Option<RuntimeKind> {
        match self {
            SiteRuntime::Php { .. } => Some(RuntimeKind::Php),
            SiteRuntime::Node { .. } => Some(RuntimeKind::Node),
            SiteRuntime::Static => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Site {
    /// `myapp` in `myapp.stack`.
    pub name: String,
    /// Domain ending this site was created with, e.g. `.stack`.
    pub tld: String,
    pub path: PathBuf,
    pub runtime: SiteRuntime,
    pub https: bool,
    /// Node sites only: Stackyard runs the project's `npm run dev` while the
    /// web server runs, so the site works without a terminal.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub dev_server: bool,
}

impl Site {
    pub fn domain(&self) -> String {
        domain::site_domain(&self.name, &self.tld)
    }

    pub fn url(&self) -> String {
        let scheme = if self.https { "https" } else { "http" };
        format!("{scheme}://{}", self.domain())
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct SiteStore {
    #[serde(default, rename = "site")]
    sites: Vec<Site>,
    #[serde(skip)]
    file: PathBuf,
}

impl SiteStore {
    pub fn load() -> Result<Self> {
        Self::load_from(&paths::sites_file())
    }

    pub fn load_from(path: &Path) -> Result<Self> {
        let mut store: SiteStore = read_toml(path)?.unwrap_or_default();
        store.file = path.to_owned();
        Ok(store)
    }

    pub fn save(&self) -> Result<()> {
        write_toml(&self.file, self)
    }

    pub fn list(&self) -> &[Site] {
        &self.sites
    }

    pub fn add(&mut self, site: Site) -> Result<&Site> {
        domain::validate_site_name(&site.name)?;
        let tld = domain::normalize_tld(&site.tld)?;
        let path = paths::expand_home(&site.path);
        let site = Site { tld, path, ..site };
        if self.sites.iter().any(|s| s.domain() == site.domain()) {
            return Err(Error::SiteExists(site.domain()));
        }
        self.sites.push(site);
        Ok(self.sites.last().expect("just pushed"))
    }

    /// A site by its domain (`myapp.stack`) or bare name (`myapp`), to change it.
    pub fn get_mut(&mut self, name_or_domain: &str) -> Result<&mut Site> {
        self.sites
            .iter_mut()
            .find(|s| s.domain() == name_or_domain || s.name == name_or_domain)
            .ok_or_else(|| Error::SiteNotFound(name_or_domain.to_string()))
    }

    /// Removes a site by its domain (`myapp.stack`) or bare name (`myapp`).
    pub fn remove(&mut self, name_or_domain: &str) -> Result<Site> {
        let index = self
            .sites
            .iter()
            .position(|s| s.domain() == name_or_domain || s.name == name_or_domain)
            .ok_or_else(|| Error::SiteNotFound(name_or_domain.to_string()))?;
        Ok(self.sites.remove(index))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn php_site(name: &str) -> Site {
        Site {
            name: name.into(),
            tld: ".stack".into(),
            path: PathBuf::from("/code").join(name),
            runtime: SiteRuntime::Php {
                version: "8.3".into(),
            },
            https: true,
            dev_server: false,
        }
    }

    #[test]
    fn adds_lists_and_removes() {
        let path = crate::testutil::tempdir().join("sites.toml");
        let mut store = SiteStore::load_from(&path).unwrap();
        assert!(store.list().is_empty());

        store.add(php_site("myapp")).unwrap();
        store
            .add(Site {
                runtime: SiteRuntime::Node {
                    version: "22".into(),
                    port: 3000,
                },
                ..php_site("shop")
            })
            .unwrap();
        store.save().unwrap();

        let mut reloaded = SiteStore::load_from(&path).unwrap();
        assert_eq!(reloaded.list().len(), 2);
        assert_eq!(reloaded.list()[0].url(), "https://myapp.stack");

        reloaded.remove("myapp.stack").unwrap();
        reloaded.remove("shop").unwrap();
        assert!(reloaded.list().is_empty());
        assert!(matches!(
            reloaded.remove("nope"),
            Err(Error::SiteNotFound(_))
        ));
    }

    #[test]
    fn rejects_duplicates_and_bad_names() {
        let mut store = SiteStore::default();
        store.add(php_site("myapp")).unwrap();
        assert!(matches!(
            store.add(php_site("myapp")),
            Err(Error::SiteExists(_))
        ));
        assert!(store.add(php_site("My App")).is_err());
    }

    #[test]
    fn same_name_with_another_ending_is_allowed() {
        let mut store = SiteStore::default();
        store.add(php_site("myapp")).unwrap();
        let test_site = store
            .add(Site {
                tld: "test".into(),
                ..php_site("myapp")
            })
            .unwrap();
        assert_eq!(test_site.domain(), "myapp.test");
    }
}
