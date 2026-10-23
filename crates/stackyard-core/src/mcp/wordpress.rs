//! The WordPress tools: environments, stagings, WP-CLI and merges. Pushing
//! to a live site is never offered here; the user does that in the app.

use serde_json::{json, Value};

use super::tools::{
    database_client, fail, flag, required, settings, sites_changed, text, Args, Ctx,
};
use crate::remote::Sessions;
use crate::sites::{Site, SiteStore};
use crate::wordpress::cli::{self, Load};
use crate::wordpress::merge::{self, Choice, Level};
use crate::wordpress::staging::{self, MadeFrom, NewStaging};
use crate::wordpress::{env, WpSite};

/// Characters of WP-CLI's output returned; the rest is cut off.
const MAX_OUTPUT: usize = 20_000;

fn store(ctx: &Ctx) -> Result<SiteStore, String> {
    SiteStore::load_from(&ctx.home.join("sites.toml")).map_err(fail)
}

fn find(ctx: &Ctx, name: &str) -> Result<Site, String> {
    store(ctx)?
        .list()
        .iter()
        .find(|s| s.domain() == name || s.name == name)
        .cloned()
        .ok_or_else(|| format!("no site called {name}; list_wordpress_environments lists them"))
}

fn wordpress(ctx: &Ctx, name: &str) -> Result<WpSite, String> {
    settings(ctx)?.require_module("wordpress").map_err(fail)?;
    WpSite::open(&find(ctx, name)?).map_err(fail)
}

pub(super) fn environments(ctx: &Ctx, args: &Args) -> Result<Value, String> {
    settings(ctx)?.require_module("wordpress").map_err(fail)?;
    let only = text(args, "site")?
        .filter(|s| !s.is_empty())
        .map(|s| find(ctx, s).map(|site| staging::main_of(ctx.home, &site.domain())))
        .transpose()?;
    let mut out = Vec::new();
    for site in store(ctx)?.list() {
        let domain = site.domain();
        let main = staging::main_of(ctx.home, &domain);
        if main != domain || only.as_ref().is_some_and(|o| o != &domain) {
            continue;
        }
        if WpSite::open(site).is_err() {
            continue;
        }
        let live = env::load(ctx.home, &domain)
            .ok()
            .flatten()
            .and_then(|e| e.live)
            .map(|l| {
                json!({
                    "url": l.url,
                    "last_in_step": (l.synced_at > 0).then_some(l.synced_at),
                    "can_push_and_pull": l.connection.is_some(),
                })
            });
        let stagings: Vec<Value> = staging::list(ctx.home, &domain)
            .map_err(fail)?
            .into_iter()
            .map(|s| {
                json!({
                    "domain": s.domain,
                    "url": s.url,
                    "made_from": s.made_from,
                    "created_at": s.created_at,
                    "media_shared": s.shared_media,
                })
            })
            .collect();
        out.push(json!({
            "site": domain,
            "url": site.url(),
            "folder": site.path,
            "live": live,
            "stagings": stagings,
        }));
    }
    Ok(json!({
        "sites": out,
        "note": "Pushing to and pulling from the live site is done by the user in Stackyard › Sites › Manage › WordPress › Environments.",
    }))
}

pub(super) fn create_staging(ctx: &Ctx, args: &Args) -> Result<Value, String> {
    let site = wordpress(ctx, required(args, "site")?)?;
    let parent = staging::main_of(ctx.home, &site.site.domain());
    let from = match text(args, "from")?.filter(|s| !s.is_empty()) {
        None => MadeFrom::Site {
            domain: parent.clone(),
        },
        Some("live") => MadeFrom::Live,
        Some(domain) => MadeFrom::Site {
            domain: domain.to_string(),
        },
    };
    database_client(ctx)?;
    let new = NewStaging {
        parent,
        name: required(args, "name")?.to_string(),
        from,
        copy_media: flag(args, "copy_media")?.unwrap_or(true),
    };
    let made =
        staging::create(ctx.home, &Sessions::default(), &new, &mut |_| true).map_err(fail)?;
    let mut answer = json!({ "domain": made.domain(), "url": made.url(), "folder": made.path });
    if let Some(warning) = sites_changed(ctx) {
        answer["warning"] = json!(warning);
    }
    Ok(answer)
}

pub(super) fn run_cli(ctx: &Ctx, args: &Args) -> Result<Value, String> {
    let wp = wordpress(ctx, required(args, "site")?)?;
    let list: Vec<String> = match args.get("args") {
        Some(Value::Array(items)) => items
            .iter()
            .map(|v| v.as_str().map(str::to_string))
            .collect::<Option<_>>()
            .ok_or("args must be a list of strings")?,
        _ => return Err("args must be a list of strings".into()),
    };
    if list.is_empty() {
        return Err("args is empty; e.g. [\"plugin\", \"list\"]".into());
    }
    // Commands that wait for typing would hang the call.
    if matches!(list[0].as_str(), "shell" | "server")
        || (list[0] == "db" && list.get(1).is_some_and(|c| c == "cli"))
    {
        return Err(format!(
            "wp {} waits for typing and can't run here",
            list[..list.len().min(2)].join(" ")
        ));
    }
    let (ok, output) =
        cli::run_raw(ctx.home, &wp.root, Load::Everything, &list, false).map_err(fail)?;
    let cut = |s: String| {
        if s.chars().count() > MAX_OUTPUT {
            s.chars().take(MAX_OUTPUT).collect::<String>() + "…"
        } else {
            s
        }
    };
    Ok(json!({
        "site": wp.site.domain(),
        "succeeded": ok,
        "stdout": cut(output.stdout),
        "stderr": cut(output.stderr),
    }))
}

pub(super) fn merge_staging(ctx: &Ctx, args: &Args) -> Result<Value, String> {
    let source = wordpress(ctx, required(args, "staging")?)?.site.domain();
    let main = staging::main_of(ctx.home, &source);
    if main == source {
        return Err(format!("{source} is a main site, not a staging"));
    }
    let target = match text(args, "into")?.filter(|s| !s.is_empty()) {
        Some(name) => wordpress(ctx, name)?.site.domain(),
        None => main,
    };
    let level = match text(args, "level")?.unwrap_or("code") {
        "code" => Level::Code,
        "settings" => Level::Settings,
        "everything" => Level::Everything,
        other => {
            return Err(format!(
                "unknown level {other:?}; use code, settings or everything"
            ))
        }
    };
    database_client(ctx)?;
    let list = merge::changes(ctx.home, &source, &target).map_err(fail)?;
    if flag(args, "apply")? != Some(true) {
        return Ok(json!({
            "changes": list,
            "note": "Items with conflict true were also changed on the target and are left out of a merge. Call again with apply true to merge.",
        }));
    }
    let choice = Choice {
        level,
        files: list
            .files
            .iter()
            .filter(|f| !f.conflict)
            .map(|f| f.path.clone())
            .collect(),
        switches: list
            .switches
            .iter()
            .filter(|s| !s.conflict && level != Level::Everything)
            .map(|s| s.id.clone())
            .collect(),
        settings: list
            .settings
            .iter()
            .filter(|s| !s.conflict && level == Level::Settings)
            .map(|s| s.name.clone())
            .collect(),
    };
    let merged = merge::merge(ctx.home, &source, &target, &choice).map_err(fail)?;
    Ok(json!({
        "target": target,
        "merged": merged,
        "undo": "The user can undo it in Stackyard › Sites › Manage › WordPress › Environments › Snapshots.",
    }))
}

pub(super) fn destroy_staging(ctx: &Ctx, args: &Args) -> Result<Value, String> {
    let domain = wordpress(ctx, required(args, "staging")?)?.site.domain();
    if staging::main_of(ctx.home, &domain) == domain {
        return Err(format!(
            "{domain} is a main site; only stagings can be destroyed here"
        ));
    }
    database_client(ctx)?;
    let trashed = staging::destroy(ctx.home, &domain).map_err(fail)?;
    let mut answer = json!({
        "destroyed": domain,
        "trash_id": trashed.id,
        "note": "It stays in Stackyard's trash for 7 days; the user can bring it back from there.",
    });
    if let Some(warning) = sites_changed(ctx) {
        answer["warning"] = json!(warning);
    }
    Ok(answer)
}
