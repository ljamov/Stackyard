//! The one-time PHP helper that reads a live site's database when the
//! server has no `mysqldump` Stackyard can run, e.g. on hosting with FTP only.
//!
//! Stackyard writes it for one clone or push, with the database settings
//! from the live `wp-config.php` and the hash of a random key in it, and
//! uploads it into the site's folder. It works from the command line (over
//! SSH) and over HTTPS, where it answers only POST requests carrying the
//! key, and it deletes itself when Stackyard is done or, at the latest,
//! when its time is up, even if the transfer stopped halfway.
//!
//! A helper made for a clone only reads: it lists the tables and exports
//! them in chunks, so a large table goes over in many small requests and a
//! broken connection costs one chunk, not the whole table. A helper made
//! for a push ([`make_writing`]) can also load a `.sql.gz` file Stackyard
//! uploaded next to it, statement by statement, a few seconds at a time.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::config::quote;

/// How long a helper works before it deletes itself.
pub const LIFETIME_SECS: u64 = 30 * 60;

/// The database settings the helper connects with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DbSettings {
    pub host: String,
    pub user: String,
    pub password: String,
    pub name: String,
    pub charset: String,
    pub prefix: String,
}

/// A helper made for one transfer.
#[derive(Debug, Clone)]
pub struct Helper {
    /// The file name it is uploaded as, e.g. `stackyard-k3j9x2.php`.
    pub file: String,
    /// The key Stackyard sends; only its hash is in the file.
    pub key: String,
    pub source: String,
}

/// Writes a helper that only reads, working for `lifetime` seconds after
/// it lands on the server. The server's own clock counts them, so a
/// computer whose clock is off can't make it expire at once.
pub fn make(db: &DbSettings, lifetime: u64) -> crate::Result<Helper> {
    make_with(db, lifetime, false)
}

/// Writes a helper that can also load SQL files into the database.
pub fn make_writing(db: &DbSettings, lifetime: u64) -> crate::Result<Helper> {
    make_with(db, lifetime, true)
}

/// A name for an SQL file uploaded for the helper to load: random, so it
/// can't be guessed while it is on the server.
pub fn sql_file_name() -> crate::Result<String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes)
        .map_err(|e| crate::Error::WordPress(format!("no randomness: {e}")))?;
    let tail: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    Ok(format!("stackyard-{tail}.sql.gz"))
}

fn make_with(db: &DbSettings, lifetime: u64, write: bool) -> crate::Result<Helper> {
    let mut bytes = [0u8; 24];
    getrandom::fill(&mut bytes)
        .map_err(|e| crate::Error::WordPress(format!("no randomness: {e}")))?;
    let key: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    // A name of its own: one made from the key would show part of it in
    // every server log line that names the file.
    let mut name = [0u8; 6];
    getrandom::fill(&mut name)
        .map_err(|e| crate::Error::WordPress(format!("no randomness: {e}")))?;
    let name: String = name.iter().map(|b| format!("{b:02x}")).collect();
    let file = format!("stackyard-{name}.php");
    let hash: String = Sha256::digest(key.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let source = SOURCE
        .replace("{KEY_HASH}", &hash)
        .replace("{LIFETIME}", &lifetime.to_string())
        .replace("{WRITE}", if write { "true" } else { "false" })
        .replace("{HOST}", &quote(&db.host))
        .replace("{USER}", &quote(&db.user))
        .replace("{PASSWORD}", &quote(&db.password))
        .replace("{NAME}", &quote(&db.name))
        .replace("{CHARSET}", &quote(&db.charset))
        .replace("{PREFIX}", &quote(&db.prefix))
        .replace("{CONTENT_SQL}", &quote(CONTENT_SQL));
    Ok(Helper { file, key, source })
}

/// What `do=info` answers.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct Info {
    pub php: String,
    pub mysql: String,
    #[serde(default)]
    pub wordpress: String,
    #[serde(default)]
    pub siteurl: String,
    #[serde(default)]
    pub home: String,
    /// The folder the helper is in: WordPress's own folder on the server.
    #[serde(default)]
    pub abspath: String,
    pub tables: Vec<Table>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct Table {
    pub name: String,
    /// An estimate, as MySQL keeps it.
    pub rows: u64,
    pub bytes: u64,
}

/// How much a part of the site's files holds.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Part {
    pub files: u64,
    pub bytes: u64,
    /// Themes or plugins in it (their folders, and single-file plugins).
    #[serde(default)]
    pub items: u64,
}

/// What `do=sizes` answers: the WordPress folder's files by part.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sizes {
    /// `wp-admin`, `wp-includes` and WordPress's own files at the top.
    pub core: Part,
    pub themes: Part,
    pub plugins: Part,
    pub uploads: Part,
    /// Everything else that is copied, e.g. `wp-content/languages`.
    pub other: Part,
    /// Caches and backups the clone leaves out; not in [`Sizes::total`].
    pub skipped: Part,
    /// `false` when counting took too long and stopped partway.
    pub complete: bool,
    /// Each theme and plugin with its size, biggest first, when the
    /// Stackyard Connect plugin counted; the helper leaves it out.
    #[serde(default)]
    pub items: Vec<Item>,
}

/// A theme or plugin and how much it holds.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Item {
    /// `theme` or `plugin`.
    pub kind: String,
    /// Its folder, or the file of a single-file plugin.
    pub slug: String,
    pub bytes: u64,
}

/// The seconds the helper counts files for before it answers with what it has.
pub const SIZES_SECS: u64 = 15;

/// The seconds one `do=files` request lists for over HTTPS, kept well under
/// the time proxies and hosts allow a request; over SSH nothing cuts it off.
pub const FILES_SECS_HTTP: u64 = 20;
pub const FILES_SECS_CLI: u64 = 120;

/// One answer of `do=files`: each file as `[path, size, date]`, and the
/// folders still to list when its time ran out.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct FilesPage {
    pub files: Vec<(String, u64, i64)>,
    #[serde(default)]
    pub next: Option<Vec<String>>,
}

impl Sizes {
    /// Everything that is copied.
    pub fn total(&self) -> Part {
        [
            self.core,
            self.themes,
            self.plugins,
            self.uploads,
            self.other,
        ]
        .iter()
        .fold(Part::default(), |a, p| Part {
            files: a.files + p.files,
            bytes: a.bytes + p.bytes,
            items: 0,
        })
    }

    /// Counts one file at `rel` (relative to WordPress's folder, with `/`).
    pub fn add(&mut self, rel: &str, bytes: u64, skip: &[String]) {
        let part = match part_of(rel, skip) {
            "core" => &mut self.core,
            "themes" => &mut self.themes,
            "plugins" => &mut self.plugins,
            "uploads" => &mut self.uploads,
            "skipped" => &mut self.skipped,
            _ => &mut self.other,
        };
        part.files += 1;
        part.bytes += bytes;
    }
}

/// Which part of the site a file or folder belongs to; the helper's
/// `sy_part` does the same on the server.
pub fn part_of(rel: &str, skip: &[String]) -> &'static str {
    if crate::remote::sync::skipped(rel, skip) {
        return "skipped";
    }
    for dir in ["themes", "plugins", "uploads"] {
        if rel.starts_with(&format!("wp-content/{dir}/")) {
            return dir;
        }
    }
    if ["wp-admin", "wp-includes"]
        .iter()
        .any(|d| rel == *d || rel.starts_with(&format!("{d}/")))
    {
        return "core";
    }
    if !rel.contains('/') && rel != "wp-config.php" && is_core_file(rel) {
        return "core";
    }
    "other"
}

fn is_core_file(name: &str) -> bool {
    matches!(
        name,
        "index.php" | "xmlrpc.php" | "license.txt" | "readme.html"
    ) || (name.starts_with("wp-")
        && name.ends_with(".php")
        && name[3..name.len() - 4]
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b == b'-'))
}

/// What `do=import` answers.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct Imported {
    pub statements: u64,
    /// Where to continue in the uncompressed file; pass it back as `from`.
    pub next: u64,
    pub done: bool,
}

/// One option as `do=options` answers it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct OptionValue {
    /// Base64, since option values need not be text.
    pub value: String,
    #[serde(default)]
    pub autoload: String,
}

/// Post types and comment types that belong to a store's orders, left out
/// of the content fingerprint (see [`CONTENT_SQL`]).
pub const ORDER_POST_TYPES: &str =
    "'shop_order','shop_order_refund','shop_order_placehold','shop_subscription'";

/// The query behind a database's content fingerprint: posts and comments,
/// orders and their notes left out. `{posts}` and `{comments}` are the
/// tables. The helper runs the same one on the live side.
pub const CONTENT_SQL: &str = "SELECT CONCAT_WS(':', \
    (SELECT COUNT(*) FROM {posts} WHERE post_type NOT IN ('shop_order','shop_order_refund','shop_order_placehold','shop_subscription','revision') AND post_status <> 'auto-draft'), \
    (SELECT COALESCE(MAX(ID), 0) FROM {posts} WHERE post_type NOT IN ('shop_order','shop_order_refund','shop_order_placehold','shop_subscription','revision') AND post_status <> 'auto-draft'), \
    (SELECT COALESCE(MAX(post_modified_gmt), '') FROM {posts} WHERE post_type NOT IN ('shop_order','shop_order_refund','shop_order_placehold','shop_subscription','revision') AND post_status <> 'auto-draft'), \
    (SELECT COUNT(*) FROM {comments} WHERE comment_type NOT IN ('order_note','action_log','webhook_delivery')), \
    (SELECT COALESCE(MAX(comment_ID), 0) FROM {comments} WHERE comment_type NOT IN ('order_note','action_log','webhook_delivery')))";

/// What a `do=table` chunk says about itself.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct Chunk {
    pub rows: u64,
    /// Where the next chunk starts; pass it back as `from`.
    pub next: String,
    /// The table is complete with this chunk.
    pub done: bool,
    /// The chunk is gzip-compressed.
    pub gz: bool,
}

/// The line before the chunk's metadata in the command line's error output.
pub const META_MARK: &str = "STACKYARD-META ";

/// Finds the metadata in what the helper wrote to its error output.
pub fn meta_from_errors(errors: &str) -> Option<Chunk> {
    errors
        .lines()
        .rev()
        .find_map(|line| line.strip_prefix(META_MARK))
        .and_then(|json| serde_json::from_str(json).ok())
}

/// The largest file `do=pack` sends; larger ones are downloaded one by one.
pub const PACK_FILE_BYTES: u64 = 8 * 1024 * 1024;
/// About how much one `do=pack` answer holds.
pub const PACK_BYTES: u64 = 64 * 1024 * 1024;

/// One line of a `do=pack` answer.
#[derive(Debug, Deserialize)]
struct PackLine {
    #[serde(default)]
    p: Option<String>,
    #[serde(default)]
    s: Option<u64>,
    #[serde(default)]
    m: i64,
    #[serde(default)]
    missing: bool,
    #[serde(default)]
    next: Option<usize>,
    #[serde(default)]
    done: bool,
}

/// Writes the files of a `do=pack` answer under `root` as the answer
/// arrives. Only paths in `wanted` are written, so an answer can't put a
/// file anywhere else.
pub struct Unpacker<'a> {
    root: &'a std::path::Path,
    wanted: &'a std::collections::BTreeSet<String>,
    buf: Vec<u8>,
    body: Option<(String, u64, i64, Vec<u8>)>,
    /// Paths written so far.
    pub written: Vec<String>,
    /// Where the next request starts, when the answer stopped early.
    pub next: Option<usize>,
    /// The answer reached its end.
    pub done: bool,
    error: Option<String>,
}

impl<'a> Unpacker<'a> {
    pub fn new(root: &'a std::path::Path, wanted: &'a std::collections::BTreeSet<String>) -> Self {
        Unpacker {
            root,
            wanted,
            buf: Vec::new(),
            body: None,
            written: Vec::new(),
            next: None,
            done: false,
            error: None,
        }
    }

    /// What went wrong reading the answer, if anything.
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    fn step(&mut self) -> std::result::Result<bool, String> {
        if let Some((_, left, _, data)) = self.body.as_mut() {
            let take = (*left).min(self.buf.len() as u64) as usize;
            data.extend(self.buf.drain(..take));
            *left -= take as u64;
            if *left > 0 {
                return Ok(false);
            }
            let (rel, _, modified, data) = self.body.take().unwrap_or_default();
            self.save(&rel, modified, &data)?;
            self.written.push(rel);
            return Ok(true);
        }
        let Some(end) = self.buf.iter().position(|&b| b == b'\n') else {
            return Ok(false);
        };
        let line: Vec<u8> = self.buf.drain(..=end).collect();
        let line: PackLine = serde_json::from_slice(&line).map_err(|_| {
            format!(
                "the helper sent {:?}",
                String::from_utf8_lossy(&line[..line.len().min(120)])
            )
        })?;
        match line {
            PackLine { done: true, .. } => self.done = true,
            PackLine { next: Some(n), .. } => self.next = Some(n),
            PackLine {
                p: Some(_),
                missing: true,
                ..
            } => {}
            PackLine {
                p: Some(p),
                s: Some(s),
                m,
                ..
            } if s <= PACK_FILE_BYTES => {
                self.body = Some((p, s, m, Vec::with_capacity(s as usize)));
            }
            _ => return Err("the helper's answer could not be read".into()),
        }
        Ok(true)
    }

    fn save(&self, rel: &str, modified: i64, data: &[u8]) -> std::result::Result<(), String> {
        // A backslash or colon would be a separator or a drive on Windows.
        if !self.wanted.contains(rel)
            || rel.contains(['\\', ':'])
            || rel.split('/').any(|p| p.is_empty() || p == "..")
        {
            return Err(format!("the helper sent {rel}, which was not asked for"));
        }
        let path = self.root.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        std::fs::write(&path, data).map_err(|e| format!("{}: {e}", path.display()))?;
        if modified > 0 {
            let time = std::time::UNIX_EPOCH + std::time::Duration::from_secs(modified as u64);
            if let Ok(file) = std::fs::File::options().write(true).open(&path) {
                let _ = file.set_modified(time);
            }
        }
        Ok(())
    }
}

impl std::io::Write for Unpacker<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self.error.is_none() {
            self.buf.extend_from_slice(bytes);
            loop {
                match self.step() {
                    Ok(true) => continue,
                    Ok(false) => break,
                    Err(e) => {
                        self.error = Some(e);
                        self.buf.clear();
                        break;
                    }
                }
            }
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

const SOURCE: &str = r#"<?php
/*
 * Stackyard helper. Stackyard put this file here to copy this site's
 * database to or from a computer. It answers only requests that carry its
 * key, and deletes itself when Stackyard is done or when its time is up.
 * It is safe to delete by hand.
 */
define( 'SY_KEY_HASH', '{KEY_HASH}' );
define( 'SY_LIFETIME', {LIFETIME} );
define( 'SY_WRITE', {WRITE} );
define( 'SY_PREFIX', {PREFIX} );
define( 'SY_CHUNK_BYTES', 8 * 1024 * 1024 );
define( 'SY_CHUNK_ROWS', 20000 );
define( 'SY_STATEMENT_BYTES', 1024 * 1024 );
$sy_db = array(
	'host'     => {HOST},
	'user'     => {USER},
	'password' => {PASSWORD},
	'name'     => {NAME},
	'charset'  => {CHARSET},
);

error_reporting( 0 );
@ini_set( 'display_errors', '0' );
@set_time_limit( 300 );

function sy_cli() {
	return PHP_SAPI === 'cli';
}

function sy_fail( $status, $message ) {
	if ( sy_cli() ) {
		fwrite( STDERR, $message . "\n" );
		exit( 1 );
	}
	http_response_code( $status );
	header( 'Content-Type: application/json' );
	header( 'Cache-Control: no-store' );
	echo json_encode( array( 'error' => $message ) );
	exit;
}

function sy_meta( $meta ) {
	if ( sy_cli() ) {
		fwrite( STDERR, "\nSTACKYARD-META " . json_encode( $meta ) . "\n" );
	} else {
		header( 'X-Stackyard-Meta: ' . json_encode( $meta ) );
	}
}

function sy_json( $data ) {
	if ( ! sy_cli() ) {
		header( 'Content-Type: application/json' );
		header( 'Cache-Control: no-store' );
	}
	echo json_encode( $data );
	exit;
}

// Counted from when the file landed here (its inode change time, which
// an upload can't set), by this server's clock.
clearstatcache();
$sy_born = @filectime( __FILE__ );
if ( ! $sy_born || time() > $sy_born + SY_LIFETIME ) {
	@unlink( __FILE__ );
	sy_fail( 410, 'This Stackyard helper has expired and removed itself.' );
}

if ( sy_cli() ) {
	$sy_args = array();
	foreach ( array_slice( $argv, 1 ) as $sy_arg ) {
		$sy_at = strpos( $sy_arg, '=' );
		if ( false !== $sy_at ) {
			$sy_args[ substr( $sy_arg, 0, $sy_at ) ] = substr( $sy_arg, $sy_at + 1 );
		}
	}
} else {
	$sy_key = '';
	if ( isset( $_SERVER['HTTP_X_STACKYARD_KEY'] ) ) {
		$sy_key = (string) $_SERVER['HTTP_X_STACKYARD_KEY'];
	} elseif ( isset( $_POST['key'] ) ) {
		$sy_key = (string) $_POST['key'];
	}
	if ( 'POST' !== $_SERVER['REQUEST_METHOD'] || '' === $sy_key || ! hash_equals( SY_KEY_HASH, hash( 'sha256', $sy_key ) ) ) {
		sy_fail( 404, 'Not found' );
	}
	$sy_args = $_POST;
}

$sy_do = isset( $sy_args['do'] ) ? (string) $sy_args['do'] : '';

if ( 'finish' === $sy_do ) {
	sy_json( array( 'removed' => @unlink( __FILE__ ) ) );
}
if ( 'ping' === $sy_do ) {
	sy_json( array( 'php' => PHP_VERSION, 'abspath' => __DIR__ ) );
}

// Which part of the site a file belongs to; Rust's `part_of` does the same.
function sy_part( $path, $skip ) {
	foreach ( $skip as $pattern ) {
		$pattern = (string) $pattern;
		if ( false !== strpos( $pattern, '/' ) ) {
			if ( $path === $pattern || 0 === strpos( $path, $pattern . '/' ) ) {
				return 'skipped';
			}
		} else {
			foreach ( explode( '/', $path ) as $name ) {
				if ( function_exists( 'fnmatch' ) ? fnmatch( $pattern, $name ) : $pattern === $name ) {
					return 'skipped';
				}
			}
		}
	}
	foreach ( array( 'themes', 'plugins', 'uploads' ) as $dir ) {
		if ( 0 === strpos( $path, 'wp-content/' . $dir . '/' ) ) {
			return $dir;
		}
	}
	if ( 'wp-admin' === $path || 'wp-includes' === $path || 0 === strpos( $path, 'wp-admin/' ) || 0 === strpos( $path, 'wp-includes/' ) ) {
		return 'core';
	}
	if ( false === strpos( $path, '/' ) && 'wp-config.php' !== $path && preg_match( '/^(wp-[a-z-]+\.php|index\.php|xmlrpc\.php|license\.txt|readme\.html)$/', $path ) ) {
		return 'core';
	}
	return 'other';
}

// Counts the files under this folder by part, for at most $seconds.
function sy_sizes( $root, $skip, $seconds ) {
	$parts = array();
	foreach ( array( 'core', 'themes', 'plugins', 'uploads', 'other', 'skipped' ) as $name ) {
		$parts[ $name ] = array( 'files' => 0, 'bytes' => 0, 'items' => 0 );
	}
	$until    = microtime( true ) + $seconds;
	$complete = true;
	$self     = basename( __FILE__ );
	$stack    = array( '' );
	while ( $stack ) {
		if ( microtime( true ) > $until ) {
			$complete = false;
			break;
		}
		$rel    = array_pop( $stack );
		$handle = @opendir( '' === $rel ? $root : $root . '/' . $rel );
		if ( ! $handle ) {
			continue;
		}
		while ( false !== ( $name = readdir( $handle ) ) ) {
			if ( '.' === $name || '..' === $name ) {
				continue;
			}
			if ( '' === $rel && ( $self === $name || preg_match( '/^stackyard-[a-f0-9]+\.sql\.gz$/', $name ) ) ) {
				continue;
			}
			$path = '' === $rel ? $name : $rel . '/' . $name;
			$full = $root . '/' . $path;
			if ( is_link( $full ) ) {
				continue;
			}
			$part = sy_part( $path, $skip );
			$top  = 'wp-content/themes' === $rel || 'wp-content/plugins' === $rel;
			if ( is_dir( $full ) ) {
				if ( $top && 'skipped' !== $part ) {
					++$parts[ $part ]['items'];
				}
				$stack[] = $path;
			} else {
				++$parts[ $part ]['files'];
				$parts[ $part ]['bytes'] += (int) @filesize( $full );
				if ( 'wp-content/plugins' === $rel && 'index.php' !== $name && '.php' === substr( $name, -4 ) ) {
					++$parts[ $part ]['items'];
				}
			}
		}
		closedir( $handle );
	}
	$parts['complete'] = $complete;
	return $parts;
}

if ( 'sizes' === $sy_do ) {
	$sy_skip = isset( $sy_args['skip'] ) ? json_decode( (string) $sy_args['skip'], true ) : array();
	sy_json( sy_sizes( __DIR__, is_array( $sy_skip ) ? $sy_skip : array(), isset( $sy_args['seconds'] ) ? max( 1, (int) $sy_args['seconds'] ) : 15 ) );
}

// Lists the files under this folder with their size and date, for at most
// $seconds; `next` holds the folders still to list, for the next request.
// Linked folders are left out (they can loop); linked files are listed.
function sy_files( $root, $skip, $seconds, $stack ) {
	$files = array();
	$until = microtime( true ) + $seconds;
	$self  = basename( __FILE__ );
	while ( $stack ) {
		if ( microtime( true ) > $until ) {
			break;
		}
		$rel = (string) array_pop( $stack );
		// Never outside this folder, whatever `from` asked for.
		if ( false !== strpos( '/' . $rel . '/', '/../' ) || 0 === strpos( $rel, '/' ) ) {
			continue;
		}
		$handle = @opendir( '' === $rel ? $root : $root . '/' . $rel );
		if ( ! $handle ) {
			continue;
		}
		while ( false !== ( $name = readdir( $handle ) ) ) {
			if ( '.' === $name || '..' === $name ) {
				continue;
			}
			if ( '' === $rel && ( $self === $name || 0 === strpos( $name, 'stackyard-' ) ) ) {
				continue;
			}
			$path = '' === $rel ? $name : $rel . '/' . $name;
			if ( 'skipped' === sy_part( $path, $skip ) ) {
				continue;
			}
			$full = $root . '/' . $path;
			if ( is_dir( $full ) ) {
				if ( ! is_link( $full ) ) {
					$stack[] = $path;
				}
			} elseif ( is_file( $full ) ) {
				$files[] = array( $path, (int) @filesize( $full ), (int) @filemtime( $full ) );
			}
		}
		closedir( $handle );
	}
	return array(
		'files' => $files,
		'next'  => $stack ? array_values( $stack ) : null,
	);
}

// Sends the files asked for in one answer, each as a JSON line followed by
// exactly `s` bytes, until $bytes or $seconds are spent; the last line says
// where to go on from. Only files up to SY_PACK_FILE_BYTES are packed, read
// whole, so the size sent is the size of what follows even if a file
// changes meanwhile.
define( 'SY_PACK_FILE_BYTES', 8 * 1024 * 1024 );
function sy_pack( $root, $paths, $start, $bytes, $seconds ) {
	if ( ! sy_cli() ) {
		header( 'Content-Type: application/octet-stream' );
		header( 'Cache-Control: no-store' );
	}
	$until = microtime( true ) + $seconds;
	$sent  = 0;
	$count = count( $paths );
	for ( $i = $start; $i < $count; $i++ ) {
		if ( $i > $start && ( $sent >= $bytes || microtime( true ) > $until ) ) {
			echo json_encode( array( 'next' => $i ) ) . "\n";
			return;
		}
		$path = (string) $paths[ $i ];
		$full = $root . '/' . $path;
		$safe = '' !== $path && 0 !== strpos( $path, '/' ) && false === strpos( '/' . $path . '/', '/../' );
		if ( ! $safe || ! is_file( $full ) || @filesize( $full ) > SY_PACK_FILE_BYTES ) {
			echo json_encode( array( 'p' => $path, 'missing' => true ) ) . "\n";
			continue;
		}
		$data = @file_get_contents( $full );
		if ( false === $data ) {
			echo json_encode( array( 'p' => $path, 'missing' => true ) ) . "\n";
			continue;
		}
		echo json_encode( array( 'p' => $path, 's' => strlen( $data ), 'm' => (int) @filemtime( $full ) ) ) . "\n";
		echo $data;
		$sent += strlen( $data );
		flush();
	}
	echo json_encode( array( 'done' => true ) ) . "\n";
}

if ( 'pack' === $sy_do ) {
	$sy_paths = isset( $sy_args['paths'] ) ? json_decode( (string) $sy_args['paths'], true ) : array();
	sy_pack(
		__DIR__,
		is_array( $sy_paths ) ? array_values( $sy_paths ) : array(),
		isset( $sy_args['start'] ) ? max( 0, (int) $sy_args['start'] ) : 0,
		isset( $sy_args['bytes'] ) ? max( 1, (int) $sy_args['bytes'] ) : 64 * 1024 * 1024,
		isset( $sy_args['seconds'] ) ? max( 1, (int) $sy_args['seconds'] ) : 20
	);
	exit;
}

if ( 'files' === $sy_do ) {
	$sy_skip = isset( $sy_args['skip'] ) ? json_decode( (string) $sy_args['skip'], true ) : array();
	$sy_from = isset( $sy_args['from'] ) ? json_decode( (string) $sy_args['from'], true ) : array( '' );
	sy_json( sy_files( __DIR__, is_array( $sy_skip ) ? $sy_skip : array(), isset( $sy_args['seconds'] ) ? max( 1, (int) $sy_args['seconds'] ) : 20, is_array( $sy_from ) ? $sy_from : array( '' ) ) );
}

if ( ! class_exists( 'mysqli' ) ) {
	sy_fail( 500, 'PHP on this server has no mysqli extension.' );
}

function sy_connect( $db ) {
	$host   = $db['host'];
	$port   = null;
	$socket = null;
	if ( 0 === strpos( $host, 'p:' ) ) {
		$host = substr( $host, 2 );
	}
	if ( preg_match( '/^(.*):(\d+)$/', $host, $m ) ) {
		$host = $m[1];
		$port = (int) $m[2];
	} elseif ( false !== strpos( $host, ':/' ) ) {
		$socket = substr( $host, strpos( $host, ':' ) + 1 );
		$host   = substr( $host, 0, strpos( $host, ':' ) );
	}
	if ( '' === $host ) {
		$host = 'localhost';
	}
	mysqli_report( MYSQLI_REPORT_OFF );
	$link = mysqli_init();
	if ( ! @$link->real_connect( $host, $db['user'], $db['password'], $db['name'], $port, $socket ) ) {
		sy_fail( 500, 'Could not connect to the database: ' . mysqli_connect_error() );
	}
	$charset = '' !== $db['charset'] ? $db['charset'] : 'utf8mb4';
	if ( ! $link->set_charset( $charset ) ) {
		$link->set_charset( 'utf8' );
	}
	return $link;
}

function sy_tables( $link ) {
	$tables = array();
	$result = $link->query( 'SHOW TABLE STATUS' );
	if ( ! $result ) {
		sy_fail( 500, 'Could not list the tables: ' . $link->error );
	}
	while ( $row = $result->fetch_assoc() ) {
		// Views have no engine; only the site's own tables are copied.
		if ( null === $row['Engine'] || 0 !== strpos( $row['Name'], SY_PREFIX ) ) {
			continue;
		}
		$tables[ $row['Name'] ] = array(
			'name'  => $row['Name'],
			'rows'  => (int) $row['Rows'],
			'bytes' => (int) $row['Data_length'] + (int) $row['Index_length'],
		);
	}
	return $tables;
}

function sy_name( $name ) {
	return '`' . str_replace( '`', '``', $name ) . '`';
}

$sy_link = sy_connect( $sy_db );

if ( 'info' === $sy_do ) {
	$sy_options = array();
	$sy_result  = $sy_link->query( 'SELECT option_name, option_value FROM ' . sy_name( SY_PREFIX . 'options' ) . " WHERE option_name IN ('siteurl', 'home')" );
	if ( $sy_result ) {
		while ( $sy_row = $sy_result->fetch_row() ) {
			$sy_options[ $sy_row[0] ] = $sy_row[1];
		}
	}
	$wp_version = '';
	if ( is_file( __DIR__ . '/wp-includes/version.php' ) ) {
		include __DIR__ . '/wp-includes/version.php';
	}
	sy_json(
		array(
			'php'       => PHP_VERSION,
			'mysql'     => $sy_link->server_info,
			'wordpress' => $wp_version,
			'siteurl'   => isset( $sy_options['siteurl'] ) ? $sy_options['siteurl'] : '',
			'home'      => isset( $sy_options['home'] ) ? $sy_options['home'] : '',
			'abspath'   => __DIR__,
			'tables'    => array_values( sy_tables( $sy_link ) ),
		)
	);
}

if ( 'facts' === $sy_do ) {
	$sy_o     = sy_name( SY_PREFIX . 'options' );
	$sy_facts = array(
		'prefix'         => SY_PREFIX,
		'options'        => new stdClass(),
		'active_plugins' => array(),
		'stylesheet'     => '',
		'template'       => '',
	);
	$sy_hashes = array();
	$sy_result = $sy_link->query( "SELECT option_name, SHA2(option_value, 256) FROM $sy_o" );
	while ( $sy_result && $sy_row = $sy_result->fetch_row() ) {
		$sy_hashes[ $sy_row[0] ] = $sy_row[1];
	}
	if ( $sy_hashes ) {
		$sy_facts['options'] = $sy_hashes;
	}
	$sy_result = $sy_link->query( "SELECT option_name, option_value FROM $sy_o WHERE option_name IN ('active_plugins', 'stylesheet', 'template')" );
	while ( $sy_result && $sy_row = $sy_result->fetch_row() ) {
		if ( 'active_plugins' === $sy_row[0] ) {
			$sy_list = PHP_VERSION_ID >= 70000 ? @unserialize( $sy_row[1], array( 'allowed_classes' => false ) ) : @unserialize( $sy_row[1] );
			$sy_facts['active_plugins'] = is_array( $sy_list ) ? array_values( array_map( 'strval', $sy_list ) ) : array();
		} else {
			$sy_facts[ $sy_row[0] ] = $sy_row[1];
		}
	}
	foreach ( array( 'max_post_id' => array( 'posts', 'ID' ), 'max_comment_id' => array( 'comments', 'comment_ID' ), 'max_user_id' => array( 'users', 'ID' ) ) as $sy_key => $sy_tc ) {
		$sy_result         = $sy_link->query( 'SELECT COALESCE(MAX(' . sy_name( $sy_tc[1] ) . '), 0) FROM ' . sy_name( SY_PREFIX . $sy_tc[0] ) );
		$sy_row            = $sy_result ? $sy_result->fetch_row() : null;
		$sy_facts[ $sy_key ] = $sy_row ? (int) $sy_row[0] : 0;
	}
	$sy_result           = $sy_link->query( str_replace( array( '{posts}', '{comments}' ), array( sy_name( SY_PREFIX . 'posts' ), sy_name( SY_PREFIX . 'comments' ) ), {CONTENT_SQL} ) );
	$sy_row              = $sy_result ? $sy_result->fetch_row() : null;
	$sy_facts['content'] = $sy_row ? (string) $sy_row[0] : '';
	sy_json( $sy_facts );
}

if ( 'options' === $sy_do ) {
	$sy_names = isset( $sy_args['names'] ) ? json_decode( (string) $sy_args['names'], true ) : array();
	$sy_out   = new stdClass();
	if ( is_array( $sy_names ) && $sy_names ) {
		$sy_list = array();
		foreach ( $sy_names as $sy_name ) {
			$sy_list[] = "'" . $sy_link->real_escape_string( (string) $sy_name ) . "'";
		}
		$sy_result = $sy_link->query( 'SELECT option_name, option_value, autoload FROM ' . sy_name( SY_PREFIX . 'options' ) . ' WHERE option_name IN (' . implode( ',', $sy_list ) . ')' );
		while ( $sy_result && $sy_row = $sy_result->fetch_row() ) {
			$sy_out->{$sy_row[0]} = array(
				'value'    => base64_encode( $sy_row[1] ),
				'autoload' => $sy_row[2],
			);
		}
	}
	sy_json( $sy_out );
}

if ( 'import' === $sy_do ) {
	if ( ! SY_WRITE ) {
		sy_fail( 403, 'This helper only reads.' );
	}
	$sy_file = isset( $sy_args['file'] ) ? basename( (string) $sy_args['file'] ) : '';
	if ( ! preg_match( '/^stackyard-[a-f0-9]{24,}\.sql\.gz$/', $sy_file ) || ! is_file( __DIR__ . '/' . $sy_file ) ) {
		sy_fail( 404, 'No such file.' );
	}
	$sy_from  = isset( $sy_args['from'] ) ? max( 0, (int) $sy_args['from'] ) : 0;
	$sy_whole = ! empty( $sy_args['whole'] );
	$sy_gz    = gzopen( __DIR__ . '/' . $sy_file, 'rb' );
	if ( ! $sy_gz ) {
		sy_fail( 500, 'Could not open the file.' );
	}
	if ( $sy_from > 0 && 0 !== gzseek( $sy_gz, $sy_from ) ) {
		sy_fail( 500, 'Could not continue in the file.' );
	}
	$sy_link->query( 'SET FOREIGN_KEY_CHECKS = 0' );
	$sy_link->query( "SET SESSION sql_mode = 'NO_AUTO_VALUE_ON_ZERO'" );
	$sy_started   = time();
	$sy_statement = '';
	$sy_count     = 0;
	$sy_pos       = $sy_from;
	$sy_next      = $sy_from;
	$sy_done      = false;
	while ( true ) {
		// A line, however long: dumps keep each row group on one line.
		$sy_line = '';
		while ( false !== ( $sy_part = gzgets( $sy_gz, 1048576 ) ) ) {
			$sy_line .= $sy_part;
			if ( "\n" === substr( $sy_part, -1 ) ) {
				break;
			}
		}
		if ( '' === $sy_line ) {
			$sy_done = true;
			break;
		}
		$sy_pos += strlen( $sy_line );
		$sy_trim = rtrim( $sy_line );
		if ( '' === $sy_statement && ( '' === $sy_trim || 0 === strpos( $sy_trim, '--' ) ) ) {
			$sy_next = $sy_pos;
			continue;
		}
		$sy_statement .= $sy_line;
		if ( ';' === substr( $sy_trim, -1 ) ) {
			if ( ! $sy_link->query( $sy_statement ) ) {
				sy_fail( 500, 'The database refused a statement: ' . $sy_link->error . ' (' . substr( $sy_statement, 0, 160 ) . ')' );
			}
			$sy_statement = '';
			$sy_next      = $sy_pos;
			++$sy_count;
			if ( ! $sy_whole && time() - $sy_started >= 20 ) {
				break;
			}
		}
	}
	gzclose( $sy_gz );
	if ( $sy_done && '' !== trim( $sy_statement ) ) {
		sy_fail( 500, 'The file ends in the middle of a statement.' );
	}
	sy_json(
		array(
			'statements' => $sy_count,
			'next'       => $sy_next,
			'done'       => $sy_done,
		)
	);
}

if ( 'table' !== $sy_do ) {
	sy_fail( 400, 'Unknown request.' );
}

$sy_table = isset( $sy_args['table'] ) ? (string) $sy_args['table'] : '';
if ( ! array_key_exists( $sy_table, sy_tables( $sy_link ) ) ) {
	sy_fail( 404, 'No such table.' );
}
$sy_from = isset( $sy_args['from'] ) ? (string) $sy_args['from'] : '';
$sy_q    = sy_name( $sy_table );
$sy_out  = '';

if ( '' === $sy_from ) {
	$sy_create = $sy_link->query( "SHOW CREATE TABLE $sy_q" );
	$sy_create = $sy_create ? $sy_create->fetch_row() : null;
	if ( ! $sy_create ) {
		sy_fail( 500, 'Could not read the table: ' . $sy_link->error );
	}
	$sy_out .= "DROP TABLE IF EXISTS $sy_q;\n" . $sy_create[1] . ";\n";
}

$sy_columns = array();
$sy_binary  = array();
$sy_keys    = array();
$sy_key_int = false;
$sy_result  = $sy_link->query( "SHOW COLUMNS FROM $sy_q" );
while ( $sy_result && $sy_column = $sy_result->fetch_assoc() ) {
	$sy_type      = strtolower( $sy_column['Type'] );
	$sy_columns[] = sy_name( $sy_column['Field'] );
	$sy_binary[]  = (bool) preg_match( '/blob|binary|^bit|geometry|point|linestring|polygon/', $sy_type );
	if ( 'PRI' === $sy_column['Key'] ) {
		$sy_keys[]  = $sy_column['Field'];
		$sy_key_int = (bool) preg_match( '/int/', $sy_type );
	}
}
// One whole-number key: continue after the last one sent, which stays fast
// on big tables. Otherwise count rows in, in key order where there is one.
$sy_keyset = 1 === count( $sy_keys ) && $sy_key_int;
$sy_key_at = $sy_keyset ? array_search( sy_name( $sy_keys[0] ), $sy_columns, true ) : -1;
$sy_offset = 0;
if ( $sy_keyset ) {
	$sy_sql = "SELECT * FROM $sy_q";
	if ( 0 === strpos( $sy_from, 'k:' ) && preg_match( '/^-?\d+$/', substr( $sy_from, 2 ) ) ) {
		$sy_sql .= ' WHERE ' . sy_name( $sy_keys[0] ) . ' > ' . substr( $sy_from, 2 );
	}
	$sy_sql .= ' ORDER BY ' . sy_name( $sy_keys[0] ) . ' LIMIT ' . SY_CHUNK_ROWS;
} else {
	if ( 0 === strpos( $sy_from, 'o:' ) ) {
		$sy_offset = max( 0, (int) substr( $sy_from, 2 ) );
	}
	$sy_order = $sy_keys ? ' ORDER BY ' . implode( ', ', array_map( 'sy_name', $sy_keys ) ) : '';
	$sy_sql   = "SELECT * FROM $sy_q$sy_order LIMIT $sy_offset, " . SY_CHUNK_ROWS;
}

$sy_result = $sy_link->query( $sy_sql, MYSQLI_USE_RESULT );
if ( ! $sy_result ) {
	sy_fail( 500, 'Could not read the table: ' . $sy_link->error );
}
$sy_head   = "INSERT INTO $sy_q (" . implode( ', ', $sy_columns ) . ") VALUES\n";
$sy_values = array();
$sy_size   = 0;
$sy_rows   = 0;
$sy_last   = null;
$sy_full   = false;
while ( $sy_row = $sy_result->fetch_row() ) {
	$sy_cells = array();
	foreach ( $sy_row as $sy_i => $sy_value ) {
		if ( null === $sy_value ) {
			$sy_cells[] = 'NULL';
		} elseif ( $sy_binary[ $sy_i ] ) {
			$sy_cells[] = '' === $sy_value ? "''" : '0x' . bin2hex( $sy_value );
		} else {
			$sy_cells[] = "'" . $sy_link->real_escape_string( $sy_value ) . "'";
		}
	}
	$sy_line     = '(' . implode( ',', $sy_cells ) . ')';
	$sy_values[] = $sy_line;
	$sy_size    += strlen( $sy_line );
	++$sy_rows;
	if ( $sy_keyset ) {
		$sy_last = $sy_row[ $sy_key_at ];
	}
	if ( $sy_size >= SY_STATEMENT_BYTES ) {
		$sy_out   .= $sy_head . implode( ",\n", $sy_values ) . ";\n";
		$sy_values = array();
		$sy_size   = 0;
		if ( strlen( $sy_out ) >= SY_CHUNK_BYTES ) {
			$sy_full = true;
			break;
		}
	}
}
$sy_result->free();
if ( $sy_values ) {
	$sy_out .= $sy_head . implode( ",\n", $sy_values ) . ";\n";
}

$sy_done = ! $sy_full && $sy_rows < SY_CHUNK_ROWS;
if ( $sy_keyset ) {
	$sy_next = null === $sy_last ? $sy_from : 'k:' . $sy_last;
} else {
	$sy_next = 'o:' . ( $sy_offset + $sy_rows );
}
$sy_gz = function_exists( 'gzencode' );
sy_meta(
	array(
		'rows' => $sy_rows,
		'next' => $sy_next,
		'done' => $sy_done,
		'gz'   => $sy_gz,
	)
);
if ( ! sy_cli() ) {
	header( 'Content-Type: application/octet-stream' );
	header( 'Cache-Control: no-store' );
}
echo $sy_gz ? gzencode( $sy_out, 6 ) : $sy_out;
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> DbSettings {
        DbSettings {
            host: "localhost:/var/run/mysqld/mysqld.sock".into(),
            user: "shop".into(),
            password: "it's secret".into(),
            name: "shop_db".into(),
            charset: "utf8mb4".into(),
            prefix: "wp_".into(),
        }
    }

    #[test]
    fn writes_settings_and_only_the_key_hash() {
        let helper = make(&db(), 1800).unwrap();
        assert!(helper.file.starts_with("stackyard-") && helper.file.ends_with(".php"));
        assert_eq!(helper.file.len(), "stackyard-.php".len() + 12);
        assert!(
            !helper.file.contains(&helper.key[..12]),
            "the name shows the key"
        );
        assert_eq!(helper.key.len(), 48);
        assert!(!helper.source.contains(&helper.key));
        assert!(helper.source.contains("define( 'SY_LIFETIME', 1800 );"));
        assert!(helper.source.contains(r"'password' => 'it\'s secret',"));
        assert!(helper.source.contains("define( 'SY_PREFIX', 'wp_' );"));
        assert!(!helper.source.contains("{HOST}"));
        let other = make(&db(), 1800).unwrap();
        assert_ne!(helper.key, other.key);
    }

    #[test]
    fn reads_chunk_metadata_from_error_output() {
        let errors = "PHP Warning: something\n\nSTACKYARD-META {\"rows\":3,\"next\":\"k:9\",\"done\":true,\"gz\":true}\n";
        let chunk = meta_from_errors(errors).unwrap();
        assert_eq!(chunk.rows, 3);
        assert_eq!(chunk.next, "k:9");
        assert!(chunk.done && chunk.gz);
        assert_eq!(meta_from_errors("nothing"), None);
    }

    /// The helper's time counts from when it lands on the server, not from
    /// the computer's clock: an upload that keeps an old modification time
    /// (a computer whose clock is behind) doesn't make it expire.
    #[test]
    fn expires_by_the_servers_clock() {
        let dir = crate::testutil::tempdir();
        let run = |lifetime: u64| {
            let helper = make(&db(), lifetime).unwrap();
            let file = dir.join(&helper.file);
            std::fs::write(&file, &helper.source).unwrap();
            let hour_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
            std::fs::File::options()
                .write(true)
                .open(&file)
                .unwrap()
                .set_modified(hour_ago)
                .unwrap();
            std::thread::sleep(std::time::Duration::from_millis(1100));
            let out = std::process::Command::new("php")
                .arg(&file)
                .arg("do=info")
                .output()
                .ok()?;
            Some((
                String::from_utf8_lossy(&out.stderr).into_owned(),
                file.exists(),
            ))
        };
        let Some((said, kept)) = run(1800) else {
            return; // no PHP here
        };
        assert!(!said.contains("expired"), "{said}");
        assert!(kept);
        let (said, kept) = run(0).unwrap();
        assert!(said.contains("expired"), "{said}");
        assert!(!kept);
    }

    /// Runs the helper with the PHP on this computer, when there is one,
    /// to catch syntax errors.
    #[test]
    fn is_valid_php() {
        let helper = make(&db(), 1800).unwrap();
        let dir = crate::testutil::tempdir();
        let file = dir.join(&helper.file);
        std::fs::write(&file, &helper.source).unwrap();
        let Ok(out) = std::process::Command::new("php")
            .arg("-l")
            .arg(&file)
            .output()
        else {
            return;
        };
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stdout)
        );
    }

    fn wanted(paths: &[&str]) -> std::collections::BTreeSet<String> {
        paths.iter().map(|p| p.to_string()).collect()
    }

    #[test]
    fn unpacks_files_however_the_answer_is_cut() {
        let dir = crate::testutil::tempdir();
        let want = wanted(&["a.php", "wp-content/b.css", "empty.txt"]);
        let answer = concat!(
            "{\"p\":\"a.php\",\"s\":5,\"m\":1000000000}\nhello",
            "{\"p\":\"gone.php\",\"missing\":true}\n",
            "{\"p\":\"wp-content/b.css\",\"s\":3,\"m\":0}\nb{}",
            "{\"p\":\"empty.txt\",\"s\":0,\"m\":0}\n",
            "{\"done\":true}\n",
        );
        // Byte by byte: the network may cut it anywhere.
        let mut unpacker = Unpacker::new(&dir, &want);
        for byte in answer.bytes() {
            std::io::Write::write(&mut unpacker, &[byte]).unwrap();
        }
        assert_eq!(unpacker.error(), None);
        assert!(unpacker.done);
        assert_eq!(unpacker.written, ["a.php", "wp-content/b.css", "empty.txt"]);
        assert_eq!(std::fs::read(dir.join("a.php")).unwrap(), b"hello");
        assert_eq!(std::fs::read(dir.join("wp-content/b.css")).unwrap(), b"b{}");
        assert_eq!(std::fs::read(dir.join("empty.txt")).unwrap(), b"");
        let modified = std::fs::metadata(dir.join("a.php"))
            .unwrap()
            .modified()
            .unwrap();
        assert_eq!(
            modified
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            1_000_000_000
        );
    }

    #[test]
    fn writes_nothing_it_did_not_ask_for() {
        let dir = crate::testutil::tempdir();
        for (asked, sent) in [
            ("a.php", "b.php"),
            ("../evil.php", "../evil.php"),
            ("x\\..\\evil.php", "x\\..\\evil.php"),
            ("c:evil.php", "c:evil.php"),
        ] {
            let want = wanted(&[asked]);
            let mut unpacker = Unpacker::new(&dir, &want);
            let answer = format!(
                "{{\"p\":{},\"s\":1,\"m\":0}}\nx",
                serde_json::to_string(sent).unwrap()
            );
            std::io::Write::write(&mut unpacker, answer.as_bytes()).unwrap();
            assert!(unpacker.error().is_some(), "{sent} was accepted");
            assert!(unpacker.written.is_empty());
        }
        assert!(!dir.parent().unwrap().join("evil.php").exists());
    }

    #[test]
    fn a_broken_answer_is_an_error() {
        let dir = crate::testutil::tempdir();
        let want = wanted(&["a.php"]);
        let mut unpacker = Unpacker::new(&dir, &want);
        std::io::Write::write(&mut unpacker, b"<html>Service Unavailable</html>\n").unwrap();
        assert!(unpacker.error().is_some());
        assert!(!unpacker.done);
    }

    /// The helper's own `files` and `pack` run with the PHP on this
    /// computer, when there is one, and Rust reads what they answer.
    #[test]
    fn lists_and_packs_with_real_php() {
        let site = crate::testutil::tempdir();
        let files = [
            ("index.php", "<?php // index".to_string()),
            ("wp-content/plugins/p/p.php", "plugin".to_string()),
            ("wp-content/cache/c.html", "cached".to_string()),
            ("wp-content/uploads/big.bin", "x".repeat(3000)),
        ];
        for (rel, text) in &files {
            let path = site.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        let helper = make(&db(), 600).unwrap();
        std::fs::write(site.join(&helper.file), &helper.source).unwrap();
        let php = |args: &[&str]| {
            std::process::Command::new("php")
                .current_dir(&site)
                .arg(&helper.file)
                .args(args)
                .output()
                .ok()
        };
        let Some(out) = php(&["do=files", r#"skip=["wp-content/cache"]"#]) else {
            return; // no PHP here
        };
        let page: FilesPage = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(page.next, None);
        let mut listed: Vec<&str> = page.files.iter().map(|(p, _, _)| p.as_str()).collect();
        listed.sort();
        assert_eq!(
            listed,
            [
                "index.php",
                "wp-content/plugins/p/p.php",
                "wp-content/uploads/big.bin"
            ]
        );

        // A byte budget of 1 sends one file per answer; `next` goes on.
        let paths = [
            "index.php",
            "wp-content/plugins/p/p.php",
            "wp-content/uploads/big.bin",
            "gone.php",
        ];
        let list = format!("paths={}", serde_json::to_string(&paths).unwrap());
        let want = wanted(&paths);
        let copy = crate::testutil::tempdir();
        let (mut start, mut rounds, mut written) = (0, 0, Vec::new());
        loop {
            rounds += 1;
            let start_arg = format!("start={start}");
            let out = php(&["do=pack", &list, &start_arg, "bytes=1"]).unwrap();
            let mut unpacker = Unpacker::new(&copy, &want);
            std::io::Write::write(&mut unpacker, &out.stdout).unwrap();
            assert_eq!(unpacker.error(), None);
            written.extend(unpacker.written.clone());
            if unpacker.done {
                break;
            }
            start = unpacker.next.unwrap();
        }
        // One file an answer, and the missing one is said in a fourth.
        assert_eq!(rounds, 4);
        assert_eq!(written, &paths[..3]);
        for (rel, text) in &files[..2] {
            assert_eq!(&std::fs::read_to_string(copy.join(rel)).unwrap(), text);
        }
        assert_eq!(
            std::fs::read(copy.join("wp-content/uploads/big.bin"))
                .unwrap()
                .len(),
            3000
        );
    }
}
