//! Pushing a copy's work to its live site, and pulling the live site's
//! later changes into a copy.
//!
//! Both sides are compared with where they stood when the copy was last in
//! step with the live site: its *sync base*, kept when the copy is cloned,
//! pulled or pushed. Files are compared by checksum on this computer and by
//! size and date on the server; settings and plugins through the
//! database's [`Facts`]. What only the side being taken from changed is
//! offered; what both sides changed is a conflict the user decides.
//!
//! A push works at the same three [`Level`]s as a merge, and nothing is
//! sent before the user has seen the list ([`changes`]). Then: a backup of
//! the live database and of every file about to be replaced, maintenance
//! mode, the files, the database part, maintenance off, and a check that
//! the home page and the login page answer. If a step fails, the backup is
//! put back. An Everything push keeps the live users, orders and customers
//! and the live payment settings, and refuses to run when the live posts
//! or comments changed since the last pull.
//!
//! A pull takes a snapshot of the copy first, so it can be undone like a
//! merge.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use base64::Engine as _;
use serde::{Deserialize, Serialize};

use super::env::{self, Environment, Live, Media, Route};
use super::helper::{self, Imported, Info, OptionValue};
use super::live::{self, OnProgress, Progress, Server, Stage};
use super::merge::{self, Change, Choice, FileChange, Level, SettingChange, Switch};
use super::snapshot::{self, Facts, FileEntry, Manifest};
use super::{posts, replace, staging, ExtensionAction, Kind, WpSite};
use crate::databases::{ident, literal, Client};
use crate::remote::{self, RemoteStore, Sessions};
use crate::sites::{Site, SiteStore};
use crate::{Error, Result};

fn fail(message: impl Into<String>) -> Error {
    Error::WordPress(message.into())
}

fn stopped() -> Error {
    fail("stopped")
}

fn io(path: &Path) -> impl FnOnce(std::io::Error) -> Error + '_ {
    move |source| Error::Io {
        path: path.to_owned(),
        source,
    }
}

fn say(progress: OnProgress, stage: Stage, message: impl Into<String>) -> Result<()> {
    if progress(Progress {
        stage,
        message: message.into(),
        done: 0,
        total: 0,
    }) {
        Ok(())
    } else {
        Err(stopped())
    }
}

/// Which way the changes go.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Way {
    /// From the copy to the live site.
    Push,
    /// From the live site into the copy.
    Pull,
}

// ---------------------------------------------------------------------------
// The sync base

/// Where a copy and its live site stood when they were last in step.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Base {
    pub at: u64,
    /// The live site's files: size and date, no checksums.
    pub live_files: Manifest,
    pub local_files: Manifest,
    pub live_facts: Option<Facts>,
    pub local_facts: Option<Facts>,
}

fn base_file(home: &Path, domain: &str) -> PathBuf {
    home.join("wordpress")
        .join("sync")
        .join(format!("{domain}.json.gz"))
}

pub fn load_base(home: &Path, domain: &str) -> Option<Base> {
    let file = std::fs::File::open(base_file(home, domain)).ok()?;
    serde_json::from_reader(flate2::read::GzDecoder::new(std::io::BufReader::new(file))).ok()
}

fn save_base(home: &Path, domain: &str, base: &Base) -> Result<()> {
    let file = base_file(home, domain);
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).map_err(io(dir))?;
    }
    let partial = file.with_extension("partial");
    let out = std::fs::File::create(&partial).map_err(io(&partial))?;
    let mut gz = flate2::write::GzEncoder::new(out, flate2::Compression::fast());
    serde_json::to_writer(&mut gz, base).map_err(|e| fail(e.to_string()))?;
    gz.finish().map_err(io(&partial))?;
    std::fs::rename(&partial, &file).map_err(io(&file))
}

/// Gives a new staging the base of the copy it was made from: its files
/// and database were that copy's.
pub fn copy_base(home: &Path, from: &str, to: &str) -> Result<()> {
    let (source, dest) = (base_file(home, from), base_file(home, to));
    if source.is_file() {
        std::fs::copy(&source, &dest).map_err(io(&dest))?;
    } else {
        let _ = std::fs::remove_file(&dest);
    }
    Ok(())
}

pub fn forget_base(home: &Path, domain: &str) {
    let _ = std::fs::remove_file(base_file(home, domain));
}

/// The live side as a fresh download shows it: the same files, without
/// checksums, as the server only tells sizes and dates.
fn as_live(files: &Manifest, content: &str) -> Manifest {
    files
        .iter()
        .filter(|(rel, _)| !left_out(rel, content))
        .map(|(rel, e)| {
            (
                rel.clone(),
                FileEntry {
                    size: e.size,
                    modified: e.modified,
                    hash: None,
                },
            )
        })
        .collect()
}

/// Keeps the base of a copy just cloned from a live server: the downloaded
/// files are the live ones, `live_facts` were read on the server.
pub(super) fn record_after_clone(
    home: &Path,
    site: &Site,
    live_facts: Option<Facts>,
) -> Result<()> {
    let wp = WpSite::open(site)?;
    let content = merge::content_rel(&wp);
    let local_files = snapshot::manifest(&site.path)?;
    let client = Client::open(home)?;
    let local_facts = snapshot::read_facts(&client, &wp.database()?, wp.prefix()).ok();
    save_base(
        home,
        &site.domain(),
        &Base {
            at: env::now(),
            live_files: as_live(&local_files, &content),
            local_files,
            live_facts,
            local_facts,
        },
    )
}

// ---------------------------------------------------------------------------
// Comparing

/// Files neither push nor pull touches: each side's config, the live
/// object cache, Stackyard's own files, caches and backups kept inside
/// the site.
fn left_out(rel: &str, content: &str) -> bool {
    merge::kept_apart(rel, content)
        || super::one_side_only(rel, content)
        || live::SKIP
            .iter()
            .any(|s| rel == *s || rel.starts_with(&format!("{s}/")))
}

/// The same file on this computer and on the server, which gives no
/// checksums: media by size, other files by size and date when both are
/// known. A changed file whose date the server doesn't tell is sent again.
fn same_across(local: Option<&FileEntry>, live: Option<&FileEntry>, media: bool) -> bool {
    match (local, live) {
        (None, None) => true,
        (Some(a), Some(b)) => {
            a.size == b.size
                && (media
                    || (a.modified != 0 && b.modified != 0 && (a.modified - b.modified).abs() <= 2))
        }
        _ => false,
    }
}

/// How a file changed on one side since the base. Live files have no
/// checksums, so their dates count too.
fn change_of(base: Option<&FileEntry>, now: Option<&FileEntry>) -> Option<Change> {
    let same = match (base, now) {
        (Some(a), Some(b)) if a.hash.is_none() || b.hash.is_none() => {
            a.size == b.size
                && (a.modified == 0 || b.modified == 0 || (a.modified - b.modified).abs() <= 2)
        }
        _ => merge::same(base, now),
    };
    if same {
        return None;
    }
    Some(match (base, now) {
        (None, _) => Change::Added,
        (_, None) => Change::Removed,
        _ => Change::Changed,
    })
}

/// The files that changed on the side changes are taken from.
fn file_list(
    way: Way,
    base: Option<&Base>,
    local: &Manifest,
    live: &Manifest,
    content: &str,
    media_from_live: bool,
) -> Vec<FileChange> {
    let empty = Manifest::new();
    let (base_local, base_live) =
        base.map_or((&empty, &empty), |b| (&b.local_files, &b.live_files));
    let (from_base, from_now, to_base, to_now) = match way {
        Way::Push => (base_local, local, base_live, live),
        Way::Pull => (base_live, live, base_local, local),
    };
    let mut paths: BTreeSet<&String> = from_base.keys().chain(from_now.keys()).collect();
    if base.is_none() {
        paths.extend(to_now.keys());
    }
    let mut out = Vec::new();
    for rel in paths {
        if left_out(rel, content) {
            continue;
        }
        let media = snapshot::is_media(rel);
        if media && media_from_live && way == Way::Pull {
            // Media comes from the live site when a page needs it.
            continue;
        }
        let (f, t) = (from_now.get(rel), to_now.get(rel));
        let (l, v) = match way {
            Way::Push => (f, t),
            Way::Pull => (t, f),
        };
        if same_across(l, v, media) {
            continue;
        }
        let change = match base {
            Some(_) => match change_of(from_base.get(rel), f) {
                Some(c) => c,
                None => continue,
            },
            // No base: offer what differs, but never a removal.
            None => match (f, t) {
                (Some(_), Some(_)) => Change::Changed,
                (Some(_), None) => Change::Added,
                _ => continue,
            },
        };
        if way == Way::Push && media && media_from_live && change == Change::Removed {
            continue;
        }
        let target = base.and_then(|_| change_of(to_base.get(rel), t));
        out.push(FileChange {
            path: rel.clone(),
            group: merge::group_of(rel, content),
            change,
            target,
            conflict: base.is_none() || target.is_some(),
        });
    }
    out
}

/// Options changed on the side changes are taken from.
fn option_list(
    way: Way,
    base: Option<&Base>,
    local: &Facts,
    live: &Facts,
) -> Vec<(String, Change, bool)> {
    let Some((Some(base_local), Some(base_live))) =
        base.map(|b| (b.local_facts.as_ref(), b.live_facts.as_ref()))
    else {
        return Vec::new();
    };
    let (fb, fnow, tb, tnow) = match way {
        Way::Push => (
            &base_local.options,
            &local.options,
            &base_live.options,
            &live.options,
        ),
        Way::Pull => (
            &base_live.options,
            &live.options,
            &base_local.options,
            &local.options,
        ),
    };
    let names: BTreeSet<&String> = fb.keys().chain(fnow.keys()).collect();
    names
        .into_iter()
        .filter(|n| !merge::option_kept_apart(n))
        .filter_map(|name| {
            let (b, n) = (fb.get(name), fnow.get(name));
            if b == n {
                return None;
            }
            let change = match (b, n) {
                (None, _) => Change::Added,
                (_, None) => Change::Removed,
                _ => Change::Changed,
            };
            Some((name.clone(), change, tb.get(name) != tnow.get(name)))
        })
        .collect()
}

/// Plugins and the theme switched on or off on the side changes are taken from.
fn switch_list(way: Way, base: Option<&Base>, local: &Facts, live: &Facts) -> Vec<Switch> {
    let (from, to) = match way {
        Way::Push => (local, live),
        Way::Pull => (live, local),
    };
    let base_from = base.and_then(|b| match way {
        Way::Push => b.local_facts.as_ref(),
        Way::Pull => b.live_facts.as_ref(),
    });
    if let Some(base_from) = base_from {
        return merge::switch_changes(base_from, from, to);
    }
    // No base: what differs, each a conflict for the user to decide.
    let set = |f: &Facts| -> BTreeSet<String> {
        f.active_plugins
            .iter()
            .map(|p| merge::plugin_slug(p))
            .collect()
    };
    let (a, b) = (set(from), set(to));
    let mut out: Vec<Switch> = a
        .symmetric_difference(&b)
        .map(|slug| Switch {
            id: format!("plugin:{slug}"),
            title: slug.clone(),
            on: a.contains(slug),
            conflict: true,
        })
        .collect();
    if !from.stylesheet.is_empty() && from.stylesheet != to.stylesheet {
        out.push(Switch {
            id: format!("theme:{}", from.stylesheet),
            title: from.stylesheet.clone(),
            on: true,
            conflict: true,
        });
    }
    out
}

/// What a push or pull would do, before anything is sent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SyncChanges {
    pub way: Way,
    pub domain: String,
    pub live_url: String,
    /// When the copy was last in step with the live site; 0 when Stackyard
    /// has no record of it (copies cloned before it kept one).
    pub base_at: u64,
    pub files: Vec<FileChange>,
    pub switches: Vec<Switch>,
    pub settings: Vec<SettingChange>,
    /// Posts or comments changed on the live site since then, orders and
    /// their notes left out; also true when it can't be told.
    pub live_content_changed: bool,
    pub local_content_changed: bool,
    pub live_wordpress: String,
    pub live_php: String,
    pub local_wordpress: String,
    pub notes: Vec<String>,
}

// ---------------------------------------------------------------------------
// The live server

impl Server<'_> {
    pub(super) fn facts(&self) -> Result<Facts> {
        let mut out = Vec::new();
        self.call(&[("do", "facts")], &mut out)?;
        serde_json::from_slice(&out).map_err(|_| {
            fail(format!(
                "the helper's answer could not be read: {}",
                String::from_utf8_lossy(&out[..out.len().min(300)])
            ))
        })
    }

    /// Option values and their autoload setting, by name.
    fn option_values(&self, names: &[String]) -> Result<BTreeMap<String, (Vec<u8>, String)>> {
        if names.is_empty() {
            return Ok(BTreeMap::new());
        }
        let list = serde_json::to_string(names).map_err(|e| fail(e.to_string()))?;
        let mut out = Vec::new();
        self.call(&[("do", "options"), ("names", &list)], &mut out)?;
        let found: BTreeMap<String, OptionValue> = serde_json::from_slice(&out)
            .map_err(|_| fail("the helper's answer about settings could not be read"))?;
        Ok(found
            .into_iter()
            .map(|(name, v)| {
                let value = base64::engine::general_purpose::STANDARD
                    .decode(v.value.as_bytes())
                    .unwrap_or_default();
                (name, (value, v.autoload))
            })
            .collect())
    }

    /// Uploads a `.sql.gz` file next to the helper and has it loaded into
    /// the live database. `whole` loads it in one request, for small files
    /// whose statements share variables.
    fn import(
        &self,
        file: &Path,
        whole: bool,
        progress: &mut dyn FnMut(u64) -> bool,
    ) -> Result<()> {
        let name = helper::sql_file_name()?;
        let remote_file = remote::join(&self.root, &name);
        self.sessions
            .upload(&self.connection, file, &remote_file)
            .map_err(|e| fail(format!("could not upload the database changes: {e}")))?;
        let result = (|| {
            let mut from = 0u64;
            loop {
                let mut out = Vec::new();
                let at = from.to_string();
                let mut args = vec![
                    ("do", "import"),
                    ("file", name.as_str()),
                    ("from", at.as_str()),
                ];
                if whole {
                    args.push(("whole", "1"));
                }
                self.call(&args, &mut out)?;
                let done: Imported = serde_json::from_slice(&out).map_err(|_| {
                    fail(format!(
                        "the helper's answer could not be read: {}",
                        String::from_utf8_lossy(&out[..out.len().min(300)])
                    ))
                })?;
                if done.done {
                    return Ok(());
                }
                if done.next <= from {
                    return Err(fail(
                        "loading the database on the live site stopped making progress",
                    ));
                }
                from = done.next;
                if !progress(from) {
                    return Err(stopped());
                }
            }
        })();
        let _ = self.sessions.remove_file(&self.connection, &remote_file);
        result
    }

    /// Every file of the live site, by path relative to WordPress's folder.
    fn files(&self, skip_media: bool, progress: OnProgress) -> Result<Manifest> {
        let mut skip = remote::default_skip();
        skip.extend(live::SKIP.iter().map(|s| s.to_string()));
        if skip_media {
            skip.push("wp-content/uploads".into());
        }
        let listed = self
            .sessions
            .files(&self.connection, &self.root, &skip, &mut |p| {
                let message = match &p {
                    remote::Progress::Scanning { found, .. } => {
                        format!("Listing files on the server… {found} found")
                    }
                    remote::Progress::Transfer { path, .. } => path.clone(),
                };
                progress(Progress {
                    stage: Stage::Files,
                    message,
                    done: 0,
                    total: 0,
                })
            })?;
        Ok(listed
            .into_iter()
            .filter(|(rel, _)| !(rel.starts_with("stackyard-") && !rel.contains('/')))
            .map(|(rel, (size, modified))| {
                (
                    rel,
                    FileEntry {
                        size,
                        modified: modified.unwrap_or(0),
                        hash: None,
                    },
                )
            })
            .collect())
    }

    /// Downloads every table into `dir`, returning the parts in order.
    fn backup_database(
        &self,
        tables: &[helper::Table],
        dir: &Path,
        progress: OnProgress,
    ) -> Result<Vec<PathBuf>> {
        std::fs::create_dir_all(dir).map_err(io(dir))?;
        let mut parts = Vec::new();
        let total: u64 = tables.iter().map(|t| t.bytes).sum();
        let mut done = 0u64;
        for (index, table) in tables.iter().enumerate() {
            if !progress(Progress {
                stage: Stage::Database,
                message: format!(
                    "Backing up the table {} ({} of {})",
                    table.name,
                    index + 1,
                    tables.len()
                ),
                done,
                total,
            }) {
                return Err(stopped());
            }
            done += table.bytes;
            if self.dump_tool.is_some() {
                let dest = dir.join(format!("{index:05}-00000.sql.gz"));
                if self.dump_table(&table.name, &dest).is_ok() {
                    parts.push(dest);
                    continue;
                }
            }
            let mut next = String::new();
            for part in 0.. {
                let dest = dir.join(format!("{index:05}-{part:05}.sql.gz"));
                let chunk = live::fetch_chunk(self, &table.name, &next, &dest)?;
                parts.push(dest);
                next = chunk.next;
                if chunk.done {
                    break;
                }
            }
        }
        Ok(parts)
    }

    fn maintenance(&self, on: bool, work: &Path) -> Result<()> {
        let remote_file = remote::join(&self.root, ".maintenance");
        if on {
            let local = work.join("maintenance.php");
            std::fs::write(&local, format!("<?php $upgrading = {};\n", env::now()))
                .map_err(io(&local))?;
            self.sessions.upload(&self.connection, &local, &remote_file)
        } else {
            self.sessions.remove_file(&self.connection, &remote_file)
        }
    }
}

/// A copy's live site, reached and with a helper on it, which is taken
/// off again when this is dropped.
struct Reached<'a> {
    site: Site,
    wp: WpSite,
    env: Environment,
    live: Live,
    server: Server<'a>,
    info: Info,
    work: PathBuf,
}

impl Drop for Reached<'_> {
    fn drop(&mut self) {
        self.server.remove_helper();
        let _ = std::fs::remove_dir_all(&self.work);
    }
}

fn site_of(home: &Path, domain: &str) -> Result<Site> {
    let store = SiteStore::load_from(&home.join("sites.toml"))?;
    store
        .list()
        .iter()
        .find(|s| s.domain() == domain)
        .cloned()
        .ok_or_else(|| Error::SiteNotFound(domain.to_string()))
}

fn reach<'a>(
    home: &Path,
    sessions: &'a Sessions,
    domain: &str,
    write: bool,
    progress: OnProgress,
) -> Result<Reached<'a>> {
    let site = site_of(home, domain)?;
    let wp = WpSite::open(&site)?;
    let env = env::load(home, domain)?.unwrap_or_default();
    let live = env
        .live
        .clone()
        .ok_or_else(|| fail(format!("{domain} is not a copy of a live site")))?;
    if live.route == Route::Files {
        return Err(fail(format!(
            "{domain} was cloned from backup files, so Stackyard can't reach the live server; clone it again from the server (Sites › Clone from live) to push and pull"
        )));
    }
    let id = live.connection.clone().ok_or_else(|| {
        fail(format!(
            "{domain} has no saved connection to the live server; add it under Manage › Servers"
        ))
    })?;
    let connection = RemoteStore::load()?.get(&id)?.clone();
    let work = home.join("tmp").join(format!(
        "sync-{}-{}",
        domain.replace('.', "-"),
        std::process::id()
    ));
    std::fs::create_dir_all(&work).map_err(io(&work))?;
    say(
        progress,
        Stage::Connecting,
        "Connecting to the live server…",
    )?;
    let mut roots = Vec::new();
    let root = Some(live.path.as_str()).filter(|p| !p.is_empty());
    let mut server = Server::open(
        sessions,
        &connection,
        root,
        Some(&live.url),
        &work,
        &mut roots,
    )?;
    server.upload_helper_with(&work, write)?;
    let info = match server.info() {
        Ok(info) => info,
        Err(e) => {
            server.remove_helper();
            return Err(e);
        }
    };
    Ok(Reached {
        site,
        wp,
        env,
        live,
        server,
        info,
        work,
    })
}

fn media_from_live(r: &Reached) -> bool {
    r.live.media == Media::FromLive
}

/// The address rules from one side to the other.
fn rules(r: &Reached, home: &Path, way: Way) -> replace::Rules {
    let local_url = super::home_url(home, &r.wp);
    let local_path = r.site.path.to_string_lossy().replace('\\', "/");
    let live_path = if r.info.abspath.is_empty() {
        r.server.root.clone()
    } else {
        r.info.abspath.clone()
    };
    match way {
        Way::Push => {
            replace::Rules::site_move(&local_url, &r.live.url).and_path(&local_path, &live_path)
        }
        Way::Pull => {
            replace::Rules::site_move(&r.live.url, &local_url).and_path(&live_path, &local_path)
        }
    }
}

fn preview(value: &[u8]) -> String {
    let text = String::from_utf8_lossy(value);
    let mut out: String = text.chars().take(80).collect();
    if text.chars().count() > 80 {
        out.push('…');
    }
    out
}

/// Both sides as they are now, compared with the base.
struct Compared {
    changes: SyncChanges,
    local_files: Manifest,
    live_files: Manifest,
    local_facts: Facts,
    live_facts: Facts,
}

fn compare(home: &Path, r: &Reached, way: Way, progress: OnProgress) -> Result<Compared> {
    let domain = r.site.domain();
    let base = load_base(home, &domain);
    let content = merge::content_rel(&r.wp);
    let from_live = media_from_live(r);

    say(progress, Stage::Files, "Listing the files here…")?;
    let known = base
        .as_ref()
        .map(|b| b.local_files.clone())
        .unwrap_or_default();
    let local_files = snapshot::manifest_with(&r.site.path, &known)?;
    let live_files = r.server.files(from_live, progress)?;

    say(
        progress,
        Stage::Database,
        "Reading the settings on both sides…",
    )?;
    let client = Client::open(home)?;
    let local_db = r.wp.database()?;
    let local_facts = snapshot::read_facts(&client, &local_db, r.wp.prefix())?;
    let live_facts = r.server.facts()?;

    let mut files = file_list(
        way,
        base.as_ref(),
        &local_files,
        &live_files,
        &content,
        from_live,
    );
    let mut notes = Vec::new();
    if way == Way::Push && from_live {
        // Media the copy fetched from the live site as pages asked for it
        // is already there: only what the live site lacks goes up.
        let mut by_folder: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for (i, f) in files.iter().enumerate() {
            if f.group == "media" && f.change == Change::Added {
                let folder = f.path.rsplit_once('/').map_or("", |(d, _)| d).to_string();
                by_folder.entry(folder).or_default().push(i);
            }
        }
        let mut drop = BTreeSet::new();
        for (folder, items) in by_folder {
            let listed = r
                .server
                .sessions
                .list(&r.server.connection, &remote::join(&r.server.root, &folder))
                .unwrap_or_default();
            for i in items {
                let name = files[i].path.rsplit('/').next().unwrap_or_default();
                let size = local_files.get(&files[i].path).map_or(0, |e| e.size);
                if listed
                    .iter()
                    .any(|e| !e.dir && e.name == name && e.size == size)
                {
                    drop.insert(i);
                }
            }
        }
        let mut i = 0;
        files.retain(|_| {
            i += 1;
            !drop.contains(&(i - 1))
        });
    }

    let options = option_list(way, base.as_ref(), &local_facts, &live_facts);
    let names: Vec<String> = options.iter().map(|(n, _, _)| n.clone()).collect();
    let local_values = local_option_values(&client, &local_db, r.wp.prefix(), &names)?;
    let live_values = r.server.option_values(&names)?;
    let settings = options
        .into_iter()
        .map(|(name, change, conflict)| {
            let here = local_values.get(&name).map(|(v, _)| preview(v));
            let there = live_values.get(&name).map(|(v, _)| preview(v));
            let (staging, target) = match way {
                Way::Push => (here, there),
                Way::Pull => (there, here),
            };
            SettingChange {
                name,
                change,
                conflict,
                staging,
                target,
            }
        })
        .collect();
    let switches = switch_list(way, base.as_ref(), &local_facts, &live_facts);

    let base_live = base.as_ref().and_then(|b| b.live_facts.as_ref());
    let base_local = base.as_ref().and_then(|b| b.local_facts.as_ref());
    let live_content_changed =
        base_live.is_none_or(|b| b.content.is_empty() || b.content != live_facts.content);
    let local_content_changed = base_local.is_none_or(|b| b.content != local_facts.content);
    if base.is_none() {
        notes.push(format!(
            "Stackyard has no record of when {domain} was last in step with the live site, so every difference is listed and left for you to tick. Settings can't be compared this time; after this, they can."
        ));
    }
    let local_wordpress = r.wp.version().unwrap_or_default();
    if way == Way::Push
        && !r.info.wordpress.is_empty()
        && !local_wordpress.is_empty()
        && version_less(&r.info.wordpress, &local_wordpress)
    {
        notes.push(format!(
            "The live site runs WordPress {}, this copy {local_wordpress}. Update WordPress on the live site first if your changes need the newer version.",
            r.info.wordpress
        ));
    }
    Ok(Compared {
        changes: SyncChanges {
            way,
            domain,
            live_url: r.live.url.clone(),
            base_at: base.as_ref().map_or(0, |b| b.at),
            files,
            switches,
            settings,
            live_content_changed,
            local_content_changed,
            live_wordpress: r.info.wordpress.clone(),
            live_php: r.info.php.clone(),
            local_wordpress,
            notes,
        },
        local_files,
        live_files,
        local_facts,
        live_facts,
    })
}

/// `a` is an older version than `b` (dotted numbers).
fn version_less(a: &str, b: &str) -> bool {
    let parts = |v: &str| -> Vec<u64> {
        v.split(|c: char| !c.is_ascii_digit())
            .filter(|p| !p.is_empty())
            .map(|p| p.parse().unwrap_or(0))
            .collect()
    };
    parts(a) < parts(b)
}

fn local_option_values(
    client: &Client,
    database: &str,
    prefix: &str,
    names: &[String],
) -> Result<BTreeMap<String, (Vec<u8>, String)>> {
    if names.is_empty() {
        return Ok(BTreeMap::new());
    }
    let list: Vec<String> = names.iter().map(|n| literal(n)).collect();
    let rows = client.rows(
        database,
        &format!(
            "SELECT option_name, option_value, autoload FROM {} WHERE option_name IN ({})",
            ident(&format!("{prefix}options")),
            list.join(",")
        ),
    )?;
    Ok(rows
        .rows
        .into_iter()
        .filter_map(|r| {
            let name = r.first()?.clone()?;
            let value = r.get(1).cloned().flatten().unwrap_or_default();
            let autoload = r.get(2).cloned().flatten().unwrap_or_else(|| "yes".into());
            Some((name, (value.into_bytes(), autoload)))
        })
        .collect())
}

/// What would change, both ways in one look: nothing is sent or changed.
pub fn changes(
    home: &Path,
    sessions: &Sessions,
    domain: &str,
    way: Way,
    progress: OnProgress,
) -> Result<SyncChanges> {
    let r = reach(home, sessions, domain, false, progress)?;
    Ok(compare(home, &r, way, progress)?.changes)
}

/// Checks the picked items are ones the list offered.
fn check_choice(changes: &SyncChanges, choice: &Choice) -> Result<()> {
    for rel in &choice.files {
        if !changes.files.iter().any(|f| &f.path == rel) {
            return Err(fail(format!(
                "{rel} is no longer in the list of changes; look at the changes again"
            )));
        }
    }
    for id in &choice.switches {
        if !changes.switches.iter().any(|s| &s.id == id) {
            return Err(fail(format!(
                "{id} is no longer in the list of changes; look at the changes again"
            )));
        }
    }
    Ok(())
}

/// A file of SQL statements, one per line, for the helper to load.
fn sql_file(dir: &Path, name: &str, sql: &str) -> Result<PathBuf> {
    let path = dir.join(name);
    let out = std::fs::File::create(&path).map_err(io(&path))?;
    let mut gz = flate2::write::GzEncoder::new(out, flate2::Compression::fast());
    gz.write_all(b"SET NAMES utf8mb4;\n")
        .and_then(|_| gz.write_all(sql.as_bytes()))
        .map_err(io(&path))?;
    gz.finish().map_err(io(&path))?;
    Ok(path)
}

/// SQL setting options on the other side, with addresses changed.
fn options_sql(
    table: &str,
    names: &[String],
    values: &BTreeMap<String, (Vec<u8>, String)>,
    rules: &replace::Rules,
) -> String {
    let mut sql = String::new();
    for name in names.iter().filter(|n| !merge::option_kept_apart(n)) {
        match values.get(name) {
            Some((value, autoload)) => {
                let value = replace::replace_value(value, rules).unwrap_or_else(|| value.clone());
                sql.push_str(&format!(
                    "INSERT INTO {table} (option_name, option_value, autoload) VALUES ({}, {}, {}) ON DUPLICATE KEY UPDATE option_value = VALUES(option_value), autoload = VALUES(autoload);\n",
                    literal(name),
                    literal(&String::from_utf8_lossy(&value)),
                    literal(autoload)
                ));
            }
            None => sql.push_str(&format!(
                "DELETE FROM {table} WHERE option_name = {};\n",
                literal(name)
            )),
        }
    }
    sql
}

/// The serialized `active_plugins` after switching the picked plugins.
fn switched_plugins(current: &[String], from: &Facts, picked: &[&Switch]) -> Vec<String> {
    let mut list: Vec<String> = current.to_vec();
    for s in picked {
        let Some(slug) = s.id.strip_prefix("plugin:") else {
            continue;
        };
        list.retain(|p| merge::plugin_slug(p) != slug);
        if s.on {
            if let Some(file) = from
                .active_plugins
                .iter()
                .find(|p| merge::plugin_slug(p) == slug)
            {
                list.push(file.clone());
            }
        }
    }
    list.sort();
    list.dedup();
    list
}

fn serialize_list(items: &[String]) -> String {
    let value = super::php::Value::Array(
        items
            .iter()
            .enumerate()
            .map(|(i, p)| (super::php::Value::Int(i as i64), super::php::Value::str(p)))
            .collect(),
    );
    String::from_utf8_lossy(&value.serialize()).into_owned()
}

/// A dump from this computer, made loadable on the live server: collations
/// only the newest MariaDB and MySQL know are changed to one every server has.
fn portable(input: &Path, output: &Path) -> Result<()> {
    let reader = std::io::BufReader::new(flate2::read::GzDecoder::new(
        std::fs::File::open(input).map_err(io(input))?,
    ));
    let out = std::fs::File::create(output).map_err(io(output))?;
    let mut gz = flate2::write::GzEncoder::new(out, flate2::Compression::fast());
    for line in reader.split(b'\n') {
        let line = line.map_err(io(input))?;
        let line = if line.starts_with(b"INSERT") {
            line
        } else {
            portable_line(&String::from_utf8_lossy(&line)).into_bytes()
        };
        gz.write_all(&line)
            .and_then(|_| gz.write_all(b"\n"))
            .map_err(io(output))?;
    }
    gz.finish().map_err(io(output))?;
    Ok(())
}

fn portable_line(line: &str) -> String {
    let mut out = line.to_string();
    for (family, to) in [
        ("utf8mb4_uca1400_", "utf8mb4_unicode_ci"),
        ("utf8mb4_0900_", "utf8mb4_unicode_ci"),
        ("utf8mb3_uca1400_", "utf8mb3_unicode_ci"),
    ] {
        while let Some(at) = out.find(family) {
            let end = out[at + family.len()..]
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .map_or(out.len(), |n| at + family.len() + n);
            out.replace_range(at..end, to);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Live backups

/// A backup of the live site taken before a push.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LiveBackup {
    pub id: String,
    pub domain: String,
    pub created_at: u64,
    /// The database parts, relative to the backup's folder, in load order.
    pub database: Vec<String>,
    /// Live files about to be replaced or removed, kept under `files/`.
    pub saved: Vec<String>,
    /// Files the push added, removed again on restore.
    pub added: Vec<String>,
    pub bytes: u64,
}

fn backups_dir(home: &Path, domain: &str) -> PathBuf {
    home.join("backups").join("live").join(domain)
}

fn backup_info(dir: &Path) -> Option<LiveBackup> {
    let text = std::fs::read(dir.join("backup.json")).ok()?;
    serde_json::from_slice(&text).ok()
}

fn save_backup_info(dir: &Path, backup: &LiveBackup) -> Result<()> {
    let file = dir.join("backup.json");
    let text = serde_json::to_vec_pretty(backup).map_err(|e| fail(e.to_string()))?;
    std::fs::write(&file, text).map_err(io(&file))
}

/// Backups of the live site, newest first.
pub fn backups(home: &Path, domain: &str) -> Vec<LiveBackup> {
    let mut list: Vec<LiveBackup> = std::fs::read_dir(backups_dir(home, domain))
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|e| backup_info(&e.path()))
                .collect()
        })
        .unwrap_or_default();
    list.sort_by_key(|b| std::cmp::Reverse(b.created_at));
    list
}

pub fn remove_backup(home: &Path, domain: &str, id: &str) -> Result<()> {
    if id.contains(['/', '\\', '.']) {
        return Err(fail("not a backup"));
    }
    let dir = backups_dir(home, domain).join(id);
    std::fs::remove_dir_all(&dir).map_err(io(&dir))
}

/// Puts a backup back on the live site: its database, the files it kept,
/// and removes the files the push added.
pub fn restore_backup(
    home: &Path,
    sessions: &Sessions,
    domain: &str,
    id: &str,
    progress: OnProgress,
) -> Result<()> {
    if id.contains(['/', '\\', '.']) {
        return Err(fail("not a backup"));
    }
    let dir = backups_dir(home, domain).join(id);
    let backup = backup_info(&dir).ok_or_else(|| fail(format!("no backup {id}")))?;
    let r = reach(home, sessions, domain, true, progress)?;
    put_back(&r, &dir, &backup, progress)
}

fn put_back(r: &Reached, dir: &Path, backup: &LiveBackup, progress: OnProgress) -> Result<()> {
    let s = &r.server;
    let _ = s.maintenance(true, &r.work);
    let result = (|| {
        say(progress, Stage::Files, "Putting the live files back…")?;
        for rel in &backup.saved {
            let local = dir.join("files").join(rel);
            s.sessions
                .upload_into(&s.connection, &local, &remote::join(&s.root, rel))?;
        }
        for rel in &backup.added {
            let _ = s
                .sessions
                .remove_file(&s.connection, &remote::join(&s.root, rel));
        }
        if !backup.database.is_empty() {
            say(progress, Stage::Loading, "Putting the live database back…")?;
            let parts: Vec<PathBuf> = backup.database.iter().map(|p| dir.join(p)).collect();
            let whole = r.work.join("restore.sql.gz");
            let out = std::fs::File::create(&whole).map_err(io(&whole))?;
            let mut gz = flate2::write::GzEncoder::new(out, flate2::Compression::fast());
            std::io::copy(&mut live::Concat::new(parts), &mut gz).map_err(io(&whole))?;
            gz.finish().map_err(io(&whole))?;
            s.import(&whole, false, &mut |_| true)?;
        }
        Ok(())
    })();
    let _ = s.maintenance(false, &r.work);
    result
}

// ---------------------------------------------------------------------------
// Push

/// What a push did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Pushed {
    /// The backup taken first, restorable with [`restore_backup`].
    pub backup: String,
    pub files: usize,
    pub switches: usize,
    pub settings: usize,
    /// The home page and login page answered after the push.
    pub site_answers: bool,
    pub notes: Vec<String>,
}

/// Option names a push at the Everything level keeps from the live site:
/// payment settings (the copy's are in test mode).
fn kept_live_options(home: &Path, wp: &WpSite) -> Vec<String> {
    let mut names: Vec<String> = super::woo::gateways(home, wp)
        .map(|g| {
            g.into_iter()
                .map(|g| format!("woocommerce_{}_settings", g.id))
                .collect()
        })
        .unwrap_or_default();
    for known in [
        "woocommerce_stripe_settings",
        "woocommerce_ppcp-gateway_settings",
        "woocommerce_paypal_settings",
        "woocommerce_woocommerce_payments_settings",
        "woocommerce_square_credit_card_settings",
        "woocommerce_mollie_settings",
    ] {
        if !names.iter().any(|n| n == known) {
            names.push(known.into());
        }
    }
    names
}

/// Sends the picked changes to the live site. `confirm` must be the main
/// site's name for an Everything push.
pub fn push(
    home: &Path,
    sessions: &Sessions,
    domain: &str,
    choice: &Choice,
    confirm: &str,
    progress: OnProgress,
) -> Result<Pushed> {
    let r = reach(home, sessions, domain, true, progress)?;
    let compared = compare(home, &r, Way::Push, progress)?;
    check_choice(&compared.changes, choice)?;
    let main = staging::main_of(home, domain);
    let name = main.split('.').next().unwrap_or_default();
    if choice.level == Level::Everything {
        if confirm.trim() != name {
            return Err(fail(format!("type {name} to confirm a push of everything")));
        }
        if compared.changes.live_content_changed {
            return Err(fail(
                "posts or comments changed on the live site since this copy was last in step with it; pull from live first, then push",
            ));
        }
    }
    let s = &r.server;
    let rules = rules(&r, home, Way::Push);
    let client = Client::open(home)?;
    let local_db = r.wp.database()?;
    let mut notes = Vec::new();

    // Backup: the whole live database and every file about to change.
    let id = format!("{}", env::now());
    let dir = backups_dir(home, domain).join(&id);
    std::fs::create_dir_all(&dir).map_err(io(&dir))?;
    let parts = s.backup_database(&r.info.tables, &dir.join("database"), progress)?;
    let mut backup = LiveBackup {
        id: id.clone(),
        domain: domain.to_string(),
        created_at: env::now(),
        database: parts
            .iter()
            .filter_map(|p| p.strip_prefix(&dir).ok())
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .collect(),
        ..LiveBackup::default()
    };
    say(
        progress,
        Stage::Files,
        "Backing up the live files about to change…",
    )?;
    for rel in &choice.files {
        if compared.live_files.contains_key(rel) {
            let local = dir.join("files").join(rel);
            s.sessions.download(
                &s.connection,
                &remote::join(&s.root, rel),
                &local,
                &mut |_| true,
            )?;
            backup.saved.push(rel.clone());
        } else if r.site.path.join(rel).is_file() {
            backup.added.push(rel.clone());
        }
    }
    backup.bytes = dir_size(&dir);
    save_backup_info(&dir, &backup)?;

    say(
        progress,
        Stage::Files,
        "Switching the live site to maintenance mode…",
    )?;
    s.maintenance(true, &r.work)?;
    let applied = (|| -> Result<(usize, usize)> {
        let count = choice.files.len() as u64;
        for (i, rel) in choice.files.iter().enumerate() {
            if !progress(Progress {
                stage: Stage::Files,
                message: format!("Uploading {rel}"),
                done: i as u64,
                total: count,
            }) {
                return Err(stopped());
            }
            let local = r.site.path.join(rel);
            let remote_path = remote::join(&s.root, rel);
            if local.is_file() {
                s.sessions
                    .upload_into(&s.connection, &local, &remote_path)?;
            } else {
                let _ = s.sessions.remove_file(&s.connection, &remote_path);
            }
        }

        let mut sql = String::new();
        let options = ident(&format!("{}options", s.db.prefix));
        let done = match choice.level {
            Level::Code | Level::Settings => {
                let mut settings = 0;
                let picked: Vec<&Switch> = compared
                    .changes
                    .switches
                    .iter()
                    .filter(|sw| choice.switches.contains(&sw.id))
                    .collect();
                if picked.iter().any(|sw| sw.id.starts_with("plugin:")) {
                    let list = switched_plugins(
                        &compared.live_facts.active_plugins,
                        &compared.local_facts,
                        &picked,
                    );
                    sql.push_str(&format!(
                        "UPDATE {options} SET option_value = {} WHERE option_name = 'active_plugins';\n",
                        literal(&serialize_list(&list))
                    ));
                }
                if let Some(theme) = picked.iter().find_map(|sw| sw.id.strip_prefix("theme:")) {
                    let template = if compared.local_facts.stylesheet == theme {
                        compared.local_facts.template.clone()
                    } else {
                        theme.to_string()
                    };
                    for (option, value) in
                        [("stylesheet", theme.to_string()), ("template", template)]
                    {
                        sql.push_str(&format!(
                            "UPDATE {options} SET option_value = {} WHERE option_name = {};\n",
                            literal(&value),
                            literal(option)
                        ));
                    }
                }
                let switched = picked.len();
                if !picked.is_empty() {
                    notes.push("Plugins were switched on or off in the live settings, without running their own setup; if a plugin needs that, switch it off and on again in wp-admin.".into());
                }
                if choice.level == Level::Settings && !choice.settings.is_empty() {
                    let values =
                        local_option_values(&client, &local_db, r.wp.prefix(), &choice.settings)?;
                    sql.push_str(&options_sql(&options, &choice.settings, &values, &rules));
                    settings = choice.settings.len();
                }
                if !sql.is_empty() {
                    say(progress, Stage::Loading, "Changing the live settings…")?;
                    let file = sql_file(&r.work, "push.sql.gz", &sql)?;
                    s.import(&file, true, &mut |_| true)?;
                }
                (switched, settings)
            }
            Level::Everything => {
                if r.wp.prefix() != s.db.prefix {
                    return Err(fail("this copy and the live site use different table prefixes; push at the Code or Settings level"));
                }
                say(
                    progress,
                    Stage::Loading,
                    "Putting together the new live database…",
                )?;
                let scratch = format!("sy_push_{}_{}", env::now(), std::process::id());
                let built = (|| -> Result<PathBuf> {
                    let live_dump = r.work.join("live.sql.gz");
                    let out = std::fs::File::create(&live_dump).map_err(io(&live_dump))?;
                    let mut gz = flate2::write::GzEncoder::new(out, flate2::Compression::fast());
                    std::io::copy(&mut live::Concat::new(parts.clone()), &mut gz)
                        .map_err(io(&live_dump))?;
                    gz.finish().map_err(io(&live_dump))?;
                    client.load(&scratch, &live_dump)?;
                    let _ = std::fs::remove_file(&live_dump);
                    let kept_names = kept_live_options(home, &r.wp);
                    let kept = local_option_values(&client, &scratch, &s.db.prefix, &kept_names)?;
                    let base_users = load_base(home, domain)
                        .and_then(|b| b.local_facts)
                        .map_or(0, |f| f.max_user_id);
                    notes.extend(merge::everything_between(
                        home,
                        &client,
                        &s.db.prefix,
                        &local_db,
                        &scratch,
                        base_users,
                        &rules,
                    )?);
                    let table = ident(&format!("{}options", s.db.prefix));
                    let mut fix = String::new();
                    for (name, (value, autoload)) in &kept {
                        fix.push_str(&format!(
                            "INSERT INTO {table} (option_name, option_value, autoload) VALUES ({}, {}, {}) ON DUPLICATE KEY UPDATE option_value = VALUES(option_value);\n",
                            literal(name),
                            literal(&String::from_utf8_lossy(value)),
                            literal(autoload)
                        ));
                    }
                    fix.push_str(&format!(
                        "DELETE FROM {table} WHERE option_name LIKE 'stackyard\\_%';\n"
                    ));
                    client.execute(&scratch, &fix)?;
                    let dump = r.work.join("everything.sql.gz");
                    client.dump_portable(&scratch, &dump)?;
                    let ready = r.work.join("everything-portable.sql.gz");
                    portable(&dump, &ready)?;
                    let _ = std::fs::remove_file(&dump);
                    Ok(ready)
                })();
                let _ = client.drop(&scratch);
                let ready = built?;
                say(
                    progress,
                    Stage::Loading,
                    "Loading the new database on the live site…",
                )?;
                s.import(&ready, false, &mut |at| {
                    progress(Progress {
                        stage: Stage::Loading,
                        message: "Loading the new database on the live site…".into(),
                        done: at,
                        total: 0,
                    })
                })?;
                notes.push("Live users, orders, customers and payment settings were kept.".into());
                (0, 0)
            }
        };
        Ok(done)
    })();

    let (switches, settings) = match applied {
        Ok(done) => done,
        Err(e) => {
            say(
                progress,
                Stage::Finishing,
                "Something went wrong; putting the backup back…",
            )
            .ok();
            let put = put_back(&r, &dir, &backup, &mut |_| true);
            let _ = s.maintenance(false, &r.work);
            return Err(match put {
                Ok(()) => fail(format!("the push failed ({e}); the live site was put back as it was")),
                Err(again) => fail(format!(
                    "the push failed ({e}) and putting the backup back failed too ({again}); the backup is in {}",
                    dir.display()
                )),
            });
        }
    };
    say(
        progress,
        Stage::Finishing,
        "Switching maintenance mode off…",
    )?;
    s.maintenance(false, &r.work)?;

    say(
        progress,
        Stage::Finishing,
        "Checking the live site answers…",
    )?;
    let site_answers = answers(&s.http, &r.live.url);
    if !site_answers {
        notes.push("The live home page or login page did not answer as expected after the push. Open the live site; if it is broken, put the backup back.".into());
    }

    // In step again: the new base.
    say(progress, Stage::Finishing, "Noting where both sides stand…")?;
    let live_files = s.files(media_from_live(&r), &mut |_| true)?;
    let live_facts = s.facts().ok();
    let content = merge::content_rel(&r.wp);
    let base = advance_base(
        load_base(home, domain),
        choice,
        Now {
            live_files,
            local_files: compared.local_files,
            live_facts,
            local_facts: Some(compared.local_facts),
        },
        &content,
    );
    save_base(home, domain, &base)?;
    mark_synced(home, domain, &r.env)?;
    Ok(Pushed {
        backup: id,
        files: choice.files.len(),
        switches,
        settings,
        site_answers,
        notes,
    })
}

fn dir_size(dir: &Path) -> u64 {
    super::copy::tree_size(dir, &[])
}

/// The home page and the login page answer without a server error.
fn answers(http: &ureq::Agent, url: &str) -> bool {
    let ok = |u: &str| {
        http.get(u).call().is_ok_and(|mut r| {
            let status = r.status().as_u16();
            let body = r
                .body_mut()
                .with_config()
                .limit(256 * 1024)
                .read_to_string()
                .unwrap_or_default();
            status < 500 && !body.contains("Error establishing a database connection")
        })
    };
    let base = url.trim_end_matches('/');
    ok(&format!("{base}/")) && ok(&format!("{base}/wp-login.php"))
}

/// Both sides as they are after a push or pull.
struct Now {
    live_files: Manifest,
    local_files: Manifest,
    live_facts: Option<Facts>,
    local_facts: Option<Facts>,
}

/// The new base after a push or pull: only what moved is in step again,
/// so changes left behind on either side still show next time. With no
/// base yet, or after an Everything move, both sides are taken whole.
fn advance_base(old: Option<Base>, choice: &Choice, now: Now, content: &str) -> Base {
    let whole = choice.level == Level::Everything;
    let Some(mut base) = old.filter(|b| b.live_facts.is_some() && b.local_facts.is_some()) else {
        let mut base = Base {
            at: env::now(),
            live_files: now.live_files,
            local_files: now.local_files,
            live_facts: now.live_facts,
            local_facts: now.local_facts,
        };
        base.live_files.retain(|rel, _| !left_out(rel, content));
        return base;
    };
    base.at = env::now();
    for rel in &choice.files {
        for (side, current) in [
            (&mut base.live_files, &now.live_files),
            (&mut base.local_files, &now.local_files),
        ] {
            match current.get(rel) {
                Some(entry) => side.insert(rel.clone(), entry.clone()),
                None => side.remove(rel),
            };
        }
    }
    let (Some(live), Some(local)) = (now.live_facts, now.local_facts) else {
        return base;
    };
    if whole {
        base.live_facts = Some(live);
        base.local_facts = Some(local);
        return base;
    }
    for (side, current) in [
        (base.live_facts.as_mut(), &live),
        (base.local_facts.as_mut(), &local),
    ] {
        let Some(side) = side else { continue };
        for name in &choice.settings {
            match current.options.get(name) {
                Some(hash) => side.options.insert(name.clone(), hash.clone()),
                None => side.options.remove(name),
            };
        }
        for id in &choice.switches {
            if let Some(slug) = id.strip_prefix("plugin:") {
                side.active_plugins
                    .retain(|p| merge::plugin_slug(p) != slug);
                side.active_plugins.extend(
                    current
                        .active_plugins
                        .iter()
                        .filter(|p| merge::plugin_slug(p) == slug)
                        .cloned(),
                );
            } else if id.starts_with("theme:") {
                side.stylesheet.clone_from(&current.stylesheet);
                side.template.clone_from(&current.template);
            }
        }
    }
    base
}

fn mark_synced(home: &Path, domain: &str, environment: &Environment) -> Result<()> {
    let mut environment = environment.clone();
    if let Some(live) = &mut environment.live {
        live.synced_at = env::now();
    }
    env::save(home, domain, &environment)
}

// ---------------------------------------------------------------------------
// Pull

/// What a pull did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Pulled {
    /// The copy's snapshot from before, to undo the pull.
    pub snapshot: String,
    pub files: usize,
    pub switches: usize,
    pub settings: usize,
    pub notes: Vec<String>,
}

/// Brings the picked live changes into the copy.
pub fn pull(
    home: &Path,
    sessions: &Sessions,
    domain: &str,
    choice: &Choice,
    progress: OnProgress,
) -> Result<Pulled> {
    let r = reach(home, sessions, domain, false, progress)?;
    let compared = compare(home, &r, Way::Pull, progress)?;
    check_choice(&compared.changes, choice)?;
    let s = &r.server;
    let rules = rules(&r, home, Way::Pull);
    let client = Client::open(home)?;
    let local_db = r.wp.database()?;
    let mut notes = Vec::new();
    let mut snap = snapshot::take(home, &r.wp, "Pull from live")?;

    let count = choice.files.len() as u64;
    for (i, rel) in choice.files.iter().enumerate() {
        if !progress(Progress {
            stage: Stage::Files,
            message: format!("Downloading {rel}"),
            done: i as u64,
            total: count,
        }) {
            return Err(stopped());
        }
        let local = r.site.path.join(rel);
        if local.is_file() {
            snap.save_file(home, &r.site.path, rel)?;
        } else {
            snap.note_added(home, rel)?;
        }
        if compared.live_files.contains_key(rel) {
            s.sessions.download(
                &s.connection,
                &remote::join(&s.root, rel),
                &local,
                &mut |_| true,
            )?;
        } else if local.is_file() {
            std::fs::remove_file(&local).map_err(io(&local))?;
        }
    }

    let mut switches = 0;
    let mut settings = 0;
    match choice.level {
        Level::Code | Level::Settings => {
            for id in &choice.switches {
                let Some(sw) = compared.changes.switches.iter().find(|sw| &sw.id == id) else {
                    continue;
                };
                let (kind, name) = match id.split_once(':') {
                    Some(("plugin", name)) => (Kind::Plugin, name),
                    Some(("theme", name)) => (Kind::Theme, name),
                    _ => continue,
                };
                let action = if sw.on {
                    ExtensionAction::Activate
                } else {
                    ExtensionAction::Deactivate
                };
                match super::change_extension(home, &r.wp, kind, name, action) {
                    Ok(()) => switches += 1,
                    Err(e) => notes.push(format!("{name} could not be switched: {e}")),
                }
            }
            if choice.level == Level::Settings && !choice.settings.is_empty() {
                say(progress, Stage::Database, "Copying the settings…")?;
                let values = s.option_values(&choice.settings)?;
                let table = ident(&r.wp.table("options"));
                let sql = options_sql(&table, &choice.settings, &values, &rules);
                client.execute(&local_db, &sql)?;
                settings = choice.settings.len();
            }
        }
        Level::Everything => {
            let dir = r.work.join("database");
            let parts = s.backup_database(&r.info.tables, &dir, progress)?;
            say(
                progress,
                Stage::Loading,
                "Changing addresses and loading the database…",
            )?;
            let load = r.work.join("load.sql.gz");
            let out = std::fs::File::create(&load).map_err(io(&load))?;
            let mut gz = flate2::write::GzEncoder::new(out, flate2::Compression::fast());
            replace::rewrite(live::Concat::new(parts), &mut gz, &rules)
                .map_err(|e| fail(format!("could not prepare the database: {e}")))?;
            gz.finish().map_err(io(&load))?;
            client.load(&local_db, &load)?;
            if r.wp.wp_content().join("plugins/woocommerce").is_dir() {
                notes.extend(live::payments_to_test_mode(home, &r.wp));
            }
        }
    }
    let _ = super::muplugin::install(&r.wp, &super::muplugin::Options::for_site(home, &r.wp));

    say(progress, Stage::Finishing, "Noting where both sides stand…")?;
    let content = merge::content_rel(&r.wp);
    let base = advance_base(
        load_base(home, domain),
        choice,
        Now {
            live_files: compared.live_files,
            local_files: snapshot::manifest_with(&r.site.path, &compared.local_files)?,
            live_facts: Some(compared.live_facts),
            local_facts: snapshot::read_facts(&client, &local_db, r.wp.prefix()).ok(),
        },
        &content,
    );
    save_base(home, domain, &base)?;
    mark_synced(home, domain, &r.env)?;
    Ok(Pulled {
        snapshot: snap.id,
        files: choice.files.len(),
        switches,
        settings,
        notes,
    })
}

// ---------------------------------------------------------------------------
// Single posts and pages

/// Live posts and pages, read from a fresh copy of the live database kept
/// on this computer for picking (see [`read_live`]).
pub fn live_posts(home: &Path, domain: &str, search: &str) -> Result<Vec<posts::PostSummary>> {
    let side = live_side(home, domain)?;
    posts::list_side(home, &side, search)
}

fn mirror_name(domain: &str) -> String {
    format!("sy_live_{}", domain.replace(['.', '-'], "_"))
}

/// Downloads the live database into a copy on this computer, to pick live
/// posts from and plan moves against. It is replaced each time.
pub fn read_live(
    home: &Path,
    sessions: &Sessions,
    domain: &str,
    progress: OnProgress,
) -> Result<u64> {
    let r = reach(home, sessions, domain, false, progress)?;
    let dir = r.work.join("database");
    let parts = r.server.backup_database(&r.info.tables, &dir, progress)?;
    say(progress, Stage::Loading, "Loading the live database here…")?;
    let load = r.work.join("live.sql.gz");
    let out = std::fs::File::create(&load).map_err(io(&load))?;
    let mut gz = flate2::write::GzEncoder::new(out, flate2::Compression::fast());
    std::io::copy(&mut live::Concat::new(parts), &mut gz).map_err(io(&load))?;
    gz.finish().map_err(io(&load))?;
    let client = Client::open(home)?;
    let name = mirror_name(domain);
    let _ = client.drop(&name);
    client.load(&name, &load)?;
    let at = env::now();
    let marker = home
        .join("wordpress")
        .join("sync")
        .join(format!("{domain}.live-read"));
    if let Some(dir) = marker.parent() {
        std::fs::create_dir_all(dir).map_err(io(dir))?;
    }
    let text = serde_json::to_string(&(at, &r.live.url, &r.info.abspath, &r.server.db.prefix))
        .map_err(|e| fail(e.to_string()))?;
    std::fs::write(&marker, text).map_err(io(&marker))?;
    Ok(at)
}

/// When the live database was last read with [`read_live`].
pub fn live_read_at(home: &Path, domain: &str) -> Option<u64> {
    read_marker(home, domain).map(|m| m.0)
}

fn read_marker(home: &Path, domain: &str) -> Option<(u64, String, String, String)> {
    let marker = home
        .join("wordpress")
        .join("sync")
        .join(format!("{domain}.live-read"));
    serde_json::from_slice(&std::fs::read(marker).ok()?).ok()
}

fn live_side(home: &Path, domain: &str) -> Result<posts::Side> {
    let (_, url, abspath, prefix) =
        read_marker(home, domain).ok_or_else(|| fail("read the live site's posts first"))?;
    Ok(posts::Side {
        label: "the live site".into(),
        database: mirror_name(domain),
        prefix,
        url,
        root: abspath,
        uploads: None,
        shared_uploads: None,
    })
}

/// Plans moving posts between the copy and the live site, through the
/// copy of the live database read last.
pub fn plan_posts(
    home: &Path,
    domain: &str,
    way: Way,
    choice: &posts::MoveChoice,
) -> Result<posts::MovePlan> {
    let here = posts::side_of(home, domain)?;
    let there = live_side(home, domain)?;
    match way {
        Way::Push => posts::plan_sides(home, &here, &there, choice),
        Way::Pull => posts::plan_sides(home, &there, &here, choice),
    }
}

/// What moving posts to or from the live site did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PostsMoved {
    pub posts: usize,
    pub files: usize,
    pub warnings: Vec<posts::Warning>,
    /// A pull's snapshot of the copy, or a push's backup of the live site.
    pub undo: String,
}

/// Moves the picked posts to or from the live site. A push reads the live
/// database again first, under maintenance mode, so the numbers it gives
/// out are free.
pub fn move_posts(
    home: &Path,
    sessions: &Sessions,
    domain: &str,
    way: Way,
    choice: &posts::MoveChoice,
    progress: OnProgress,
) -> Result<PostsMoved> {
    match way {
        Way::Pull => {
            let r = reach(home, sessions, domain, false, progress)?;
            let plan = plan_posts(home, domain, Way::Pull, choice)?;
            let mut snap = snapshot::take(home, &r.wp, "Posts pulled from live")?;
            for rel in plan.files.iter() {
                let rel = format!("{}/uploads/{rel}", merge::content_rel(&r.wp));
                let local = r.site.path.join(&rel);
                if local.is_file() {
                    continue;
                }
                snap.note_added(home, &rel)?;
                if let Err(e) = r.server.sessions.download(
                    &r.server.connection,
                    &remote::join(&r.server.root, &rel),
                    &local,
                    &mut |_| true,
                ) {
                    return Err(fail(format!("could not download {rel}: {e}")));
                }
            }
            let client = Client::open(home)?;
            client.execute(&r.wp.database()?, plan.sql())?;
            Ok(PostsMoved {
                posts: plan.counts.posts,
                files: plan.files.len(),
                warnings: plan.warnings,
                undo: snap.id,
            })
        }
        Way::Push => {
            let r = reach(home, sessions, domain, true, progress)?;
            let s = &r.server;
            let id = format!("{}", env::now());
            let dir = backups_dir(home, domain).join(&id);
            s.maintenance(true, &r.work)?;
            let result = (|| -> Result<PostsMoved> {
                // A fresh read, so new numbers don't clash with orders taken meanwhile.
                let parts = s.backup_database(&r.info.tables, &dir.join("database"), progress)?;
                let mut backup = LiveBackup {
                    id: id.clone(),
                    domain: domain.to_string(),
                    created_at: env::now(),
                    database: parts
                        .iter()
                        .filter_map(|p| p.strip_prefix(&dir).ok())
                        .map(|p| p.to_string_lossy().replace('\\', "/"))
                        .collect(),
                    ..LiveBackup::default()
                };
                let load = r.work.join("live.sql.gz");
                let out = std::fs::File::create(&load).map_err(io(&load))?;
                let mut gz = flate2::write::GzEncoder::new(out, flate2::Compression::fast());
                std::io::copy(&mut live::Concat::new(parts), &mut gz).map_err(io(&load))?;
                gz.finish().map_err(io(&load))?;
                let client = Client::open(home)?;
                let mirror = mirror_name(domain);
                let _ = client.drop(&mirror);
                client.load(&mirror, &load)?;
                let plan = plan_posts(home, domain, Way::Push, choice)?;
                let content = merge::content_rel(&r.wp);
                for rel in &plan.files {
                    let rel = format!("{content}/uploads/{rel}");
                    let local = r.site.path.join(&rel);
                    if local.is_file() {
                        s.sessions.upload_into(
                            &s.connection,
                            &local,
                            &remote::join(&s.root, &rel),
                        )?;
                        backup.added.push(rel);
                    }
                }
                backup.bytes = dir_size(&dir);
                save_backup_info(&dir, &backup)?;
                let file = sql_file(&r.work, "posts.sql.gz", plan.sql())?;
                if let Err(e) = s.import(&file, true, &mut |_| true) {
                    let _ = put_back(&r, &dir, &backup, &mut |_| true);
                    return Err(fail(format!(
                        "moving the posts failed ({e}); the live site was put back as it was"
                    )));
                }
                Ok(PostsMoved {
                    posts: plan.counts.posts,
                    files: plan.files.len(),
                    warnings: plan.warnings,
                    undo: id.clone(),
                })
            })();
            let _ = s.maintenance(false, &r.work);
            result
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(size: u64, modified: i64, hash: Option<&str>) -> FileEntry {
        FileEntry {
            size,
            modified,
            hash: hash.map(str::to_string),
        }
    }

    fn files(items: &[(&str, FileEntry)]) -> Manifest {
        items
            .iter()
            .map(|(p, e)| (p.to_string(), e.clone()))
            .collect()
    }

    #[test]
    fn lists_what_changed_on_each_side() {
        let base = Base {
            at: 1,
            local_files: files(&[
                ("wp-content/themes/a/style.css", entry(10, 100, Some("x"))),
                ("wp-content/themes/a/header.php", entry(20, 100, Some("h"))),
                ("index.php", entry(5, 100, Some("i"))),
            ]),
            live_files: files(&[
                ("wp-content/themes/a/style.css", entry(10, 100, None)),
                ("wp-content/themes/a/header.php", entry(20, 100, None)),
                ("index.php", entry(5, 100, None)),
            ]),
            ..Base::default()
        };
        let local = files(&[
            ("wp-content/themes/a/style.css", entry(11, 200, Some("y"))),
            ("wp-content/themes/a/header.php", entry(21, 200, Some("h2"))),
            ("wp-content/plugins/new/new.php", entry(3, 200, Some("n"))),
            ("wp-config.php", entry(9, 200, Some("c"))),
        ]);
        let live = files(&[
            ("wp-content/themes/a/style.css", entry(10, 100, None)),
            ("wp-content/themes/a/header.php", entry(25, 300, None)),
            ("index.php", entry(5, 100, None)),
            ("wp-content/uploads/2026/10/a.jpg", entry(7, 300, None)),
        ]);
        let push = file_list(Way::Push, Some(&base), &local, &live, "wp-content", false);
        let got: Vec<(&str, Change, bool)> = push
            .iter()
            .map(|f| (f.path.as_str(), f.change, f.conflict))
            .collect();
        assert_eq!(
            got,
            vec![
                ("index.php", Change::Removed, false),
                ("wp-content/plugins/new/new.php", Change::Added, false),
                ("wp-content/themes/a/header.php", Change::Changed, true),
                ("wp-content/themes/a/style.css", Change::Changed, false),
            ]
        );
        let pull = file_list(Way::Pull, Some(&base), &local, &live, "wp-content", false);
        let got: Vec<(&str, Change, bool)> = pull
            .iter()
            .map(|f| (f.path.as_str(), f.change, f.conflict))
            .collect();
        assert_eq!(
            got,
            vec![
                ("wp-content/themes/a/header.php", Change::Changed, true),
                ("wp-content/uploads/2026/10/a.jpg", Change::Added, false),
            ]
        );
        // Media left on the live site is not pulled.
        let pull = file_list(Way::Pull, Some(&base), &local, &live, "wp-content", true);
        assert!(pull.iter().all(|f| f.group != "media"));
        // Without a base, differences are offered as conflicts, never removals.
        let none = file_list(Way::Push, None, &local, &live, "wp-content", false);
        assert!(none
            .iter()
            .all(|f| f.conflict && f.change != Change::Removed));
        assert!(none
            .iter()
            .any(|f| f.path == "wp-content/plugins/new/new.php"));
    }

    #[test]
    fn compares_files_across_sides() {
        let a = entry(10, 100, Some("x"));
        assert!(same_across(Some(&a), Some(&entry(10, 101, None)), false));
        assert!(!same_across(Some(&a), Some(&entry(10, 0, None)), false));
        assert!(same_across(Some(&a), Some(&entry(10, 0, None)), true));
        assert!(!same_across(Some(&a), Some(&entry(11, 100, None)), true));
    }

    #[test]
    fn switches_plugins_in_the_live_list() {
        let from = Facts {
            active_plugins: vec![
                "fast-checkout/fast.php".into(),
                "woocommerce/woocommerce.php".into(),
            ],
            ..Facts::default()
        };
        let on = Switch {
            id: "plugin:fast-checkout".into(),
            title: "fast-checkout".into(),
            on: true,
            conflict: false,
        };
        let off = Switch {
            id: "plugin:hello".into(),
            title: "hello".into(),
            on: false,
            conflict: false,
        };
        let list = switched_plugins(
            &["hello.php".into(), "woocommerce/woocommerce.php".into()],
            &from,
            &[&on, &off],
        );
        assert_eq!(
            list,
            vec!["fast-checkout/fast.php", "woocommerce/woocommerce.php"]
        );
        assert_eq!(
            serialize_list(&list),
            r#"a:2:{i:0;s:22:"fast-checkout/fast.php";i:1;s:27:"woocommerce/woocommerce.php";}"#
        );
    }

    #[test]
    fn makes_dumps_portable() {
        assert_eq!(
            portable_line(") ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_uca1400_ai_ci;"),
            ") ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;"
        );
        assert_eq!(
            portable_line("  `a` text COLLATE utf8mb4_0900_ai_ci,"),
            "  `a` text COLLATE utf8mb4_unicode_ci,"
        );
        assert!(version_less("6.4.2", "6.5"));
        assert!(!version_less("6.5", "6.5"));
    }

    /// Clones the stand-in live server (`STACKYARD_TEST_LIVE`, as for the
    /// clone test), then pushes and pulls files, settings, plugins, the
    /// whole database and single posts, and puts a backup back.
    #[test]
    #[ignore = "needs a live server in STACKYARD_TEST_LIVE"]
    fn push_and_pull_with_a_live_server() {
        use super::super::cli::Load;
        use super::super::live::{clone, CloneOptions, Source};

        let Ok(spec) = std::env::var("STACKYARD_TEST_LIVE") else {
            return;
        };
        let parts: Vec<&str> = spec.split('|').collect();
        let home = crate::paths::home();
        let sessions = Sessions::default();
        let draft = remote::draft(remote::ConnectionInput {
            protocol: Some(remote::Protocol::Sftp),
            host: parts[1].into(),
            port: parts[2].parse().ok(),
            user: parts[3].into(),
            ..Default::default()
        })
        .unwrap();
        sessions.login(&draft, Some(parts[4].to_string())).unwrap();
        let name = "e2e-sync";
        // A run that failed half way left its copy behind.
        let stale = SiteStore::load_from(&home.join("sites.toml"))
            .unwrap()
            .list()
            .iter()
            .find(|s| s.name == name)
            .cloned();
        if let Some(site) = stale {
            forget_test_site(&home, &site);
        }
        let cloned = clone(
            &home,
            &sessions,
            &Source::Server(Box::new(draft)),
            &CloneOptions {
                name: name.into(),
                https: false,
                ..CloneOptions::default()
            },
            &mut |_| true,
        )
        .unwrap();
        let domain = cloned.site.domain();
        let wp = WpSite::open(&cloned.site).unwrap();
        let client = Client::open(&home).unwrap();
        let live_db = "live_blog";
        let live_value = |sql: &str| -> String {
            client
                .rows(live_db, sql)
                .unwrap()
                .rows
                .first()
                .and_then(|r| r.first().cloned().flatten())
                .unwrap_or_default()
        };
        let connection_id = env::load(&home, &domain)
            .unwrap()
            .unwrap()
            .live
            .unwrap()
            .connection
            .unwrap();
        let connection = RemoteStore::load()
            .unwrap()
            .get(&connection_id)
            .unwrap()
            .clone();
        let live_root = env::load(&home, &domain)
            .unwrap()
            .unwrap()
            .live
            .unwrap()
            .path;
        let run = |args: &[&str]| wp.wp(&home, Load::Core, args).unwrap().trim().to_string();
        let go = &mut |_: Progress| true;

        // Just cloned: in step.
        let base = load_base(&home, &domain).expect("a base after the clone");
        assert!(base.live_facts.is_some() && base.local_facts.is_some());
        let push = changes(&home, &sessions, &domain, Way::Push, go).unwrap();
        assert!(push.files.is_empty(), "{:?}", push.files);
        assert!(push.settings.is_empty(), "{:?}", push.settings);
        assert!(!push.live_content_changed);

        // Here: a new plugin, switched on, and a setting. There: a new file.
        let plugin = wp.wp_content().join("plugins/sync-check");
        std::fs::create_dir_all(&plugin).unwrap();
        std::fs::write(
            plugin.join("sync-check.php"),
            "<?php\n/*\nPlugin Name: Sync check\n*/\n",
        )
        .unwrap();
        run(&["plugin", "activate", "sync-check"]);
        run(&[
            "option",
            "update",
            "blogdescription",
            "Pushed from Stackyard",
        ]);
        let note = crate::testutil::tempdir().join("live-note.txt");
        std::fs::write(&note, "made on the live site").unwrap();
        sessions
            .upload_into(
                &connection,
                &note,
                &remote::join(&live_root, "wp-content/live-note.txt"),
            )
            .unwrap();

        let push = changes(&home, &sessions, &domain, Way::Push, go).unwrap();
        let plugin_file = "wp-content/plugins/sync-check/sync-check.php";
        assert!(
            push.files
                .iter()
                .any(|f| f.path == plugin_file && !f.conflict),
            "{:?}",
            push.files
        );
        assert!(push
            .files
            .iter()
            .all(|f| f.path != "wp-content/live-note.txt"));
        assert!(push.settings.iter().any(|c| c.name == "blogdescription"));
        assert!(push
            .switches
            .iter()
            .any(|s| s.id == "plugin:sync-check" && s.on));
        let incoming = changes(&home, &sessions, &domain, Way::Pull, go).unwrap();
        assert!(
            incoming
                .files
                .iter()
                .any(|f| f.path == "wp-content/live-note.txt"),
            "{:?}",
            incoming.files
        );

        // Push at the Settings level.
        let pushed = push_once(
            &home,
            &sessions,
            &domain,
            &Choice {
                level: Level::Settings,
                files: vec![plugin_file.into()],
                switches: vec!["plugin:sync-check".into()],
                settings: vec!["blogdescription".into()],
            },
            "",
        );
        assert!(pushed.site_answers, "{:?}", pushed.notes);
        assert_eq!(
            live_value("SELECT option_value FROM wp_options WHERE option_name = 'blogdescription'"),
            "Pushed from Stackyard"
        );
        assert!(live_value(
            "SELECT option_value FROM wp_options WHERE option_name = 'active_plugins'"
        )
        .contains("sync-check/sync-check.php"));
        assert_eq!(
            live_value("SELECT option_value FROM wp_options WHERE option_name = 'home'"),
            cloned.live
        );
        assert_eq!(backups(&home, &domain).len(), 1);
        let push = changes(&home, &sessions, &domain, Way::Push, go).unwrap();
        assert!(
            push.files.is_empty() && push.settings.is_empty(),
            "{push:?}"
        );

        // Pull the live file.
        let pulled = pull(
            &home,
            &sessions,
            &domain,
            &Choice {
                level: Level::Code,
                files: vec!["wp-content/live-note.txt".into()],
                ..Default::default()
            },
            go,
        )
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(wp.wp_content().join("live-note.txt")).unwrap(),
            "made on the live site"
        );
        assert!(snapshot::get(&home, &domain, &pulled.snapshot).is_ok());

        // Everything: a post made here goes up, an order taken there stays.
        run(&[
            "post",
            "create",
            "--post_title=Pushed whole",
            "--post_name=pushed-whole",
            "--post_status=publish",
        ]);
        client
            .execute(
                live_db,
                "INSERT INTO wp_posts (post_author, post_date, post_date_gmt, post_content, post_title, post_excerpt, post_status, post_name, to_ping, pinged, post_modified, post_modified_gmt, post_content_filtered, post_type) VALUES (1, NOW(), NOW(), '', 'Order', '', 'wc-processing', 'order-live', '', '', NOW(), NOW(), '', 'shop_order');",
            )
            .unwrap();
        let order = live_value("SELECT MAX(ID) FROM wp_posts WHERE post_type = 'shop_order'");
        assert!(push_everything_needs_the_name(&home, &sessions, &domain));
        let pushed = push_once(
            &home,
            &sessions,
            &domain,
            &Choice {
                level: Level::Everything,
                ..Default::default()
            },
            name,
        );
        assert!(pushed.site_answers, "{:?}", pushed.notes);
        assert_eq!(
            live_value("SELECT COUNT(*) FROM wp_posts WHERE post_name = 'pushed-whole'"),
            "1"
        );
        assert_eq!(
            live_value(&format!(
                "SELECT post_type FROM wp_posts WHERE ID = {order}"
            )),
            "shop_order"
        );
        assert_eq!(
            live_value("SELECT option_value FROM wp_options WHERE option_name = 'home'"),
            cloned.live
        );
        assert_eq!(
            live_value("SELECT COUNT(*) FROM wp_options WHERE option_name LIKE 'stackyard\\_%'"),
            "0"
        );
        // The backup puts the live site back.
        restore_backup(&home, &sessions, &domain, &pushed.backup, go).unwrap();
        assert_eq!(
            live_value("SELECT COUNT(*) FROM wp_posts WHERE post_name = 'pushed-whole'"),
            "0"
        );

        // Single posts: one up, one down.
        read_live(&home, &sessions, &domain, go).unwrap();
        let mine: u64 = run(&[
            "post",
            "list",
            "--name=pushed-whole",
            "--field=ID",
            "--post_type=post",
        ])
        .parse()
        .unwrap();
        let up = posts::MoveChoice {
            ids: vec![mine],
            ..Default::default()
        };
        let moved = move_posts(&home, &sessions, &domain, Way::Push, &up, go).unwrap();
        assert_eq!(moved.posts, 1);
        assert_eq!(
            live_value("SELECT COUNT(*) FROM wp_posts WHERE post_name = 'pushed-whole'"),
            "1"
        );
        client
            .execute(
                live_db,
                "INSERT INTO wp_posts (post_author, post_date, post_date_gmt, post_content, post_title, post_excerpt, post_status, post_name, to_ping, pinged, post_modified, post_modified_gmt, post_content_filtered, post_type) VALUES (1, NOW(), NOW(), 'Written there', 'Written live', '', 'publish', 'written-live', '', '', NOW(), NOW(), '', 'post');",
            )
            .unwrap();
        read_live(&home, &sessions, &domain, go).unwrap();
        let found = live_posts(&home, &domain, "written live").unwrap();
        assert_eq!(found.len(), 1);
        let down = posts::MoveChoice {
            ids: vec![found[0].id],
            ..Default::default()
        };
        move_posts(&home, &sessions, &domain, Way::Pull, &down, go).unwrap();
        assert_eq!(
            run(&[
                "post",
                "list",
                "--name=written-live",
                "--field=post_title",
                "--post_type=post"
            ]),
            "Written live"
        );

        // Pull everything: the live database, with local addresses.
        pull(
            &home,
            &sessions,
            &domain,
            &Choice {
                level: Level::Everything,
                ..Default::default()
            },
            go,
        )
        .unwrap();
        assert_eq!(run(&["option", "get", "home"]), cloned.url);
        assert_eq!(
            run(&["option", "get", "blogdescription"]),
            "Pushed from Stackyard"
        );

        // Clean up, here and there.
        let _ = sessions.remove_file(
            &connection,
            &remote::join(&live_root, "wp-content/live-note.txt"),
        );
        let _ = sessions.remove_file(&connection, &remote::join(&live_root, plugin_file));
        forget_test_site(&home, &cloned.site);
    }

    /// Removes the test's copy and all it left behind.
    fn forget_test_site(home: &Path, site: &Site) {
        let domain = site.domain();
        for b in backups(home, &domain) {
            let _ = remove_backup(home, &domain, &b.id);
        }
        let client = Client::open(home).unwrap();
        if let Ok(wp) = WpSite::open(site) {
            if let Ok(db) = wp.database() {
                let _ = client.drop(&db);
            }
        }
        let _ = client.drop(&mirror_name(&domain));
        let mut store = SiteStore::load_from(&home.join("sites.toml")).unwrap();
        let _ = store.remove(&domain);
        store.save().unwrap();
        let _ = std::fs::remove_dir_all(&site.path);
        env::forget(home, &domain);
        forget_base(home, &domain);
        let mut remotes = RemoteStore::load().unwrap();
        remotes.forget_site(&domain);
        remotes.save().unwrap();
    }

    fn push_once(
        home: &Path,
        sessions: &Sessions,
        domain: &str,
        choice: &Choice,
        confirm: &str,
    ) -> Pushed {
        push(home, sessions, domain, choice, confirm, &mut |_| true).unwrap()
    }

    fn push_everything_needs_the_name(home: &Path, sessions: &Sessions, domain: &str) -> bool {
        let choice = Choice {
            level: Level::Everything,
            ..Default::default()
        };
        push(home, sessions, domain, &choice, "wrong", &mut |_| true)
            .is_err_and(|e| e.to_string().contains("to confirm"))
    }
}
