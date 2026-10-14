//! `stackyard` command-line tool. Every command goes through `stackyard-core`,
//! so the CLI, the desktop app and the MCP server always agree.

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};
use stackyard_core::catalog::{self, RuntimeKind, RUNTIMES, SERVICES};
use stackyard_core::domain;
use stackyard_core::modules::MODULES;
use stackyard_core::runtimes::{RuntimeManager, Step};
use stackyard_core::services::ServiceManager;
use stackyard_core::settings::Settings;
use stackyard_core::sites::{Site, SiteRuntime, SiteStore};

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
    /// Port the Node dev server listens on
    #[arg(long, default_value_t = 3000, requires = "node")]
    port: u16,
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
            let store = SiteStore::load()?;
            if store.list().is_empty() {
                println!("No sites yet. Add one with `stackyard sites add <name>`.");
            }
            for site in store.list() {
                println!("{:<32} {}", site.url(), site.path.display());
            }
        }
        Command::Sites(SitesCommand::Add(args)) => {
            let settings = Settings::load()?;
            let path = match args.path {
                Some(path) => path,
                None => std::env::current_dir().map_err(|source| stackyard_core::Error::Io {
                    path: PathBuf::from("."),
                    source,
                })?,
            };
            let runtime = match (args.php, args.node) {
                (Some(version), _) => SiteRuntime::Php { version },
                (None, Some(version)) => SiteRuntime::Node {
                    version,
                    port: args.port,
                },
                (None, None) => SiteRuntime::Static,
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

fn parse_kind(id: &str) -> stackyard_core::Result<RuntimeKind> {
    RuntimeKind::from_id(&id.to_ascii_lowercase())
        .ok_or_else(|| stackyard_core::Error::UnknownModule(id.to_string()))
}
