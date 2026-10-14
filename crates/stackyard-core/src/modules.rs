//! Modules: every runtime, service and feature can be switched off.
//! A module that is off downloads nothing, starts nothing and hides its screen.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ModuleGroup {
    Runtime,
    Service,
    Feature,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModuleDef {
    pub id: &'static str,
    pub name: &'static str,
    pub group: ModuleGroup,
    pub description: &'static str,
    /// State on a fresh install.
    pub on_by_default: bool,
    /// `false` = listed as "coming soon" and cannot be switched on yet.
    pub available: bool,
}

const fn module(
    id: &'static str,
    name: &'static str,
    group: ModuleGroup,
    description: &'static str,
    on_by_default: bool,
    available: bool,
) -> ModuleDef {
    ModuleDef {
        id,
        name,
        group,
        description,
        on_by_default,
        available,
    }
}

use ModuleGroup::{Feature, Runtime, Service};

pub const MODULES: &[ModuleDef] = &[
    module(
        "php",
        "PHP",
        Runtime,
        "PHP versions for sites and the shell",
        true,
        true,
    ),
    module(
        "node",
        "Node.js",
        Runtime,
        "Node.js versions for sites and the shell",
        true,
        true,
    ),
    module("python", "Python", Runtime, "Python versions", false, false),
    module("go", "Go", Runtime, "Go toolchains", false, false),
    module("java", "Java", Runtime, "Java JDKs", false, false),
    module(
        "nginx",
        "Nginx",
        Service,
        "Web server for PHP sites",
        true,
        true,
    ),
    module(
        "mariadb",
        "MariaDB",
        Service,
        "MySQL-compatible database",
        true,
        true,
    ),
    module(
        "mailpit",
        "Mailpit",
        Service,
        "Catches outgoing mail for testing",
        false,
        true,
    ),
    module(
        "postgres",
        "PostgreSQL",
        Service,
        "PostgreSQL database",
        false,
        false,
    ),
    module("redis", "Redis", Service, "Cache and queues", false, false),
    module(
        "meilisearch",
        "Meilisearch",
        Service,
        "Search engine",
        false,
        false,
    ),
    module(
        "shell",
        "Stackyard Shell",
        Feature,
        "Terminal and PATH shims with each project's versions",
        true,
        false,
    ),
    module(
        "mcp",
        "MCP server",
        Feature,
        "Lets AI coding tools manage sites and services",
        false,
        false,
    ),
    module(
        "databases",
        "Databases screen",
        Feature,
        "Browse tables, run queries, back up and restore",
        false,
        false,
    ),
    module(
        "logs",
        "Log viewer",
        Feature,
        "Live logs from every service and app",
        false,
        false,
    ),
    module(
        "scheduler",
        "Task scheduler",
        Feature,
        "Cron-style jobs for scripts and commands",
        false,
        false,
    ),
];

pub fn find(id: &str) -> Option<&'static ModuleDef> {
    MODULES.iter().find(|m| m.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique() {
        let mut ids: Vec<_> = MODULES.iter().map(|m| m.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), MODULES.len());
    }

    #[test]
    fn every_catalog_entry_has_a_module() {
        for service in crate::catalog::SERVICES {
            assert!(find(service.id).is_some(), "no module for {}", service.id);
        }
    }
}
