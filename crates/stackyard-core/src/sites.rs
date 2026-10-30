//! Local sites such as `https://myapp.stack`, stored in `sites.toml`.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::catalog::RuntimeKind;
use crate::projects::Project;
use crate::settings::{read_toml, write_toml};
use crate::{domain, paths, Error, Result};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase", tag = "kind")]
pub enum SiteRuntime {
    /// Served through PHP-FPM (or php-cgi on Windows).
    Php { version: String },
    /// Proxied to the app's own dev server.
    Node { version: String, port: u16 },
    /// Proxied to the app's server (Django, FastAPI, Flask), which Stackyard runs.
    Python { version: String, port: u16 },
    /// Proxied to the app's server (`go run`), which Stackyard runs.
    Go { version: String, port: u16 },
    /// Proxied to the app's server (Spring Boot, Quarkus, Micronaut), which Stackyard runs.
    Java { version: String, port: u16 },
    /// Plain files from the project folder.
    Static,
}

impl SiteRuntime {
    /// A site of `kind` at `version`; `port` is unused for PHP.
    pub fn new(kind: RuntimeKind, version: String, port: u16) -> Self {
        match kind {
            RuntimeKind::Php => SiteRuntime::Php { version },
            RuntimeKind::Node => SiteRuntime::Node { version, port },
            RuntimeKind::Python => SiteRuntime::Python { version, port },
            RuntimeKind::Go => SiteRuntime::Go { version, port },
            RuntimeKind::Java => SiteRuntime::Java { version, port },
        }
    }

    pub fn kind(&self) -> Option<RuntimeKind> {
        match self {
            SiteRuntime::Php { .. } => Some(RuntimeKind::Php),
            SiteRuntime::Node { .. } => Some(RuntimeKind::Node),
            SiteRuntime::Python { .. } => Some(RuntimeKind::Python),
            SiteRuntime::Go { .. } => Some(RuntimeKind::Go),
            SiteRuntime::Java { .. } => Some(RuntimeKind::Java),
            SiteRuntime::Static => None,
        }
    }

    pub fn version(&self) -> Option<&str> {
        match self {
            SiteRuntime::Php { version }
            | SiteRuntime::Node { version, .. }
            | SiteRuntime::Python { version, .. }
            | SiteRuntime::Go { version, .. }
            | SiteRuntime::Java { version, .. } => Some(version),
            SiteRuntime::Static => None,
        }
    }

    pub fn set_version(&mut self, new: &str) {
        match self {
            SiteRuntime::Php { version }
            | SiteRuntime::Node { version, .. }
            | SiteRuntime::Python { version, .. }
            | SiteRuntime::Go { version, .. }
            | SiteRuntime::Java { version, .. } => *version = new.to_string(),
            SiteRuntime::Static => {}
        }
    }

    /// The port of the app's own server; `None` for PHP and plain files.
    pub fn port(&self) -> Option<u16> {
        match self {
            SiteRuntime::Node { port, .. }
            | SiteRuntime::Python { port, .. }
            | SiteRuntime::Go { port, .. }
            | SiteRuntime::Java { port, .. } => Some(*port),
            SiteRuntime::Php { .. } | SiteRuntime::Static => None,
        }
    }

    fn port_mut(&mut self) -> Option<&mut u16> {
        match self {
            SiteRuntime::Node { port, .. }
            | SiteRuntime::Python { port, .. }
            | SiteRuntime::Go { port, .. }
            | SiteRuntime::Java { port, .. } => Some(port),
            SiteRuntime::Php { .. } | SiteRuntime::Static => None,
        }
    }

    /// Python, Go and Java sites only work with their server running, so
    /// Stackyard always runs it. (Node sites have [`Site::dev_server`].)
    pub fn runs_own_server(&self) -> bool {
        matches!(
            self,
            SiteRuntime::Python { .. } | SiteRuntime::Go { .. } | SiteRuntime::Java { .. }
        )
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
    /// What the site settings panel changes; empty for most sites.
    #[serde(default, skip_serializing_if = "SiteSettings::is_empty")]
    pub settings: SiteSettings,
}

/// A site's own settings, beyond its runtime version: the folder served,
/// environment variables, PHP settings and a command Stackyard runs.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SiteSettings {
    /// The folder the web server serves, inside the project folder, e.g.
    /// `public`. `None` lets Stackyard pick it from the framework.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root: Option<String>,
    /// Environment variables for the site's PHP and for the servers and
    /// command Stackyard runs for it.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    /// `php.ini` values for this site only, e.g. `memory_limit = 1G`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub php: BTreeMap<String, String>,
    /// A command Stackyard runs in the project folder while the web server
    /// runs, e.g. `npm run start` or `php artisan queue:work`. For Node,
    /// Python, Go and Java sites it replaces the usual server command and
    /// gets the site's port in `PORT`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    /// The database that goes with the site in backups and exports. `None`
    /// looks for one in the project's `.env` or `wp-config.php`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub database: Option<SiteDatabase>,
    /// No local address of its own (`name.stack`): an app that runs on its
    /// own port (`http://localhost:3000`) or only in a terminal. A PHP or
    /// plain-files app is still served at its `.localhost` name.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub no_address: bool,
    /// What the site is for. Changed only by [`crate::projects::change`],
    /// which makes or unlinks a WordPress; saving settings keeps it.
    #[serde(default, skip_serializing_if = "Project::is_default")]
    pub project: Project,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SiteDatabase {
    /// `mysql` (MySQL or MariaDB) or `postgres`.
    pub server: String,
    pub name: String,
}

impl SiteSettings {
    pub fn is_empty(&self) -> bool {
        *self == SiteSettings::default()
    }

    /// Trims what was typed and refuses what would break the web server's
    /// or PHP's settings files.
    pub fn validated(mut self) -> Result<Self> {
        let bad = |message: String| Err(Error::Shell(message));
        self.root = self
            .root
            .map(|r| r.trim().trim_matches(['/', '\\']).replace('\\', "/"))
            .filter(|r| !r.is_empty() && r != ".");
        if let Some(root) = &self.root {
            let inside = Path::new(root)
                .components()
                .all(|c| matches!(c, Component::Normal(_)));
            if !inside || root.contains(['"', '\n', ':']) {
                return bad(format!(
                    "the site folder {root:?} must be a folder inside the project, like public"
                ));
            }
        }
        let env_name = |k: &str| {
            k.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
                && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        };
        for (key, value) in &self.env {
            if !env_name(key) {
                return bad(format!(
                    "{key:?} is not a usable variable name: use letters, digits and _"
                ));
            }
            if value.contains(['\n', '\r', '\0']) {
                return bad(format!("the value of {key} must be one line"));
            }
        }
        for (key, value) in &self.php {
            let name_ok = !key.is_empty()
                && key
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.');
            if !name_ok {
                return bad(format!("{key:?} is not a PHP setting name"));
            }
            if key.ends_with("extension") {
                return bad(format!(
                    "{key} can't be set per site; turn extensions on in the PHP version's php.ini"
                ));
            }
            if value.contains(['\n', '\r', '\0', '"']) {
                return bad(format!(
                    "the value of {key} must be one line without double quotes"
                ));
            }
        }
        self.command = self
            .command
            .map(|c| c.trim().to_string())
            .filter(|c| !c.is_empty());
        if self
            .command
            .as_deref()
            .is_some_and(|c| c.contains(['\n', '\r', '\0']))
        {
            return bad("the start command must be one line".into());
        }
        if let Some(db) = &self.database {
            if !["mysql", "postgres"].contains(&db.server.as_str()) {
                return bad(format!("unknown database server {:?}", db.server));
            }
            crate::databases::check_name(&db.name)?;
        }
        Ok(self)
    }
}

impl Site {
    pub fn domain(&self) -> String {
        domain::site_domain(&self.name, &self.tld)
    }

    /// A name for the site that no other site has, for process ids and
    /// `.localhost` names: `shop` for `shop.stack`, and `checkout.shop` for
    /// a staging `checkout.shop.stack`, so it can't clash with `checkout.stack`.
    pub fn key(&self) -> String {
        let labels: Vec<&str> = self.tld.trim_matches('.').split('.').collect();
        let middle = &labels[..labels.len().saturating_sub(1)];
        std::iter::once(self.name.as_str())
            .chain(middle.iter().copied())
            .collect::<Vec<_>>()
            .join(".")
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
        let mut site = site;
        if site.runtime.runs_own_server() {
            self.give_free_port(&mut site.runtime);
        }
        self.sites.push(site);
        Ok(self.sites.last().expect("just pushed"))
    }

    /// Stackyard starts these servers itself, so two sites must not share a
    /// port: a taken one moves up to the next free port.
    fn give_free_port(&self, runtime: &mut SiteRuntime) {
        let taken: Vec<u16> = self.sites.iter().filter_map(|s| s.runtime.port()).collect();
        if let Some(port) = runtime.port_mut() {
            if let Some(free) = (*port..port.saturating_add(100))
                .find(|p| !taken.contains(p) && !web_server_port(*p) && crate::ports::is_free(*p))
            {
                *port = free;
            }
        }
    }

    /// A site by its domain (`myapp.stack`) or bare name (`myapp`), to change it.
    pub fn get_mut(&mut self, name_or_domain: &str) -> Result<&mut Site> {
        self.sites
            .iter_mut()
            .find(|s| s.domain() == name_or_domain || s.name == name_or_domain)
            .ok_or_else(|| Error::SiteNotFound(name_or_domain.to_string()))
    }

    /// Changes a site's domain ending, HTTPS and settings. Returns the site
    /// as it was and as it is now, so callers can tell what changed.
    pub fn configure(
        &mut self,
        name_or_domain: &str,
        tld: &str,
        https: bool,
        settings: SiteSettings,
    ) -> Result<(Site, Site)> {
        let tld = domain::normalize_tld(tld)?;
        let settings = settings.validated()?;
        let before = self.get_mut(name_or_domain)?.clone();
        let new_domain = domain::site_domain(&before.name, &tld);
        if new_domain != before.domain() && self.sites.iter().any(|s| s.domain() == new_domain) {
            return Err(Error::SiteExists(new_domain));
        }
        let site = self.get_mut(name_or_domain)?;
        site.tld = tld;
        site.https = https;
        // The settings form doesn't carry the project; it stays as it was.
        site.settings = SiteSettings {
            project: before.settings.project.clone(),
            ..settings
        };
        Ok((before, site.clone()))
    }

    /// Points a site at another folder, keeping its settings, e.g. a copy
    /// of the project on another drive. Returns the site as it is now.
    pub fn set_folder(&mut self, name_or_domain: &str, folder: &Path) -> Result<Site> {
        let folder = paths::expand_home(folder);
        if !folder.is_dir() {
            return Err(Error::NotAFolder(folder));
        }
        let domain = self.get_mut(name_or_domain)?.domain();
        if let Some(other) = self
            .sites
            .iter()
            .find(|s| s.domain() != domain && same_folder(&s.path, &folder))
        {
            return Err(Error::Shell(format!(
                "{} already uses {}",
                other.domain(),
                folder.display()
            )));
        }
        let site = self.get_mut(name_or_domain)?;
        site.path = folder;
        Ok(site.clone())
    }

    /// Points sites whose folder is gone at a folder of the same name in a
    /// parked folder: the user moved their projects, e.g. to another drive,
    /// and parked the new place. The sites keep their settings. Returns the
    /// domains that changed.
    pub fn relink_moved(&mut self, parked: &[PathBuf]) -> Vec<String> {
        let mut moved = Vec::new();
        for i in 0..self.sites.len() {
            let site = &self.sites[i];
            if site.path.exists() {
                continue;
            }
            let Some(name) = site.path.file_name() else {
                continue;
            };
            let found = parked
                .iter()
                .map(|p| p.join(name))
                .find(|p| p.is_dir() && !self.sites.iter().any(|s| same_folder(&s.path, p)));
            if let Some(path) = found {
                self.sites[i].path = path;
                moved.push(self.sites[i].domain());
            }
        }
        moved
    }

    /// Saves a site found in a parked folder as it is listed, so it can be
    /// changed like one made in the app; a saved site is left alone.
    /// Returns the saved site.
    pub fn adopt(&mut self, site: &Site) -> Result<Site> {
        if let Some(saved) = self.sites.iter().find(|s| s.domain() == site.domain()) {
            return Ok(saved.clone());
        }
        Ok(self.add(site.clone())?.clone())
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

/// Whether two paths name the same folder; Windows and macOS ignore case.
pub fn same_folder(a: &Path, b: &Path) -> bool {
    let spell = |p: &Path| {
        let p = std::path::absolute(p).unwrap_or_else(|_| p.to_owned());
        let p = p.to_string_lossy().replace('\\', "/");
        let p = p.trim_end_matches('/').to_string();
        if cfg!(any(windows, target_os = "macos")) {
            p.to_lowercase()
        } else {
            p
        }
    };
    spell(a) == spell(b)
}

/// The folders a site's files are in: its own and, for a theme or plugin,
/// the project folder it is built in.
pub fn site_folders(site: &Site) -> Vec<PathBuf> {
    let mut folders = vec![site.path.clone()];
    if let Some(source) = &site.settings.project.source {
        if !folders.iter().any(|f| same_folder(f, source)) {
            folders.push(source.clone());
        }
    }
    folders
}

/// Whether `folder` is `other` or holds it.
fn holds(folder: &Path, other: &Path) -> bool {
    let full = |p: &Path| std::path::absolute(p).unwrap_or_else(|_| p.to_owned());
    let (folder, other) = (full(folder), full(other));
    other.ancestors().any(|a| same_folder(a, &folder))
}

/// Refuses to delete a site's folders when one is, or holds, a folder of
/// `keep` (a parked folder, the folder new sites go in, another site, the
/// data home) or is a drive's root.
pub fn check_delete(site: &Site, keep: &[PathBuf]) -> Result<()> {
    for folder in site_folders(site).iter().filter(|f| f.exists()) {
        let root = folder.parent().is_none() || folder.components().count() < 2;
        if let Some(kept) = keep
            .iter()
            .find(|k| holds(folder, k))
            .or(root.then_some(folder))
        {
            return Err(Error::Shell(format!(
                "{} was not deleted: it holds {}, which Stackyard still uses",
                folder.display(),
                kept.display()
            )));
        }
    }
    Ok(())
}

/// Deletes the folders of a site that was removed ([`site_folders`]),
/// after [`check_delete`]. Files another program has open are tried again
/// for a moment; what still can't go is named in the error.
pub fn delete_folders(site: &Site, keep: &[PathBuf]) -> Result<()> {
    check_delete(site, keep)?;
    let folders: Vec<PathBuf> = site_folders(site)
        .into_iter()
        .filter(|f| f.exists())
        .collect();
    for folder in &folders {
        let mut tries = 0;
        loop {
            match std::fs::remove_dir_all(folder) {
                Ok(()) => break,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => break,
                Err(_) if tries < 10 => {
                    tries += 1;
                    std::thread::sleep(std::time::Duration::from_millis(300));
                }
                Err(e) => {
                    return Err(Error::Shell(format!(
                        "could not delete {}: {e}. Close programs that use files in it (an editor, a terminal) and delete it by hand",
                        folder.display()
                    )))
                }
            }
        }
    }
    Ok(())
}

/// Moves a site's folder into `into` (a folder that holds projects, e.g. on
/// another drive) and returns where it is now. Within one drive this is a
/// rename; across drives every file is copied first, and the old folder is
/// removed only once the whole copy has worked, so a failed move loses
/// nothing. `progress` gets the bytes copied and the total. The second
/// value is true when some of the old folder could not be removed (a file
/// still open in another program); the copy is complete either way.
pub fn move_folder(
    from: &Path,
    into: &Path,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<(PathBuf, bool)> {
    let io = |e: std::io::Error| Error::Shell(e.to_string());
    let name = from
        .file_name()
        .ok_or_else(|| Error::NotAFolder(from.to_owned()))?;
    if !from.is_dir() {
        return Err(Error::NotAFolder(from.to_owned()));
    }
    std::fs::create_dir_all(into).map_err(io)?;
    let to = into.join(name);
    if same_folder(from, &to) {
        return Err(Error::Shell(format!(
            "the site is already in {}",
            into.display()
        )));
    }
    if to.exists() {
        return Err(Error::Shell(format!(
            "{} already exists; pick another folder, or use Change folder to point the site at it",
            to.display()
        )));
    }
    let inside = std::path::absolute(into)
        .unwrap_or_else(|_| into.to_owned())
        .starts_with(std::path::absolute(from).unwrap_or_else(|_| from.to_owned()));
    if inside {
        return Err(Error::Shell("a site can't move into its own folder".into()));
    }
    if std::fs::rename(from, &to).is_ok() {
        return Ok((to, false));
    }
    let total = folder_size(from);
    progress(0, total);
    let mut done = 0;
    if let Err(e) = copy_all(from, &to, &mut |bytes| {
        done += bytes;
        progress(done, total);
    }) {
        let _ = std::fs::remove_dir_all(&to);
        return Err(Error::Shell(format!(
            "could not copy the site to {}: {e}. Nothing was moved.",
            into.display()
        )));
    }
    let left = std::fs::remove_dir_all(from).is_err();
    Ok((to, left))
}

/// Copies every file and folder, with their dates, from `from` to `to`.
fn copy_all(from: &Path, to: &Path, copied: &mut dyn FnMut(u64)) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        // A link is copied as what it points at.
        if entry.path().is_dir() {
            copy_all(&entry.path(), &target, copied)?;
        } else {
            copied(crate::wordpress::copy::copy_file(&entry.path(), &target)?);
        }
    }
    Ok(())
}

fn folder_size(folder: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return 0;
    };
    entries
        .flatten()
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() => folder_size(&e.path()),
            _ => e.metadata().map(|m| m.len()).unwrap_or(0),
        })
        .sum()
}

/// Ports the web server may take: 80 and 443, or 8080 and 8443 and up.
fn web_server_port(port: u16) -> bool {
    let fallback = crate::web::Ports::FALLBACK;
    [80, 443].contains(&port)
        || (fallback.http..fallback.http + 20).contains(&port)
        || (fallback.https..fallback.https + 20).contains(&port)
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
            settings: SiteSettings::default(),
        }
    }

    #[test]
    fn changes_and_moves_a_site_folder() {
        let dir = crate::testutil::tempdir();
        let mut store = SiteStore::load_from(&dir.join("sites.toml")).unwrap();
        let old = dir.join("c/www/shop");
        std::fs::create_dir_all(old.join("wp-content/uploads")).unwrap();
        std::fs::write(old.join("index.php"), "<?php echo 1;").unwrap();
        std::fs::write(old.join("wp-content/uploads/a.jpg"), vec![7u8; 5000]).unwrap();
        store
            .add(Site {
                path: old.clone(),
                ..php_site("shop")
            })
            .unwrap();
        store.add(php_site("blog")).unwrap();

        // Pointing at a missing folder, or one another site uses, is refused.
        assert!(store.set_folder("shop", &dir.join("nope")).is_err());
        std::fs::create_dir_all(dir.join("blog")).unwrap();
        store.set_folder("blog", &dir.join("blog")).unwrap();
        assert!(store.set_folder("shop", &dir.join("blog")).is_err());

        let mut last = (0, 0);
        let (moved, left) = move_folder(&old, &dir.join("d/Sites"), &mut |done, total| {
            last = (done, total)
        })
        .unwrap();
        assert_eq!(moved, dir.join("d/Sites/shop"));
        assert!(!left);
        assert!(!old.exists());
        assert_eq!(
            std::fs::read(moved.join("wp-content/uploads/a.jpg"))
                .unwrap()
                .len(),
            5000
        );
        let _ = last;
        assert_eq!(store.set_folder("shop", &moved).unwrap().path, moved);

        // Moving onto an existing folder, or into itself, is refused.
        std::fs::create_dir_all(dir.join("e/shop")).unwrap();
        assert!(move_folder(&moved, &dir.join("e"), &mut |_, _| {}).is_err());
        assert!(move_folder(&moved, &moved.join("inner"), &mut |_, _| {}).is_err());
        assert!(moved.join("index.php").exists());
    }

    #[test]
    fn copies_when_a_rename_is_not_possible() {
        let dir = crate::testutil::tempdir();
        let from = dir.join("a/site");
        std::fs::create_dir_all(from.join("sub")).unwrap();
        std::fs::write(from.join("sub/f.txt"), "hello").unwrap();
        let to = dir.join("b/site");
        let mut seen = Vec::new();
        copy_all(&from, &to, &mut |b| seen.push(b)).unwrap();
        assert_eq!(
            std::fs::read_to_string(to.join("sub/f.txt")).unwrap(),
            "hello"
        );
        assert_eq!(seen, [5]);
        assert_eq!(folder_size(&from), 5);
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
    fn app_servers_get_ports_of_their_own() {
        let mut store = SiteStore::default();
        let django = |name: &str| Site {
            runtime: SiteRuntime::new(RuntimeKind::Python, "3.13".into(), 8000),
            ..php_site(name)
        };
        let first = store.add(django("one")).unwrap().runtime.port().unwrap();
        let second = store.add(django("two")).unwrap().runtime.port().unwrap();
        assert_ne!(first, second);
        assert!(second > first);
        assert!(store.list()[1].runtime.runs_own_server());
        let spring = store
            .add(Site {
                runtime: SiteRuntime::new(RuntimeKind::Java, "21".into(), 8080),
                ..php_site("three")
            })
            .unwrap();
        assert!(
            spring.runtime.port().unwrap() >= 8100,
            "kept clear of the web server"
        );
        assert_eq!(store.list()[1].runtime.version(), Some("3.13"));
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

    #[test]
    fn settings_are_checked_and_saved() {
        let path = crate::testutil::tempdir().join("sites.toml");
        let mut store = SiteStore::load_from(&path).unwrap();
        store.add(php_site("myapp")).unwrap();
        store.add(php_site("other")).unwrap();
        let mut settings = SiteSettings {
            root: Some("/web/".into()),
            command: Some("  npm run dev  ".into()),
            ..SiteSettings::default()
        };
        settings.env.insert("APP_ENV".into(), "local".into());
        settings.php.insert("memory_limit".into(), "1G".into());
        let (before, after) = store
            .configure("myapp", ".test", false, settings.clone())
            .unwrap();
        assert_eq!(before.domain(), "myapp.stack");
        assert_eq!(after.domain(), "myapp.test");
        store.save().unwrap();

        let reloaded = SiteStore::load_from(&path).unwrap();
        let site = &reloaded.list()[0];
        assert_eq!(site.url(), "http://myapp.test");
        assert_eq!(site.settings.root.as_deref(), Some("web"));
        assert_eq!(site.settings.command.as_deref(), Some("npm run dev"));
        assert_eq!(site.settings.env["APP_ENV"], "local");
        // Sites without settings keep a short entry.
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(text.matches("[site.settings").count(), 3, "{text}");

        let mut store = reloaded;
        let refuse = |s: SiteSettings| s.validated().is_err();
        assert!(refuse(SiteSettings {
            root: Some("../elsewhere".into()),
            ..SiteSettings::default()
        }));
        let mut env = SiteSettings::default();
        env.env.insert("BAD NAME".into(), "x".into());
        assert!(refuse(env));
        let mut php = SiteSettings::default();
        php.php.insert("extension".into(), "gd".into());
        assert!(refuse(php));
        let mut quoted = SiteSettings::default();
        quoted.php.insert("error_log".into(), "a\"b".into());
        assert!(refuse(quoted));
        // Two sites can't end up with one domain.
        store.add(php_site("myapp")).unwrap();
        assert!(matches!(
            store.configure("myapp.stack", ".test", true, SiteSettings::default()),
            Err(Error::SiteExists(_))
        ));
    }

    #[test]
    fn deletes_a_sites_folders_but_not_what_others_use() {
        let dir = crate::testutil::tempdir();
        let www = dir.join("www");
        std::fs::create_dir_all(www.join("blog/wp-content")).unwrap();
        std::fs::write(www.join("blog/index.php"), "<?php").unwrap();
        std::fs::create_dir_all(www.join("other")).unwrap();
        let site = |path: PathBuf| Site {
            name: "blog".into(),
            tld: ".stack".into(),
            path,
            runtime: SiteRuntime::Static,
            https: false,
            dev_server: false,
            settings: Default::default(),
        };
        // A folder holding a parked folder or another site is refused.
        let err = delete_folders(&site(www.clone()), &[www.join("other")]).unwrap_err();
        assert!(err.to_string().contains("was not deleted"), "{err}");
        assert!(www.join("blog/index.php").exists());
        assert!(delete_folders(&site(www.clone()), std::slice::from_ref(&www)).is_err());

        delete_folders(&site(www.join("blog")), &[www.clone(), www.join("other")]).unwrap();
        assert!(!www.join("blog").exists());
        assert!(www.join("other").exists());
        // Gone already: nothing to do.
        delete_folders(&site(www.join("blog")), &[]).unwrap();
    }
}
