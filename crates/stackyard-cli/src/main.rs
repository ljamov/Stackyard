//! `stackyard` command-line tool. Every command goes through `stackyard-core`,
//! so the CLI, the desktop app and the MCP server always agree.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};
use stackyard_core::catalog::{RUNTIMES, SERVICES};
use stackyard_core::domain;
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
    /// List runtimes Stackyard can install
    Runtimes,
    /// List services Stackyard can run
    Services,
    /// Show or change the domain ending for new sites (default .stack)
    Tld {
        /// New ending, e.g. `.stack` or `.test`
        value: Option<String>,
    },
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
        Command::Runtimes => {
            for family in RUNTIMES {
                let note = if family.available {
                    ""
                } else {
                    "  (coming soon)"
                };
                println!("{:<8} {}{note}", family.name, family.versions.join(", "));
            }
        }
        Command::Services => {
            for service in SERVICES {
                let ports: Vec<String> = service.ports.iter().map(u16::to_string).collect();
                println!(
                    "{:<8} {:<6} {:<13} port {}",
                    service.name,
                    service.version,
                    service.role,
                    ports.join(", ")
                );
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
