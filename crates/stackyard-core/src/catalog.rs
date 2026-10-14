//! What Stackyard can install. Download sources and checksums are added
//! in the runtime-manager milestone; for now this is the list the UI shows.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RuntimeKind {
    Php,
    Node,
    Python,
    Go,
    Java,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeFamily {
    pub kind: RuntimeKind,
    pub name: &'static str,
    /// Newest first.
    pub versions: &'static [&'static str],
    /// `false` = shown as "coming soon" in the prototype.
    pub available: bool,
}

pub const RUNTIMES: &[RuntimeFamily] = &[
    RuntimeFamily {
        kind: RuntimeKind::Php,
        name: "PHP",
        versions: &["8.4", "8.3", "8.2", "8.1"],
        available: true,
    },
    RuntimeFamily {
        kind: RuntimeKind::Node,
        name: "Node.js",
        versions: &["24", "22", "20"],
        available: true,
    },
    RuntimeFamily {
        kind: RuntimeKind::Python,
        name: "Python",
        versions: &["3.13", "3.12", "3.11"],
        available: false,
    },
    RuntimeFamily {
        kind: RuntimeKind::Go,
        name: "Go",
        versions: &["1.23", "1.22"],
        available: false,
    },
    RuntimeFamily {
        kind: RuntimeKind::Java,
        name: "Java",
        versions: &["21", "17"],
        available: false,
    },
];

#[derive(Debug, Clone, Serialize)]
pub struct ServiceDef {
    pub id: &'static str,
    pub name: &'static str,
    pub version: &'static str,
    pub ports: &'static [u16],
    pub role: &'static str,
}

pub const SERVICES: &[ServiceDef] = &[
    ServiceDef {
        id: "nginx",
        name: "Nginx",
        version: "1.27",
        ports: &[8080],
        role: "Web server",
    },
    ServiceDef {
        id: "mariadb",
        name: "MariaDB",
        version: "11.4",
        ports: &[3306],
        role: "Database",
    },
    ServiceDef {
        id: "mailpit",
        name: "Mailpit",
        version: "1.20",
        ports: &[1025, 8025],
        role: "Mail catcher",
    },
];

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
    fn every_kind_is_listed_once() {
        for kind in [
            RuntimeKind::Php,
            RuntimeKind::Node,
            RuntimeKind::Python,
            RuntimeKind::Go,
            RuntimeKind::Java,
        ] {
            assert_eq!(RUNTIMES.iter().filter(|r| r.kind == kind).count(), 1);
            assert!(!runtime(kind).versions.is_empty());
        }
    }

    #[test]
    fn service_ids_are_unique() {
        let mut ids: Vec<_> = SERVICES.iter().map(|s| s.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), SERVICES.len());
    }
}
