//! The tools AI coding tools can call. Tools that only read are always on;
//! each action tool, and reading what is inside databases, is off until
//! the user allows it. Nothing here can delete a site, a whole database or
//! a file.

use std::collections::HashMap;
use std::path::Path;

use serde_json::{json, Map, Value};

use super::sql;

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
        "Lists the services Stackyard manages (web server, databases, mail catcher): whether each is switched on, installed and running, and its ports.",
    ),
    read(
        "list_runtimes",
        "List runtimes",
        "Lists PHP and Node.js versions: which are installed (with exact releases), which is the default, and which could be installed.",
    ),
    read(
        "list_databases",
        "List databases",
        "Lists the MySQL or MariaDB databases with their table count and size, or with database set, that database's tables and views with row estimates and sizes. Reads names only, never what is in the tables.",
    ),
    read(
        "read_logs",
        "Read a log",
        "Returns the last lines of a log: a service (caddy, mysql, mariadb, postgres, redis, mongodb, mailpit, meilisearch, seaweedfs, nats), PHP for one version (php-8.4), a scheduled task (task-<id>) or the background service (daemon). Use it to find out why a site or service fails.",
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
        "Switches the PHP or Node.js version a site runs on. For a site found in a parked folder, the version is written to the project's .stackyard file.",
    ),
    private_read(
        "query_database",
        "Query a database",
        "Runs one SELECT, SHOW, DESCRIBE or EXPLAIN statement on MySQL or MariaDB and returns the rows. It runs read-only, so it cannot change anything.",
    ),
    ToolInfo {
        destructive: true,
        ..action(
            "change_database",
            "Change a database",
            "Runs SQL that changes data or tables on MySQL or MariaDB (INSERT, UPDATE, DELETE, CREATE TABLE, ALTER TABLE, DROP TABLE...), one or more statements, and returns how many rows the last one changed. Dropping a whole database and reading or writing files are refused.",
        )
    },
];

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
            }),
            &[],
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
                    "enum": ["php", "node", "static"],
                    "description": "How to serve it; detected from the folder when left out",
                },
                "version": {
                    "type": "string",
                    "description": "PHP or Node.js version as list_runtimes shows it, e.g. 8.4 or 22; the project's pinned or default version when left out",
                },
                "port": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 65535,
                    "description": "Node.js only: the port the project's dev server listens on",
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
        _ => (json!({}), &[]),
    };
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false,
    })
}

const MAX_LOG_LINES: usize = 500;
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

type Args = Map<String, Value>;

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
        "list_databases" => args
            .get("database")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        "query_database" | "change_database" => format!("{}: {}", text("database"), text("sql")),
        "create_site" => format!("{} in {}", text("name"), text("folder")),
        "set_site_runtime" => format!("{} → {}", text("site"), text("version")),
        _ => String::new(),
    }
    .chars()
    .take(200)
    .collect()
}

fn fail(e: Error) -> String {
    e.to_string()
}

fn settings(ctx: &Ctx) -> Result<Settings, String> {
    Settings::load_from(&ctx.home.join("settings.toml")).map_err(fail)
}

fn text<'a>(args: &'a Args, key: &str) -> Result<Option<&'a str>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.trim())),
        Some(_) => Err(format!("{key} must be a string")),
    }
}

fn required<'a>(args: &'a Args, key: &str) -> Result<&'a str, String> {
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

fn flag(args: &Args, key: &str) -> Result<Option<bool>, String> {
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
fn sites_changed(ctx: &Ctx) -> Option<String> {
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

fn database_client(ctx: &Ctx) -> Result<databases::Client, String> {
    databases::Client::open(ctx.home).map_err(|e| {
        format!("{e}. Database tools work with MySQL or MariaDB; the user can install one in Stackyard › Services")
    })
}

fn list_databases(ctx: &Ctx, args: &Args) -> Result<Value, String> {
    let client = database_client(ctx)?;
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
    let detected_port = || match detect::detect(&folder).needs {
        detect::Needs::Node { port } => port,
        _ => 3000,
    };
    let runtime = match text(args, "runtime")? {
        None => web::runtime_for(&folder, &settings, &runtimes),
        Some("php") => SiteRuntime::Php {
            version: web::default_version(&settings, &runtimes, RuntimeKind::Php),
        },
        Some("node") => SiteRuntime::Node {
            version: web::default_version(&settings, &runtimes, RuntimeKind::Node),
            port: detected_port(),
        },
        Some("static") => SiteRuntime::Static,
        Some(other) => {
            return Err(format!(
                "runtime must be php, node or static, not {other:?}"
            ))
        }
    };
    let runtime = match (runtime, version, port) {
        (SiteRuntime::Static, Some(_), _) | (SiteRuntime::Static, _, Some(_)) => {
            return Err("a static site has no version or port; pass runtime php or node".into())
        }
        (SiteRuntime::Php { .. }, _, Some(_)) => {
            return Err("port is for Node.js sites; PHP sites are served by the web server".into())
        }
        (SiteRuntime::Php { version: v }, version, None) => SiteRuntime::Php {
            version: version.map_or(v, str::to_string),
        },
        (
            SiteRuntime::Node {
                version: v,
                port: p,
            },
            version,
            port,
        ) => SiteRuntime::Node {
            version: version.map_or(v, str::to_string),
            port: port.unwrap_or(p),
        },
        (SiteRuntime::Static, None, None) => SiteRuntime::Static,
    };
    if let (Some(kind), SiteRuntime::Php { version } | SiteRuntime::Node { version, .. }) =
        (runtime.kind(), &runtime)
    {
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
    let kind =
        served.site.runtime.kind().ok_or_else(|| {
            format!("{domain} serves plain files and has no PHP or Node.js version")
        })?;
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
