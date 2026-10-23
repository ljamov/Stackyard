//! `stackyard wp`: clone live WordPress sites, make and merge stagings,
//! push to and pull from the live site, and run WP-CLI in a site.

use std::io::{IsTerminal, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use clap::{Args, Subcommand};
use stackyard_core::control::Request;
use stackyard_core::remote::{self, ConnectionInput, Protocol, RemoteStore, Sessions};
use stackyard_core::settings::Settings;
use stackyard_core::sites::{Site, SiteStore};
use stackyard_core::wordpress::cli::{self, Load};
use stackyard_core::wordpress::env::{self, Media};
use stackyard_core::wordpress::live::{self, CloneOptions, Progress, Source};
use stackyard_core::wordpress::merge::{
    self, Change, Choice, FileChange, Level, SettingChange, Switch,
};
use stackyard_core::wordpress::posts::{self, MoveChoice};
use stackyard_core::wordpress::staging::{self, MadeFrom, NewStaging};
use stackyard_core::wordpress::{snapshot, sync, WpSite};
use stackyard_core::{paths, Error, Result};

use crate::{current_dir, ensure_database, notify};

#[derive(Subcommand)]
pub enum WpCommand {
    /// Clone a live WordPress site over SFTP or FTP, or from backup files
    Clone(CloneArgs),
    /// List, make, merge or destroy stagings of a site
    Staging {
        #[command(subcommand)]
        action: StagingCommand,
    },
    /// Show what changed here since the copy was last in step with the live site; --yes sends it
    Push(SyncArgs),
    /// Show what changed on the live site since this copy was last in step; --yes brings it in
    Pull(SyncArgs),
    /// List a copy's snapshots, or put one back with --undo <id>
    Snapshots {
        /// The site, e.g. `shop.stack`; the site of this folder when left out
        site: Option<String>,
        /// Put the copy back as it was before this snapshot's step
        #[arg(long)]
        undo: Option<String>,
    },
    /// Run WP-CLI in this folder's site, e.g. `stackyard wp plugin list`; `--site=shop.stack` picks one
    #[command(external_subcommand)]
    Cli(Vec<String>),
}

#[derive(Args)]
pub struct CloneArgs {
    /// The new site's name, e.g. `shop` for `shop.stack`
    name: String,
    /// The server's address
    #[arg(long, conflicts_with_all = ["saved", "files"])]
    host: Option<String>,
    /// sftp (default), ftps or ftp
    #[arg(long, default_value = "sftp")]
    protocol: String,
    #[arg(long)]
    port: Option<u16>,
    /// The user name the host gives you; the password is read from
    /// STACKYARD_LIVE_PASSWORD, or asked for
    #[arg(long)]
    user: Option<String>,
    /// A private key for SFTP instead of a password
    #[arg(long)]
    key: Option<PathBuf>,
    /// A connection saved for another site, by name or id
    #[arg(long)]
    saved: Option<String>,
    /// Backup files instead of a server: a panel backup or a zip, plus a .sql file
    #[arg(long, num_args = 1..)]
    files: Vec<PathBuf>,
    /// The WordPress folder on the server, when there are several
    #[arg(long)]
    root: Option<String>,
    /// The live site's address, when the server has no shell
    #[arg(long)]
    url: Option<String>,
    /// Leave media on the live site and fetch each file when a page needs it
    #[arg(long)]
    media_from_live: bool,
    /// Serve the copy over plain HTTP
    #[arg(long)]
    no_https: bool,
    /// When a site of this name exists, back it up and replace it with the copy
    #[arg(long)]
    replace: bool,
}

#[derive(Subcommand)]
pub enum StagingCommand {
    /// List a site's stagings
    List {
        /// The main site or one of its stagings; this folder's site when left out
        site: Option<String>,
    },
    /// Make a staging, e.g. `staging new checkout --site shop.stack` for checkout.shop.stack
    New {
        name: String,
        /// The main site; this folder's site when left out
        #[arg(long)]
        site: Option<String>,
        /// Copy from `live`, or from another staging by domain; the main site when left out
        #[arg(long)]
        from: Option<String>,
        /// Copy media as pages ask for it instead of all of it now
        #[arg(long)]
        no_media: bool,
    },
    /// Show what changed on a staging since it was made; --yes merges it
    Merge {
        /// The staging, e.g. `checkout.shop.stack`
        staging: String,
        /// The copy to merge into; the main site when left out
        #[arg(long)]
        into: Option<String>,
        #[command(flatten)]
        how: How,
    },
    /// Move a staging to the trash, where it stays 7 days
    Destroy {
        staging: String,
        /// Confirm
        #[arg(long)]
        yes: bool,
    },
}

#[derive(Args)]
pub struct SyncArgs {
    /// The site, e.g. `shop.stack`; the site of this folder when left out
    site: Option<String>,
    #[command(flatten)]
    how: How,
}

#[derive(Args)]
pub struct How {
    /// code (files and which plugins and theme are on), settings (code and
    /// changed settings) or everything (code and the whole database)
    #[arg(long, default_value = "code")]
    level: String,
    /// Take what both sides changed too, from this side
    #[arg(long)]
    all: bool,
    /// Move only these posts and pages, by id, e.g. `--posts 12,40`
    #[arg(long, value_delimiter = ',')]
    posts: Vec<u64>,
    /// Take their comments too (with --posts)
    #[arg(long)]
    comments: bool,
    /// Do it; without it, only shows what would change
    #[arg(long)]
    yes: bool,
    /// The main site's name, to confirm --level everything
    #[arg(long)]
    confirm: Option<String>,
}

pub fn run(command: WpCommand) -> Result<()> {
    Settings::load()?.require_module("wordpress")?;
    let home = paths::home();
    match command {
        WpCommand::Cli(args) => wp_cli(args),
        WpCommand::Clone(args) => clone(args),
        WpCommand::Staging { action } => staging_command(action),
        WpCommand::Push(args) => sync_command(sync::Way::Push, args),
        WpCommand::Pull(args) => sync_command(sync::Way::Pull, args),
        WpCommand::Snapshots { site, undo } => {
            let site = site_or_here(site)?;
            let domain = site.domain();
            if let Some(id) = undo {
                ensure_database("Putting a snapshot back")?;
                let wp = WpSite::open(&site)?;
                snapshot::restore(&home, &wp, &id)?;
                println!("{domain} is back as it was before that step.");
                return Ok(());
            }
            let list = snapshot::list(&home, &domain);
            if list.is_empty() {
                println!("No snapshots of {domain} yet.");
            }
            for s in list {
                println!("{:<15} {:<12} {}", s.id, ago(s.created_at), s.reason);
            }
            Ok(())
        }
    }
}

/// The site named, or the one whose folder this is.
fn site_or_here(name: Option<String>) -> Result<Site> {
    let store = SiteStore::load()?;
    if let Some(name) = name {
        return store
            .list()
            .iter()
            .find(|s| s.domain() == name || s.name == name)
            .cloned()
            .ok_or_else(|| {
                Error::Shell(format!(
                    "no site called {name}; `stackyard sites` lists them"
                ))
            });
    }
    let here = current_dir()?;
    store
        .list()
        .iter()
        .filter(|s| here.starts_with(&s.path))
        .max_by_key(|s| s.path.as_os_str().len())
        .cloned()
        .ok_or_else(|| {
            Error::Shell("this folder is not in a site; name one, e.g. `shop.stack`".into())
        })
}

fn wp_cli(mut args: Vec<String>) -> Result<()> {
    // --site picks the site; WP-CLI gets everything else.
    let mut site = None;
    let mut i = 0;
    while i < args.len() {
        if let Some(v) = args[i].strip_prefix("--site=") {
            site = Some(v.to_string());
            args.remove(i);
        } else if args[i] == "--site" && i + 1 < args.len() {
            site = Some(args.remove(i + 1));
            args.remove(i);
        } else {
            i += 1;
        }
    }
    let site = site_or_here(site)?;
    let wp = WpSite::open(&site)?;
    let (ok, _) = cli::run_raw(&paths::home(), &wp.root, Load::Everything, &args, true)?;
    if ok {
        Ok(())
    } else {
        Err(Error::WordPress("WP-CLI stopped with an error".into()))
    }
}

/// Prints progress on one line in a terminal, a line per step elsewhere;
/// Ctrl+C asks the step to stop where it safely can.
fn progress() -> impl FnMut(Progress) -> bool {
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    let _ = ctrlc::set_handler(move || flag.store(true, Ordering::SeqCst));
    let tty = std::io::stderr().is_terminal();
    let mut last = String::new();
    move |p: Progress| {
        if tty {
            eprint!("\r\x1b[2K{}", p.message);
            let _ = std::io::stderr().flush();
        } else if p.message != last {
            eprintln!("{}", p.message);
            last = p.message;
        }
        !stop.load(Ordering::SeqCst)
    }
}

fn end_progress() {
    if std::io::stderr().is_terminal() {
        eprint!("\r\x1b[2K");
    }
}

fn ago(at: u64) -> String {
    let secs = env::now().saturating_sub(at);
    match secs {
        s if s < 3600 => format!("{} min ago", s / 60),
        s if s < 86_400 => format!("{} h ago", s / 3600),
        s => format!("{} days ago", s / 86_400),
    }
}

fn clone(args: CloneArgs) -> Result<()> {
    Settings::load()?.require_module("remote")?;
    let home = paths::home();
    let sessions = Sessions::default();
    let source = if !args.files.is_empty() {
        Source::Files(
            args.files
                .iter()
                .map(|f| current_dir().map(|d| d.join(f)))
                .collect::<Result<_>>()?,
        )
    } else if let Some(saved) = &args.saved {
        let store = RemoteStore::load()?;
        let connection = store
            .list()
            .iter()
            .find(|c| &c.id == saved || &c.name == saved)
            .cloned()
            .ok_or_else(|| Error::Remote(format!("no saved connection called {saved}")))?;
        Source::Server(Box::new(connection))
    } else {
        let host = args.host.clone().ok_or_else(|| {
            Error::Shell("say where the live site is: --host, --saved or --files".into())
        })?;
        let protocol = match args.protocol.as_str() {
            "sftp" => Protocol::Sftp,
            "ftps" => Protocol::Ftps,
            "ftp" => Protocol::Ftp,
            other => {
                return Err(Error::Shell(format!(
                    "unknown protocol {other:?}; pick sftp, ftps or ftp"
                )))
            }
        };
        let user = args
            .user
            .clone()
            .ok_or_else(|| Error::Shell("--user is needed with --host".into()))?;
        let password = match std::env::var("STACKYARD_LIVE_PASSWORD") {
            Ok(p) => Some(p),
            Err(_) if args.key.is_some() => None,
            Err(_) => Some(ask_password(&user, &host)?),
        };
        let mut draft = remote::draft(ConnectionInput {
            protocol: Some(protocol),
            host,
            port: args.port,
            user,
            key_file: args.key.clone(),
            ..Default::default()
        })?;
        draft.server_key = sessions.login(&draft, password)?;
        Source::Server(Box::new(draft))
    };
    ensure_database("A clone")?;
    let options = CloneOptions {
        name: args.name.clone(),
        root: args.root.clone(),
        url: args.url.clone(),
        media: if args.media_from_live {
            Media::FromLive
        } else {
            Media::Download
        },
        https: !args.no_https,
        replace: args.replace,
        ..CloneOptions::default()
    };
    let cloned = live::clone(&home, &sessions, &source, &options, &mut progress());
    end_progress();
    let cloned = cloned?;
    notify(Request::SitesChanged);
    println!("Cloned {} into {}", cloned.live, cloned.url);
    for note in cloned.notes {
        println!("  {note}");
    }
    Ok(())
}

/// Reads a password without showing it where the terminal allows.
fn ask_password(user: &str, host: &str) -> Result<String> {
    if !std::io::stdin().is_terminal() {
        return Err(Error::Shell(
            "no password: set STACKYARD_LIVE_PASSWORD, or use --key".into(),
        ));
    }
    eprint!("Password for {user}@{host} (shown as you type; or set STACKYARD_LIVE_PASSWORD): ");
    let _ = std::io::stderr().flush();
    let mut line = String::new();
    std::io::stdin()
        .read_line(&mut line)
        .map_err(|e| Error::Shell(e.to_string()))?;
    Ok(line.trim_end_matches(['\r', '\n']).to_string())
}

fn staging_command(action: StagingCommand) -> Result<()> {
    let home = paths::home();
    match action {
        StagingCommand::List { site } => {
            let site = site_or_here(site)?;
            let main = staging::main_of(&home, &site.domain());
            let list = staging::list(&home, &main)?;
            println!("{main} (main site)");
            if list.is_empty() {
                println!(
                    "  no stagings; make one with `stackyard wp staging new <name> --site {main}`"
                );
            }
            for s in list {
                let from = match s.made_from.as_deref() {
                    Some("live") => " from the live site".to_string(),
                    Some(d) => format!(" from {d}"),
                    None => String::new(),
                };
                println!("  {:<36} made {}{from}", s.domain, ago(s.created_at));
            }
        }
        StagingCommand::New {
            name,
            site,
            from,
            no_media,
        } => {
            let site = site_or_here(site)?;
            let parent = staging::main_of(&home, &site.domain());
            let from = match from.as_deref() {
                None => MadeFrom::Site {
                    domain: parent.clone(),
                },
                Some("live") => MadeFrom::Live,
                Some(d) => MadeFrom::Site { domain: d.into() },
            };
            ensure_database("A staging")?;
            let sessions = Sessions::default();
            let made = staging::create(
                &home,
                &sessions,
                &NewStaging {
                    parent,
                    name,
                    from,
                    copy_media: !no_media,
                },
                &mut progress(),
            );
            end_progress();
            let made = made?;
            notify(Request::SitesChanged);
            println!("Made {}", made.url());
        }
        StagingCommand::Merge { staging, into, how } => {
            let target = into.unwrap_or_else(|| staging::main_of(&home, &staging));
            ensure_database("A merge")?;
            if !how.posts.is_empty() {
                let choice = move_choice(&how);
                let plan = posts::plan(&home, &staging, &target, &choice)?;
                print_plan(&plan, &target);
                if !how.yes {
                    println!("Run again with --yes to move them.");
                    return Ok(());
                }
                let moved = posts::move_posts(&home, &staging, &target, &choice)?;
                println!(
                    "Moved {} items and {} media files to {target}. Undo with `stackyard wp snapshots {target} --undo {}`.",
                    moved.posts, moved.files, moved.snapshot
                );
                return Ok(());
            }
            let list = merge::changes(&home, &staging, &target)?;
            println!("Changes on {staging} since {}:", ago(list.base_at));
            print_changes(&list.files, &list.switches, &list.settings, &target);
            let level = level(&how.level)?;
            if level == Level::Everything {
                if list.target_new_posts > 0 {
                    println!(
                        "  {} posts or pages made on {target} since then would be replaced.",
                        list.target_new_posts
                    );
                }
                if let Some(store) = &list.store {
                    println!(
                        "  {target}'s {} orders are kept, with their customers.",
                        store.orders
                    );
                }
            }
            if !how.yes {
                println!("Run again with --yes to merge into {target}.");
                return Ok(());
            }
            confirm_everything(&home, &target, level, &how)?;
            let merged = merge::merge(
                &home,
                &staging,
                &target,
                &choice_of(level, how.all, &list.files, &list.switches, &list.settings),
            )?;
            println!(
                "Merged into {target}: {} files, {} switches, {} settings. Undo with `stackyard wp snapshots {target} --undo {}`.",
                merged.files, merged.switches, merged.settings, merged.snapshot
            );
            for note in merged.notes {
                println!("  {note}");
            }
        }
        StagingCommand::Destroy { staging, yes } => {
            if !yes {
                return Err(Error::Shell(format!(
                    "add --yes to move {staging} to the trash (it stays there 7 days)"
                )));
            }
            ensure_database("Destroying a staging")?;
            let trashed = staging::destroy(&home, &staging)?;
            notify(Request::SitesChanged);
            println!("{staging} is in the trash (id {}).", trashed.id);
        }
    }
    Ok(())
}

fn sync_command(way: sync::Way, args: SyncArgs) -> Result<()> {
    Settings::load()?.require_module("remote")?;
    let home = paths::home();
    let site = site_or_here(args.site)?;
    let domain = site.domain();
    let how = args.how;
    let push = way == sync::Way::Push;
    ensure_database(if push { "A push" } else { "A pull" })?;
    let sessions = Sessions::default();
    if !how.posts.is_empty() {
        let read = sync::read_live(&home, &sessions, &domain, &mut progress());
        end_progress();
        read?;
        let choice = move_choice(&how);
        let plan = sync::plan_posts(&home, &domain, way, &choice)?;
        let target = if push {
            "the live site"
        } else {
            domain.as_str()
        };
        print_plan(&plan, target);
        if !how.yes {
            println!("Run again with --yes to move them.");
            return Ok(());
        }
        let moved = sync::move_posts(&home, &sessions, &domain, way, &choice, &mut progress());
        end_progress();
        let moved = moved?;
        println!(
            "Moved {} items and {} media files to {target}.",
            moved.posts, moved.files
        );
        println!("{}", undo_hint(&domain, push, &moved.undo));
        return Ok(());
    }
    let list = sync::changes(&home, &sessions, &domain, way, &mut progress());
    end_progress();
    let list = list?;
    let other = if push {
        "the live site"
    } else {
        domain.as_str()
    };
    println!(
        "Changes on {} since they were last in step ({}):",
        if push {
            domain.as_str()
        } else {
            "the live site"
        },
        if list.base_at > 0 {
            ago(list.base_at)
        } else {
            "never".into()
        }
    );
    print_changes(&list.files, &list.switches, &list.settings, other);
    for note in &list.notes {
        println!("  {note}");
    }
    let level = level(&how.level)?;
    if level == Level::Everything && push && list.live_content_changed {
        println!("  Posts or comments changed on the live site since then; pull them first, or move single posts with --posts.");
    }
    if !how.yes {
        println!(
            "Run again with --yes to {}.",
            if push { "push" } else { "pull" }
        );
        return Ok(());
    }
    let choice = choice_of(level, how.all, &list.files, &list.switches, &list.settings);
    if push {
        let main = staging::main_of(&home, &domain);
        let name = main.split('.').next().unwrap_or_default().to_string();
        let confirm = how.confirm.clone().unwrap_or_default();
        if level == Level::Everything && confirm != name {
            return Err(Error::Shell(format!(
                "add --confirm {name} to push everything"
            )));
        }
        let pushed = sync::push(
            &home,
            &sessions,
            &domain,
            &choice,
            &confirm,
            &mut progress(),
        );
        end_progress();
        let pushed = pushed?;
        println!(
            "Pushed: {} files, {} switches, {} settings.",
            pushed.files, pushed.switches, pushed.settings
        );
        if !pushed.site_answers {
            println!("The live site did not answer as expected afterwards; open it, and put the backup back if it is broken.");
        }
        for note in pushed.notes {
            println!("  {note}");
        }
        println!("{}", undo_hint(&domain, true, &pushed.backup));
    } else {
        let pulled = sync::pull(&home, &sessions, &domain, &choice, &mut progress());
        end_progress();
        let pulled = pulled?;
        println!(
            "Pulled: {} files, {} switches, {} settings.",
            pulled.files, pulled.switches, pulled.settings
        );
        for note in pulled.notes {
            println!("  {note}");
        }
        println!("{}", undo_hint(&domain, false, &pulled.snapshot));
    }
    Ok(())
}

fn undo_hint(domain: &str, push: bool, id: &str) -> String {
    if push {
        format!("A backup of the live site was taken first ({id}); put it back in Stackyard › Sites › {domain} › WordPress › Environments.")
    } else {
        format!("Undo with `stackyard wp snapshots {domain} --undo {id}`.")
    }
}

fn level(text: &str) -> Result<Level> {
    match text {
        "code" => Ok(Level::Code),
        "settings" => Ok(Level::Settings),
        "everything" => Ok(Level::Everything),
        other => Err(Error::Shell(format!(
            "unknown level {other:?}; pick code, settings or everything"
        ))),
    }
}

fn confirm_everything(home: &std::path::Path, target: &str, level: Level, how: &How) -> Result<()> {
    let main = staging::main_of(home, target);
    let name = main.split('.').next().unwrap_or_default();
    if level == Level::Everything && how.confirm.as_deref() != Some(name) {
        return Err(Error::Shell(format!(
            "add --confirm {name} to replace {target}'s content"
        )));
    }
    Ok(())
}

/// What `--all` and the level take from a change list: without `--all`,
/// only what the other side left alone.
fn choice_of(
    level: Level,
    all: bool,
    files: &[FileChange],
    switches: &[Switch],
    settings: &[SettingChange],
) -> Choice {
    Choice {
        level,
        files: files
            .iter()
            .filter(|f| all || !f.conflict)
            .map(|f| f.path.clone())
            .collect(),
        switches: if level == Level::Everything {
            Vec::new()
        } else {
            switches
                .iter()
                .filter(|s| all || !s.conflict)
                .map(|s| s.id.clone())
                .collect()
        },
        settings: if level == Level::Settings {
            settings
                .iter()
                .filter(|s| all || !s.conflict)
                .map(|s| s.name.clone())
                .collect()
        } else {
            Vec::new()
        },
    }
}

fn move_choice(how: &How) -> MoveChoice {
    MoveChoice {
        ids: how.posts.clone(),
        comments: how.comments,
        ..MoveChoice::default()
    }
}

fn change_word(change: Change) -> &'static str {
    match change {
        Change::Added => "added",
        Change::Changed => "changed",
        Change::Removed => "removed",
    }
}

fn print_changes(
    files: &[FileChange],
    switches: &[Switch],
    settings: &[SettingChange],
    other: &str,
) {
    if files.is_empty() && switches.is_empty() && settings.is_empty() {
        println!("  nothing");
        return;
    }
    let both = |conflict: bool| {
        if conflict {
            format!("  (also changed on {other}; taken only with --all)")
        } else {
            String::new()
        }
    };
    if !files.is_empty() {
        println!("Files:");
        for f in files {
            println!(
                "  {:<8} {}{}",
                change_word(f.change),
                f.path,
                both(f.conflict)
            );
        }
    }
    if !switches.is_empty() {
        println!("Plugins and theme:");
        for s in switches {
            let on = if s.on { "on" } else { "off" };
            println!("  switch {} {on}{}", s.title, both(s.conflict));
        }
    }
    if !settings.is_empty() {
        println!("Settings (taken with --level settings):");
        for s in settings {
            println!(
                "  {:<8} {}{}",
                change_word(s.change),
                s.name,
                both(s.conflict)
            );
        }
    }
}

fn print_plan(plan: &posts::MovePlan, target: &str) {
    for p in &plan.posts {
        let what = match p.action {
            posts::Action::Add => format!("added as #{}", p.target_id),
            posts::Action::Update => format!("replaces #{}", p.target_id),
            posts::Action::Skip => "left out".into(),
            posts::Action::Reuse => format!("already there as #{}", p.target_id),
        };
        let along = if p.picked { "" } else { " (comes along)" };
        println!("  {:<10} {}{along}: {what}", p.post_type, p.title);
    }
    println!(
        "  {} media files, {} categories and tags ({} new), {} comments, into {target}",
        plan.counts.files, plan.counts.terms, plan.counts.new_terms, plan.counts.comments
    );
    for w in &plan.warnings {
        println!("  can't be changed safely: {}: {}", w.title, w.text);
    }
}
