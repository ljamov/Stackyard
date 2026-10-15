//! What Stackyard can install. Where each runtime is downloaded from lives
//! in [`crate::runtimes`].

use serde::Serialize;

use crate::platform::Os;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RuntimeKind {
    Php,
    Node,
    Python,
    Go,
    Java,
}

impl RuntimeKind {
    pub const ALL: [RuntimeKind; 5] = [Self::Php, Self::Node, Self::Python, Self::Go, Self::Java];

    /// Lowercase id used in settings, folder names and commands.
    pub fn id(self) -> &'static str {
        match self {
            Self::Php => "php",
            Self::Node => "node",
            Self::Python => "python",
            Self::Go => "go",
            Self::Java => "java",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.id() == id)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeFamily {
    pub kind: RuntimeKind,
    pub name: &'static str,
    /// Newest first.
    pub versions: &'static [&'static str],
    /// `false` = shown as "coming soon" in the prototype.
    pub available: bool,
    /// Versions only Windows gets: Stackyard's macOS and Linux PHP builds
    /// start at 8.1, the first release that builds against OpenSSL 3.
    #[serde(skip)]
    pub windows_only: &'static [&'static str],
}

pub const RUNTIMES: &[RuntimeFamily] = &[
    RuntimeFamily {
        kind: RuntimeKind::Php,
        name: "PHP",
        versions: &["8.5", "8.4", "8.3", "8.2", "8.1", "8.0", "7.4"],
        available: true,
        windows_only: &["8.0", "7.4"],
    },
    RuntimeFamily {
        kind: RuntimeKind::Node,
        name: "Node.js",
        versions: &["26", "24", "22", "20"],
        available: true,
        windows_only: &[],
    },
    RuntimeFamily {
        kind: RuntimeKind::Python,
        name: "Python",
        versions: &["3.13", "3.12", "3.11"],
        available: false,
        windows_only: &[],
    },
    RuntimeFamily {
        kind: RuntimeKind::Go,
        name: "Go",
        versions: &["1.23", "1.22"],
        available: false,
        windows_only: &[],
    },
    RuntimeFamily {
        kind: RuntimeKind::Java,
        name: "Java",
        versions: &["21", "17"],
        available: false,
        windows_only: &[],
    },
];

#[derive(Debug, Clone, Serialize)]
pub struct ServiceDef {
    pub id: &'static str,
    pub name: &'static str,
    pub version: &'static str,
    pub ports: &'static [u16],
    pub role: &'static str,
    /// Page to open in a browser while it runs, if it has one.
    pub web: Option<&'static str>,
}

/// `version` is the release line Stackyard installs and keeps updated:
/// the newest `8.4.x` for MySQL 8.4, the newest `2.x` for Caddy.
pub const SERVICES: &[ServiceDef] = &[
    ServiceDef {
        id: "caddy",
        name: "Caddy",
        version: "2",
        // 8080 and 8443 when these are taken; see `web::Ports`.
        ports: &[80, 443],
        role: "Web server",
        web: Some("http://localhost"),
    },
    ServiceDef {
        id: "mysql",
        name: "MySQL",
        version: "8.4",
        ports: &[3306],
        role: "Database",
        web: None,
    },
    ServiceDef {
        id: "mariadb",
        name: "MariaDB",
        version: "11.8",
        ports: &[3306],
        role: "Database",
        web: None,
    },
    ServiceDef {
        id: "postgres",
        name: "PostgreSQL",
        version: "18",
        ports: &[5432],
        role: "Database",
        web: None,
    },
    ServiceDef {
        id: "redis",
        name: "Redis",
        version: "8",
        ports: &[6379],
        role: "Cache and queues",
        web: None,
    },
    ServiceDef {
        id: "mailpit",
        name: "Mailpit",
        version: "1",
        ports: &[1025, 8025],
        role: "Mail catcher",
        web: Some("http://127.0.0.1:8025"),
    },
    ServiceDef {
        id: "meilisearch",
        name: "Meilisearch",
        version: "1",
        ports: &[7700],
        role: "Search engine",
        web: Some("http://127.0.0.1:7700"),
    },
];

pub fn service(id: &str) -> Option<&'static ServiceDef> {
    SERVICES.iter().find(|s| s.id == id)
}

impl RuntimeFamily {
    pub fn offers(&self, version: &str) -> bool {
        self.versions.contains(&version)
    }

    /// Whether `version` can be installed on `os`.
    pub fn offers_on(&self, version: &str, os: Os) -> bool {
        self.offers(version) && (os == Os::Windows || !self.windows_only.contains(&version))
    }

    /// The versions that can be installed on `os`, newest first.
    pub fn versions_on(&self, os: Os) -> impl Iterator<Item = &'static str> + '_ {
        self.versions
            .iter()
            .copied()
            .filter(move |v| self.offers_on(v, os))
    }
}

pub fn runtime(kind: RuntimeKind) -> &'static RuntimeFamily {
    RUNTIMES
        .iter()
        .find(|r| r.kind == kind)
        .expect("every RuntimeKind has a catalog entry")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_php_is_windows_only() {
        let php = runtime(RuntimeKind::Php);
        assert!(php.offers_on("7.4", Os::Windows));
        assert!(!php.offers_on("7.4", Os::Macos));
        assert!(!php.offers_on("8.0", Os::Linux));
        assert!(php.offers_on("8.1", Os::Linux));
        assert_eq!(php.versions_on(Os::Macos).last(), Some("8.1"));
        assert_eq!(php.versions_on(Os::Windows).last(), Some("7.4"));
        assert!(!php.offers_on("9.9", Os::Windows));
    }

    #[test]
    fn every_kind_is_listed_once() {
        for kind in RuntimeKind::ALL {
            assert_eq!(RUNTIMES.iter().filter(|r| r.kind == kind).count(), 1);
            assert!(!runtime(kind).versions.is_empty());
            assert_eq!(RuntimeKind::from_id(kind.id()), Some(kind));
        }
        assert_eq!(RuntimeKind::from_id("../php"), None);
    }

    #[test]
    fn service_ids_are_unique() {
        let mut ids: Vec<_> = SERVICES.iter().map(|s| s.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), SERVICES.len());
    }
}
