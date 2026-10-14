//! `stackyard` command-line tool. Every command goes through `stackyard-core`,
//! so the CLI, the desktop app and the MCP server always agree.

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};
use stackyard_core::catalog::{self, RuntimeKind, RUNTIMES, SERVICES};
use stackyard_core::detect::{self, Needs};
use stackyard_core::dns::{self, DnsServer};
use stackyard_core::domain;
use stackyard_core::modules::MODULES;
use stackyard_core::runtimes::{RuntimeManager, Step};
use stackyard_core::services::ServiceManager;
use stackyard_core::settings::Settings;
use stackyard_core::sites::{Site, SiteRuntime, SiteStore};
use stackyard_core::supervisor::Supervisor;
use stackyard_core::system::{self, Elevate, Resolver};
use stackyard_core::web::{self, Backend, Ports};
use stackyard_core::{paths, platform, Error};

#[derive(Parser)]
#[command(name = "stackyard", version, about = "Local development stack manager")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List, add or remove local sites
    #[command(subcommand)]
    Sites(SitesCommand),
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
    /// Run the web server (and any services named) until Ctrl+C
    Up {
        /// Other services to run too, e.g. `mysql mailpit`
        services: Vec<String>,
    },
    /// Show or run the one-time setup for local domains and HTTPS
    System {
        #[command(subcommand)]
        action: Option<SystemAction>,
    },
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
    /// Remove a site by name or domain
    Remove { name: String },
    /// Make every project folder inside a folder a site (default: this folder)
    Park { folder: Option<PathBuf> },
    /// Stop making sites from a parked folder
    Unpark { folder: Option<PathBuf> },
    /// List parked folders
    Parked,
}

#[derive(Args)]
struct AddSite {
    /// Site name, e.g. `myapp` for myapp.stack
    name: String,
    /// Project folder (defaults to the current folder)
    #[arg(long)]
    path: Option<PathBuf>,
    /// Serve with this PHP version
    #[arg(long, conflicts_with = "node")]
    php: Option<String>,
    /// Proxy to a Node dev server with this Node version
    #[arg(long)]
    node: Option<String>,
    /// Port the Node dev server listens on (default: detected, else 3000)
    #[arg(long, requires = "node")]
    port: Option<u16>,
    /// Serve the files as they are, even if the folder looks like PHP or Node
    #[arg(long = "static", conflicts_with_all = ["php", "node"])]
    plain: bool,
    /// Serve over plain HTTP only
    #[arg(long)]
    no_https: bool,
}

fn main() -> ExitCode {
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
        Command::Sites(SitesCommand::List) => {
            let manager = ServiceManager::open();
            let ports = Ports::current(manager.home());
            let sites = manager.sites()?;
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
        Command::Sites(SitesCommand::Add(args)) => {
            let settings = Settings::load()?;
            let path = match args.path {
                Some(path) => paths::expand_home(&path),
                None => current_dir()?,
            };
            let runtime = match (args.php, args.node) {
                (Some(version), _) => SiteRuntime::Php { version },
                (None, Some(version)) => SiteRuntime::Node {
                    version,
                    port: args.port.unwrap_or(match detect::detect(&path).needs {
                        Needs::Node { port } => port,
                        _ => 3000,
                    }),
                },
                _ if args.plain => SiteRuntime::Static,
                _ => web::runtime_for(&path, &settings, &RuntimeManager::open()),
            };
            let mut store = SiteStore::load()?;
            let url = store
                .add(Site {
                    name: args.name,
                    tld: settings.tld,
                    path,
                    runtime,
                    https: !args.no_https,
                })?
                .url();
            store.save()?;
            println!("Added {url}");
        }
        Command::Sites(SitesCommand::Park { folder }) => {
            let folder = match folder {
                Some(folder) => folder,
                None => current_dir()?,
            };
            let mut settings = Settings::load()?;
            let folder = settings.park(&folder)?;
            settings.save()?;
            println!(
                "Parked {}: each project folder in it is now a site ending in {}",
                folder.display(),
                settings.tld
            );
        }
        Command::Sites(SitesCommand::Unpark { folder }) => {
            let folder = match folder {
                Some(folder) => folder,
                None => current_dir()?,
            };
            let mut settings = Settings::load()?;
            settings.unpark(&folder)?;
            settings.save()?;
            println!("Unparked {}", paths::expand_home(&folder).display());
        }
        Command::Sites(SitesCommand::Parked) => {
            let settings = Settings::load()?;
            if settings.parked.is_empty() {
                println!("No parked folders. Park one with `stackyard sites park <folder>`.");
            }
            for folder in &settings.parked {
                println!("{}", folder.display());
            }
        }
        Command::Sites(SitesCommand::Remove { name }) => {
            let mut store = SiteStore::load()?;
            let site = store.remove(&name)?;
            store.save()?;
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
                    .versions
                    .iter()
                    .map(|v| match manager.get(family.kind, v) {
                        Some(i) if default == Some(*v) => format!("{v} [{}, default]", i.release),
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
            for service in SERVICES {
                let ports: Vec<String> = service.ports.iter().map(u16::to_string).collect();
                let state = match manager.get(service.id) {
                    _ if !settings.module_enabled(service.id) => "off".to_string(),
                    Some(i) => format!("installed {}", i.release),
                    None => "not installed".to_string(),
                };
                println!(
                    "{:<8} {:<5} {:<13} port {:<11} {state}",
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
            println!("{id} is now {}", if on { "on" } else { "off" });
        }
        Command::Up { services } => up(&services)?,
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
        Command::Tld { value: None } => println!("{}", Settings::load()?.tld),
        Command::Tld { value: Some(value) } => {
            let mut settings = Settings::load()?;
            settings.set_tld(&value)?;
            settings.save()?;
            println!("New sites will end in {}", settings.tld);
            if let Some(warning) = domain::tld_warning(&settings.tld) {
                eprintln!("warning: {warning}");
            }
        }
    }
    Ok(())
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
    let settings = Settings::load()?;
    settings.require_module("caddy")?;
    for id in services {
        settings.require_module(id)?;
    }
    let manager = ServiceManager::open();
    let supervisor = Supervisor::new(paths::logs_dir());
    let wanted = manager.system_wanted()?;
    let dns = match wanted.resolver {
        Resolver::HostsFile => None,
        _ => match DnsServer::start(dns::PORT, &manager.endings()?) {
            Ok(server) => Some(server),
            Err(e) => {
                eprintln!("warning: local domains are unavailable: {e}");
                None
            }
        },
    };
    let (stop_tx, stop_rx) = std::sync::mpsc::channel();
    ctrlc::set_handler(move || {
        let _ = stop_tx.send(());
    })
    .map_err(|e| Error::SystemSetup(format!("could not watch for Ctrl+C: {e}")))?;

    let started = (|| {
        manager.start_web(&supervisor)?;
        for id in services {
            supervisor.start(manager.spec(id)?)?;
        }
        Ok::<_, Error>(())
    })();
    if let Err(e) = started {
        supervisor.stop_all();
        return Err(e);
    }
    let ports = Ports::current(manager.home());
    println!("Web server on ports {} and {}", ports.http, ports.https);
    for served in manager.sites()? {
        let site = &served.site;
        println!("  {}", ports.url(&site.domain(), site.https));
    }
    let status = wanted.status(&system::applied(manager.home()));
    if !status.needed.is_empty() {
        println!(
            "Local domains need `stackyard system setup` first; .localhost addresses work now."
        );
    }
    println!("Logs: {}", paths::logs_dir().display());
    println!("Press Ctrl+C to stop.");
    let _ = stop_rx.recv();
    eprintln!("Stopping...");
    supervisor.stop_all();
    drop(dns);
    Ok(())
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
