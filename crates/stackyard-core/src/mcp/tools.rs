//! The tools AI coding tools can call. Tools that only read are always on;
//! each action tool, and reading what is inside databases, is off until
//! the user allows it. Nothing here can delete a site, a whole database or
//! a file for good: a destroyed WordPress staging waits in the trash.

use std::collections::HashMap;
use std::path::Path;

use serde_json::{json, Map, Value};

use super::{sql, wordpress};

use crate::catalog::{self, RuntimeKind, RUNTIMES, SERVICES};
use crate::control::{self, Request, Response};
use crate::runtimes::RuntimeManager;
use crate::services::ServiceManager;
use crate::settings::Settings;
use crate::shell::pins;
use crate::sites::{Site, SiteRuntime, SiteStore};
use crate::supervisor::{self, ProcessState};
use crate::web::{self, Backend, Ports, ServedSite};
use crate::{databases, detect, modules, paths, ports, Error};

pub struct ToolInfo {
    pub name: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    /// Stays off until the user allows it.
    pub action: bool,
    /// Changes nothing.
    pub read_only: bool,
    /// Can delete data.
    pub destructive: bool,
}

const fn read(name: &'static str, title: &'static str, description: &'static str) -> ToolInfo {
    ToolInfo {
        name,
        title,
        description,
        action: false,
        read_only: true,
        destructive: false,
    }
}

const fn action(name: &'static str, title: &'static str, description: &'static str) -> ToolInfo {
    ToolInfo {
        name,
        title,
        description,
        action: true,
        read_only: false,
        destructive: false,
    }
}

/// Only reads, but what it reads (data in databases) is the user's to share.
const fn private_read(
    name: &'static str,
    title: &'static str,
    description: &'static str,
) -> ToolInfo {
    ToolInfo {
        read_only: true,
        ..action(name, title, description)
    }
}

pub const TOOLS: &[ToolInfo] = &[
    read(
        "list_sites",
        "List sites",
        "Lists the local sites Stackyard serves: domain and URL, project folder, web root, runtime (PHP or Node.js version), detected framework, and any problem that stops the site from working.",
    ),
    read(
        "list_services",
        "List services",
        "Lists the services Stackyard manages (web server, MySQL, MariaDB, PostgreSQL, Redis, MongoDB, Mailpit, search, storage and queues): whether each is switched on, installed and running, and its ports.",
    ),
    read(
        "list_runtimes",
        "List runtimes",
        "Lists PHP, Node.js, Python, Go and Java versions: which are installed (with exact releases), which is the default, and which could be installed with install_runtime.",
    ),
    read(
        "list_databases",
        "List databases",
        "Lists the MySQL or MariaDB databases (or PostgreSQL's, with server set to postgres) with their table count and size, or with database set, that database's tables and views with row estimates and sizes. Reads names only, never what is in the tables.",
    ),
    read(
        "read_logs",
        "Read a log",
        "Returns the last lines of a log: a service (caddy, mysql, mariadb, postgres, redis, mongodb, mailpit, meilisearch, seaweedfs, nats), PHP for one version (php-8.4), a scheduled task (task-<id>, ids from list_tasks) or the background service (daemon). Use it to find out why a site or service fails.",
    ),
    read(
        "list_tasks",
        "List scheduled tasks",
        "Lists the scheduled tasks: id, name, command, project folder, schedule, whether each is on, when it runs next, how its last run went and anything that stops it. Read a task's output with read_logs and task-<id>.",
    ),
    read(
        "list_backups",
        "List backups",
        "Lists the backups Stackyard has made: database backups (MySQL, MariaDB and PostgreSQL) and site backups (a site's files and database in one zip), with their size and when they were made. Set site or database to see only theirs.",
    ),
    action(
        "start_service",
        "Start a service",
        "Starts an installed service in the background. Starting caddy, the web server, also starts PHP for every site.",
    ),
    action(
        "stop_service",
        "Stop a service",
        "Stops a running service.",
    ),
    action(
        "restart_service",
        "Restart a service",
        "Stops a service and starts it again, e.g. after changing its settings or when it hangs. Restarting caddy also restarts PHP for every site.",
    ),
    action(
        "create_site",
        "Create a site",
        "Adds a local site for a project folder, served at https://<name><ending> (e.g. https://myapp.stack). Without a runtime, Stackyard detects PHP, Node.js or plain files from the folder and uses the project's pinned or default version.",
    ),
    action(
        "set_site_runtime",
        "Switch a site's version",
        "Switches the PHP, Node.js, Python, Go or Java version a site runs on. For a site found in a parked folder, the version is written to the project's .stackyard file.",
    ),
    action(
        "install_runtime",
        "Install a runtime version",
        "Downloads and installs a PHP, Node.js, Python, Go or Java version that list_runtimes shows as not installed, e.g. before switching a site to it. It becomes the default only when it is the first of its kind.",
    ),
    action(
        "backup_database",
        "Back up a database",
        "Saves a MySQL, MariaDB or PostgreSQL database to a new gzipped file in Stackyard's backups folder, which the user can restore on the Databases screen. Make one before changing a database.",
    ),
    action(
        "backup_site",
        "Back up a site",
        "Packs a site's files and its database into a new zip in Stackyard's backups folder, which the user can restore in Sites › Manage › Backups. Make one before a big change.",
    ),
    private_read(
        "query_database",
        "Query a database",
        "Runs one SELECT, SHOW, DESCRIBE or EXPLAIN statement on MySQL or MariaDB and returns the rows. It runs read-only, so it cannot change anything.",
    ),
    read(
        "list_wordpress_environments",
        "List WordPress environments",
        "Lists the WordPress sites with their environments: each main site, its stagings (such as checkout.shop.stack) and what each was made from, and the live site a copy was cloned from, with when they were last in step.",
    ),
    read(
        "wordpress_overview",
        "Look at a WordPress site",
        "Returns a local WordPress site's versions (WordPress, PHP, WooCommerce), database and table prefix, theme, plugins and themes with their status, debug settings (and the live site's, for a copy of one), snapshots that can undo a merge or pull, live backups made before pushes, and warnings such as a DB_HOST pointing at the wrong port.",
    ),
    read(
        "read_wordpress_log",
        "Read a WordPress debug log",
        "Returns the last lines of a local WordPress site's debug.log, where PHP errors, warnings and notices go while WP_DEBUG_LOG is on (set_wordpress_debug turns it on). Use it to find out why a WordPress page fails.",
    ),
    action(
        "create_wordpress_staging",
        "Create a WordPress staging",
        "Makes a staging of a WordPress site: a full copy with its own files and database at <name>.<site>, e.g. checkout.shop.stack, made from the main site, another staging or the live site. Mail goes to Mailpit and payments stay in test mode. Use it to try changes before they reach the main site.",
    ),
    action(
        "set_wordpress_debug",
        "Switch WordPress debugging",
        "Switches WP_DEBUG, WP_DEBUG_LOG, WP_DEBUG_DISPLAY, SCRIPT_DEBUG and SAVEQUERIES in a local WordPress site's wp-config.php; settings left out stay as they are. Only this computer's copy changes: wp-config.php is never pushed to a live site.",
    ),
    ToolInfo {
        destructive: true,
        ..action(
            "run_wp_cli",
            "Run WP-CLI",
            "Runs a WP-CLI command in a local WordPress site (never the live site) with the site's PHP, e.g. [\"plugin\", \"list\"] or [\"option\", \"get\", \"home\"], and returns what it printed. Prefer a staging for changes.",
        )
    },
    ToolInfo {
        destructive: true,
        ..action(
            "merge_wordpress_staging",
            "Merge a WordPress staging",
            "Lists what changed on a staging since it was made, or with apply set, merges it into the main site or another staging: code (files, plugins and theme switched on or off), settings (code plus changed settings) or everything (code plus the staging's whole database; the target's users, orders and customers are kept). Changes the target also made are left out. A snapshot is taken first so the user can undo it in Stackyard.",
        )
    },
    ToolInfo {
        destructive: true,
        ..action(
            "destroy_wordpress_staging",
            "Destroy a WordPress staging",
            "Moves a staging (never a main site) to Stackyard's trash, with its files and database; the user can bring it back for 7 days.",
        )
    },
    ToolInfo {
        destructive: true,
        ..action(
            "change_database",
            "Change a database",
            "Runs SQL that changes data or tables on MySQL or MariaDB (INSERT, UPDATE, DELETE, CREATE TABLE, ALTER TABLE, DROP TABLE...), one or more statements, and returns how many rows the last one changed. Dropping a whole database and reading or writing files are refused.",
        )
    },
];

/// The module a tool needs besides the MCP server, if any.
pub fn module_of(name: &str) -> Option<&'static str> {
    if name.contains("wordpress") || name == "run_wp_cli" {
        Some("wordpress")
    } else if name == "list_tasks" {
        Some("scheduler")
    } else if name == "backup_site" {
        Some("sitebackups")
    } else {
        None
    }
}

pub fn find(name: &str) -> Option<&'static ToolInfo> {
    TOOLS.iter().find(|t| t.name == name)
}

/// The JSON Schema of a tool's arguments.
pub fn input_schema(name: &str) -> Value {
    let service = |what: &str| {
        json!({
            "type": "string",
            "enum": SERVICES.iter().map(|s| s.id).collect::<Vec<_>>(),
            "description": what,
        })
    };
    let (properties, required): (Value, &[&str]) = match name {
        "read_logs" => (
            json!({
                "service": {
                    "type": "string",
                    "description": "caddy, mysql, mariadb, postgres, redis, mongodb, mailpit, meilisearch, seaweedfs, nats, php-<version> such as php-8.4, task-<id> for a scheduled task, or daemon",
                },
                "lines": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": MAX_LOG_LINES,
                    "default": 50,
                    "description": "How many lines from the end",
                },
            }),
            &["service"],
        ),
        "start_service" => (
            json!({ "service": service("The service to start") }),
            &["service"],
        ),
        "restart_service" => (
            json!({ "service": service("The service to restart") }),
            &["service"],
        ),
        "list_databases" => (
            json!({
                "database": {
                    "type": "string",
                    "description": "List this database's tables; leave out to list the databases",
                },
                "server": database_server(),
            }),
            &[],
        ),
        "backup_database" => (
            json!({
                "database": {
                    "type": "string",
                    "description": "The database, as list_databases shows it",
                },
                "server": database_server(),
            }),
            &["database"],
        ),
        "list_backups" => (
            json!({
                "site": {
                    "type": "string",
                    "description": "Only this site's backups, by domain (shop.stack) or name",
                },
                "database": {
                    "type": "string",
                    "description": "Only this database's backups",
                },
            }),
            &[],
        ),
        "backup_site" => (
            json!({
                "site": {
                    "type": "string",
                    "description": "The site's domain (shop.stack) or name (shop)",
                },
            }),
            &["site"],
        ),
        "install_runtime" => (
            json!({
                "runtime": {
                    "type": "string",
                    "enum": ["php", "node", "python", "go", "java"],
                },
                "version": {
                    "type": "string",
                    "description": "A version list_runtimes shows under not_installed, e.g. 8.4, 22, 3.13, 1.26 or 21",
                },
            }),
            &["runtime", "version"],
        ),
        "query_database" | "change_database" => (
            json!({
                "database": {
                    "type": "string",
                    "description": "The database to run it in, as list_databases shows it",
                },
                "sql": {
                    "type": "string",
                    "description": if name == "query_database" {
                        "One SELECT, SHOW, DESCRIBE or EXPLAIN statement"
                    } else {
                        "One or more SQL statements, separated by ;"
                    },
                },
                "max_rows": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": databases::MAX_ROWS,
                    "default": DEFAULT_ROWS,
                    "description": "Rows to return at most from each result",
                },
            }),
            &["database", "sql"],
        ),
        "stop_service" => (
            json!({ "service": service("The service to stop") }),
            &["service"],
        ),
        "create_site" => (
            json!({
                "name": {
                    "type": "string",
                    "description": "Lowercase letters, digits and hyphens, e.g. myapp for https://myapp.stack",
                },
                "folder": {
                    "type": "string",
                    "description": "Full path of the project folder",
                },
                "runtime": {
                    "type": "string",
                    "enum": ["php", "node", "python", "go", "java", "static"],
                    "description": "How to serve it; detected from the folder when left out",
                },
                "version": {
                    "type": "string",
                    "description": "Runtime version as list_runtimes shows it, e.g. 8.4, 22, 3.13, 1.26 or 21; the project's pinned or default version when left out",
                },
                "port": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 65535,
                    "description": "Node.js, Python, Go and Java only: the port the project's server listens on",
                },
                "https": {
                    "type": "boolean",
                    "default": true,
                    "description": "Serve over HTTPS (default) or plain HTTP",
                },
            }),
            &["name", "folder"],
        ),
        "set_site_runtime" => (
            json!({
                "site": {
                    "type": "string",
                    "description": "The site's domain (myapp.stack) or name (myapp)",
                },
                "version": {
                    "type": "string",
                    "description": "PHP or Node.js version as list_runtimes shows it, e.g. 8.3",
                },
            }),
            &["site", "version"],
        ),
        "list_wordpress_environments" => (
            json!({
                "site": {
                    "type": "string",
                    "description": "One site's environments, by domain (shop.stack) or name; every WordPress site when left out",
                },
            }),
            &[],
        ),
        "wordpress_overview" => (
            json!({
                "site": {
                    "type": "string",
                    "description": "The local WordPress site, e.g. shop.stack or checkout.shop.stack",
                },
            }),
            &["site"],
        ),
        "read_wordpress_log" => (
            json!({
                "site": {
                    "type": "string",
                    "description": "The local WordPress site, e.g. shop.stack",
                },
                "lines": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": MAX_LOG_LINES,
                    "default": 100,
                    "description": "How many lines from the end",
                },
            }),
            &["site"],
        ),
        "set_wordpress_debug" => (
            json!({
                "site": {
                    "type": "string",
                    "description": "The local WordPress site, e.g. shop.stack",
                },
                "debug": { "type": "boolean", "description": "WP_DEBUG" },
                "log": { "type": "boolean", "description": "WP_DEBUG_LOG: errors written to debug.log" },
                "display": { "type": "boolean", "description": "WP_DEBUG_DISPLAY: errors shown on the page" },
                "scripts": { "type": "boolean", "description": "SCRIPT_DEBUG: full, unminified scripts and styles" },
                "queries": { "type": "boolean", "description": "SAVEQUERIES: every database query kept for the page" },
            }),
            &["site"],
        ),
        "create_wordpress_staging" => (
            json!({
                "site": {
                    "type": "string",
                    "description": "The main site, e.g. shop.stack",
                },
                "name": {
                    "type": "string",
                    "description": "Lowercase letters, digits and hyphens, e.g. checkout for checkout.shop.stack",
                },
                "from": {
                    "type": "string",
                    "description": "live, or the domain of the main site or a staging to copy; the main site when left out",
                },
                "copy_media": {
                    "type": "boolean",
                    "default": true,
                    "description": "Copy wp-content/uploads now; when false, each file is copied the first time a page asks for it",
                },
            }),
            &["site", "name"],
        ),
        "run_wp_cli" => (
            json!({
                "site": {
                    "type": "string",
                    "description": "The local site, e.g. checkout.shop.stack",
                },
                "args": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "WP-CLI's arguments without `wp`, e.g. [\"plugin\", \"list\", \"--format=json\"]",
                },
            }),
            &["site", "args"],
        ),
        "merge_wordpress_staging" => (
            json!({
                "staging": {
                    "type": "string",
                    "description": "The staging, e.g. checkout.shop.stack",
                },
                "into": {
                    "type": "string",
                    "description": "The copy to merge into; the main site when left out",
                },
                "level": {
                    "type": "string",
                    "enum": ["code", "settings", "everything"],
                    "default": "code",
                },
                "apply": {
                    "type": "boolean",
                    "default": false,
                    "description": "Merge; when false only lists the changes",
                },
            }),
            &["staging"],
        ),
        "destroy_wordpress_staging" => (
            json!({
                "staging": {
                    "type": "string",
                    "description": "The staging, e.g. checkout.shop.stack",
                },
            }),
            &["staging"],
        ),
        _ => (json!({}), &[]),
    };
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false,
    })
}

pub(super) const MAX_LOG_LINES: usize = 500;

fn database_server() -> Value {
    json!({
        "type": "string",
        "enum": ["mysql", "postgres"],
        "default": "mysql",
        "description": "mysql for MySQL or MariaDB (whichever runs), or postgres for PostgreSQL",
    })
}
/// Rows a query returns unless the AI tool asks for more.
const DEFAULT_ROWS: usize = 100;
/// Characters of one value a query returns; the rest is cut off.
const MAX_VALUE: usize = 2000;

/// Where a call runs: the Stackyard folder, and the `stackyard` program to
/// start in the background when a service is started.
pub(super) struct Ctx<'a> {
    pub home: &'a Path,
    pub program: &'a Path,
}

/// What a call did: a short description for the activity log, and the
/// answer or the reason it failed.
pub(super) struct Outcome {
    pub summary: String,
    pub result: Result<Value, String>,
}

pub(super) type Args = Map<String, Value>;

pub(super) fn call(ctx: &Ctx, tool: &ToolInfo, args: &Args) -> Outcome {
    let summary = summarize(tool.name, args);
    let result = match tool.name {
        "list_sites" => list_sites(ctx),
        "list_services" => list_services(ctx),
        "list_runtimes" => list_runtimes(ctx),
        "read_logs" => read_logs(ctx, args),
        "start_service" => start_service(ctx, args),
        "stop_service" => stop_service(ctx, args),
        "restart_service" => restart_service(ctx, args),
        "list_databases" => list_databases(ctx, args),
        "query_database" => run_sql(ctx, args, false),
        "change_database" => run_sql(ctx, args, true),
        "create_site" => create_site(ctx, args),
        "set_site_runtime" => set_site_runtime(ctx, args),
        "list_tasks" => list_tasks(ctx),
        "list_backups" => list_backups(ctx, args),
        "install_runtime" => install_runtime(ctx, args),
        "backup_database" => backup_database(ctx, args),
        "backup_site" => backup_site(ctx, args),
        "wordpress_overview" => wordpress::overview(ctx, args),
        "read_wordpress_log" => wordpress::read_log(ctx, args),
        "set_wordpress_debug" => wordpress::set_debug(ctx, args),
        "list_wordpress_environments" => wordpress::environments(ctx, args),
        "create_wordpress_staging" => wordpress::create_staging(ctx, args),
        "run_wp_cli" => wordpress::run_cli(ctx, args),
        "merge_wordpress_staging" => wordpress::merge_staging(ctx, args),
        "destroy_wordpress_staging" => wordpress::destroy_staging(ctx, args),
        other => Err(format!("unknown tool {other}")),
    };
    Outcome { summary, result }
}

pub(super) fn summarize(tool: &str, args: &Args) -> String {
    let text = |key: &str| args.get(key).and_then(Value::as_str).unwrap_or("?");
    match tool {
        "read_logs" | "start_service" | "stop_service" | "restart_service" => {
            text("service").to_string()
        }
        "list_databases" | "list_backups" => ["site", "database", "server"]
            .iter()
            .filter_map(|key| args.get(*key).and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join(" "),
        "backup_database" => text("database").to_string(),
        "backup_site" | "wordpress_overview" | "read_wordpress_log" => text("site").to_string(),
        "install_runtime" => format!("{} {}", text("runtime"), text("version")),
        "set_wordpress_debug" => {
            let switched: Vec<String> = ["debug", "log", "display", "scripts", "queries"]
                .iter()
                .filter_map(|key| {
                    args.get(*key)
                        .and_then(Value::as_bool)
                        .map(|on| format!("{key} {}", if on { "on" } else { "off" }))
                })
                .collect();
            format!("{}: {}", text("site"), switched.join(", "))
        }
        "query_database" | "change_database" => format!("{}: {}", text("database"), text("sql")),
        "create_site" => format!("{} in {}", text("name"), text("folder")),
        "set_site_runtime" => format!("{} → {}", text("site"), text("version")),
        "list_wordpress_environments" => args
            .get("site")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        "create_wordpress_staging" => format!("{} of {}", text("name"), text("site")),
        "run_wp_cli" => format!(
            "{}: wp {}",
            text("site"),
            args.get("args")
                .and_then(Value::as_array)
                .map(|a| a
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(" "))
                .unwrap_or_default()
        ),
        "merge_wordpress_staging" => format!(
            "{} → {}{}",
            text("staging"),
            args.get("into")
                .and_then(Value::as_str)
                .unwrap_or("main site"),
            if args.get("apply").and_then(Value::as_bool) == Some(true) {
                ""
            } else {
                " (list only)"
            }
        ),
        "destroy_wordpress_staging" => text("staging").to_string(),
        _ => String::new(),
    }
    .chars()
    .take(200)
    .collect()
}

pub(super) fn fail(e: Error) -> String {
    e.to_string()
}

pub(super) fn settings(ctx: &Ctx) -> Result<Settings, String> {
    Settings::load_from(&ctx.home.join("settings.toml")).map_err(fail)
}

pub(super) fn text<'a>(args: &'a Args, key: &str) -> Result<Option<&'a str>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.trim())),
        Some(_) => Err(format!("{key} must be a string")),
    }
}

pub(super) fn required<'a>(args: &'a Args, key: &str) -> Result<&'a str, String> {
    text(args, key)?
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("{key} is required"))
}

fn number(args: &Args, key: &str) -> Result<Option<u64>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v
            .as_u64()
            .map(Some)
            .ok_or_else(|| format!("{key} must be a whole number")),
    }
}

pub(super) fn flag(args: &Args, key: &str) -> Result<Option<bool>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(_) => Err(format!("{key} must be true or false")),
    }
}

/// What the running Stackyard reports, by service id, and where it runs.
fn running(ctx: &Ctx) -> (HashMap<String, ProcessState>, Option<&'static str>) {
    let Some(client) = control::find(ctx.home) else {
        return (HashMap::new(), None);
    };
    let states = match client.send(Request::Status) {
        Ok(Response::Services { services }) => {
            services.into_iter().map(|s| (s.id, s.state)).collect()
        }
        _ => HashMap::new(),
    };
    (states, Some(owner_name(client.owner)))
}

fn owner_name(owner: control::Owner) -> &'static str {
    match owner {
        control::Owner::App => "the Stackyard app",
        control::Owner::Daemon => "the background service",
        control::Owner::Up => "`stackyard up` in a terminal",
    }
}

fn web_running(ctx: &Ctx) -> bool {
    matches!(
        running(ctx).0.get("caddy"),
        Some(ProcessState::Running { .. })
    )
}

/// Tells the running Stackyard that sites changed, if one runs.
pub(super) fn sites_changed(ctx: &Ctx) -> Option<String> {
    let client = control::find(ctx.home)?;
    client
        .send(Request::SitesChanged)
        .err()
        .map(|e| format!("the running web server did not pick up the change: {e}"))
}

fn site_json(served: &ServedSite, ports: Ports) -> Value {
    let site = &served.site;
    let (runtime, problem) = match &served.backend {
        Backend::Php {
            version, installed, ..
        } => (
            json!({ "kind": "php", "version": version, "installed": installed }),
            (!installed).then(|| {
                format!("PHP {version} is not installed; the user can install it in Stackyard › Runtimes")
            }),
        ),
        Backend::Node { version, port } => (
            json!({ "kind": "node", "version": version, "dev_server_port": port }),
            ports::is_free(*port).then(|| {
                format!("nothing listens on port {port}: start the project's dev server (e.g. npm run dev)")
            }),
        ),
        Backend::App {
            runtime,
            version,
            port,
        } => (
            json!({ "kind": runtime.id(), "version": version, "server_port": port }),
            ports::is_free(*port).then(|| {
                format!("nothing listens on port {port}: Stackyard starts the app's server with the web server; its log is in Stackyard › Logs")
            }),
        ),
        Backend::Static => (json!({ "kind": "static" }), None),
    };
    let problem = problem.or_else(|| {
        (!served.root.is_dir()).then(|| format!("{} does not exist", served.root.display()))
    });
    json!({
        "name": site.name,
        "domain": site.domain(),
        "url": ports.url(&site.domain(), site.https),
        "localhost_url": ports.url(&served.localhost(), site.https),
        "folder": site.path,
        "web_root": served.root,
        "runtime": runtime,
        "framework": served.framework,
        "https": site.https,
        "parked": served.parked,
        "problem": problem,
    })
}

fn served_sites(ctx: &Ctx) -> Result<Vec<ServedSite>, String> {
    ServiceManager::new(ctx.home).sites().map_err(fail)
}

fn list_sites(ctx: &Ctx) -> Result<Value, String> {
    let ports = Ports::current(ctx.home);
    let sites: Vec<Value> = ServiceManager::new(ctx.home)
        .user_sites()
        .map_err(fail)?
        .iter()
        .map(|s| site_json(s, ports))
        .collect();
    let settings = settings(ctx)?;
    Ok(json!({
        "sites": sites,
        "parked_folders": settings.parked,
        "domain_ending": settings.tld,
        "web_server_running": web_running(ctx),
    }))
}

fn list_services(ctx: &Ctx) -> Result<Value, String> {
    let settings = settings(ctx)?;
    let manager = ServiceManager::new(ctx.home);
    let (states, owner) = running(ctx);
    let services: Vec<Value> = SERVICES
        .iter()
        .filter(|def| modules::find(def.id).is_some_and(|m| m.available_here()))
        .map(|def| {
            let ports = crate::services::ports(ctx.home, def);
            let state = states.get(def.id).cloned().unwrap_or(ProcessState::Stopped);
            let mut value = json!({
                "id": def.id,
                "name": def.name,
                "role": def.role,
                "version": def.version,
                "ports": ports,
                "switched_on": settings.module_enabled(def.id),
                "installed": manager.get(def.id).map(|i| i.release),
            });
            if let (Some(map), Ok(Value::Object(state))) =
                (value.as_object_mut(), serde_json::to_value(state))
            {
                map.extend(state);
            }
            value
        })
        .collect();
    Ok(json!({ "services": services, "running_in": owner }))
}

fn list_runtimes(ctx: &Ctx) -> Result<Value, String> {
    let settings = settings(ctx)?;
    let manager = RuntimeManager::new(ctx.home);
    let runtimes: Vec<Value> = RUNTIMES
        .iter()
        .filter(|family| family.available)
        .map(|family| {
            let installed: Vec<Value> = family
                .versions
                .iter()
                .filter_map(|v| manager.get(family.kind, v))
                .map(|i| json!({ "version": i.version, "release": i.release }))
                .collect();
            let not_installed: Vec<&str> = family
                .versions_on(manager.platform().os)
                .filter(|v| manager.get(family.kind, v).is_none())
                .collect();
            json!({
                "kind": family.kind.id(),
                "name": family.name,
                "switched_on": settings.module_enabled(family.kind.id()),
                "default": manager.default_for(&settings, family.kind).map(|i| i.version),
                "installed": installed,
                "not_installed": not_installed,
            })
        })
        .collect();
    Ok(json!({ "runtimes": runtimes }))
}

fn read_logs(ctx: &Ctx, args: &Args) -> Result<Value, String> {
    let id = required(args, "service")?;
    let lines = number(args, "lines")?
        .unwrap_or(50)
        .clamp(1, MAX_LOG_LINES as u64) as usize;
    let file = supervisor::log_file(ctx.home, id).map_err(fail)?;
    let lines = supervisor::tail_file(&file, lines).map_err(fail)?;
    let note = lines
        .is_empty()
        .then_some("The log is empty: this has not run yet, or wrote nothing.");
    Ok(json!({ "service": id, "file": file, "lines": lines, "note": note }))
}

/// A service the user switched on and installed, or why it cannot start.
fn startable(ctx: &Ctx, id: &str) -> Result<&'static catalog::ServiceDef, String> {
    let def = service(id)?;
    settings(ctx)?.require_module(def.id).map_err(fail)?;
    if ServiceManager::new(ctx.home).get(def.id).is_none() {
        return Err(format!(
            "{} is not installed; the user can install it in Stackyard › Services",
            def.name
        ));
    }
    Ok(def)
}

fn service(id: &str) -> Result<&'static catalog::ServiceDef, String> {
    catalog::service(id).ok_or_else(|| {
        let ids: Vec<&str> = SERVICES.iter().map(|s| s.id).collect();
        format!("no service named {id:?}; use one of {}", ids.join(", "))
    })
}

fn service_state(response: Response, id: &str) -> Value {
    match response {
        Response::Services { services } => services
            .into_iter()
            .find(|s| s.id == id)
            .and_then(|s| serde_json::to_value(s.state).ok())
            .unwrap_or(Value::Null),
        _ => Value::Null,
    }
}

fn start_service(ctx: &Ctx, args: &Args) -> Result<Value, String> {
    let def = startable(ctx, required(args, "service")?)?;
    let client = control::start_background(ctx.home, ctx.program).map_err(fail)?;
    let response = client
        .send(Request::Start {
            ids: vec![def.id.into()],
        })
        .map_err(fail)?;
    let mut answer = json!({
        "service": def.id,
        "state": service_state(response, def.id),
        "running_in": owner_name(client.owner),
    });
    if def.id == "caddy" {
        let ports = Ports::current(ctx.home);
        let urls: Vec<String> = served_sites(ctx)?
            .iter()
            .map(|s| ports.url(&s.domain(), s.site.https))
            .collect();
        answer["sites"] = json!(urls);
    }
    Ok(answer)
}

fn stop_service(ctx: &Ctx, args: &Args) -> Result<Value, String> {
    let def = service(required(args, "service")?)?;
    let Some(client) = control::find(ctx.home) else {
        return Ok(
            json!({ "service": def.id, "state": "stopped", "note": "Nothing was running." }),
        );
    };
    let response = client
        .send(Request::Stop {
            ids: vec![def.id.into()],
        })
        .map_err(fail)?;
    Ok(json!({ "service": def.id, "state": service_state(response, def.id) }))
}

fn restart_service(ctx: &Ctx, args: &Args) -> Result<Value, String> {
    let def = startable(ctx, required(args, "service")?)?;
    if let Some(client) = control::find(ctx.home) {
        client
            .send(Request::Stop {
                ids: vec![def.id.into()],
            })
            .map_err(fail)?;
    }
    start_service(ctx, args)
}

pub(super) fn database_client(ctx: &Ctx) -> Result<databases::Client, String> {
    databases::Client::open(ctx.home).map_err(|e| {
        format!("{e}. Database tools work with MySQL or MariaDB; the user can install one in Stackyard › Services")
    })
}

/// The client for the server an AI tool named: `postgres`, or MySQL and
/// MariaDB when left out.
fn server_client(ctx: &Ctx, args: &Args) -> Result<databases::Client, String> {
    match text(args, "server")?.filter(|s| !s.is_empty()) {
        None | Some("mysql") | Some("mariadb") => database_client(ctx),
        Some("postgres") => databases::Client::open_server(ctx.home, Some("postgres"))
            .map_err(|e| format!("{e}. The user can install PostgreSQL in Stackyard › Services")),
        Some(other) => Err(format!("server must be mysql or postgres, not {other:?}")),
    }
}

fn list_databases(ctx: &Ctx, args: &Args) -> Result<Value, String> {
    let client = server_client(ctx, args)?;
    let server = client.server();
    if let Some(database) = text(args, "database")? {
        let tables = client.tables(database).map_err(fail)?;
        return Ok(json!({ "server": server, "database": database, "tables": tables }));
    }
    let list = client.databases().map_err(fail)?;
    Ok(json!({ "server": server, "databases": list }))
}

fn run_sql(ctx: &Ctx, args: &Args, change: bool) -> Result<Value, String> {
    let database = required(args, "database")?;
    let sql = required(args, "sql")?;
    let max_rows = number(args, "max_rows")?
        .unwrap_or(DEFAULT_ROWS as u64)
        .clamp(1, databases::MAX_ROWS as u64) as usize;
    let checked = if change {
        sql::changes(sql)?
    } else {
        sql::read_only(sql)?
    };
    let mut sets = database_client(ctx)?
        .query(Some(database), &checked)
        .map_err(fail)?;
    let changed_rows = if change {
        sets.pop()
            .and_then(|set| set.rows.into_iter().next())
            .and_then(|row| row.into_iter().next().flatten())
            .and_then(|n| n.parse::<i64>().ok())
            .filter(|n| *n >= 0)
    } else {
        None
    };
    let results: Vec<Value> = sets
        .into_iter()
        .map(|mut set| {
            if set.rows.len() > max_rows {
                set.rows.truncate(max_rows);
                set.truncated = true;
            }
            for value in set.rows.iter_mut().flatten().flatten() {
                if value.chars().count() > MAX_VALUE {
                    *value = value.chars().take(MAX_VALUE).collect::<String>() + "…";
                }
            }
            json!({ "columns": set.columns, "rows": set.rows, "truncated": set.truncated })
        })
        .collect();
    let mut answer = json!({ "database": database, "results": results });
    if change {
        answer["changed_rows"] = json!(changed_rows);
    }
    Ok(answer)
}

/// Checks a version is one Stackyard offers and its runtime is switched on.
fn check_version(settings: &Settings, kind: RuntimeKind, version: &str) -> Result<(), String> {
    settings.require_module(kind.id()).map_err(fail)?;
    let family = catalog::runtime(kind);
    if !family.offers(version) {
        return Err(format!(
            "{} {version} is not offered; use one of {}",
            family.name,
            family.versions.join(", ")
        ));
    }
    Ok(())
}

fn create_site(ctx: &Ctx, args: &Args) -> Result<Value, String> {
    let name = required(args, "name")?;
    let folder = paths::expand_home(Path::new(required(args, "folder")?));
    if !folder.is_absolute() {
        return Err("folder must be a full path, e.g. /home/me/code/myapp".into());
    }
    if !folder.is_dir() {
        return Err(fail(Error::NotAFolder(folder)));
    }
    let version = text(args, "version")?.filter(|v| !v.is_empty());
    let port = match number(args, "port")? {
        Some(p) => Some(
            u16::try_from(p)
                .ok()
                .filter(|p| *p > 0)
                .ok_or("port must be 1-65535")?,
        ),
        None => None,
    };
    let settings = settings(ctx)?;
    let runtimes = RuntimeManager::new(ctx.home);
    let detected = detect::detect(&folder).needs;
    let runtime = match text(args, "runtime")? {
        None => web::runtime_for(&folder, &settings, &runtimes),
        Some("static") => SiteRuntime::Static,
        Some(id) => {
            let kind = RuntimeKind::from_id(id).ok_or_else(|| {
                format!("runtime must be php, node, python, go, java or static, not {id:?}")
            })?;
            let usual = match detected {
                detect::Needs::Node { port } if kind == RuntimeKind::Node => port,
                detect::Needs::App { runtime, port } if runtime == kind => port,
                _ if kind == RuntimeKind::Node => 3000,
                _ => 8000,
            };
            SiteRuntime::new(
                kind,
                web::default_version(&settings, &runtimes, kind),
                usual,
            )
        }
    };
    let runtime = match (runtime, version, port) {
        (SiteRuntime::Static, Some(_), _) | (SiteRuntime::Static, _, Some(_)) => {
            return Err(
                "a static site has no version or port; pass a runtime such as php or node".into(),
            )
        }
        (SiteRuntime::Php { .. }, _, Some(_)) => {
            return Err(
                "port is for sites with their own server; PHP sites are served by the web server"
                    .into(),
            )
        }
        (mut runtime, version, port) => {
            if let Some(version) = version {
                runtime.set_version(version);
            }
            if let (Some(port), Some(kind)) = (port, runtime.kind()) {
                runtime =
                    SiteRuntime::new(kind, runtime.version().unwrap_or_default().into(), port);
            }
            runtime
        }
    };
    if let (Some(kind), Some(version)) = (runtime.kind(), runtime.version()) {
        check_version(&settings, kind, version)?;
    }
    let mut store = SiteStore::load_from(&ctx.home.join("sites.toml")).map_err(fail)?;
    let domain = store
        .add(Site {
            name: name.into(),
            tld: settings.tld.clone(),
            path: folder,
            runtime,
            https: flag(args, "https")?.unwrap_or(true),
            dev_server: false,
            settings: Default::default(),
        })
        .map_err(fail)?
        .domain();
    store.save().map_err(fail)?;
    answer_for_site(ctx, &domain)
}

fn set_site_runtime(ctx: &Ctx, args: &Args) -> Result<Value, String> {
    let wanted = required(args, "site")?;
    let version = required(args, "version")?;
    let served = served_sites(ctx)?
        .into_iter()
        .find(|s| s.domain() == wanted || s.site.name == wanted)
        .ok_or_else(|| fail(Error::SiteNotFound(wanted.into())))?;
    let domain = served.domain();
    let kind = served
        .site
        .runtime
        .kind()
        .ok_or_else(|| format!("{domain} serves plain files and has no runtime version"))?;
    check_version(&settings(ctx)?, kind, version)?;
    let file = ctx.home.join("sites.toml");
    let mut store = SiteStore::load_from(&file).map_err(fail)?;
    crate::shell::set_site_version(&mut store, &served.site, kind, Some(version)).map_err(fail)?;
    let saved_in = match pins::pinned(&served.site.path, kind) {
        Ok(Some(pin)) => pin.file,
        _ => file,
    };
    let mut answer = answer_for_site(ctx, &domain)?;
    answer["saved_in"] = json!(saved_in);
    Ok(answer)
}

/// The site as `list_sites` shows it, after the running web server picked
/// up the change, with what is still needed for it to work.
fn answer_for_site(ctx: &Ctx, domain: &str) -> Result<Value, String> {
    let warning = sites_changed(ctx);
    let ports = Ports::current(ctx.home);
    let served = served_sites(ctx)?;
    let site = served
        .iter()
        .find(|s| s.domain() == domain)
        .ok_or_else(|| fail(Error::SiteNotFound(domain.into())))?;
    let mut notes: Vec<String> = warning.into_iter().collect();
    if !web_running(ctx) {
        notes.push(
            "The web server is not running; start the caddy service to open the site.".into(),
        );
    }
    Ok(json!({ "site": site_json(site, ports), "notes": notes }))
}

fn list_tasks(ctx: &Ctx) -> Result<Value, String> {
    let tasks = crate::scheduler::views(ctx.home).map_err(fail)?;
    Ok(json!({ "tasks": tasks }))
}

fn list_backups(ctx: &Ctx, args: &Args) -> Result<Value, String> {
    let site = text(args, "site")?.filter(|s| !s.is_empty());
    let database = text(args, "database")?.filter(|s| !s.is_empty());
    let database_backups: Vec<databases::Backup> = if site.is_some() && database.is_none() {
        Vec::new()
    } else {
        databases::backups(ctx.home)
            .into_iter()
            .filter(|b| database.is_none_or(|d| b.database == d))
            .collect()
    };
    let site_backups: Vec<crate::sitepack::SiteBackup> = if database.is_some() && site.is_none() {
        Vec::new()
    } else {
        let domains: Vec<String> = match site {
            Some(wanted) => vec![find_site(ctx, wanted)?.domain()],
            None => served_sites(ctx)?.iter().map(ServedSite::domain).collect(),
        };
        domains
            .iter()
            .flat_map(|d| crate::sitepack::backups(ctx.home, d))
            .collect()
    };
    Ok(json!({
        "database_backups": database_backups,
        "site_backups": site_backups,
        "folder": databases::backups_dir(ctx.home),
        "note": "The user restores backups in Stackyard: databases on the Databases screen, sites in Sites › Manage › Backups.",
    }))
}

/// A site by domain or name, parked ones included.
fn find_site(ctx: &Ctx, wanted: &str) -> Result<Site, String> {
    served_sites(ctx)?
        .into_iter()
        .find(|s| s.domain() == wanted || s.site.name == wanted)
        .map(|s| s.site)
        .ok_or_else(|| fail(Error::SiteNotFound(wanted.into())))
}

fn install_runtime(ctx: &Ctx, args: &Args) -> Result<Value, String> {
    let id = required(args, "runtime")?;
    let kind = RuntimeKind::from_id(id)
        .ok_or_else(|| format!("runtime must be php, node, python, go or java, not {id:?}"))?;
    let version = required(args, "version")?;
    let mut settings = settings(ctx)?;
    check_version(&settings, kind, version)?;
    let manager = RuntimeManager::new(ctx.home);
    let name = catalog::runtime(kind).name;
    if let Some(installed) = manager.get(kind, version) {
        return Ok(json!({
            "runtime": kind.id(),
            "version": installed.version,
            "release": installed.release,
            "note": format!("{name} {version} was already installed."),
        }));
    }
    let installed = manager.install(kind, version, &mut |_| {}).map_err(fail)?;
    if manager.fix_default(&mut settings, kind) {
        settings
            .save_to(&ctx.home.join("settings.toml"))
            .map_err(fail)?;
    }
    Ok(json!({
        "runtime": kind.id(),
        "version": installed.version,
        "release": installed.release,
        "folder": installed.path,
        "default": manager.default_for(&settings, kind).map(|i| i.version),
    }))
}

fn backup_database(ctx: &Ctx, args: &Args) -> Result<Value, String> {
    let database = required(args, "database")?;
    let backup = server_client(ctx, args)?.backup(database).map_err(fail)?;
    Ok(json!({
        "backup": backup,
        "folder": databases::backups_dir(ctx.home).join(&backup.database),
        "note": "The user can restore it on Stackyard's Databases screen.",
    }))
}

fn backup_site(ctx: &Ctx, args: &Args) -> Result<Value, String> {
    let site = find_site(ctx, required(args, "site")?)?;
    let packed = crate::sitepack::backup(ctx.home, &site, &mut |_| {}).map_err(fail)?;
    Ok(json!({
        "site": site.domain(),
        "backup": packed,
        "note": "The user can restore it in Stackyard › Sites › Manage › Backups.",
    }))
}
