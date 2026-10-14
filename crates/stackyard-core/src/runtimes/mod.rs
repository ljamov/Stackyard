//! Installs and removes runtime versions (PHP, Node.js).
//!
//! Each version lives in its own folder, `runtimes/<kind>/<version>/`, e.g.
//! `runtimes/php/8.4/`, so versions sit side by side and removing one is a
//! single folder delete. A receipt file in that folder records which exact
//! release is installed and where it came from.

mod node;
mod php;

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use serde::{Deserialize, Serialize};

use crate::catalog::{self, RuntimeKind};
use crate::download::{self, Http};
use crate::settings::{self, Settings};
use crate::{archive, paths, Error, Result};

pub use php::build_platform as php_build_platform;

const NODE_DIST: &str = "https://nodejs.org/dist";
const PHP_WINDOWS: &str = "https://www.php.net/~windows/releases";
const PHP_MANIFEST: &str =
    "https://github.com/ljamov/stackyard-php/releases/download/php-builds/manifest.json";
const RECEIPT: &str = ".stackyard-install.toml";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    Linux,
    Macos,
    Windows,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arch {
    X64,
    Arm64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Platform {
    pub os: Os,
    pub arch: Arch,
}

impl Platform {
    pub fn current() -> Self {
        let os = if cfg!(windows) {
            Os::Windows
        } else if cfg!(target_os = "macos") {
            Os::Macos
        } else {
            Os::Linux
        };
        let arch = if cfg!(target_arch = "aarch64") {
            Arch::Arm64
        } else {
            Arch::X64
        };
        Self { os, arch }
    }
}

impl fmt::Display for Platform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let os = match self.os {
            Os::Linux => "Linux",
            Os::Macos => "macOS",
            Os::Windows => "Windows",
        };
        let arch = match self.arch {
            Arch::X64 => "x64",
            Arch::Arm64 => "ARM64",
        };
        write!(f, "{os} {arch}")
    }
}

/// Where downloads come from. Each can be pointed at a mirror with an
/// environment variable, which also lets tests use a local server.
#[derive(Debug, Clone)]
pub struct Sources {
    pub node_dist: String,
    pub php_windows: String,
    pub php_manifest: String,
}

impl Default for Sources {
    fn default() -> Self {
        let env =
            |name: &str, default: &str| std::env::var(name).unwrap_or_else(|_| default.into());
        Self {
            node_dist: env("STACKYARD_NODE_MIRROR", NODE_DIST),
            php_windows: env("STACKYARD_PHP_WINDOWS_MIRROR", PHP_WINDOWS),
            php_manifest: env("STACKYARD_PHP_MANIFEST", PHP_MANIFEST),
        }
    }
}

/// One downloadable release: an exact version and its verified archive.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Release {
    pub version: String,
    pub url: String,
    pub file_name: String,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Installed {
    pub kind: RuntimeKind,
    /// The catalog version, e.g. `8.4` or `22`.
    pub version: String,
    /// The exact release, e.g. `8.4.26` or `22.23.3`.
    pub release: String,
    pub path: PathBuf,
    pub binary: PathBuf,
}

/// What an install is doing right now, for progress bars.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(tag = "step", rename_all = "lowercase")]
pub enum Step {
    Resolving,
    Downloading { done: u64, total: Option<u64> },
    Unpacking,
}

pub struct RuntimeManager {
    root: PathBuf,
    downloads: PathBuf,
    http: Http,
    sources: Sources,
    platform: Platform,
}

impl RuntimeManager {
    /// The manager for Stackyard's data folder.
    pub fn open() -> Self {
        Self::new(&paths::home())
    }

    pub fn new(home: &Path) -> Self {
        Self {
            root: home.join("runtimes"),
            downloads: home.join("downloads"),
            http: Http::new(),
            sources: Sources::default(),
            platform: Platform::current(),
        }
    }

    pub fn with_sources(mut self, sources: Sources) -> Self {
        self.sources = sources;
        self
    }

    pub fn platform(&self) -> Platform {
        self.platform
    }

    pub fn dir(&self, kind: RuntimeKind, version: &str) -> PathBuf {
        self.root.join(kind.id()).join(version)
    }

    /// Every installed version, in catalog order.
    pub fn installed(&self) -> Vec<Installed> {
        catalog::RUNTIMES
            .iter()
            .flat_map(|family| {
                family
                    .versions
                    .iter()
                    .filter_map(|v| self.get(family.kind, v))
            })
            .collect()
    }

    pub fn get(&self, kind: RuntimeKind, version: &str) -> Option<Installed> {
        let path = self.dir(kind, version);
        let receipt: Receipt = settings::read_toml(&path.join(RECEIPT)).ok()??;
        Some(Installed {
            kind,
            version: version.to_string(),
            release: receipt.release,
            binary: path.join(binary_path(kind, self.platform.os)),
            path,
        })
    }

    /// Looks up the newest release of `version` for this computer.
    pub fn resolve(&self, kind: RuntimeKind, version: &str) -> Result<Release> {
        check_offered(kind, version)?;
        match kind {
            RuntimeKind::Node => node::resolve(
                &self.http,
                self.sources.node_dist.trim_end_matches('/'),
                version,
                self.platform,
            ),
            RuntimeKind::Php => php::resolve(&self.http, &self.sources, version, self.platform),
            _ => Err(Error::ModuleUnavailable(catalog::runtime(kind).name.into())),
        }
    }

    /// Downloads, verifies and unpacks the newest release of `version`,
    /// replacing an older release of the same version if one is installed.
    pub fn install(
        &self,
        kind: RuntimeKind,
        version: &str,
        progress: &mut dyn FnMut(Step),
    ) -> Result<Installed> {
        progress(Step::Resolving);
        let release = self.resolve(kind, version)?;
        self.install_release(kind, version, &release, progress)
    }

    pub fn install_release(
        &self,
        kind: RuntimeKind,
        version: &str,
        release: &Release,
        progress: &mut dyn FnMut(Step),
    ) -> Result<Installed> {
        check_offered(kind, version)?;
        let archive_path = self.downloads.join(&release.file_name);
        let reusable = archive_path.is_file()
            && download::sha256_file(&archive_path)
                .is_ok_and(|sha| sha.eq_ignore_ascii_case(&release.sha256));
        if !reusable {
            self.http.download(
                &release.url,
                &archive_path,
                &release.sha256,
                &mut |done, total| progress(Step::Downloading { done, total }),
            )?;
        }

        progress(Step::Unpacking);
        let kind_dir = self.root.join(kind.id());
        let staging = kind_dir.join(unique(".staging", version));
        let result = self.unpack_into_place(kind, version, release, &archive_path, &staging);
        let _ = std::fs::remove_dir_all(&staging);
        // Keep disk use down: the archive is only needed again for a reinstall.
        let _ = std::fs::remove_file(&archive_path);
        result
    }

    fn unpack_into_place(
        &self,
        kind: RuntimeKind,
        version: &str,
        release: &Release,
        archive_path: &Path,
        staging: &Path,
    ) -> Result<Installed> {
        let content = archive::extract(archive_path, staging)?;
        let binary = binary_path(kind, self.platform.os);
        if !content.join(binary).is_file() {
            return Err(Error::Archive {
                path: archive_path.to_owned(),
                message: format!("it does not contain {binary}"),
            });
        }
        let receipt = Receipt {
            release: release.version.clone(),
            url: release.url.clone(),
            sha256: release.sha256.clone(),
        };
        settings::write_toml(&content.join(RECEIPT), &receipt)?;

        // Swap the new folder in; the old one (if any) is removed afterwards
        // so a failure part-way never leaves this version half-installed.
        let dest = self.dir(kind, version);
        let old = dest.with_file_name(unique(".old", version));
        let had_old = dest.exists();
        if had_old {
            std::fs::rename(&dest, &old).map_err(|source| in_use(&dest, source))?;
        }
        if let Err(source) = std::fs::rename(&content, &dest) {
            if had_old {
                let _ = std::fs::rename(&old, &dest);
            }
            return Err(Error::Io { path: dest, source });
        }
        if had_old {
            let _ = std::fs::remove_dir_all(&old);
        }
        self.get(kind, version).ok_or(Error::NotInstalled {
            name: catalog::runtime(kind).name.into(),
            version: version.into(),
        })
    }

    pub fn uninstall(&self, kind: RuntimeKind, version: &str) -> Result<()> {
        check_offered(kind, version)?;
        let dest = self.dir(kind, version);
        if self.get(kind, version).is_none() {
            return Err(Error::NotInstalled {
                name: catalog::runtime(kind).name.into(),
                version: version.into(),
            });
        }
        // Move it aside first so the version disappears in one step, even if
        // deleting thousands of files is interrupted.
        let trash = dest.with_file_name(unique(".trash", version));
        std::fs::rename(&dest, &trash).map_err(|source| in_use(&dest, source))?;
        std::fs::remove_dir_all(&trash).map_err(|source| Error::Io {
            path: trash,
            source,
        })
    }
}

impl RuntimeManager {
    /// The default version for `kind`, if it is set and still installed.
    pub fn default_for(&self, settings: &Settings, kind: RuntimeKind) -> Option<Installed> {
        self.get(kind, settings.default_runtime(kind)?)
    }

    pub fn set_default(
        &self,
        settings: &mut Settings,
        kind: RuntimeKind,
        version: &str,
    ) -> Result<()> {
        check_offered(kind, version)?;
        if self.get(kind, version).is_none() {
            return Err(Error::NotInstalled {
                name: catalog::runtime(kind).name.into(),
                version: version.into(),
            });
        }
        settings.set_default_runtime(kind, Some(version));
        Ok(())
    }

    /// Keeps the default pointing at an installed version after an install
    /// or uninstall: the first install becomes the default, and removing the
    /// default hands it to the newest version left. Returns whether it changed.
    pub fn fix_default(&self, settings: &mut Settings, kind: RuntimeKind) -> bool {
        if self.default_for(settings, kind).is_some() {
            return false;
        }
        let newest = catalog::runtime(kind)
            .versions
            .iter()
            .copied()
            .find(|v| self.get(kind, v).is_some());
        let changed = settings.default_runtime(kind) != newest;
        settings.set_default_runtime(kind, newest);
        changed
    }
}

#[derive(Serialize, Deserialize)]
struct Receipt {
    release: String,
    url: String,
    sha256: String,
}

/// The main program inside an installed runtime folder.
pub fn binary_path(kind: RuntimeKind, os: Os) -> &'static str {
    match (kind, os) {
        (RuntimeKind::Php, Os::Windows) => "php.exe",
        (RuntimeKind::Php, _) => "bin/php",
        (RuntimeKind::Node, Os::Windows) => "node.exe",
        (RuntimeKind::Node, _) => "bin/node",
        (RuntimeKind::Python, Os::Windows) => "python.exe",
        (RuntimeKind::Python, _) => "bin/python3",
        (RuntimeKind::Go, Os::Windows) => "bin/go.exe",
        (RuntimeKind::Go, _) => "bin/go",
        (RuntimeKind::Java, Os::Windows) => "bin/java.exe",
        (RuntimeKind::Java, _) => "bin/java",
    }
}

/// Only catalog versions are accepted, which also keeps user input out of paths.
fn check_offered(kind: RuntimeKind, version: &str) -> Result<()> {
    let family = catalog::runtime(kind);
    if !family.available {
        return Err(Error::ModuleUnavailable(family.name.into()));
    }
    if !family.offers(version) {
        return Err(Error::UnknownVersion {
            name: family.name.into(),
            version: version.into(),
        });
    }
    Ok(())
}

fn unique(prefix: &str, version: &str) -> String {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    format!(
        "{prefix}-{version}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}

fn in_use(path: &Path, source: std::io::Error) -> Error {
    Error::Io {
        path: path.to_owned(),
        source: std::io::Error::new(
            source.kind(),
            format!("{source} (is a site or terminal still using it?)"),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::archive::fixtures;
    use crate::download::{sha256_file, testserver};

    fn node_archive(dir: &Path, release: &str) -> (Vec<u8>, String) {
        let path = dir.join("fixture.tar.gz");
        let top = format!("node-v{release}-test");
        fixtures::tar_gz(
            &path,
            &[
                (&format!("{top}/bin/node"), b"#!/bin/sh\necho node\n", 0o755),
                (&format!("{top}/node.exe"), b"MZ", 0o755),
                (&format!("{top}/LICENSE"), b"MIT", 0o644),
            ],
        );
        (std::fs::read(&path).unwrap(), sha256_file(&path).unwrap())
    }

    fn release(base: &str, version: &str, sha256: String) -> Release {
        let file_name = format!("node-v{version}-test.tar.gz");
        Release {
            version: version.into(),
            url: format!("{base}/{file_name}"),
            file_name,
            sha256,
        }
    }

    #[test]
    fn installs_upgrades_and_uninstalls() {
        let home = crate::testutil::tempdir();
        let manager = RuntimeManager::new(&home);
        assert!(manager.installed().is_empty());

        let (bytes, sha) = node_archive(&home, "22.1.0");
        let base = testserver::serve(bytes);
        let mut steps = Vec::new();
        let installed = manager
            .install_release(
                RuntimeKind::Node,
                "22",
                &release(&base, "22.1.0", sha),
                &mut |s| steps.push(s),
            )
            .unwrap();
        assert_eq!(installed.release, "22.1.0");
        assert_eq!(installed.path, home.join("runtimes/node/22"));
        assert!(installed.binary.is_file());
        assert!(installed.path.join("LICENSE").is_file());
        assert_eq!(steps.last(), Some(&Step::Unpacking));
        assert!(steps.iter().any(|s| matches!(s, Step::Downloading { .. })));
        assert_eq!(manager.installed(), vec![installed]);

        // A newer release of the same version replaces the old folder.
        let (bytes, sha) = node_archive(&home, "22.2.0");
        let base = testserver::serve(bytes);
        let upgraded = manager
            .install_release(
                RuntimeKind::Node,
                "22",
                &release(&base, "22.2.0", sha),
                &mut |_| {},
            )
            .unwrap();
        assert_eq!(upgraded.release, "22.2.0");
        assert_eq!(manager.installed().len(), 1);
        let leftovers: Vec<_> = std::fs::read_dir(home.join("runtimes/node"))
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(leftovers, vec!["22"], "no staging or old folders left");
        assert_eq!(
            std::fs::read_dir(home.join("downloads")).unwrap().count(),
            0
        );

        let mut settings = Settings::default();
        assert!(manager.fix_default(&mut settings, RuntimeKind::Node));
        assert_eq!(settings.default_runtime(RuntimeKind::Node), Some("22"));
        assert!(!manager.fix_default(&mut settings, RuntimeKind::Node));
        assert!(manager
            .set_default(&mut settings, RuntimeKind::Node, "24")
            .is_err());

        manager.uninstall(RuntimeKind::Node, "22").unwrap();
        assert!(manager.installed().is_empty());
        assert!(manager.fix_default(&mut settings, RuntimeKind::Node));
        assert_eq!(settings.default_runtime(RuntimeKind::Node), None);
        assert!(!home.join("runtimes/node/22").exists());
        assert!(manager.uninstall(RuntimeKind::Node, "22").is_err());
    }

    #[test]
    fn a_bad_download_leaves_the_old_install_alone() {
        let home = crate::testutil::tempdir();
        let manager = RuntimeManager::new(&home);
        let (bytes, sha) = node_archive(&home, "22.1.0");
        let base = testserver::serve(bytes);
        manager
            .install_release(
                RuntimeKind::Node,
                "22",
                &release(&base, "22.1.0", sha),
                &mut |_| {},
            )
            .unwrap();

        let wrong = "0".repeat(64);
        let err = manager
            .install_release(
                RuntimeKind::Node,
                "22",
                &release(&base, "22.9.0", wrong),
                &mut |_| {},
            )
            .unwrap_err();
        assert!(matches!(err, Error::Checksum { .. }), "{err}");
        assert_eq!(
            manager.get(RuntimeKind::Node, "22").unwrap().release,
            "22.1.0"
        );
    }

    #[test]
    fn refuses_versions_outside_the_catalog() {
        let home = crate::testutil::tempdir();
        let manager = RuntimeManager::new(&home);
        let fake = Release {
            version: "1".into(),
            url: "http://127.0.0.1:9/x.tar.gz".into(),
            file_name: "x.tar.gz".into(),
            sha256: "0".repeat(64),
        };
        for version in ["../../etc", "19", ""] {
            let err = manager
                .install_release(RuntimeKind::Node, version, &fake, &mut |_| {})
                .unwrap_err();
            assert!(matches!(err, Error::UnknownVersion { .. }), "{err}");
        }
        let err = manager.resolve(RuntimeKind::Python, "3.13").unwrap_err();
        assert!(matches!(err, Error::ModuleUnavailable(_)), "{err}");
    }

    #[test]
    fn archives_without_the_program_are_rejected() {
        let home = crate::testutil::tempdir();
        let manager = RuntimeManager::new(&home);
        let path = home.join("empty.tar.gz");
        fixtures::tar_gz(&path, &[("node-v1/README.md", b"hi", 0o644)]);
        let sha = sha256_file(&path).unwrap();
        let base = testserver::serve(std::fs::read(&path).unwrap());
        let err = manager
            .install_release(
                RuntimeKind::Node,
                "22",
                &release(&base, "22.1.0", sha),
                &mut |_| {},
            )
            .unwrap_err();
        assert!(err.to_string().contains("does not contain"), "{err}");
        assert!(manager.get(RuntimeKind::Node, "22").is_none());
    }

    /// Downloads real Node.js: `cargo test -p stackyard-core -- --ignored live_`.
    #[test]
    #[ignore]
    fn live_installs_node() {
        let home = crate::testutil::tempdir();
        let manager = RuntimeManager::new(&home);
        let installed = manager
            .install(RuntimeKind::Node, "22", &mut |_| {})
            .unwrap();
        let out = std::process::Command::new(&installed.binary)
            .arg("--version")
            .output()
            .unwrap();
        let printed = String::from_utf8_lossy(&out.stdout);
        assert_eq!(printed.trim(), format!("v{}", installed.release));
        let _ = std::fs::remove_dir_all(&home);
    }
}
