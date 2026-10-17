//! `stackyard` command-line tool. Every command goes through `stackyard-core`,
//! so the CLI, the desktop app and the MCP server always agree.

use std::ffi::OsString;
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::{Command as Process, ExitCode};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use clap::{Args, Parser, Subcommand};
use stackyard_core::catalog::{self, RuntimeKind, RUNTIMES, SERVICES};
use stackyard_core::cleanup;
use stackyard_core::control::{self, ControlServer, Owner, Request, Response};
use stackyard_core::databases::{self, Client as DbClient};
use stackyard_core::detect::{self, Needs};
use stackyard_core::domain;
use stackyard_core::engine::Engine;
use stackyard_core::logs;
use stackyard_core::mcp;
use stackyard_core::modules::MODULES;
use stackyard_core::runtimes::{RuntimeManager, Step};
use stackyard_core::scheduler::{self, clock, Outcome, TaskInput};
use stackyard_core::services::ServiceManager;
use stackyard_core::settings::Settings;
use stackyard_core::shell::{self, path as user_path, pins, shims, Tool};
use stackyard_core::sites::{Site, SiteRuntime, SiteStore};
use stackyard_core::supervisor::ProcessState;
use stackyard_core::system::{self, Elevate};
use stackyard_core::web::{self, Backend, Ports};
use stackyard_core::{ai, images, paths, platform, sitepack, starters, Error};

#[derive(Parser)]
#[command(name = "stackyard", version, about = "Local development stack manager")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List sites, or add, remove or park them
    Sites {
        #[command(subcommand)]
        action: Option<SitesCommand>,
    },
    /// List runtimes, or install, remove or pick a default version
    Runtimes {
        #[command(subcommand)]
        action: Option<RuntimeAction>,
    },
    /// List services, or install or remove one
    Services {
        #[command(subcommand)]
        action: Option<ServiceAction>,
    },
    /// List modules, or switch one on or off
    Modules {
        #[command(subcommand)]
        action: Option<ModuleAction>,
    },
    /// Show or change the domain ending for new sites (default .stack)
    Tld {
        /// New ending, e.g. `.stack` or `.test`
        value: Option<String>,
    },
    /// Start services in the background: the web server with `start caddy`, everything with `start`
    Start {
        /// e.g. `caddy mysql`; every installed service when left out
        services: Vec<String>,
    },
    /// Stop services: everything when none are named
    Stop {
        /// e.g. `mysql`
        services: Vec<String>,
    },
    /// Show what is running
    Status,
    /// Show the end of a service's log, e.g. `logs caddy`; list the logs when none is named
    Logs {
        /// `caddy`, `mysql`, `php-8.4`…
        id: Option<String>,
        /// How many lines
        #[arg(short = 'n', long, default_value_t = 50)]
        lines: usize,
        /// Keep printing new lines as they are written, until Ctrl+C
        #[arg(short, long)]
        follow: bool,
    },
    /// Run the web server (and any services named) in this terminal until Ctrl+C
    Up {
        /// Other services to run too, e.g. `mysql mailpit`
        services: Vec<String>,
    },
    /// Run services in the background for `start` (started for you)
    #[command(hide = true)]
    Daemon,
    /// Show or run the one-time setup for local domains and HTTPS
    System {
        #[command(subcommand)]
        action: Option<SystemAction>,
    },
    /// Open Stackyard Shell: php, node, npm, composer and mysql use each project's versions
    Shell,
    /// Show the terminal commands, or put them on your PATH for every terminal
    Shims {
        #[command(subcommand)]
        action: Option<ShimsAction>,
    },
    /// Show which version a command runs in this folder, and why
    Which {
        /// e.g. `php`, `node` or `composer`; all of them when left out
        tool: Option<String>,
    },
    /// Pin a version for this project in its `.stackyard` file, e.g. `use php 8.3`
    Use(RuntimeArgs),
    /// Show what takes disk space and what can go; `cleanup --recommended` frees the safe part
    Cleanup {
        /// What to clean, by the ids `stackyard cleanup` lists
        ids: Vec<String>,
        /// Clean everything marked as safe to remove
        #[arg(long)]
        recommended: bool,
        /// Confirm deleting service data, which can't be undone
        #[arg(long)]
        yes: bool,
    },
    /// List MySQL or MariaDB databases (PostgreSQL with --postgres), or create, back up and restore them
    Db {
        /// Work with PostgreSQL instead of MySQL or MariaDB
        #[arg(long, global = true)]
        postgres: bool,
        #[command(subcommand)]
        action: Option<DbAction>,
    },
    /// Make WebP and AVIF copies of JPG and PNG images, or smaller JPGs and PNGs
    Images(ImagesArgs),
    /// List scheduled tasks, or add, run and remove them
    Tasks {
        #[command(subcommand)]
        action: Option<TasksAction>,
    },
    /// Serve AI coding tools over MCP on stdin and stdout; `mcp setup` shows how to add it
    Mcp {
        #[command(subcommand)]
        action: Option<McpAction>,
    },
    /// List AI coding tools (Claude Code, Codex, Gemini CLI...), or install, update and open them
    Ai {
        #[command(subcommand)]
        action: Option<AiAction>,
    },
}

#[derive(Subcommand)]
enum AiAction {
    /// List the tools, which are installed, and whether updates are out
    List,
    /// Install a tool with npm on Stackyard's Node.js, e.g. `install claude`
    Install { id: String },
    /// Update a tool to its newest version
    Update { id: String },
    /// Remove a tool
    Remove { id: String },
    /// Open a tool in a new terminal window, in a site's folder or this folder
    Open {
        id: String,
        /// A site such as `shop.stack`, or a folder
        place: Option<String>,
    },
}

#[derive(Subcommand)]
enum DbAction {
    /// Create an empty database
    Create { name: String },
    /// Drop a database and everything in it
    Drop {
        name: String,
        /// Confirm: the data is gone afterwards (back it up first)
        #[arg(long)]
        yes: bool,
    },
    /// List a database's tables
    Tables { name: String },
    /// Run SQL and print what it returns, e.g. `db query shop "SELECT * FROM orders LIMIT 5"`
    Query { name: String, sql: String },
    /// Save a database to a gzipped SQL file in Stackyard's backups folder
    Backup { name: String },
    /// List backups, of every database or one
    Backups { name: Option<String> },
    /// Load a backup (the newest when no file is given) into its database, or another
    Restore {
        name: String,
        file: Option<String>,
        /// Restore into this database instead, creating it if needed
        #[arg(long)]
        into: Option<String>,
    },
}

#[derive(Subcommand)]
enum TasksAction {
    /// Add a task, e.g. `tasks add "Laravel scheduler" --run "php artisan schedule:run" --every "* * * * *"`
    Add {
        name: String,
        /// The command, run by the system shell
        #[arg(long)]
        run: String,
        /// When: crontab's five fields (minute hour day month weekday) or @hourly, @daily, @weekly
        #[arg(long)]
        every: String,
        /// The folder to run it in; this one when left out
        #[arg(long = "in")]
        folder: Option<PathBuf>,
    },
    /// Run a task now, here in the terminal
    Run { task: String },
    /// Switch a task on
    On { task: String },
    /// Switch a task off; it stays in the list
    Off { task: String },
    /// Remove a task with its log
    Remove { task: String },
    /// Show when a schedule runs, e.g. `tasks check "0 9 * * mon-fri"`
    Check { schedule: String },
}

#[derive(Subcommand)]
enum McpAction {
    /// Show how to add Stackyard to Claude Code, Cursor and other AI tools
    Setup,
    /// List the tools AI tools get, and which actions are allowed
    Tools,
    /// Let AI tools use an action, e.g. `allow start_service`, or `allow all`
    Allow { tool: String },
    /// Turn an action off again, or `deny all`
    Deny { tool: String },
    /// Show what AI tools did recently
    Log {
        /// How many entries
        #[arg(short = 'n', long, default_value_t = 20)]
        lines: usize,
    },
}

#[derive(Subcommand)]
enum ShimsAction {
    /// Create the commands and add them to your PATH (no password needed)
    On {
        /// Print the PATH change instead of making it
        #[arg(long)]
        print: bool,
    },
    /// Remove the commands and take them off your PATH
    Off,
}

#[derive(Subcommand)]
enum SystemAction {
    /// Apply setup with administrator rights (asks for your password)
    Setup {
        /// Print the commands instead of running them
        #[arg(long)]
        print: bool,
    },
}

#[derive(Subcommand)]
enum ServiceAction {
    /// Download and install a service, e.g. `install mysql`
    Install { id: String },
    /// Remove a service's program (its data is kept)
    Uninstall { id: String },
}

#[derive(Subcommand)]
enum ModuleAction {
    /// Switch a module on
    On { id: String },
    /// Switch a module off
    Off { id: String },
}

#[derive(Subcommand)]
enum RuntimeAction {
    /// Download and install a version, e.g. `install php 8.4`
    Install(RuntimeArgs),
    /// Remove an installed version
    Uninstall(RuntimeArgs),
    /// Use a version when a project does not pin its own
    Default(RuntimeArgs),
}

#[derive(Args)]
struct RuntimeArgs {
    /// `php` or `node`
    kind: String,
    /// Version as listed by `stackyard runtimes`, e.g. 8.4 or 22
    version: String,
}

#[derive(Subcommand)]
enum SitesCommand {
    /// List sites
    List,
    /// Add a site
    Add(AddSite),
    /// Make a new site from a starter: wordpress, laravel or astro
    New {
        /// Site name, e.g. `blog` for blog.stack
        name: String,
        /// wordpress, laravel or astro
        starter: String,
        /// Project folder, new or empty (default: the sites folder)
        #[arg(long)]
        path: Option<PathBuf>,
        /// Serve over plain HTTP only
        #[arg(long)]
        no_https: bool,
    },
    /// Remove a site by name or domain
    Remove { name: String },
    /// Make every project folder inside a folder a site (default: this folder)
    Park { folder: Option<PathBuf> },
    /// Stop making sites from a parked folder
    Unpark { folder: Option<PathBuf> },
    /// List parked folders
    Parked,
    /// Back up a site's files and database into one zip
    Backup { site: String },
    /// List a site's backups
    Backups { site: String },
    /// Put a site back as it was in a backup (the site as it is now is backed up first)
    Restore {
        site: String,
        /// The backup's file name, from `stackyard sites backups <site>`
        file: String,
    },
    /// Pack a site, with its database and settings, into one file for another computer
    Export {
        site: String,
        /// The file to write, e.g. `blog.zip`
        file: PathBuf,
    },
    /// Add a site from a file made by `sites export` or a backup
    Import {
        file: PathBuf,
        /// Site name (default: the one in the file)
        #[arg(long)]
        name: Option<String>,
        /// Project folder, new or empty (default: the sites folder)
        #[arg(long)]
        path: Option<PathBuf>,
    },
}

#[derive(Args)]
struct ImagesArgs {
    /// Images, or folders to search (default: this folder)
    paths: Vec<PathBuf>,
    /// Make WebP copies (the default, with --avif, when none is picked)
    #[arg(long)]
    webp: bool,
    /// Make AVIF copies
    #[arg(long)]
    avif: bool,
    /// Make smaller copies in the same format, named like photo.min.jpg
    #[arg(long)]
    min: bool,
    /// 1 to 100, like a JPG quality setting
    #[arg(long, default_value_t = 80)]
    quality: u8,
    /// Scale wider images down to this width
    #[arg(long)]
    max_width: Option<u32>,
    /// With --max-width: make every image exactly this high, cut from the middle
    #[arg(long, requires = "max_width")]
    height: Option<u32>,
    /// Put new files in this folder instead of next to the originals
    #[arg(long)]
    out: Option<PathBuf>,
    /// With --min: overwrite originals when the copy is smaller
    #[arg(long, requires = "min")]
    replace: bool,
}

#[derive(Args)]
struct AddSite {
    /// Site name, e.g. `myapp` for myapp.stack
    name: String,
    /// Project folder (defaults to the current folder)
    #[arg(long)]
    path: Option<PathBuf>,
    /// Serve with this PHP version
    #[arg(long, group = "runtime")]
    php: Option<String>,
    /// Proxy to a Node dev server with this Node version
    #[arg(long, group = "runtime")]
    node: Option<String>,
    /// Run the Django, FastAPI or Flask app with this Python version
    #[arg(long, group = "runtime")]
    python: Option<String>,
    /// Run the Go app with this Go version
    #[arg(long, group = "runtime")]
    go: Option<String>,
    /// Run the Spring Boot, Quarkus or Micronaut app with this Java version
    #[arg(long, group = "runtime")]
    java: Option<String>,
    /// Port the app's server listens on (default: detected)
    #[arg(long, conflicts_with_all = ["php", "plain"])]
    port: Option<u16>,
    /// Serve the files as they are, even if the folder looks like an app
    #[arg(long = "static", group = "runtime")]
    plain: bool,
    /// Serve over plain HTTP only
    #[arg(long)]
    no_https: bool,
}

fn main() -> ExitCode {
    // Started as `php`, `node`, `claude`... through a shim: run that tool.
    let mut args = std::env::args_os();
    let name = args
        .next()
        .as_deref()
        .map(Path::new)
        .and_then(Path::file_name)
        .and_then(|name| name.to_str())
        .map(str::to_string)
        .unwrap_or_default();
    if let Some(tool) = Tool::from_name(&name) {
        return run_shim(tool, args.collect());
    }
    if let Some(tool) = ai::from_command(&name) {
        return run_ai_tool(tool, args.collect());
    }
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> stackyard_core::Result<()> {
    match cli.command {
        Command::Sites {
            action: None | Some(SitesCommand::List),
        } => {
            let manager = ServiceManager::open();
            let ports = Ports::current(manager.home());
            let sites = manager.user_sites()?;
            if sites.is_empty() {
                println!(
                    "No sites yet. Add one with `stackyard sites add <name>`, or park a folder of projects with `stackyard sites park`."
                );
            }
            for served in &sites {
                let site = &served.site;
                let runtime = match &served.backend {
                    Backend::Php {
                        version,
                        installed: true,
                        ..
                    } => format!("PHP {version}"),
                    Backend::Php { version, .. } => format!("PHP {version} (not installed)"),
                    Backend::Node { version, port } => format!("Node {version} on :{port}"),
                    Backend::App {
                        runtime,
                        version,
                        port,
                    } => format!("{} {version} on :{port}", catalog::runtime(*runtime).name),
                    Backend::Static => "static".to_string(),
                };
                let framework = served
                    .framework
                    .map(|f| format!("{f}, "))
                    .unwrap_or_default();
                let parked = if served.parked { " (parked)" } else { "" };
                println!(
                    "{:<36} {framework}{runtime}  {}{parked}",
                    ports.url(&site.domain(), site.https),
                    site.path.display()
                );
            }
        }
        Command::Sites {
            action: Some(SitesCommand::Add(args)),
        } => {
            let settings = Settings::load()?;
            let path = match args.path {
                // Relative to this folder, so the web server finds it.
                Some(path) => current_dir()?.join(paths::expand_home(&path)),
                None => current_dir()?,
            };
            let chosen = [
                (RuntimeKind::Php, args.php),
                (RuntimeKind::Node, args.node),
                (RuntimeKind::Python, args.python),
                (RuntimeKind::Go, args.go),
                (RuntimeKind::Java, args.java),
            ]
            .into_iter()
            .find_map(|(kind, version)| Some((kind, version?)));
            let mut runtime = match chosen {
                Some((kind, version)) => {
                    let usual = match detect::detect(&path).needs {
                        Needs::Node { port } if kind == RuntimeKind::Node => port,
                        Needs::App { runtime, port } if runtime == kind => port,
                        _ if kind == RuntimeKind::Node => 3000,
                        _ => 8000,
                    };
                    SiteRuntime::new(kind, version, usual)
                }
                None if args.plain => SiteRuntime::Static,
                None => web::runtime_for(&path, &settings, &RuntimeManager::open()),
            };
            if let (Some(port), Some(kind), Some(version)) =
                (args.port, runtime.kind(), runtime.version())
            {
                runtime = SiteRuntime::new(kind, version.to_string(), port);
            }
            let mut store = SiteStore::load()?;
            let url = store
                .add(Site {
                    name: args.name,
                    tld: settings.tld,
                    path,
                    runtime,
                    https: !args.no_https,
                    dev_server: false,
                    settings: Default::default(),
                })?
                .url();
            store.save()?;
            notify(Request::SitesChanged);
            println!("Added {url}");
        }
        Command::Sites {
            action:
                Some(SitesCommand::New {
                    name,
                    starter,
                    path,
                    no_https,
                }),
        } => {
            let starter = starters::Starter::from_id(&starter).ok_or_else(|| {
                Error::Shell(format!(
                    "unknown starter {starter:?}; pick wordpress, laravel or astro"
                ))
            })?;
            domain::validate_site_name(&name)?;
            let settings = Settings::load()?;
            let home = paths::home();
            let folder = match path {
                Some(path) => current_dir()?.join(paths::expand_home(&path)),
                None => settings.sites_folder(&home).join(&name),
            };
            let domain = domain::site_domain(&name, &settings.tld);
            if SiteStore::load()?.list().iter().any(|s| s.domain() == domain) {
                return Err(Error::SiteExists(domain));
            }
            if starter.needs_database() {
                // Started by the running Stackyard, so it keeps running.
                let client = running_stackyard()?;
                let services = match client.send(Request::Status)? {
                    Response::Services { services } => services,
                    _ => Vec::new(),
                };
                let running = |id: &str| {
                    services
                        .iter()
                        .any(|s| s.id == id && matches!(s.state, ProcessState::Running { .. }))
                };
                if !running("mysql") && !running("mariadb") {
                    let services = ServiceManager::open();
                    let id = ["mysql", "mariadb"]
                        .into_iter()
                        .find(|id| services.get(id).is_some())
                        .ok_or_else(|| {
                            Error::Shell(format!(
                                "{} needs MySQL or MariaDB; install one with `stackyard services install mysql`",
                                starter.name()
                            ))
                        })?;
                    client.send(Request::Start {
                        ids: vec![id.to_string()],
                    })?;
                }
            }
            let existed = folder.exists();
            let mut last = String::new();
            let made = starters::create(&home, starter, &name, &folder, &mut |message| {
                // Download percentages only every tenth.
                let shown = match message.rsplit_once(' ') {
                    Some((text, pct)) if pct.ends_with('%') => {
                        let n: u32 = pct.trim_end_matches('%').parse().unwrap_or(0);
                        format!("{text} {}%", n / 10 * 10)
                    }
                    _ => message,
                };
                if shown != last {
                    eprintln!("{shown}");
                    last = shown;
                }
            });
            let made = match made {
                Ok(made) => made,
                Err(e) => {
                    if !existed {
                        let _ = std::fs::remove_dir_all(&folder);
                    }
                    return Err(e);
                }
            };
            let mut store = SiteStore::load()?;
            let url = store
                .add(Site {
                    name,
                    tld: settings.tld,
                    path: folder,
                    runtime: made.runtime,
                    https: !no_https,
                    dev_server: made.dev_server,
                    settings: stackyard_core::sites::SiteSettings {
                        database: made.database.clone().map(|name| stackyard_core::sites::SiteDatabase {
                            server: "mysql".into(),
                            name,
                        }),
                        ..Default::default()
                    },
                })?
                .url();
            store.save()?;
            notify(Request::SitesChanged);
            if let Some(database) = made.database {
                println!("Made the database {database}");
            }
            println!("Added {url}");
        }
        Command::Sites {
            action: Some(SitesCommand::Park { folder }),
        } => {
            let folder = match folder {
                Some(folder) => folder,
                None => current_dir()?,
            };
            let mut settings = Settings::load()?;
            let folder = settings.park(&folder)?;
            settings.save()?;
            notify(Request::SitesChanged);
            println!(
                "Parked {}: each project folder in it is now a site ending in {}",
                folder.display(),
                settings.tld
            );
        }
        Command::Sites {
            action: Some(SitesCommand::Unpark { folder }),
        } => {
            let folder = match folder {
                Some(folder) => folder,
                None => current_dir()?,
            };
            let mut settings = Settings::load()?;
            settings.unpark(&folder)?;
            settings.save()?;
            notify(Request::SitesChanged);
            println!("Unparked {}", paths::expand_home(&folder).display());
        }
        Command::Sites {
            action: Some(SitesCommand::Parked),
        } => {
            let settings = Settings::load()?;
            if settings.parked.is_empty() {
                println!("No parked folders. Park one with `stackyard sites park <folder>`.");
            }
            for folder in &settings.parked {
                println!("{}", folder.display());
            }
        }
        Command::Sites {
            action:
                Some(
                    action @ (SitesCommand::Backup { .. }
                    | SitesCommand::Backups { .. }
                    | SitesCommand::Restore { .. }
                    | SitesCommand::Export { .. }
                    | SitesCommand::Import { .. }),
                ),
        } => site_pack_action(action)?,
        Command::Sites {
            action: Some(SitesCommand::Remove { name }),
        } => {
            let mut store = SiteStore::load()?;
            let site = store.remove(&name)?;
            store.save()?;
            notify(Request::SitesChanged);
            println!("Removed {}", site.domain());
        }
        Command::Runtimes { action: None } => {
            let settings = Settings::load()?;
            let manager = RuntimeManager::open();
            for family in RUNTIMES {
                if !family.available {
                    println!(
                        "{:<8} {}  (coming soon)",
                        family.name,
                        family.versions.join(", ")
                    );
                    continue;
                }
                let default = settings.default_runtime(family.kind);
                let versions: Vec<String> = family
                    .versions_on(manager.platform().os)
                    .map(|v| match manager.get(family.kind, v) {
                        Some(i) if default == Some(v) => format!("{v} [{}, default]", i.release),
                        Some(i) => format!("{v} [{}]", i.release),
                        None => v.to_string(),
                    })
                    .collect();
                println!("{:<8} {}", family.name, versions.join(", "));
            }
        }
        Command::Runtimes {
            action: Some(action),
        } => runtime_action(action)?,
        Command::Services { action: None } => {
            let manager = ServiceManager::open();
            let settings = Settings::load()?;
            let name_width = SERVICES.iter().map(|s| s.name.len()).max().unwrap_or(0);
            let role_width = SERVICES.iter().map(|s| s.role.len()).max().unwrap_or(0);
            for service in SERVICES {
                let ports: Vec<String> = stackyard_core::services::ports(manager.home(), service)
                    .iter()
                    .map(u16::to_string)
                    .collect();
                let state = match manager.get(service.id) {
                    _ if !settings.module_enabled(service.id) => "off".to_string(),
                    Some(i) => format!("installed {}", i.release),
                    None => "not installed".to_string(),
                };
                println!(
                    "{:<name_width$} {:<5} {:<role_width$} port {:<11} {state}",
                    service.name,
                    service.version,
                    service.role,
                    ports.join(", ")
                );
            }
        }
        Command::Services {
            action: Some(ServiceAction::Install { id }),
        } => {
            Settings::load()?.require_module(&id)?;
            let installed = ServiceManager::open().install(&id, &mut progress_line(&id))?;
            eprintln!();
            println!(
                "Installed {id} {} in {}",
                installed.release,
                installed.path.display()
            );
        }
        Command::Services {
            action: Some(ServiceAction::Uninstall { id }),
        } => {
            ServiceManager::open().uninstall(&id)?;
            println!("Removed {id}; its data folder was kept");
        }
        Command::Modules { action: None } => {
            let settings = Settings::load()?;
            for module in MODULES {
                let state = match (module.available, settings.module_enabled(module.id)) {
                    (false, _) => "coming soon",
                    (true, true) => "on",
                    (true, false) => "off",
                };
                println!("{:<12} {:<12} {}", module.id, state, module.description);
            }
        }
        Command::Modules {
            action: Some(action),
        } => {
            let (id, on) = match action {
                ModuleAction::On { id } => (id, true),
                ModuleAction::Off { id } => (id, false),
            };
            let mut settings = Settings::load()?;
            settings.set_module(&id, on)?;
            settings.save()?;
            if !on {
                notify(Request::Stop { ids: vec![id.clone()] });
            }
            if id == "shell" && !on {
                turn_shims_off()?;
            }
            println!("{id} is now {}", if on { "on" } else { "off" });
        }
        Command::Up { services } => up(&services)?,
        Command::Start { services } => {
            let client = running_stackyard()?;
            let request = if services.is_empty() {
                Request::StartAll
            } else {
                Request::Start { ids: services }
            };
            let result = client.send(request);
            print_status(&client, client.send(Request::Status)?);
            result?;
        }
        Command::Stop { services } => match control::find(&paths::home()) {
            None => println!("Nothing is running."),
            Some(client) if services.is_empty() => {
                client.send(Request::StopAll)?;
                // The background service quits too; wait until it is gone
                // so whatever runs next starts afresh.
                if client.owner != Owner::App {
                    for _ in 0..150 {
                        if control::find(&paths::home()).is_none() {
                            break;
                        }
                        std::thread::sleep(Duration::from_millis(100));
                    }
                }
                println!("Stopped everything.");
            }
            Some(client) => print_status(&client, client.send(Request::Stop { ids: services })?),
        },
        Command::Status => match control::find(&paths::home()) {
            None => println!(
                "Nothing is running. Start the web server with `stackyard start caddy`, or everything with `stackyard start`."
            ),
            Some(client) => print_status(&client, client.send(Request::Status)?),
        },
        Command::Logs {
            id: None,
            ..
        } => {
            let all = logs::list(&paths::home());
            if all.is_empty() {
                println!("No logs yet: they appear once a service has run.");
            }
            for log in all {
                println!("{:<10} {:<20} {:>8} KB", log.id, log.name, log.size.div_ceil(1024));
            }
        }
        Command::Logs {
            id: Some(id),
            lines,
            follow,
        } => {
            let home = paths::home();
            let mut chunk = logs::read(&home, &id, None, lines)?;
            loop {
                if chunk.reset {
                    println!("--- the log started over ---");
                }
                for line in &chunk.lines {
                    println!("{line}");
                }
                if !follow {
                    break;
                }
                std::thread::sleep(Duration::from_millis(500));
                chunk = logs::read(&home, &id, Some(chunk.offset), lines)?;
            }
        }
        Command::Cleanup {
            ids,
            recommended,
            yes,
        } => {
            let home = paths::home();
            let report = cleanup::scan(&home);
            if ids.is_empty() && !recommended {
                println!("Stackyard uses {} in {}", size(report.total), home.display());
                for part in &report.usage {
                    println!("  {:<14} {:>10}", part.name, size(part.size));
                }
                if report.items.is_empty() {
                    println!("\nNothing to clean up.");
                    return Ok(());
                }
                println!("\nCan be cleaned up (* = safe, cleaned by --recommended):");
                for item in &report.items {
                    let mark = if item.recommended { "*" } else { " " };
                    println!("{mark} {:<24} {:>10}  {}", item.id, size(item.size), item.title);
                    println!("  {:<24} {:>10}  {}", "", "", item.detail);
                }
                println!("\nClean with `stackyard cleanup --recommended` or `stackyard cleanup <id>...`.");
                return Ok(());
            }
            let mut chosen = ids;
            if recommended {
                for item in report.items.iter().filter(|i| i.recommended) {
                    if !chosen.contains(&item.id) {
                        chosen.push(item.id.clone());
                    }
                }
            }
            let permanent: Vec<&str> = report
                .items
                .iter()
                .filter(|i| i.permanent && chosen.contains(&i.id))
                .map(|i| i.title.as_str())
                .collect();
            if !permanent.is_empty() && !yes {
                return Err(Error::Cleanup(format!(
                    "{} can't be brought back once deleted. Add --yes to delete it.",
                    permanent.join(", ")
                )));
            }
            let outcome = cleanup::clean(&home, &chosen);
            for title in &outcome.cleaned {
                println!("Cleaned {title}");
            }
            for problem in &outcome.problems {
                eprintln!("Not cleaned: {problem}");
            }
            println!("Freed {}.", size(outcome.freed));
            if !outcome.problems.is_empty() {
                std::process::exit(1);
            }
        }
        Command::Daemon => daemon()?,
        Command::System { action: None } => {
            let manager = ServiceManager::open();
            let wanted = manager.system_wanted()?;
            let status = wanted.status(&system::applied(manager.home()));
            if status.needed.is_empty() {
                println!("Local domains and HTTPS are set up.");
            } else {
                println!("Run `stackyard system setup` to:");
                for item in &status.needed {
                    println!("  - {item}");
                }
            }
            if status.waiting_for_certificate {
                println!("HTTPS: start the web server once to create its certificate, then run setup again.");
            }
        }
        Command::System {
            action: Some(SystemAction::Setup { print }),
        } => {
            let manager = ServiceManager::open();
            let wanted = manager.system_wanted()?;
            if print {
                print!("{}", wanted.script());
                return Ok(());
            }
            let how = if cfg!(windows) {
                Elevate::Dialog
            } else if platform::running_as_root() {
                Elevate::AlreadyAdmin
            } else {
                Elevate::Sudo
            };
            system::apply(manager.home(), &wanted, how)?;
            println!("Done. Local domains and HTTPS are set up.");
            if wanted.certificate.is_none() {
                println!("Start the web server once, then run this again to trust its HTTPS certificate.");
            }
        }
        Command::Shell => open_shell()?,
        Command::Shims { action: None } => {
            let home = paths::home();
            let bin = shims::bin_dir(&home);
            if !shims::installed(&home) {
                println!("The terminal commands are off. Turn them on with `stackyard shims on`, or open `stackyard shell`.");
            } else {
                let status = user_path::status(&bin);
                println!("Commands: {}", bin.display());
                if status.on_path {
                    println!("On your PATH through {}", status.places.join(", "));
                } else {
                    println!(
                        "Not on your PATH yet: run `stackyard shims on`, or use `stackyard shell`."
                    );
                }
            }
        }
        Command::Shims {
            action: Some(ShimsAction::On { print }),
        } => {
            Settings::load()?.require_module("shell")?;
            let home = paths::home();
            let bin = shims::bin_dir(&home);
            if print {
                print!("{}", user_path::preview(&bin));
                return Ok(());
            }
            shims::install(&home, &own_program()?)?;
            let changed = user_path::add(&bin)?;
            println!(
                "Created php, composer, node, npm, npx, mysql and mysqldump in {}",
                bin.display()
            );
            println!("Added them to your PATH in {}", changed.join(", "));
            println!("Open a new terminal to use them.");
        }
        Command::Shims {
            action: Some(ShimsAction::Off),
        } => {
            turn_shims_off()?;
            println!("Removed the terminal commands. Open a new terminal for this to take effect.");
        }
        Command::Which { tool } => {
            let context = shell::Context::load(&paths::home())?;
            let dir = current_dir()?;
            let tools = match tool {
                Some(name) => vec![Tool::from_name(&name)
                    .ok_or_else(|| Error::Shell(format!("{name:?} is not a Stackyard command; try php, composer, node, npm, npx, mysql or mysqldump")))?],
                None => Tool::ALL.to_vec(),
            };
            for tool in tools {
                match context.launch(tool, &dir, Vec::new()) {
                    Ok(launch) => println!(
                        "{:<10} {}  {}",
                        tool.name(),
                        launch.summary,
                        launch.program.display()
                    ),
                    Err(e) => println!("{:<10} {e}", tool.name()),
                }
            }
        }
        Command::Use(args) => {
            let kind = parse_kind(&args.kind)?;
            let file = pins::pin(&current_dir()?, kind, &args.version)?;
            println!(
                "{} {} pinned in {}",
                catalog::runtime(kind).name,
                args.version,
                file.display()
            );
            if RuntimeManager::open().get(kind, &args.version).is_none() {
                println!(
                    "It is not installed yet: run `stackyard runtimes install {} {}`",
                    kind.id(),
                    args.version
                );
            }
        }
        Command::Db { postgres, action } => db(postgres, action)?,
        Command::Images(args) => optimize_images(args)?,
        Command::Tasks { action } => tasks(action)?,
        Command::Mcp { action } => mcp(action)?,
        Command::Ai { action } => ai_tools(action)?,
        Command::Tld { value: None } => println!("{}", Settings::load()?.tld),
        Command::Tld { value: Some(value) } => {
            let mut settings = Settings::load()?;
            settings.set_tld(&value)?;
            settings.save()?;
            notify(Request::SitesChanged);
            println!("New sites will end in {}", settings.tld);
            if let Some(warning) = domain::tld_warning(&settings.tld) {
                eprintln!("warning: {warning}");
            }
        }
    }
    Ok(())
}

fn db(postgres: bool, action: Option<DbAction>) -> stackyard_core::Result<()> {
    let home = paths::home();
    Settings::load()?.require_module("databases")?;
    let size = |bytes: u64| match bytes {
        b if b < 1024 * 1024 => format!("{} KB", b.div_ceil(1024)),
        b => format!("{:.1} MB", b as f64 / 1024.0 / 1024.0),
    };
    if let Some(DbAction::Backups { name }) = &action {
        let all = databases::backups(&home);
        let shown: Vec<_> = all
            .iter()
            .filter(|b| name.as_ref().is_none_or(|n| &b.database == n))
            .collect();
        if shown.is_empty() {
            println!("No backups yet. Make one with `stackyard db backup <name>`.");
        }
        for b in shown {
            println!("{:<20} {:<40} {:>9}", b.database, b.file, size(b.size));
        }
        println!("Folder: {}", databases::backups_dir(&home).display());
        return Ok(());
    }
    let client = DbClient::open_server(&home, postgres.then_some("postgres"))?;
    let kind = if postgres { "postgres" } else { "mysql" };
    match action {
        None => {
            let server = client.server();
            match &server.version {
                Some(v) => println!("{} ({v})", server.name),
                None => {
                    return Err(Error::Database(format!(
                        "{} is not running. Start it with `stackyard start {}`.",
                        server.name, server.id
                    )))
                }
            }
            let all = client.databases()?;
            if all.is_empty() {
                println!("No databases yet. Create one with `stackyard db create <name>`.");
            }
            for d in all {
                println!(
                    "  {:<30} {:>4} tables {:>10}",
                    d.name,
                    d.tables,
                    size(d.size)
                );
            }
        }
        Some(DbAction::Create { name }) => {
            client.create(&name)?;
            println!("Created {name}.");
        }
        Some(DbAction::Drop { name, yes }) => {
            if !yes {
                return Err(Error::Database(format!(
                    "this deletes {name} and all its data; run it again with --yes (or back it up first with `stackyard db backup {name}`)"
                )));
            }
            client.drop(&name)?;
            println!("Dropped {name}.");
        }
        Some(DbAction::Tables { name }) => {
            for t in client.tables(&name)? {
                let kind = if t.kind == "VIEW" { " (view)" } else { "" };
                println!(
                    "  {:<40} ~{:>9} rows {:>10}{kind}",
                    t.name,
                    t.rows,
                    size(t.size)
                );
            }
        }
        Some(DbAction::Query { name, sql }) => {
            let sets = client.query(Some(&name), &sql)?;
            if sets.is_empty() {
                println!("Done.");
            }
            for set in sets {
                println!("{}", set.columns.join("\t"));
                for row in &set.rows {
                    let cells: Vec<&str> =
                        row.iter().map(|c| c.as_deref().unwrap_or("NULL")).collect();
                    println!("{}", cells.join("\t"));
                }
                if set.truncated {
                    println!("(only the first {} rows are shown)", databases::MAX_ROWS);
                }
            }
        }
        Some(DbAction::Backup { name }) => {
            let backup = client.backup(&name)?;
            println!(
                "Saved {} ({}) in {}",
                backup.file,
                size(backup.size),
                databases::backups_dir(&home)
                    .join(&backup.database)
                    .display()
            );
        }
        Some(DbAction::Restore { name, file, into }) => {
            let file = match file {
                Some(f) => f,
                None => databases::backups(&home)
                    .into_iter()
                    .find(|b| b.database == name && b.server == kind)
                    .map(|b| b.file)
                    .ok_or_else(|| Error::Database(format!("{name} has no backups yet")))?,
            };
            client.restore(&name, &file, into.as_deref())?;
            println!("Restored {file} into {}.", into.as_deref().unwrap_or(&name));
        }
        Some(DbAction::Backups { .. }) => unreachable!("handled above"),
    }
    Ok(())
}

fn mcp(action: Option<McpAction>) -> stackyard_core::Result<()> {
    let home = paths::home();
    match action {
        None => {
            Settings::load()?.require_module("mcp")?;
            if std::io::stdin().is_terminal() {
                eprintln!(
                    "stackyard mcp is started by AI tools and talks to them on stdin and stdout."
                );
                eprintln!("Run `stackyard mcp setup` to see how to add it. Press Ctrl+D to stop.");
            }
            let stdin = std::io::stdin().lock();
            mcp::serve(&home, &own_program()?, stdin, std::io::stdout())?;
        }
        Some(McpAction::Setup) => {
            let setup = mcp::client_setup(&home, &own_program()?);
            if !Settings::load()?.module_enabled("mcp") {
                println!("First switch the MCP server on: stackyard modules on mcp\n");
            }
            println!("Claude Code:\n  {}\n", setup.claude_code);
            println!("Cursor (~/.cursor/mcp.json), Claude Desktop, Windsurf and others:");
            println!("{}\n", setup.json);
            println!(
                "AI tools can read sites, services, runtimes and logs. To let them make changes,"
            );
            println!("allow actions with `stackyard mcp allow <tool>`; see `stackyard mcp tools`.");
        }
        Some(McpAction::Tools) => {
            let settings = Settings::load()?;
            for tool in mcp::TOOLS {
                let state = match (tool.action, settings.mcp.allow.contains(tool.name)) {
                    (false, _) => "always",
                    (true, true) => "allowed",
                    (true, false) => "off",
                };
                println!("{:<17} {:<8} {}", tool.name, state, tool.title);
            }
            if !settings.module_enabled("mcp") {
                println!("\nThe MCP server is off; switch it on with `stackyard modules on mcp`.");
            }
        }
        Some(McpAction::Allow { tool }) => allow_mcp_tool(&tool, true)?,
        Some(McpAction::Deny { tool }) => allow_mcp_tool(&tool, false)?,
        Some(McpAction::Log { lines }) => {
            let entries = mcp::activity::recent(&home, lines);
            if entries.is_empty() {
                println!("No AI tool has used Stackyard yet.");
            }
            for entry in entries.iter().rev() {
                let result = match &entry.error {
                    None => "ok".to_string(),
                    Some(e) => format!("failed: {e}"),
                };
                println!(
                    "{}  {:<12} {:<17} {}  {result}",
                    utc_time(entry.at),
                    entry.client,
                    entry.tool,
                    entry.summary
                );
            }
        }
    }
    Ok(())
}

fn allow_mcp_tool(tool: &str, allowed: bool) -> stackyard_core::Result<()> {
    let mut settings = Settings::load()?;
    mcp::set_allowed(&mut settings, tool, allowed)?;
    settings.save()?;
    let what = if tool == "all" { "Every action" } else { tool };
    let state = if allowed { "allowed" } else { "off" };
    println!("{what} is now {state} for AI tools.");
    Ok(())
}

/// `2026-10-14 22:05:09 UTC` from milliseconds since 1970.
fn tasks(action: Option<TasksAction>) -> stackyard_core::Result<()> {
    let home = paths::home();
    let settings = Settings::load()?;
    if !matches!(action, Some(TasksAction::Check { .. })) {
        settings.require_module("scheduler")?;
    }
    match action {
        None => {
            let views = scheduler::views(&home)?;
            if views.is_empty() {
                println!("No tasks yet. Add one with `stackyard tasks add <name> --run <command> --every <schedule>`.");
                return Ok(());
            }
            for view in views {
                let task = &view.task;
                let when = view.description.unwrap_or_else(|| task.schedule.clone());
                let state = match (
                    &view.problem,
                    task.enabled,
                    view.running_since,
                    view.next_run,
                ) {
                    (Some(problem), _, _, _) => problem.clone(),
                    (_, _, Some(_), _) => "running now".into(),
                    (_, false, _, _) => "switched off".into(),
                    (_, _, _, Some(next)) => format!("next {}", local_time(next)),
                    _ => "never due".into(),
                };
                println!("{}  {}  ({when}; {state})", task.id, task.name);
                println!("          {}  in {}", task.command, task.folder.display());
                if let Some(last) = view.last {
                    let how = match (last.outcome, last.exit_code) {
                        (Outcome::Succeeded, _) => "succeeded".to_string(),
                        (Outcome::Failed, Some(code)) => format!("failed with exit code {code}"),
                        (Outcome::Failed, None) => "failed".to_string(),
                        (Outcome::Stopped, _) => "was stopped".to_string(),
                        (Outcome::CouldNotStart, _) => {
                            format!("could not start: {}", last.message.unwrap_or_default())
                        }
                    };
                    println!("          last run {} {how}", local_time(last.started));
                }
            }
            println!("\nTasks run while Stackyard runs: the app, or `stackyard start` in the background.");
            println!("Each task's output: stackyard logs task-<id>");
        }
        Some(TasksAction::Add {
            name,
            run,
            every,
            folder,
        }) => {
            let folder = match folder {
                Some(f) if f.is_absolute() => f,
                Some(f) => std::env::current_dir()
                    .map(|d| d.join(f))
                    .unwrap_or_default(),
                None => std::env::current_dir().unwrap_or_default(),
            };
            let task = scheduler::add(
                &home,
                TaskInput {
                    name,
                    command: run,
                    folder,
                    schedule: every,
                    enabled: true,
                },
            )?;
            let when = scheduler::cron::describe(&task.schedule).unwrap_or(task.schedule.clone());
            println!("Added {} ({}): {when}.", task.name, task.id);
            if let Some(next) = scheduler::preview(&task.schedule, 1)?.first() {
                println!("First run {}.", local_time(*next));
            }
            if control::find(&home).is_none() {
                println!("Tasks run while Stackyard runs: open the app, or `stackyard start`.");
            }
        }
        Some(TasksAction::Run { task }) => {
            let task = scheduler::find(&home, &task)?;
            eprintln!("Running {} in {}", task.name, task.folder.display());
            let status = scheduler::command(&home, &task)
                .status()
                .map_err(|source| Error::Io {
                    path: task.folder.clone(),
                    source,
                })?;
            if !status.success() {
                return Err(Error::Task(match status.code() {
                    Some(code) => format!("{} failed with exit code {code}", task.name),
                    None => format!("{} was stopped", task.name),
                }));
            }
        }
        Some(TasksAction::On { task }) => {
            let task = scheduler::find(&home, &task)?;
            scheduler::set_enabled(&home, &task.id, true)?;
            println!("{} is on.", task.name);
        }
        Some(TasksAction::Off { task }) => {
            let task = scheduler::find(&home, &task)?;
            scheduler::set_enabled(&home, &task.id, false)?;
            println!("{} is off.", task.name);
        }
        Some(TasksAction::Remove { task }) => {
            let task = scheduler::find(&home, &task)?;
            scheduler::remove(&home, &task.id)?;
            println!("Removed {}.", task.name);
        }
        Some(TasksAction::Check { schedule }) => {
            let runs = scheduler::preview(&schedule, 5)?;
            if let Some(text) = scheduler::cron::describe(&schedule) {
                println!("{text}.");
            }
            if runs.is_empty() {
                println!("It never runs: no such date comes up in the next four years.");
            }
            for run in runs {
                println!("  {}", local_time(run));
            }
        }
    }
    Ok(())
}

/// e.g. `Wed 14 Oct 22:05`, in this computer's time zone.
fn local_time(ms: u64) -> String {
    const DAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let m = clock::local((ms / 1000) as i64);
    let year = if m.year == clock::local(clock::now()).year {
        String::new()
    } else {
        format!(" {}", m.year)
    };
    format!(
        "{} {} {}{year} {:02}:{:02}",
        DAYS[m.weekday as usize % 7],
        m.day,
        MONTHS[(m.month as usize + 11) % 12],
        m.hour,
        m.minute
    )
}

fn utc_time(ms: u64) -> String {
    let secs = ms / 1000;
    let (days, rest) = ((secs / 86_400) as i64, secs % 86_400);
    // Days since 1970 to a calendar date (proleptic Gregorian).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year}-{month:02}-{day:02} {:02}:{:02}:{:02} UTC",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

fn runtime_action(action: RuntimeAction) -> stackyard_core::Result<()> {
    let manager = RuntimeManager::open();
    let mut settings = Settings::load()?;
    match action {
        RuntimeAction::Install(args) => {
            let kind = parse_kind(&args.kind)?;
            settings.require_module(kind.id())?;
            let name = catalog::runtime(kind).name;
            let installed = manager.install(
                kind,
                &args.version,
                &mut progress_line(&format!("{name} {}", args.version)),
            );
            eprintln!();
            let installed = installed?;
            manager.fix_default(&mut settings, kind);
            settings.save()?;
            println!(
                "Installed {name} {} ({}) in {}",
                installed.version,
                installed.release,
                installed.path.display()
            );
        }
        RuntimeAction::Uninstall(args) => {
            let kind = parse_kind(&args.kind)?;
            manager.uninstall(kind, &args.version)?;
            if manager.fix_default(&mut settings, kind) {
                settings.save()?;
            }
            println!("Removed {} {}", catalog::runtime(kind).name, args.version);
        }
        RuntimeAction::Default(args) => {
            let kind = parse_kind(&args.kind)?;
            manager.set_default(&mut settings, kind, &args.version)?;
            settings.save()?;
            println!(
                "{} {} is now the default",
                catalog::runtime(kind).name,
                args.version
            );
        }
    }
    Ok(())
}

/// Prints install progress on one updating line.
fn progress_line(what: &str) -> impl FnMut(Step) + '_ {
    move |step| {
        let line = match step {
            Step::Resolving => format!("Looking up {what}..."),
            Step::Downloading {
                done,
                total: Some(total),
            } if total > 0 => format!("Downloading {what}... {}%", done * 100 / total),
            Step::Downloading { done, .. } => {
                format!("Downloading {what}... {} MB", done / 1_000_000)
            }
            Step::Unpacking => format!("Unpacking {what}..."),
        };
        eprint!("\r\x1b[2K{line}");
        let _ = std::io::stderr().flush();
    }
}

/// Runs the web server, the local DNS server and any named services in
/// the foreground; Ctrl+C (or a termination signal) stops them all.
fn up(services: &[String]) -> stackyard_core::Result<()> {
    let home = paths::home();
    if let Some(client) = control::find(&home) {
        return Err(Error::Control(format!(
            "Stackyard is already running services ({}). Use `stackyard start`, `stackyard stop` and `stackyard status` instead.",
            owner_name(client.owner)
        )));
    }
    let settings = Settings::load()?;
    settings.require_module("caddy")?;
    for id in services {
        settings.require_module(id)?;
    }
    let engine = Arc::new(Engine::new(&home));
    let (quit, quit_rx) = std::sync::mpsc::channel();
    let _server = serve_control(&home, Owner::Up, &engine, quit.clone())?;
    ctrlc::set_handler(move || {
        let _ = quit.send(());
    })
    .map_err(|e| Error::SystemSetup(format!("could not watch for Ctrl+C: {e}")))?;

    let started = (|| {
        engine.start("caddy")?;
        for id in services {
            engine.start(id)?;
        }
        Ok::<_, Error>(())
    })();
    if let Err(e) = started {
        engine.stop_all();
        return Err(e);
    }
    let manager = engine.manager();
    let ports = Ports::current(manager.home());
    println!("Web server on ports {} and {}", ports.http, ports.https);
    print_sites(manager)?;
    let status = manager
        .system_wanted()?
        .status(&system::applied(manager.home()));
    if !status.needed.is_empty() {
        println!(
            "Local domains need `stackyard system setup` first; .localhost addresses work now."
        );
    }
    println!("Logs: {}", paths::logs_dir().display());
    println!("Press Ctrl+C to stop.");
    let _ = quit_rx.recv();
    eprintln!("Stopping...");
    engine.shutdown();
    Ok(())
}

/// `stackyard daemon`: runs services in the background for `stackyard
/// start`, until `stackyard stop` or the app takes over.
fn daemon() -> stackyard_core::Result<()> {
    let home = paths::home();
    if control::find(&home).is_some() {
        return Err(Error::Control(
            "Stackyard is already running services".into(),
        ));
    }
    let engine = Arc::new(Engine::new(&home));
    Engine::watch_parked(&engine, &home, || {});
    let (quit, quit_rx) = std::sync::mpsc::channel();
    let server = serve_control(&home, Owner::Daemon, &engine, quit.clone())?;
    let _ = ctrlc::set_handler(move || {
        let _ = quit.send(());
    });
    let _ = quit_rx.recv();
    // Let the answer to `stop` or `hand-over` reach the caller.
    std::thread::sleep(Duration::from_millis(300));
    // Stop everything before letting go of the control channel, so a
    // `stackyard start` right after starts afresh rather than reaching
    // this process on its way out.
    engine.shutdown();
    drop(server);
    Ok(())
}

/// Answers other processes for `engine`; `quit` fires when asked to stop
/// everything or to hand over to the app.
fn serve_control(
    home: &Path,
    owner: Owner,
    engine: &Arc<Engine>,
    quit: Sender<()>,
) -> stackyard_core::Result<ControlServer> {
    let engine = engine.clone();
    let quit = Mutex::new(quit);
    let stopping = std::sync::atomic::AtomicBool::new(false);
    ControlServer::start(home, owner, move |request| {
        use std::sync::atomic::Ordering;
        if stopping.load(Ordering::SeqCst) {
            return Response::Failed {
                message: "Stackyard is stopping; try again in a moment".into(),
            };
        }
        let ends = matches!(request, Request::StopAll | Request::HandOver);
        let response = engine.handle(request);
        if ends {
            stopping.store(true, Ordering::SeqCst);
            let _ = quit.lock().unwrap_or_else(|e| e.into_inner()).send(());
        }
        response
    })
}

/// The process running services, starting `stackyard daemon` if there is none.
fn running_stackyard() -> stackyard_core::Result<control::Client> {
    control::start_background(&paths::home(), &own_program()?)
}

/// Tells the process running services that something changed, if one runs.
fn notify(request: Request) {
    if let Some(client) = control::find(&paths::home()) {
        if let Err(e) = client.send(request) {
            eprintln!("warning: the running Stackyard did not pick up the change: {e}");
        }
    }
}

fn owner_name(owner: Owner) -> &'static str {
    match owner {
        Owner::App => "in the Stackyard app",
        Owner::Daemon => "in the background",
        Owner::Up => "in `stackyard up`",
    }
}

fn print_status(client: &control::Client, response: Response) {
    let Response::Services { services } = response else {
        return;
    };
    println!(
        "Stackyard is running {} (pid {}).",
        owner_name(client.owner),
        client.pid
    );
    let width = services.iter().map(|s| s.name.len()).max().unwrap_or(0);
    for service in &services {
        let state = match service.state {
            ProcessState::Stopped => "stopped".to_string(),
            ProcessState::Running { pid } => format!("running (pid {pid})"),
            ProcessState::Restarting { attempt } => format!("restarting (attempt {attempt})"),
            ProcessState::Crashed { code: Some(code) } => {
                format!(
                    "crashed (exit code {code}); see `stackyard logs {}`",
                    service.id
                )
            }
            ProcessState::Crashed { code: None } => {
                format!("crashed; see `stackyard logs {}`", service.id)
            }
        };
        println!("  {:<width$} {state}", service.name);
    }
    let web = services
        .iter()
        .any(|s| s.id == "caddy" && matches!(s.state, ProcessState::Running { .. }));
    if web {
        let _ = print_sites(&ServiceManager::open());
    }
}

fn print_sites(manager: &ServiceManager) -> stackyard_core::Result<()> {
    let ports = Ports::current(manager.home());
    for served in manager.sites()? {
        let site = &served.site;
        println!("  {}", ports.url(&site.domain(), site.https));
    }
    Ok(())
}

/// Runs the real program behind a shim such as `php` and passes on its exit code.
fn run_shim(tool: Tool, args: Vec<OsString>) -> ExitCode {
    let result = (|| {
        let home = paths::home();
        if tool == Tool::Composer && !shell::composer_phar(&home).exists() {
            shell::install_composer(&home, &mut progress_line("Composer"))?;
            eprintln!();
        }
        let launch = shell::Context::load(&home)?.launch(tool, &current_dir()?, args)?;
        if std::env::var_os("STACKYARD_DEBUG").is_some() {
            eprintln!(
                "stackyard: {} → {}",
                launch.summary,
                launch.program.display()
            );
        }
        let mut command = Process::new(&launch.program);
        command.args(&launch.args).env("PATH", launch.path_var());
        command.envs(launch.env.iter().map(|(k, v)| (k, v)));
        run_in_place(command, &launch.program)
    })();
    match result {
        Ok(code) => code,
        Err(e) => {
            eprintln!("stackyard ({}): {e}", tool.name());
            ExitCode::FAILURE
        }
    }
}

/// Runs an AI coding tool installed on the AI tools screen.
fn run_ai_tool(tool: &ai::AiTool, args: Vec<OsString>) -> ExitCode {
    let result = (|| {
        let launch = ai::launch(&paths::home(), tool, args)?;
        let mut command = Process::new(&launch.program);
        command.args(&launch.args).env("PATH", launch.path_var());
        run_in_place(command, &launch.program)
    })();
    match result {
        Ok(code) => code,
        Err(e) => {
            eprintln!("stackyard ({}): {e}", tool.command);
            ExitCode::FAILURE
        }
    }
}

fn ai_tools(action: Option<AiAction>) -> stackyard_core::Result<()> {
    let home = paths::home();
    match action.unwrap_or(AiAction::List) {
        AiAction::List => {
            Settings::load()?.require_module("ai")?;
            let mut tools = ai::list(&home);
            ai::check_updates(&mut tools);
            for status in &tools {
                let state = match (&status.installed, &status.latest) {
                    (Some(have), Some(newest)) if status.update => {
                        format!(
                            "{have}, update to {newest} with `stackyard ai update {}`",
                            status.tool.id
                        )
                    }
                    (Some(have), _) => have.clone(),
                    (None, _) => "not installed".to_string(),
                };
                let command = format!("`{}`", status.tool.command);
                println!(
                    "{:<9} {:<20} {command:<11} {state}",
                    status.tool.id, status.tool.name
                );
            }
        }
        AiAction::Install { id } | AiAction::Update { id } => {
            let tool = ai::find(&id)?;
            let status = ai::install(&home, tool, &mut |line| {
                eprint!("\r\x1b[2K{line}");
                let _ = std::io::stderr().flush();
            })?;
            eprintln!();
            println!(
                "{} {} is installed. Run `{}` in a terminal, or `stackyard ai open {}` in a site's folder.",
                tool.name,
                status.installed.unwrap_or_default(),
                tool.command,
                tool.id
            );
        }
        AiAction::Remove { id } => {
            let tool = ai::find(&id)?;
            ai::uninstall(&home, tool)?;
            println!("Removed {}", tool.name);
        }
        AiAction::Open { id, place } => {
            let tool = ai::find(&id)?;
            let folder = match place {
                None => current_dir()?,
                Some(place) => {
                    let store = SiteStore::load()?;
                    let site = store
                        .list()
                        .iter()
                        .find(|s| s.domain() == place || s.name == place);
                    match site {
                        Some(site) => site.path.clone(),
                        None => current_dir()?.join(paths::expand_home(Path::new(&place))),
                    }
                }
            };
            ai::open(&home, tool, &folder)?;
            println!("Opened {} in {}", tool.name, folder.display());
        }
    }
    Ok(())
}

/// Replaces this process with `command` (macOS, Linux), or runs it and exits
/// with its code (Windows), so the tool behaves as if started directly.
fn run_in_place(mut command: Process, program: &Path) -> stackyard_core::Result<ExitCode> {
    let spawn_error = |source| Error::Spawn {
        program: program.display().to_string(),
        source,
    };
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        Err(spawn_error(command.exec()))
    }
    #[cfg(not(unix))]
    {
        // Ctrl+C reaches the tool too; let it decide, and wait for it.
        let _ = ctrlc::set_handler(|| {});
        let status = command.status().map_err(spawn_error)?;
        std::process::exit(status.code().unwrap_or(1));
    }
}

/// Starts the user's shell with the shims first on its PATH.
fn open_shell() -> stackyard_core::Result<()> {
    Settings::load()?.require_module("shell")?;
    let home = paths::home();
    if !shims::installed(&home) {
        shims::install(&home, &own_program()?)?;
    }
    let bin = shims::bin_dir(&home);
    let program = if cfg!(windows) {
        std::env::var_os("STACKYARD_SHELL_PROGRAM").unwrap_or_else(|| "powershell.exe".into())
    } else {
        std::env::var_os("SHELL").unwrap_or_else(|| "/bin/sh".into())
    };
    let rest = std::env::var_os("PATH").unwrap_or_default();
    let path =
        std::env::join_paths(std::iter::once(bin.clone()).chain(std::env::split_paths(&rest)))
            .map_err(|e| Error::Shell(format!("could not build PATH: {e}")))?;
    println!(
        "Stackyard Shell: php, composer, node, npm and mysql use each project's versions here."
    );
    println!("Run `stackyard which` to see them. Type `exit` to leave.");
    let mut command = Process::new(&program);
    command.env("PATH", path).env("STACKYARD_SHELL", "1");
    let _ = ctrlc::set_handler(|| {});
    let status = command.status().map_err(|source| Error::Spawn {
        program: PathBuf::from(&program).display().to_string(),
        source,
    })?;
    std::process::exit(status.code().unwrap_or(0));
}

fn turn_shims_off() -> stackyard_core::Result<()> {
    let home = paths::home();
    user_path::remove(&shims::bin_dir(&home))?;
    shims::remove(&home)
}

/// This program, which every shim leads back to.
fn own_program() -> stackyard_core::Result<PathBuf> {
    std::env::current_exe().map_err(|source| Error::Io {
        path: PathBuf::from("stackyard"),
        source,
    })
}

fn current_dir() -> stackyard_core::Result<PathBuf> {
    std::env::current_dir().map_err(|source| Error::Io {
        path: PathBuf::from("."),
        source,
    })
}

fn parse_kind(id: &str) -> stackyard_core::Result<RuntimeKind> {
    RuntimeKind::from_id(&id.to_ascii_lowercase())
        .ok_or_else(|| stackyard_core::Error::UnknownModule(id.to_string()))
}

/// `1536` → `1.5 KB`, in steps of 1024 like file managers on Windows and Linux.
fn optimize_images(args: ImagesArgs) -> stackyard_core::Result<()> {
    Settings::load()?.require_module(images::MODULE)?;
    let mut formats = Vec::new();
    if args.webp {
        formats.push(images::Format::Webp);
    }
    if args.avif {
        formats.push(images::Format::Avif);
    }
    if args.min {
        formats.push(images::Format::Same);
    }
    if formats.is_empty() {
        formats = vec![images::Format::Webp, images::Format::Avif];
    }
    let here = current_dir()?;
    let options = images::Options {
        formats,
        quality: args.quality,
        max_width: args.max_width,
        height: args.height,
        out_dir: args.out.map(|o| here.join(o)),
        replace: args.replace,
    };
    images::check(&options)?;
    let paths: Vec<PathBuf> = if args.paths.is_empty() {
        vec![here.clone()]
    } else {
        args.paths.iter().map(|p| here.join(p)).collect()
    };
    let sources = images::find(&paths);
    if sources.is_empty() {
        println!("No JPG or PNG images there.");
        return Ok(());
    }
    let total = sources.len();
    let (mut before, mut after, mut failed) = (0u64, 0u64, 0usize);
    images::optimize_all(&sources, &options, &mut |done, result| {
        let shown = result
            .source
            .strip_prefix(&here)
            .unwrap_or(&result.source)
            .display()
            .to_string();
        let mut parts = Vec::new();
        for made in &result.made {
            let label = match made.format {
                images::Format::Webp => "WebP",
                images::Format::Avif => "AVIF",
                images::Format::Same => "smaller",
            };
            match &made.skipped {
                Some(why) => parts.push(format!("{label}: {why}")),
                None => {
                    before += result.size;
                    after += made.size;
                    parts.push(format!("{label} {}", size(made.size)));
                }
            }
        }
        if let Some(error) = &result.error {
            failed += 1;
            parts.push(error.clone());
        }
        println!(
            "[{done}/{total}] {shown} ({}): {}",
            size(result.size),
            parts.join(", ")
        );
    });
    if let Some(ratio) = (after * 100).checked_div(before) {
        let saved = 100 - ratio as i64;
        if saved >= 0 {
            println!("Done: the new files are {saved}% smaller than their originals.");
        } else {
            println!(
                "Done: the new files are {}% larger than their originals.",
                -saved
            );
        }
    }
    if failed > 0 {
        return Err(Error::Images(format!("{failed} of {total} images failed")));
    }
    Ok(())
}

/// A saved or parked site by its name or domain.
fn find_site(name: &str) -> stackyard_core::Result<Site> {
    ServiceManager::open()
        .user_sites()?
        .into_iter()
        .map(|s| s.site)
        .find(|s| s.domain() == name || s.name == name)
        .ok_or_else(|| Error::SiteNotFound(name.to_string()))
}

fn site_pack_action(action: SitesCommand) -> stackyard_core::Result<()> {
    let home = paths::home();
    let settings = Settings::load()?;
    let report = |packed: &sitepack::Packed| {
        let database = packed
            .database
            .as_deref()
            .map(|d| format!(" and the database {d}"))
            .unwrap_or_default();
        println!(
            "Saved {} ({}, {} files{database})",
            packed.path.display(),
            size(packed.size),
            packed.files
        );
        if let Some(warning) = &packed.warning {
            println!("Note: {warning}");
        }
    };
    match action {
        SitesCommand::Backup { site } => {
            settings.require_module("sitebackups")?;
            report(&sitepack::backup(&home, &find_site(&site)?)?);
        }
        SitesCommand::Backups { site } => {
            let site = find_site(&site)?;
            let backups = sitepack::backups(&home, &site.domain());
            if backups.is_empty() {
                println!(
                    "No backups of {}. Make one with `stackyard sites backup {}`.",
                    site.domain(),
                    site.name
                );
            }
            for backup in backups {
                let database = backup
                    .database
                    .map(|d| format!(", database {d}"))
                    .unwrap_or_default();
                println!("{}  {}{database}", backup.file, size(backup.size));
            }
        }
        SitesCommand::Restore { site, file } => {
            settings.require_module("sitebackups")?;
            let site = find_site(&site)?;
            let path = sitepack::backup_path(&home, &site.domain(), &file)?;
            let safety = sitepack::restore(&home, &site, &path)?;
            notify(Request::SitesChanged);
            println!(
                "Restored {} from {file}. How it was before is in {}",
                site.domain(),
                safety.path.display()
            );
        }
        SitesCommand::Export { site, file } => {
            settings.require_module("sitetransfer")?;
            let mut file = file;
            if file.extension().is_none() {
                file.set_extension("zip");
            }
            report(&sitepack::pack(&home, &find_site(&site)?, &file)?);
        }
        SitesCommand::Import { file, name, path } => {
            settings.require_module("sitetransfer")?;
            let (site, warning) =
                sitepack::import(&home, &file, sitepack::ImportOptions { name, path })?;
            notify(Request::SitesChanged);
            println!("Added {} in {}", site.url(), site.path.display());
            if let Some(warning) = warning {
                println!("Note: {warning}");
            }
        }
        _ => unreachable!("only site file commands come here"),
    }
    Ok(())
}

fn size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64 / 1024.0;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if value < 10.0 {
        format!("{value:.1} {}", UNITS[unit])
    } else {
        format!("{value:.0} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn formats_utc_times() {
        assert_eq!(super::utc_time(0), "1970-01-01 00:00:00 UTC");
        assert_eq!(super::utc_time(951_782_400_000), "2000-02-29 00:00:00 UTC");
        assert_eq!(
            super::utc_time(1_792_015_509_000),
            "2026-10-14 22:05:09 UTC"
        );
    }
}
