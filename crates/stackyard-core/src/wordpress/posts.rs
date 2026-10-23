//! Moving single posts, pages and products from one copy of a site to
//! another, with new numbers (IDs) in the target and every reference to
//! them changed to match.
//!
//! What comes along with a post: its meta, categories, tags and other
//! terms (made in the target when missing), its featured image and the
//! images and files its content and meta point at (rows and files), a
//! product's variations, reusable blocks it uses, menu items that point at
//! it (in menus the target also has), and, when asked, its comments.
//!
//! References are changed where Stackyard knows their format: the block
//! editor's attributes (`"id"`, `"mediaId"`, `"ids"`, `"ref"`) and
//! `wp-image-N` classes, the `[gallery]` shortcode and WPBakery image
//! attributes, Elementor's image data, WooCommerce galleries, grouped
//! products, upsells and cross-sells, Yoast SEO and Rank Math primary
//! terms, and ACF image, file, gallery, post and term fields. Where a
//! post holds numbers Stackyard can't change safely (another page-builder
//! template, a post that isn't being moved), the preview says so for that
//! post instead of guessing.
//!
//! A snapshot of the target is taken first, so the move can be undone.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{env, php, replace, snapshot, WpSite};
use crate::databases::{ident, literal, Client, ResultSet};
use crate::sites::SiteStore;
use crate::{Error, Result};

fn fail(message: impl Into<String>) -> Error {
    Error::WordPress(message.into())
}

fn io(path: &Path) -> impl FnOnce(std::io::Error) -> Error + '_ {
    move |source| Error::Io {
        path: path.to_owned(),
        source,
    }
}

// ---------------------------------------------------------------------------
// References inside values

/// Changes the numbers in a comma-separated list, e.g. a product gallery.
/// `map` returns the new number, or `None` to leave the number out.
pub fn map_csv(value: &str, map: &mut dyn FnMut(u64) -> Option<u64>) -> String {
    value
        .split(',')
        .filter_map(|part| {
            let part = part.trim();
            match part.parse::<u64>() {
                Ok(id) => map(id).map(|n| n.to_string()),
                Err(_) if part.is_empty() => None,
                Err(_) => Some(part.to_string()),
            }
        })
        .collect::<Vec<_>>()
        .join(",")
}

/// The numbers in a comma-separated list.
pub fn ids_in_csv(value: &str) -> Vec<u64> {
    value
        .split(',')
        .filter_map(|p| p.trim().parse().ok())
        .collect()
}

/// Changes the numbers in a serialized array of numbers (WooCommerce
/// upsells, ACF galleries and relationships); `None` leaves one out.
pub fn map_serialized(value: &str, map: &mut dyn FnMut(u64) -> Option<u64>) -> Option<String> {
    let php::Value::Array(items) = php::unserialize(value.as_bytes())? else {
        return None;
    };
    let mut out = Vec::new();
    for (_, v) in items {
        let Some(id) = v.as_int().and_then(|n| u64::try_from(n).ok()) else {
            out.push(v);
            continue;
        };
        if let Some(new) = map(id) {
            out.push(match v {
                php::Value::Int(_) => php::Value::Int(new as i64),
                _ => php::Value::str(&new.to_string()),
            });
        }
    }
    let array = php::Value::Array(
        out.into_iter()
            .enumerate()
            .map(|(i, v)| (php::Value::Int(i as i64), v))
            .collect(),
    );
    Some(String::from_utf8_lossy(&array.serialize()).into_owned())
}

/// The numbers in a serialized array of numbers.
pub fn ids_in_serialized(value: &str) -> Vec<u64> {
    match php::unserialize(value.as_bytes()) {
        Some(php::Value::Array(items)) => items
            .iter()
            .filter_map(|(_, v)| v.as_int().and_then(|n| u64::try_from(n).ok()))
            .collect(),
        _ => Vec::new(),
    }
}

/// What kind of thing a number in content points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefKind {
    /// An image or file (attachment).
    Media,
    /// A reusable block (`wp_block` post).
    Block,
}

/// Replaces the digits right after `at` in `text`; returns where they end.
fn digits_at(text: &str, at: usize) -> Option<(u64, usize)> {
    let len = text[at..].bytes().take_while(u8::is_ascii_digit).count();
    if len == 0 {
        return None;
    }
    Some((text[at..at + len].parse().ok()?, at + len))
}

/// Changes the numbers in one block's attributes JSON, as text, so the
/// rest of it stays exactly as WordPress wrote it.
fn map_block_attrs(
    attrs: &str,
    kind_of: RefKind,
    map: &mut dyn FnMut(u64, RefKind) -> Option<u64>,
) -> String {
    let mut out = String::with_capacity(attrs.len());
    let mut rest = attrs;
    loop {
        let next = ["\"id\":", "\"mediaId\":", "\"ref\":", "\"ids\":["]
            .iter()
            .filter_map(|key| rest.find(key).map(|i| (i, *key)))
            .min_by_key(|(i, _)| *i);
        let Some((i, key)) = next else {
            out.push_str(rest);
            return out;
        };
        let start = i + key.len();
        out.push_str(&rest[..start]);
        rest = &rest[start..];
        let kind = if key == "\"ref\":" {
            RefKind::Block
        } else {
            kind_of
        };
        if key.ends_with('[') {
            let end = rest.find(']').unwrap_or(rest.len());
            let list = &rest[..end];
            let mapped: Vec<String> = list
                .split(',')
                .filter_map(|p| {
                    let p = p.trim();
                    match p.parse::<u64>() {
                        Ok(id) => Some(map(id, kind).unwrap_or(id).to_string()),
                        Err(_) if p.is_empty() => None,
                        Err(_) => Some(p.to_string()),
                    }
                })
                .collect();
            out.push_str(&mapped.join(","));
            rest = &rest[end..];
        } else if let Some((id, end)) = digits_at(rest, 0) {
            out.push_str(&map(id, kind).unwrap_or(id).to_string());
            rest = &rest[end..];
        }
    }
}

/// Changes the numbers that post content uses to point at images, files
/// and reusable blocks. `map` gets each number with what it points at and
/// returns the new one (`None` keeps it).
pub fn map_content(content: &str, map: &mut dyn FnMut(u64, RefKind) -> Option<u64>) -> String {
    // Block comments: <!-- wp:image {"id":12,...} /-->
    let mut out = String::with_capacity(content.len());
    let mut rest = content;
    while let Some(i) = rest.find("<!-- wp:") {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let Some(end) = rest.find("-->") else {
            break;
        };
        let comment = &rest[..end];
        let name_end = comment[8..]
            .find([' ', '/'])
            .map_or(comment.len(), |n| n + 8);
        let name = &comment[8..name_end];
        let kind = if name == "block" {
            RefKind::Block
        } else {
            RefKind::Media
        };
        match comment.find('{') {
            Some(brace) => {
                out.push_str(&comment[..brace]);
                out.push_str(&map_block_attrs(&comment[brace..], kind, map));
            }
            None => out.push_str(comment),
        }
        rest = &rest[end..];
    }
    out.push_str(rest);

    // wp-image-12 classes, and shortcodes: [gallery ids="1,2"],
    // [vc_single_image image="3"], [vc_gallery images="4,5"].
    let mut text = out;
    for (needle, list) in [
        ("wp-image-", false),
        (" ids=\"", true),
        (" image=\"", true),
        (" images=\"", true),
    ] {
        let mut out = String::with_capacity(text.len());
        let mut rest = text.as_str();
        while let Some(i) = rest.find(needle) {
            let start = i + needle.len();
            out.push_str(&rest[..start]);
            rest = &rest[start..];
            if list {
                let end = rest.find('"').unwrap_or(rest.len());
                out.push_str(&map_csv(&rest[..end], &mut |id| {
                    Some(map(id, RefKind::Media).unwrap_or(id))
                }));
                rest = &rest[end..];
            } else if let Some((id, end)) = digits_at(rest, 0) {
                out.push_str(&map(id, RefKind::Media).unwrap_or(id).to_string());
                rest = &rest[end..];
            }
        }
        out.push_str(rest);
        text = out;
    }
    text
}

/// The images, files and reusable blocks post content points at.
pub fn refs_in_content(content: &str) -> Vec<(u64, RefKind)> {
    let mut found = Vec::new();
    map_content(content, &mut |id, kind| {
        found.push((id, kind));
        None
    });
    found
}

/// Changes the image numbers in Elementor's page data (JSON): every
/// numeric `"id"`, which Elementor uses only for media; its own element
/// ids are strings.
pub fn map_elementor(json: &str, map: &mut dyn FnMut(u64) -> Option<u64>) -> String {
    let mut out = String::with_capacity(json.len());
    let mut rest = json;
    while let Some(i) = rest.find("\"id\":") {
        let start = i + 5;
        out.push_str(&rest[..start]);
        rest = &rest[start..];
        if let Some((id, end)) = digits_at(rest, 0) {
            out.push_str(&map(id).unwrap_or(id).to_string());
            rest = &rest[end..];
        }
    }
    out.push_str(rest);
    out
}

/// Elementor templates a page embeds, which keep their own numbers.
fn elementor_templates(json: &str) -> Vec<u64> {
    let mut found = Vec::new();
    for key in ["\"template_id\":", "\"templateID\":"] {
        let mut rest = json;
        while let Some(i) = rest.find(key) {
            rest = &rest[i + key.len()..];
            let digits = rest.trim_start_matches('"');
            if let Some((id, _)) = digits_at(digits, 0) {
                found.push(id);
            }
        }
    }
    found
}

// ---------------------------------------------------------------------------
// Reading rows

struct Rows {
    columns: Vec<String>,
    rows: Vec<Vec<Option<String>>>,
}

impl Rows {
    fn from(set: ResultSet) -> Self {
        Rows {
            columns: set.columns,
            rows: set.rows,
        }
    }

    fn col(&self, name: &str) -> Option<usize> {
        self.columns.iter().position(|c| c == name)
    }

    fn get<'a>(&self, row: &'a [Option<String>], name: &str) -> Option<&'a str> {
        self.col(name)
            .and_then(|i| row.get(i))
            .and_then(|v| v.as_deref())
    }
}

fn num(text: Option<&str>) -> u64 {
    text.and_then(|t| t.trim().parse().ok()).unwrap_or(0)
}

fn id_list(ids: impl IntoIterator<Item = u64>) -> String {
    let list: Vec<String> = ids.into_iter().map(|i| i.to_string()).collect();
    if list.is_empty() {
        "0".into()
    } else {
        list.join(",")
    }
}

/// One copy's database, read for a move.
struct Db<'a> {
    client: &'a Client,
    name: String,
    prefix: String,
}

impl Db<'_> {
    fn t(&self, table: &str) -> String {
        ident(&format!("{}{table}", self.prefix))
    }

    fn rows(&self, sql: &str) -> Result<Rows> {
        Ok(Rows::from(self.client.rows(&self.name, sql)?))
    }

    fn ids(&self, sql: &str) -> Result<Vec<u64>> {
        Ok(self
            .client
            .rows(&self.name, sql)?
            .rows
            .iter()
            .filter_map(|r| r.first().cloned().flatten()?.parse().ok())
            .collect())
    }

    fn columns(&self, table: &str) -> Result<Vec<String>> {
        Ok(self
            .client
            .rows(
                &self.name,
                &format!(
                    "SELECT COLUMN_NAME FROM information_schema.COLUMNS WHERE TABLE_SCHEMA = {} AND TABLE_NAME = {} ORDER BY ORDINAL_POSITION",
                    literal(&self.name),
                    literal(&format!("{}{table}", self.prefix))
                ),
            )?
            .rows
            .iter()
            .filter_map(|r| r.first().cloned().flatten())
            .collect())
    }

    fn has_table(&self, table: &str) -> bool {
        self.columns(table).is_ok_and(|c| !c.is_empty())
    }
}

// ---------------------------------------------------------------------------
// Picking posts

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PostSummary {
    pub id: u64,
    pub title: String,
    pub post_type: String,
    pub status: String,
    pub slug: String,
    pub modified: String,
}

/// Post types offered for moving; WordPress's own internals are left out.
const NOT_OFFERED: &str = "'revision','nav_menu_item','attachment','customize_changeset','oembed_cache','user_request','wp_global_styles','wp_font_face','wp_font_family','shop_order','shop_order_refund','shop_order_placehold','shop_subscription','product_variation','scheduled-action','acf-field','acf-field-group'";

/// Posts, pages and other content of a copy, newest first, optionally
/// matching `search` in the title or slug.
pub fn list(home: &Path, domain: &str, search: &str) -> Result<Vec<PostSummary>> {
    let wp = open(home, domain)?;
    let client = Client::open(home)?;
    let db = Db {
        client: &client,
        name: wp.database()?,
        prefix: wp.prefix().to_string(),
    };
    let search = search.trim();
    let filter = if search.is_empty() {
        String::new()
    } else {
        let like = literal(&format!(
            "%{}%",
            search
                .replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_")
        ));
        format!("AND (post_title LIKE {like} OR post_name LIKE {like})")
    };
    let rows = db.rows(&format!(
        "SELECT ID, post_title, post_type, post_status, post_name, post_modified FROM {} \
         WHERE post_type NOT IN ({NOT_OFFERED}) AND post_status NOT IN ('auto-draft','trash','inherit') {filter} \
         ORDER BY post_modified DESC LIMIT 300",
        db.t("posts")
    ))?;
    Ok(rows
        .rows
        .iter()
        .map(|r| PostSummary {
            id: num(rows.get(r, "ID")),
            title: rows.get(r, "post_title").unwrap_or_default().to_string(),
            post_type: rows.get(r, "post_type").unwrap_or_default().to_string(),
            status: rows.get(r, "post_status").unwrap_or_default().to_string(),
            slug: rows.get(r, "post_name").unwrap_or_default().to_string(),
            modified: rows.get(r, "post_modified").unwrap_or_default().to_string(),
        })
        .collect())
}

fn open(home: &Path, domain: &str) -> Result<WpSite> {
    let store = SiteStore::load_from(&home.join("sites.toml"))?;
    let site = store
        .list()
        .iter()
        .find(|s| s.domain() == domain)
        .cloned()
        .ok_or_else(|| Error::SiteNotFound(domain.to_string()))?;
    WpSite::open(&site)
}

// ---------------------------------------------------------------------------
// Planning a move

/// What to do with a picked post that the target already has (same type
/// and slug).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Existing {
    /// Replace the target's post with this one, keeping its number.
    #[default]
    Update,
    Skip,
    /// Add it as a new post next to the target's.
    New,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct MoveChoice {
    pub ids: Vec<u64>,
    /// Per post id, for posts the target already has; others: update.
    #[serde(default)]
    pub existing: BTreeMap<u64, Existing>,
    #[serde(default)]
    pub comments: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    Add,
    Update,
    Skip,
    /// The target already has it (an image with the same file, a reusable
    /// block with the same name); the target's is used.
    Reuse,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PlannedPost {
    pub source_id: u64,
    pub title: String,
    pub post_type: String,
    pub action: Action,
    /// Its number in the target.
    pub target_id: u64,
    /// Picked by the user; otherwise it comes along with one that was.
    pub picked: bool,
    /// The target has a post with the same type and slug.
    pub exists: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Warning {
    pub post: u64,
    pub title: String,
    pub text: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Counts {
    pub posts: usize,
    pub meta: usize,
    pub new_terms: usize,
    pub terms: usize,
    pub comments: usize,
    pub menu_items: usize,
    pub files: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MovePlan {
    pub source: String,
    pub target: String,
    pub posts: Vec<PlannedPost>,
    pub counts: Counts,
    /// Media files copied, relative to the uploads folder.
    pub files: Vec<String>,
    pub warnings: Vec<Warning>,
    #[serde(skip)]
    sql: String,
    #[serde(skip)]
    copies: Vec<(PathBuf, PathBuf, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Moved {
    pub snapshot: String,
    pub posts: usize,
    pub files: usize,
    pub warnings: Vec<Warning>,
}

/// Meta keys never copied: editing locks and per-copy caches.
const META_SKIPPED: &[&str] = &[
    "_edit_lock",
    "_edit_last",
    "_elementor_css",
    "_wp_old_slug",
    "_wp_trash_meta_status",
];

struct Planner<'a> {
    source: Db<'a>,
    target: Db<'a>,
    rules: replace::Rules,
    /// Every post coming along, by source id.
    posts: BTreeMap<u64, PlannedPost>,
    rows: BTreeMap<u64, Vec<Option<String>>>,
    post_columns: Vec<String>,
    meta: BTreeMap<u64, Vec<(String, String)>>,
    warnings: Vec<Warning>,
    /// ACF field types by field key (`field_…`).
    acf: BTreeMap<String, String>,
}

impl Planner<'_> {
    fn warn(&mut self, post: u64, text: String) {
        let title = self
            .posts
            .get(&post)
            .map(|p| p.title.clone())
            .unwrap_or_default();
        if !self
            .warnings
            .iter()
            .any(|w| w.post == post && w.text == text)
        {
            self.warnings.push(Warning { post, title, text });
        }
    }

    /// Reads posts and their meta from the source; returns the ids found.
    fn load(&mut self, ids: &BTreeSet<u64>) -> Result<Vec<u64>> {
        let wanted: Vec<u64> = ids
            .iter()
            .copied()
            .filter(|id| !self.rows.contains_key(id))
            .collect();
        if wanted.is_empty() {
            return Ok(Vec::new());
        }
        let rows = self.source.rows(&format!(
            "SELECT * FROM {} WHERE ID IN ({})",
            self.source.t("posts"),
            id_list(wanted.iter().copied())
        ))?;
        if self.post_columns.is_empty() {
            self.post_columns = rows.columns.clone();
        }
        let mut found = Vec::new();
        for row in &rows.rows {
            let id = num(rows.get(row, "ID"));
            found.push(id);
            self.rows.insert(id, row.clone());
        }
        let meta = self.source.rows(&format!(
            "SELECT post_id, meta_key, meta_value FROM {} WHERE post_id IN ({}) ORDER BY meta_id",
            self.source.t("postmeta"),
            id_list(found.iter().copied())
        ))?;
        for row in &meta.rows {
            let id = num(meta.get(row, "post_id"));
            let key = meta.get(row, "meta_key").unwrap_or_default().to_string();
            if META_SKIPPED.contains(&key.as_str()) {
                continue;
            }
            let value = meta.get(row, "meta_value").unwrap_or_default().to_string();
            self.meta.entry(id).or_default().push((key, value));
        }
        Ok(found)
    }

    fn col(&self, id: u64, name: &str) -> String {
        let i = self.post_columns.iter().position(|c| c == name);
        i.and_then(|i| self.rows.get(&id)?.get(i)?.clone())
            .unwrap_or_default()
    }

    fn meta_value(&self, id: u64, key: &str) -> Option<&str> {
        self.meta
            .get(&id)?
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    /// What a post's meta points at: (number, is media).
    fn meta_refs(&self, id: u64) -> Vec<u64> {
        let mut out = Vec::new();
        for (key, value) in self.meta.get(&id).into_iter().flatten() {
            match key.as_str() {
                "_thumbnail_id" => out.extend(value.trim().parse::<u64>().ok()),
                "_product_image_gallery" => out.extend(ids_in_csv(value)),
                "_elementor_data" => {
                    map_elementor(value, &mut |n| {
                        out.push(n);
                        None
                    });
                }
                _ => {
                    if let Some(kind) = self.acf_type(id, key) {
                        if matches!(kind.as_str(), "image" | "file") {
                            out.extend(value.trim().parse::<u64>().ok());
                        } else if kind == "gallery" {
                            out.extend(ids_in_serialized(value));
                        }
                    }
                }
            }
        }
        out
    }

    /// The ACF field type of a meta key, from its `_key` sibling.
    fn acf_type(&self, id: u64, key: &str) -> Option<String> {
        if key.starts_with('_') {
            return None;
        }
        let field = self.meta_value(id, &format!("_{key}"))?;
        self.acf.get(field).cloned()
    }

    fn read_acf(&mut self) -> Result<()> {
        let rows = self.source.rows(&format!(
            "SELECT post_name, post_content FROM {} WHERE post_type = 'acf-field'",
            self.source.t("posts")
        ))?;
        for row in &rows.rows {
            let (Some(name), Some(content)) =
                (rows.get(row, "post_name"), rows.get(row, "post_content"))
            else {
                continue;
            };
            if let Some(kind) = php::unserialize(content.as_bytes())
                .and_then(|v| v.get("type").and_then(|t| t.as_str().map(str::to_string)))
            {
                self.acf.insert(name.to_string(), kind);
            }
        }
        Ok(())
    }
}

/// Plans moving the picked posts from `source` to `target`: what comes
/// along, what each becomes, and what can't be changed safely. Nothing
/// is changed.
pub fn plan(home: &Path, source: &str, target: &str, choice: &MoveChoice) -> Result<MovePlan> {
    if source == target {
        return Err(fail("pick another copy to move the posts to"));
    }
    if choice.ids.is_empty() {
        return Err(fail("pick at least one post or page"));
    }
    let source_wp = open(home, source)?;
    let target_wp = open(home, target)?;
    let client = Client::open(home)?;
    let mut p = Planner {
        source: Db {
            client: &client,
            name: source_wp.database()?,
            prefix: source_wp.prefix().to_string(),
        },
        target: Db {
            client: &client,
            name: target_wp.database()?,
            prefix: target_wp.prefix().to_string(),
        },
        rules: replace::Rules::site_move(
            &super::home_url(home, &source_wp),
            &super::home_url(home, &target_wp),
        )
        .and_path(
            &source_wp.site.path.to_string_lossy(),
            &target_wp.site.path.to_string_lossy(),
        ),
        posts: BTreeMap::new(),
        rows: BTreeMap::new(),
        post_columns: Vec::new(),
        meta: BTreeMap::new(),
        warnings: Vec::new(),
        acf: BTreeMap::new(),
    };
    p.read_acf()?;

    // The picked posts, then what comes along, until nothing new turns up.
    let picked: BTreeSet<u64> = choice.ids.iter().copied().collect();
    let mut pending = picked.clone();
    let mut first = true;
    while !pending.is_empty() {
        let found = p.load(&pending)?;
        if first {
            for id in &picked {
                if !found.contains(id) {
                    return Err(fail(format!("{source} has no post {id}")));
                }
            }
            first = false;
        }
        let mut next = BTreeSet::new();
        for id in &found {
            let post_type = p.col(*id, "post_type");
            p.posts.insert(
                *id,
                PlannedPost {
                    source_id: *id,
                    title: p.col(*id, "post_title"),
                    post_type: post_type.clone(),
                    action: Action::Add,
                    target_id: 0,
                    picked: picked.contains(id),
                    exists: false,
                },
            );
            // Variations and attached files belong to their post.
            next.extend(p.source.ids(&format!(
                "SELECT ID FROM {} WHERE post_parent = {id} AND post_type IN ('product_variation','attachment')",
                p.source.t("posts")
            ))?);
            for (n, _) in refs_in_content(&p.col(*id, "post_content")) {
                next.insert(n);
            }
            next.extend(p.meta_refs(*id));
        }
        // Only images, files, reusable blocks and variations come along by
        // reference; anything else must be picked.
        let candidates: Vec<u64> = next
            .into_iter()
            .filter(|n| !p.posts.contains_key(n))
            .collect();
        pending = if candidates.is_empty() {
            BTreeSet::new()
        } else {
            p.source
                .ids(&format!(
                    "SELECT ID FROM {} WHERE ID IN ({}) AND post_type IN ('attachment','wp_block','product_variation')",
                    p.source.t("posts"),
                    id_list(candidates)
                ))?
                .into_iter()
                .collect()
        };
    }

    // What each becomes in the target.
    let mut next_id = {
        let max = p.target.ids(&format!(
            "SELECT COALESCE(MAX(ID), 0) FROM {}",
            p.target.t("posts")
        ))?;
        let auto = p.target.ids(&format!(
            "SELECT COALESCE(AUTO_INCREMENT, 1) FROM information_schema.TABLES WHERE TABLE_SCHEMA = {} AND TABLE_NAME = {}",
            literal(&p.target.name),
            literal(&format!("{}posts", p.target.prefix))
        ))?;
        max.first()
            .copied()
            .unwrap_or(0)
            .max(auto.first().copied().unwrap_or(1).saturating_sub(1))
            + 1
    };
    let ids: Vec<u64> = p.posts.keys().copied().collect();
    for id in &ids {
        let post_type = p.col(*id, "post_type");
        let slug = p.col(*id, "post_name");
        let existing = if post_type == "attachment" {
            let file = p
                .meta_value(*id, "_wp_attached_file")
                .unwrap_or_default()
                .to_string();
            if file.is_empty() {
                Vec::new()
            } else {
                p.target.ids(&format!(
                    "SELECT post_id FROM {} WHERE meta_key = '_wp_attached_file' AND meta_value = {} LIMIT 1",
                    p.target.t("postmeta"),
                    literal(&file)
                ))?
            }
        } else if slug.is_empty() {
            Vec::new()
        } else {
            p.target.ids(&format!(
                "SELECT ID FROM {} WHERE post_type = {} AND post_name = {} AND post_status <> 'trash' LIMIT 1",
                p.target.t("posts"),
                literal(&post_type),
                literal(&slug)
            ))?
        };
        let post = p.posts.get_mut(id).expect("planned");
        post.exists = !existing.is_empty();
        let (action, target_id) = match existing.first() {
            Some(t) if !post.picked && matches!(post_type.as_str(), "attachment" | "wp_block") => {
                (Action::Reuse, *t)
            }
            Some(t) => match choice.existing.get(id).copied().unwrap_or_default() {
                Existing::Update => (Action::Update, *t),
                Existing::Skip => (Action::Skip, *t),
                Existing::New => (Action::Add, 0),
            },
            None => (Action::Add, 0),
        };
        post.action = action;
        post.target_id = if target_id == 0 {
            next_id += 1;
            next_id - 1
        } else {
            target_id
        };
    }

    let mut plan = MovePlan {
        source: source.to_string(),
        target: target.to_string(),
        posts: Vec::new(),
        counts: Counts::default(),
        files: Vec::new(),
        warnings: Vec::new(),
        sql: String::new(),
        copies: Vec::new(),
    };
    write_sql(&mut p, &mut plan, choice, home, &source_wp, &target_wp)?;
    plan.posts = p.posts.values().cloned().collect();
    plan.posts.sort_by_key(|x| (!x.picked, x.source_id));
    plan.counts.posts = plan
        .posts
        .iter()
        .filter(|x| matches!(x.action, Action::Add | Action::Update))
        .count();
    plan.counts.files = plan.copies.len();
    plan.warnings = p.warnings;
    Ok(plan)
}

/// Builds the SQL that writes the planned posts into the target, and the
/// list of media files to copy.
fn write_sql(
    p: &mut Planner,
    plan: &mut MovePlan,
    choice: &MoveChoice,
    home: &Path,
    source_wp: &WpSite,
    target_wp: &WpSite,
) -> Result<()> {
    let map: BTreeMap<u64, u64> = p
        .posts
        .values()
        .filter(|x| x.action != Action::Skip || x.exists)
        .map(|x| (x.source_id, x.target_id))
        .collect();
    let writes: Vec<u64> = p
        .posts
        .values()
        .filter(|x| matches!(x.action, Action::Add | Action::Update))
        .map(|x| x.source_id)
        .collect();
    let mut sql = String::from("SET FOREIGN_KEY_CHECKS = 0;\nSTART TRANSACTION;\n");

    // Authors: the same login in the target, or its first administrator.
    let authors: BTreeSet<u64> = writes
        .iter()
        .map(|id| num(Some(&p.col(*id, "post_author"))))
        .collect();
    let logins = p.source.rows(&format!(
        "SELECT ID, user_login FROM {} WHERE ID IN ({})",
        p.source.t("users"),
        id_list(authors.iter().copied())
    ))?;
    let mut user_map: BTreeMap<u64, u64> = BTreeMap::new();
    for row in &logins.rows {
        let id = num(logins.get(row, "ID"));
        let login = logins.get(row, "user_login").unwrap_or_default();
        if let Some(t) = p
            .target
            .ids(&format!(
                "SELECT ID FROM {} WHERE user_login = {}",
                p.target.t("users"),
                literal(login)
            ))?
            .first()
        {
            user_map.insert(id, *t);
        }
    }
    let admin = p
        .target
        .ids(&format!(
            "SELECT user_id FROM {} WHERE meta_key = {} AND meta_value LIKE '%\"administrator\"%' ORDER BY user_id LIMIT 1",
            p.target.t("usermeta"),
            literal(&format!("{}capabilities", p.target.prefix))
        ))?
        .first()
        .copied()
        .unwrap_or(1);

    let target_columns: BTreeSet<String> = p.target.columns("posts")?.into_iter().collect();
    let columns: Vec<String> = p
        .post_columns
        .iter()
        .filter(|c| target_columns.contains(*c))
        .cloned()
        .collect();
    let rewrite = |rules: &replace::Rules, text: &str| -> String {
        replace::replace_value(text.as_bytes(), rules)
            .map(|v| String::from_utf8_lossy(&v).into_owned())
            .unwrap_or_else(|| text.to_string())
    };

    for id in &writes {
        let post = p.posts[id].clone();
        let tid = post.target_id;
        let mut values: Vec<(String, Option<String>)> = Vec::new();
        for column in &columns {
            let i = p
                .post_columns
                .iter()
                .position(|c| c == column)
                .expect("column");
            let raw = p.rows[id].get(i).cloned().flatten();
            let value = match column.as_str() {
                "ID" => Some(tid.to_string()),
                "post_author" => {
                    let author = num(raw.as_deref());
                    Some(match user_map.get(&author) {
                        Some(t) => t.to_string(),
                        None => {
                            if post.picked {
                                p.warn(*id, "Its author is not on the target; it now belongs to the target's first administrator.".into());
                            }
                            admin.to_string()
                        }
                    })
                }
                "post_parent" => {
                    let parent = num(raw.as_deref());
                    Some(if parent == 0 {
                        "0".into()
                    } else if let Some(t) = map.get(&parent) {
                        t.to_string()
                    } else {
                        // A parent page that isn't moved: the target's page
                        // with the same slug, if it has one.
                        let slug = p.source.rows(&format!(
                            "SELECT post_type, post_name FROM {} WHERE ID = {parent}",
                            p.source.t("posts")
                        ))?;
                        let found = match slug.rows.first() {
                            Some(r) => p.target.ids(&format!(
                                "SELECT ID FROM {} WHERE post_type = {} AND post_name = {} LIMIT 1",
                                p.target.t("posts"),
                                literal(slug.get(r, "post_type").unwrap_or_default()),
                                literal(slug.get(r, "post_name").unwrap_or_default())
                            ))?,
                            None => Vec::new(),
                        };
                        match found.first() {
                            Some(t) => t.to_string(),
                            None => {
                                if post.post_type != "attachment" {
                                    p.warn(*id, "Its parent page is not on the target and is not being moved; it is placed at the top level.".into());
                                }
                                "0".into()
                            }
                        }
                    })
                }
                "post_name" if post.action == Action::Add && post.exists => {
                    raw.map(|slug| format!("{slug}-{}", tid))
                }
                "post_content" | "post_excerpt" | "post_content_filtered" => raw.map(|text| {
                    let text = map_content(&text, &mut |n, _| map.get(&n).copied());
                    rewrite(&p.rules, &text)
                }),
                "guid" | "post_title" => raw.map(|text| rewrite(&p.rules, &text)),
                _ => raw,
            };
            values.push((column.clone(), value));
        }
        let sql_value =
            |v: &Option<String>| v.as_deref().map_or_else(|| "NULL".to_string(), literal);
        match post.action {
            Action::Update => {
                let set: Vec<String> = values
                    .iter()
                    .filter(|(c, _)| c != "ID")
                    .map(|(c, v)| format!("{} = {}", ident(c), sql_value(v)))
                    .collect();
                sql.push_str(&format!(
                    "UPDATE {} SET {} WHERE ID = {tid};\n",
                    p.target.t("posts"),
                    set.join(", ")
                ));
                sql.push_str(&format!(
                    "DELETE FROM {} WHERE post_id = {tid};\n",
                    p.target.t("postmeta")
                ));
                sql.push_str(&format!(
                    "DELETE FROM {} WHERE object_id = {tid};\n",
                    p.target.t("term_relationships")
                ));
            }
            _ => {
                let cols: Vec<String> = values.iter().map(|(c, _)| ident(c)).collect();
                let vals: Vec<String> = values.iter().map(|(_, v)| sql_value(v)).collect();
                sql.push_str(&format!(
                    "INSERT INTO {} ({}) VALUES ({});\n",
                    p.target.t("posts"),
                    cols.join(","),
                    vals.join(",")
                ));
            }
        }

        // Meta, with the numbers it holds changed.
        let meta = p.meta.get(id).cloned().unwrap_or_default();
        for (key, value) in meta {
            let mut dropped = Vec::new();
            let mut keep_or_drop = |n: u64| match map.get(&n) {
                Some(t) => Some(*t),
                None => {
                    dropped.push(n);
                    None
                }
            };
            let value = match key.as_str() {
                "_thumbnail_id" => match value.trim().parse::<u64>() {
                    Ok(n) => map.get(&n).map_or(value.clone(), |t| t.to_string()),
                    Err(_) => value.clone(),
                },
                "_product_image_gallery" => {
                    map_csv(&value, &mut |n| Some(map.get(&n).copied().unwrap_or(n)))
                }
                "_children" | "_upsell_ids" | "_crosssell_ids" => {
                    let mapped = map_serialized(&value, &mut keep_or_drop).unwrap_or(value.clone());
                    if !dropped.is_empty() {
                        p.warn(
                            *id,
                            format!(
                                "Its linked products ({}) point at products that are not being moved: {}. They are left out; link them again in the target.",
                                key.trim_start_matches('_').replace('_', " "),
                                id_list(dropped.iter().copied())
                            ),
                        );
                    }
                    mapped
                }
                "_elementor_data" => {
                    for template in elementor_templates(&value) {
                        if !map.contains_key(&template) {
                            p.warn(*id, format!("Its Elementor layout uses template {template}, which is not being moved; pick it too, or check the layout in the target."));
                        }
                    }
                    map_elementor(&value, &mut |n| map.get(&n).copied())
                }
                "_fl_builder_data" | "_fl_builder_draft" => {
                    p.warn(*id, "It is built with Beaver Builder, whose layouts keep their own numbers for images; check them in the target.".into());
                    value.clone()
                }
                k if k.starts_with("_yoast_wpseo_primary_")
                    || k.starts_with("rank_math_primary_") =>
                {
                    // A term number: changed below, once terms are matched.
                    value.clone()
                }
                _ => match p.acf_type(*id, &key).as_deref() {
                    Some("image" | "file") => match value.trim().parse::<u64>() {
                        Ok(n) => map.get(&n).map_or(value.clone(), |t| t.to_string()),
                        Err(_) => value.clone(),
                    },
                    Some("gallery") => {
                        map_serialized(&value, &mut |n| Some(map.get(&n).copied().unwrap_or(n)))
                            .unwrap_or(value.clone())
                    }
                    Some("post_object" | "relationship" | "page_link") => {
                        let mapped = if let Ok(n) = value.trim().parse::<u64>() {
                            match keep_or_drop(n) {
                                Some(t) => t.to_string(),
                                None => String::new(),
                            }
                        } else {
                            map_serialized(&value, &mut keep_or_drop).unwrap_or(value.clone())
                        };
                        if !dropped.is_empty() {
                            p.warn(
                                *id,
                                format!(
                                    "Its field {key} points at posts that are not being moved ({}); they are left out.",
                                    id_list(dropped.iter().copied())
                                ),
                            );
                        }
                        mapped
                    }
                    _ => value.clone(),
                },
            };
            let value = rewrite(&p.rules, &value);
            if key.starts_with("_yoast_wpseo_primary_") || key.starts_with("rank_math_primary_") {
                // Written with the term mapping below.
                p.meta.entry(u64::MAX - *id).or_default().push((key, value));
                continue;
            }
            sql.push_str(&format!(
                "INSERT INTO {} (post_id, meta_key, meta_value) VALUES ({tid}, {}, {});\n",
                p.target.t("postmeta"),
                literal(&key),
                literal(&value)
            ));
            plan.counts.meta += 1;
        }
    }

    // Terms: matched by taxonomy and slug, made when missing.
    let rel = p.source.rows(&format!(
        "SELECT tr.object_id, tt.term_taxonomy_id, tt.term_id, tt.taxonomy, tt.description, tt.parent, t.name, t.slug \
         FROM {tr} tr JOIN {tt} tt ON tt.term_taxonomy_id = tr.term_taxonomy_id JOIN {t} t ON t.term_id = tt.term_id \
         WHERE tr.object_id IN ({ids}) AND tt.taxonomy <> 'nav_menu'",
        tr = p.source.t("term_relationships"),
        tt = p.source.t("term_taxonomy"),
        t = p.source.t("terms"),
        ids = id_list(writes.iter().copied())
    ))?;
    let mut term_vars: BTreeMap<u64, String> = BTreeMap::new();
    let mut tt_vars: BTreeMap<u64, String> = BTreeMap::new();
    for row in &rel.rows {
        let term = num(rel.get(row, "term_id"));
        ensure_term(
            p,
            &mut sql,
            &mut term_vars,
            &mut tt_vars,
            term,
            &mut plan.counts,
        )?;
        let object = num(rel.get(row, "object_id"));
        let tid = p.posts[&object].target_id;
        sql.push_str(&format!(
            "INSERT IGNORE INTO {} (object_id, term_taxonomy_id, term_order) VALUES ({tid}, {}, 0);\n",
            p.target.t("term_relationships"),
            tt_vars[&term]
        ));
        plan.counts.terms += 1;
    }
    // Primary terms (Yoast SEO, Rank Math) now that terms have numbers.
    for id in &writes {
        let tid = p.posts[id].target_id;
        for (key, value) in p.meta.get(&(u64::MAX - *id)).cloned().unwrap_or_default() {
            let term = value.trim().parse::<u64>().unwrap_or(0);
            let known = term_vars.contains_key(&term)
                || (term > 0
                    && ensure_term(
                        p,
                        &mut sql,
                        &mut term_vars,
                        &mut tt_vars,
                        term,
                        &mut plan.counts,
                    )
                    .is_ok());
            let new = if known {
                term_vars[&term].clone()
            } else {
                literal(&value)
            };
            sql.push_str(&format!(
                "INSERT INTO {} (post_id, meta_key, meta_value) VALUES ({tid}, {}, {new});\n",
                p.target.t("postmeta"),
                literal(&key)
            ));
            plan.counts.meta += 1;
        }
    }
    if !tt_vars.is_empty() {
        let tt = p.target.t("term_taxonomy");
        let tr = p.target.t("term_relationships");
        let vars: Vec<String> = tt_vars.values().cloned().collect();
        sql.push_str(&format!(
            "UPDATE {tt} SET count = (SELECT COUNT(*) FROM {tr} WHERE {tr}.term_taxonomy_id = {tt}.term_taxonomy_id) WHERE term_taxonomy_id IN ({});\n",
            vars.join(",")
        ));
    }

    // Menu items that point at moved posts, in menus the target also has.
    let added: Vec<u64> = writes
        .iter()
        .copied()
        .filter(|id| p.posts[id].action == Action::Add)
        .collect();
    if !added.is_empty() {
        let items = p.source.rows(&format!(
            "SELECT m.post_id AS item, m.meta_value AS object, t.slug AS menu FROM {pm} m \
             JOIN {pm} k ON k.post_id = m.post_id AND k.meta_key = '_menu_item_type' AND k.meta_value = 'post_type' \
             JOIN {tr} tr ON tr.object_id = m.post_id JOIN {tt} tt ON tt.term_taxonomy_id = tr.term_taxonomy_id AND tt.taxonomy = 'nav_menu' \
             JOIN {t} t ON t.term_id = tt.term_id \
             WHERE m.meta_key = '_menu_item_object_id' AND m.meta_value IN ({ids})",
            pm = p.source.t("postmeta"),
            tr = p.source.t("term_relationships"),
            tt = p.source.t("term_taxonomy"),
            t = p.source.t("terms"),
            ids = added.iter().map(|i| literal(&i.to_string())).collect::<Vec<_>>().join(",")
        ))?;
        for row in &items.rows {
            let item = num(items.get(row, "item"));
            let object = num(items.get(row, "object"));
            let menu = items.get(row, "menu").unwrap_or_default();
            let target_menu = p.target.ids(&format!(
                "SELECT tt.term_taxonomy_id FROM {tt} tt JOIN {t} t ON t.term_id = tt.term_id WHERE tt.taxonomy = 'nav_menu' AND t.slug = {}",
                literal(menu),
                tt = p.target.t("term_taxonomy"),
                t = p.target.t("terms"),
            ))?;
            let Some(menu_tt) = target_menu.first() else {
                continue;
            };
            p.load(&BTreeSet::from([item]))?;
            let new_item = {
                let max = plan_next_id(p);
                max + 1
            };
            p.posts.insert(
                item,
                PlannedPost {
                    source_id: item,
                    title: p.col(item, "post_title"),
                    post_type: "nav_menu_item".into(),
                    action: Action::Add,
                    target_id: new_item,
                    picked: false,
                    exists: false,
                },
            );
            let mut cols = Vec::new();
            let mut vals = Vec::new();
            for column in &columns {
                let i = p
                    .post_columns
                    .iter()
                    .position(|c| c == column)
                    .expect("column");
                let raw = p.rows[&item].get(i).cloned().flatten();
                let value = match column.as_str() {
                    "ID" => Some(new_item.to_string()),
                    "post_author" => Some(admin.to_string()),
                    "guid" => raw.map(|g| rewrite(&p.rules, &g)),
                    _ => raw,
                };
                cols.push(ident(column));
                vals.push(value.as_deref().map_or_else(|| "NULL".to_string(), literal));
            }
            sql.push_str(&format!(
                "INSERT INTO {} ({}) VALUES ({});\n",
                p.target.t("posts"),
                cols.join(","),
                vals.join(",")
            ));
            for (key, value) in p.meta.get(&item).cloned().unwrap_or_default() {
                let value = match key.as_str() {
                    "_menu_item_object_id" => map.get(&object).map_or(value, |t| t.to_string()),
                    // Its place under another item is not kept: that item
                    // may not be in the target's menu.
                    "_menu_item_menu_item_parent" => "0".into(),
                    "_menu_item_url" => rewrite(&p.rules, &value),
                    _ => value,
                };
                sql.push_str(&format!(
                    "INSERT INTO {} (post_id, meta_key, meta_value) VALUES ({new_item}, {}, {});\n",
                    p.target.t("postmeta"),
                    literal(&key),
                    literal(&value)
                ));
            }
            sql.push_str(&format!(
                "INSERT IGNORE INTO {} (object_id, term_taxonomy_id, term_order) VALUES ({new_item}, {menu_tt}, 0);\n\
                 UPDATE {tt} SET count = count + 1 WHERE term_taxonomy_id = {menu_tt};\n",
                p.target.t("term_relationships"),
                tt = p.target.t("term_taxonomy"),
            ));
            plan.counts.menu_items += 1;
            let title = p.posts[&object].title.clone();
            p.warn(object, format!("It was added to the menu \"{menu}\" at the top level; move it under another item in the target if it was nested."));
            let _ = title;
        }
    }

    // WooCommerce's product lookup rows, so shop pages find new products.
    if p.target.has_table("wc_product_meta_lookup") && p.source.has_table("wc_product_meta_lookup")
    {
        let lookup_columns: Vec<String> = {
            let target: BTreeSet<String> = p
                .target
                .columns("wc_product_meta_lookup")?
                .into_iter()
                .collect();
            p.source
                .columns("wc_product_meta_lookup")?
                .into_iter()
                .filter(|c| target.contains(c))
                .collect()
        };
        for id in &writes {
            let post = &p.posts[id];
            if !matches!(post.post_type.as_str(), "product" | "product_variation") {
                continue;
            }
            let cols: Vec<String> = lookup_columns.iter().map(|c| ident(c)).collect();
            let picks: Vec<String> = lookup_columns
                .iter()
                .map(|c| {
                    if c == "product_id" {
                        post.target_id.to_string()
                    } else {
                        ident(c)
                    }
                })
                .collect();
            sql.push_str(&format!(
                "DELETE FROM {t} WHERE product_id = {tid};\nINSERT INTO {t} ({}) SELECT {} FROM {s}.{st} WHERE product_id = {id};\n",
                cols.join(","),
                picks.join(","),
                t = p.target.t("wc_product_meta_lookup"),
                s = ident(&p.source.name),
                st = p.source.t("wc_product_meta_lookup"),
                tid = post.target_id,
            ));
        }
    }

    // Comments, when asked: approved ones, with replies kept under theirs.
    if choice.comments {
        let picked: Vec<u64> = writes
            .iter()
            .copied()
            .filter(|id| p.posts[id].picked)
            .collect();
        let comments = p.source.rows(&format!(
            "SELECT * FROM {} WHERE comment_post_ID IN ({}) AND comment_approved = '1' AND comment_type IN ('', 'comment', 'review') ORDER BY comment_ID",
            p.source.t("comments"),
            id_list(picked.iter().copied())
        ))?;
        let comment_columns: Vec<String> = {
            let target: BTreeSet<String> = p.target.columns("comments")?.into_iter().collect();
            comments
                .columns
                .iter()
                .filter(|c| *c != "comment_ID" && target.contains(*c))
                .cloned()
                .collect()
        };
        let mut known = BTreeSet::new();
        for row in &comments.rows {
            let cid = num(comments.get(row, "comment_ID"));
            let mut vals = Vec::new();
            for column in &comment_columns {
                let raw = comments.get(row, column).map(str::to_string);
                vals.push(match column.as_str() {
                    "comment_post_ID" => p.posts[&num(raw.as_deref())].target_id.to_string(),
                    "comment_parent" => {
                        let parent = num(raw.as_deref());
                        if known.contains(&parent) {
                            format!("@c{parent}")
                        } else {
                            "0".into()
                        }
                    }
                    "user_id" => "0".into(),
                    "comment_content" => {
                        literal(&rewrite(&p.rules, raw.as_deref().unwrap_or_default()))
                    }
                    _ => raw.as_deref().map_or_else(|| "NULL".to_string(), literal),
                });
            }
            let cols: Vec<String> = comment_columns.iter().map(|c| ident(c)).collect();
            sql.push_str(&format!(
                "INSERT INTO {} ({}) VALUES ({});\nSET @c{cid} = LAST_INSERT_ID();\n\
                 INSERT INTO {} (comment_id, meta_key, meta_value) SELECT @c{cid}, meta_key, meta_value FROM {}.{} WHERE comment_id = {cid};\n",
                p.target.t("comments"),
                cols.join(","),
                vals.join(","),
                p.target.t("commentmeta"),
                ident(&p.source.name),
                p.source.t("commentmeta"),
            ));
            known.insert(cid);
            plan.counts.comments += 1;
        }
        if plan.counts.comments > 0 {
            for id in &picked {
                let tid = p.posts[id].target_id;
                sql.push_str(&format!(
                    "UPDATE {posts} SET comment_count = (SELECT COUNT(*) FROM {c} WHERE comment_post_ID = {tid} AND comment_approved = '1') WHERE ID = {tid};\n",
                    posts = p.target.t("posts"),
                    c = p.target.t("comments"),
                ));
            }
        }
    }
    sql.push_str("COMMIT;\n");
    plan.sql = sql;

    // Media files of the images and files being added.
    let source_uploads = source_wp.wp_content().join("uploads");
    let shared = env::load(home, &source_wp.site.domain())
        .ok()
        .flatten()
        .and_then(|e| e.shared_media);
    let target_uploads = target_wp.wp_content().join("uploads");
    for id in &writes {
        if p.posts[id].post_type != "attachment" {
            continue;
        }
        for rel in attachment_files(
            p.meta_value(*id, "_wp_attached_file"),
            p.meta_value(*id, "_wp_attachment_metadata"),
        ) {
            let from = [
                Some(source_uploads.join(&rel)),
                shared.as_ref().map(|s| s.join(&rel)),
            ]
            .into_iter()
            .flatten()
            .find(|f| f.is_file());
            let Some(from) = from else {
                if p.posts[id].picked
                    || rel == p.meta_value(*id, "_wp_attached_file").unwrap_or_default()
                {
                    p.warn(*id, format!("The file {rel} is not in {}; the image's details are moved without it.", source_wp.site.domain()));
                }
                continue;
            };
            let to = target_uploads.join(&rel);
            if to.is_file() {
                let same = std::fs::metadata(&to).map(|m| m.len()).ok()
                    == std::fs::metadata(&from).map(|m| m.len()).ok();
                if !same {
                    p.warn(*id, format!("The target already has a different file at {rel}; it is left as it is."));
                }
                continue;
            }
            plan.files.push(rel.clone());
            plan.copies.push((from, to, rel));
        }
    }
    Ok(())
}

/// The highest number a planned post has, for adding one more.
fn plan_next_id(p: &Planner) -> u64 {
    p.posts.values().map(|x| x.target_id).max().unwrap_or(0)
}

/// Makes sure a source term exists in the target (with its parents),
/// adding SQL that sets `@t<id>` and `@tt<id>` to its numbers there.
fn ensure_term(
    p: &mut Planner,
    sql: &mut String,
    term_vars: &mut BTreeMap<u64, String>,
    tt_vars: &mut BTreeMap<u64, String>,
    term: u64,
    counts: &mut Counts,
) -> Result<()> {
    if term_vars.contains_key(&term) {
        return Ok(());
    }
    let rows = p.source.rows(&format!(
        "SELECT tt.taxonomy, tt.description, tt.parent, t.name, t.slug, t.term_group FROM {tt} tt JOIN {t} t ON t.term_id = tt.term_id WHERE t.term_id = {term} LIMIT 1",
        tt = p.source.t("term_taxonomy"),
        t = p.source.t("terms"),
    ))?;
    let Some(row) = rows.rows.first() else {
        return Err(fail(format!("term {term} is missing")));
    };
    let taxonomy = rows.get(row, "taxonomy").unwrap_or_default().to_string();
    let slug = rows.get(row, "slug").unwrap_or_default().to_string();
    let found = p.target.rows(&format!(
        "SELECT tt.term_id, tt.term_taxonomy_id FROM {tt} tt JOIN {t} t ON t.term_id = tt.term_id WHERE tt.taxonomy = {} AND t.slug = {} LIMIT 1",
        literal(&taxonomy),
        literal(&slug),
        tt = p.target.t("term_taxonomy"),
        t = p.target.t("terms"),
    ))?;
    if let Some(r) = found.rows.first() {
        term_vars.insert(term, num(found.get(r, "term_id")).to_string());
        tt_vars.insert(term, num(found.get(r, "term_taxonomy_id")).to_string());
        return Ok(());
    }
    let parent = num(rows.get(row, "parent"));
    let parent_var = if parent > 0 {
        ensure_term(p, sql, term_vars, tt_vars, parent, counts)?;
        term_vars[&parent].clone()
    } else {
        "0".into()
    };
    sql.push_str(&format!(
        "INSERT INTO {t} (name, slug, term_group) VALUES ({}, {}, {});\nSET @t{term} = LAST_INSERT_ID();\n\
         INSERT INTO {tt} (term_id, taxonomy, description, parent, count) VALUES (@t{term}, {}, {}, {parent_var}, 0);\nSET @tt{term} = LAST_INSERT_ID();\n\
         INSERT INTO {tm} (term_id, meta_key, meta_value) SELECT @t{term}, meta_key, meta_value FROM {sdb}.{stm} WHERE term_id = {term} AND meta_key NOT IN ('thumbnail_id');\n",
        literal(rows.get(row, "name").unwrap_or_default()),
        literal(&slug),
        literal(rows.get(row, "term_group").unwrap_or("0")),
        literal(&taxonomy),
        literal(rows.get(row, "description").unwrap_or_default()),
        t = p.target.t("terms"),
        tt = p.target.t("term_taxonomy"),
        tm = p.target.t("termmeta"),
        sdb = ident(&p.source.name),
        stm = p.source.t("termmeta"),
    ));
    term_vars.insert(term, format!("@t{term}"));
    tt_vars.insert(term, format!("@tt{term}"));
    counts.new_terms += 1;
    Ok(())
}

/// An attachment's files relative to the uploads folder: the original,
/// its resized copies and the unscaled original WordPress keeps.
pub fn attachment_files(attached: Option<&str>, metadata: Option<&str>) -> Vec<String> {
    let Some(file) = attached.filter(|f| !f.is_empty() && !f.contains("..")) else {
        return Vec::new();
    };
    let dir = file.rsplit_once('/').map_or("", |(d, _)| d);
    let in_dir = |name: &str| {
        if dir.is_empty() {
            name.to_string()
        } else {
            format!("{dir}/{name}")
        }
    };
    let mut out = vec![file.to_string()];
    if let Some(meta) = metadata.and_then(|m| php::unserialize(m.as_bytes())) {
        if let Some(php::Value::Array(sizes)) = meta.get("sizes") {
            for (_, size) in sizes {
                if let Some(name) = size.get("file").and_then(|f| f.as_str()) {
                    out.push(in_dir(name));
                }
            }
        }
        if let Some(name) = meta.get("original_image").and_then(|f| f.as_str()) {
            out.push(in_dir(name));
        }
    }
    out.retain(|f| !f.contains(".."));
    out.sort();
    out.dedup();
    out
}

/// Moves the picked posts as [`plan`] plans them. A snapshot of the target
/// is taken first; its id is returned for undo.
pub fn move_posts(home: &Path, source: &str, target: &str, choice: &MoveChoice) -> Result<Moved> {
    let plan = plan(home, source, target, choice)?;
    let target_wp = open(home, target)?;
    let titles: Vec<&str> = plan
        .posts
        .iter()
        .filter(|x| x.picked)
        .map(|x| x.title.as_str())
        .take(3)
        .collect();
    let picked = plan.posts.iter().filter(|x| x.picked).count();
    let reason = if picked == 1 {
        format!(
            "Move \"{}\" from {source}",
            titles.first().copied().unwrap_or_default()
        )
    } else {
        format!("Move {picked} posts from {source}")
    };
    let mut snap = snapshot::take(home, &target_wp, &reason)?;
    let client = Client::open(home)?;
    client.execute(&target_wp.database()?, &plan.sql)?;
    let site_root = &target_wp.site.path;
    for (from, to, _) in &plan.copies {
        let rel = to
            .strip_prefix(site_root)
            .map(|r| r.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default();
        if !rel.is_empty() {
            snap.note_added(home, &rel)?;
        }
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent).map_err(io(parent))?;
        }
        super::copy::copy_file(from, to).map_err(io(to))?;
    }
    Ok(Moved {
        snapshot: snap.id,
        posts: plan.counts.posts,
        files: plan.copies.len(),
        warnings: plan.warnings,
    })
}

// ---------------------------------------------------------------------------
// Renumbering inside one database

/// Gives posts new numbers inside one database, with every reference
/// Stackyard knows changed too. Used when merging, for staging posts whose
/// numbers clash with orders the target took meanwhile.
pub fn renumber(
    client: &Client,
    database: &str,
    prefix: &str,
    map: &BTreeMap<u64, u64>,
) -> Result<()> {
    if map.is_empty() {
        return Ok(());
    }
    let db = Db {
        client,
        name: database.to_string(),
        prefix: prefix.to_string(),
    };
    let posts = db.t("posts");
    let meta = db.t("postmeta");
    let mut sql = String::from("SET FOREIGN_KEY_CHECKS = 0;\nSTART TRANSACTION;\n");
    for (old, new) in map {
        sql.push_str(&format!(
            "UPDATE {posts} SET ID = {new} WHERE ID = {old};\n\
             UPDATE {posts} SET post_parent = {new} WHERE post_parent = {old};\n\
             UPDATE {meta} SET post_id = {new} WHERE post_id = {old};\n\
             UPDATE {tr} SET object_id = {new} WHERE object_id = {old};\n\
             UPDATE {c} SET comment_post_ID = {new} WHERE comment_post_ID = {old};\n\
             UPDATE {meta} SET meta_value = '{new}' WHERE meta_value = '{old}' AND meta_key IN ('_thumbnail_id','_menu_item_object_id','_menu_item_menu_item_parent');\n",
            tr = db.t("term_relationships"),
            c = db.t("comments"),
        ));
        if db.has_table("wc_product_meta_lookup") {
            sql.push_str(&format!(
                "UPDATE {} SET product_id = {new} WHERE product_id = {old};\n",
                db.t("wc_product_meta_lookup")
            ));
        }
    }
    sql.push_str("COMMIT;\n");
    client.execute(database, &sql)?;

    // Numbers inside content and structured meta: only rows that mention
    // one of the old numbers are read and changed.
    let pattern = map.keys().map(u64::to_string).collect::<Vec<_>>().join("|");
    let regexp = literal(&format!("(^|[^0-9])({pattern})([^0-9]|$)"));
    let lookup = |n: u64| map.get(&n).copied();
    let rows = db.rows(&format!(
        "SELECT ID, post_content FROM {posts} WHERE post_content REGEXP {regexp}"
    ))?;
    let mut sql = String::from("START TRANSACTION;\n");
    for row in &rows.rows {
        let id = num(rows.get(row, "ID"));
        let content = rows.get(row, "post_content").unwrap_or_default();
        let changed = map_content(content, &mut |n, _| lookup(n));
        if changed != content {
            sql.push_str(&format!(
                "UPDATE {posts} SET post_content = {} WHERE ID = {id};\n",
                literal(&changed)
            ));
        }
    }
    let rows = db.rows(&format!(
        "SELECT meta_id, meta_key, meta_value FROM {meta} WHERE meta_key IN ('_product_image_gallery','_children','_upsell_ids','_crosssell_ids','_elementor_data') AND meta_value REGEXP {regexp}"
    ))?;
    for row in &rows.rows {
        let meta_id = num(rows.get(row, "meta_id"));
        let value = rows.get(row, "meta_value").unwrap_or_default();
        let changed = match rows.get(row, "meta_key").unwrap_or_default() {
            "_product_image_gallery" => map_csv(value, &mut |n| Some(lookup(n).unwrap_or(n))),
            "_elementor_data" => map_elementor(value, &mut |n| lookup(n)),
            _ => map_serialized(value, &mut |n| Some(lookup(n).unwrap_or(n)))
                .unwrap_or_else(|| value.to_string()),
        };
        if changed != value {
            sql.push_str(&format!(
                "UPDATE {meta} SET meta_value = {} WHERE meta_id = {meta_id};\n",
                literal(&changed)
            ));
        }
    }
    sql.push_str("COMMIT;\n");
    client.execute(database, &sql)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mapper(pairs: &[(u64, u64)]) -> impl FnMut(u64, RefKind) -> Option<u64> + '_ {
        move |n, _| pairs.iter().find(|(a, _)| *a == n).map(|(_, b)| *b)
    }

    #[test]
    fn changes_numbers_in_block_content() {
        let content = concat!(
            "<!-- wp:image {\"id\":12,\"sizeSlug\":\"large\"} -->\n",
            "<figure class=\"wp-block-image size-large\"><img src=\"a.jpg\" class=\"wp-image-12\"/></figure>\n",
            "<!-- /wp:image -->\n",
            "<!-- wp:gallery {\"ids\":[12,13,99],\"linkTo\":\"none\"} /-->\n",
            "<!-- wp:block {\"ref\":40} /-->\n",
            "<!-- wp:media-text {\"mediaId\":13} -->x<!-- /wp:media-text -->\n",
            "[gallery ids=\"12,13\"] [vc_single_image image=\"13\"]\n",
            "<p>Order 12 is not an image.</p>",
        );
        let pairs = [(12, 512), (13, 513), (40, 540)];
        let out = map_content(content, &mut mapper(&pairs));
        assert!(out.contains("{\"id\":512,\"sizeSlug\":\"large\"}"));
        assert!(out.contains("wp-image-512"));
        assert!(out.contains("\"ids\":[512,513,99]"));
        assert!(out.contains("{\"ref\":540}"));
        assert!(out.contains("{\"mediaId\":513}"));
        assert!(out.contains("[gallery ids=\"512,513\"]"));
        assert!(out.contains("image=\"513\""));
        assert!(out.contains("Order 12 is not an image."));
        let refs = refs_in_content(content);
        assert!(refs.contains(&(40, RefKind::Block)));
        assert!(refs.contains(&(13, RefKind::Media)));
    }

    #[test]
    fn changes_numbers_in_meta_values() {
        assert_eq!(
            map_csv("4,5,6", &mut |n| (n != 5).then_some(n + 100)),
            "104,106"
        );
        assert_eq!(ids_in_csv("4, 5,x"), vec![4, 5]);
        let upsells = "a:2:{i:0;i:7;i:1;i:8;}";
        assert_eq!(ids_in_serialized(upsells), vec![7, 8]);
        assert_eq!(
            map_serialized(upsells, &mut |n| (n == 7).then_some(70)).unwrap(),
            "a:1:{i:0;i:70;}"
        );
        let acf = "a:2:{i:0;s:1:\"7\";i:1;s:1:\"8\";}";
        assert_eq!(
            map_serialized(acf, &mut |n| Some(n + 1)).unwrap(),
            "a:2:{i:0;s:1:\"8\";i:1;s:1:\"9\";}"
        );
        let elementor =
            r#"[{"id":"a1b2","settings":{"image":{"url":"x.jpg","id":31},"template_id":"77"}}]"#;
        assert_eq!(
            map_elementor(elementor, &mut |n| (n == 31).then_some(331)),
            r#"[{"id":"a1b2","settings":{"image":{"url":"x.jpg","id":331},"template_id":"77"}}]"#
        );
        assert_eq!(elementor_templates(elementor), vec![77]);
    }

    #[test]
    fn lists_an_images_files() {
        let meta = r#"a:3:{s:4:"file";s:13:"2026/10/a.jpg";s:5:"sizes";a:1:{s:9:"thumbnail";a:1:{s:4:"file";s:17:"a-150x150.jpg";}}s:14:"original_image";s:9:"a-big.jpg";}"#;
        let meta = meta.replace("s:17:\"a-150x150.jpg\"", "s:13:\"a-150x150.jpg\"");
        assert_eq!(
            attachment_files(Some("2026/10/a.jpg"), Some(&meta)),
            vec![
                "2026/10/a-150x150.jpg",
                "2026/10/a-big.jpg",
                "2026/10/a.jpg"
            ]
        );
        assert!(attachment_files(Some("../x"), None).is_empty());
    }
}
