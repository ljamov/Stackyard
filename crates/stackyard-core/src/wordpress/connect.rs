//! Copying a live site through the Stackyard Connect plugin, for sites whose
//! FTP or SSH log-in isn't known.
//!
//! The plugin (a repository of its own, `stackyard-connect`) makes a
//! connection code on the live site's Tools › Stackyard Connect page. The code
//! holds the site's address, its `admin-ajax.php` address and a random key.
//! With it Stackyard asks the plugin to export the whole site, steps the
//! export along a few seconds at a time (hosts stop long requests),
//! downloads the finished `.tar`, continuing where a broken download
//! stopped, and then clones from it as from any backup file.
//!
//! From plugin 0.4.0 the export can leave out folders and files the person
//! unticked in a folder tree (`op=tree` lists a folder with sizes), and is
//! written in pieces small enough for hosts that refuse big downloads
//! (error 413). The pieces are downloaded one after another into one file,
//! which is the whole `.tar` again.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use base64::Engine;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use super::live::{OnProgress, Progress, Stage};
use crate::{Error, Result};

/// How a code starts.
pub const PREFIX: &str = "stackyard-connect:";

/// Steps of the export that may fail in a row (a slow host timing one out)
/// before the copy gives up.
const STEP_TRIES: u32 = 4;

/// The most bytes in one piece of an export: well under the 256 MB some
/// hosts allow in one answer.
pub const PIECE_BYTES: u64 = 100 * 1024 * 1024;

/// The first plugin version that lists folders and leaves chosen ones out.
pub const TREE_VERSION: &str = "0.4.0";

fn fail(message: impl Into<String>) -> Error {
    Error::WordPress(message.into())
}

fn stopped() -> Error {
    fail("stopped; clone again with the same name to continue where it stopped")
}

fn io(path: &Path) -> impl FnOnce(std::io::Error) -> Error + '_ {
    move |source| Error::Io {
        path: path.to_owned(),
        source,
    }
}

/// A connection code, read.
#[derive(Clone, PartialEq, Eq)]
pub struct Link {
    /// The live site's address (`home`), e.g. `https://example.com`.
    pub url: String,
    /// WordPress's own address (`siteurl`), where its folder is served;
    /// the same as `url` unless WordPress is in a folder of its own.
    pub site: String,
    /// Its `admin-ajax.php`, which answers Stackyard's requests.
    pub api: String,
    /// Whether the code lets Stackyard change the site's files (push).
    pub write: bool,
    key: String,
    /// The code as pasted, kept as a connection's secret.
    code: String,
}

// The key copies the whole site, so it stays out of logs.
impl std::fmt::Debug for Link {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Link")
            .field("url", &self.url)
            .field("api", &self.api)
            .field("write", &self.write)
            .finish_non_exhaustive()
    }
}

#[derive(Deserialize)]
struct Code {
    v: u32,
    url: String,
    #[serde(default)]
    site: String,
    api: String,
    key: String,
    #[serde(default)]
    write: bool,
}

/// What the plugin says about the site.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct Info {
    pub plugin: String,
    pub url: String,
    pub wordpress: String,
    pub php: String,
    pub mysql: String,
    pub prefix: String,
    pub tables: usize,
    pub database_bytes: u64,
    #[serde(default)]
    pub multisite: bool,
    pub export: Option<Export>,
}

/// An export on the live site, finished or still being made.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Export {
    /// `database`, `scan`, `files` or `finish`.
    pub phase: String,
    pub table: u64,
    pub tables: u64,
    pub files: u64,
    pub total: u64,
    pub bytes: u64,
    pub finished: bool,
    /// The archive's file name, e.g. `example.com-2026-10-30-101500.tar`.
    pub name: String,
    pub started: i64,
    /// The archive's size once finished.
    pub size: u64,
    /// The sizes of its pieces, in order, when it is in pieces (plugin
    /// 0.4.0 and later); empty for one file.
    #[serde(default)]
    pub pieces: Vec<u64>,
    /// Folders and files the export leaves out, relative to WordPress's folder.
    #[serde(default)]
    pub left_out: Vec<String>,
}

/// What goes into an export Stackyard asks for.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Choice {
    /// Folders and files to leave out, relative to WordPress's folder.
    #[serde(default)]
    pub skip: Vec<String>,
    /// The most bytes in one piece; 0 for one file.
    #[serde(default)]
    pub piece: u64,
}

/// One folder of the live site, as `op=tree` lists it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Folder {
    /// The folder, relative to WordPress's folder; empty for WordPress's folder.
    pub path: String,
    /// Its folders and files, biggest first.
    pub entries: Vec<Entry>,
    /// Smaller entries left out of the list (a folder with very many).
    #[serde(default)]
    pub more: u64,
    /// `false` when adding up folder sizes took too long and stopped.
    #[serde(default = "yes")]
    pub complete: bool,
}

fn yes() -> bool {
    true
}

/// A folder or file in a [`Folder`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub name: String,
    /// Relative to WordPress's folder.
    pub path: String,
    pub dir: bool,
    /// Bytes in it, everything inside for a folder.
    pub bytes: u64,
    /// Files in it: 1 for a file.
    pub files: u64,
    /// Left out of every export anyway: a cache or backup folder.
    #[serde(default)]
    pub skipped: bool,
}

/// Whether plugin `version` (e.g. `0.3.0`) is at least `least`.
pub fn version_at_least(version: &str, least: &str) -> bool {
    let parts = |v: &str| -> Vec<u64> {
        v.trim()
            .split('.')
            .map(|p| {
                p.chars()
                    .take_while(char::is_ascii_digit)
                    .collect::<String>()
                    .parse()
                    .unwrap_or(0)
            })
            .collect()
    };
    let (a, b) = (parts(version), parts(least));
    for i in 0..a.len().max(b.len()) {
        let (x, y) = (
            a.get(i).copied().unwrap_or(0),
            b.get(i).copied().unwrap_or(0),
        );
        if x != y {
            return x > y;
        }
    }
    true
}

/// The plugin's answers: `wp_send_json_success` and `_error`.
#[derive(Deserialize)]
struct Answer<T> {
    success: bool,
    data: Option<T>,
}

impl Link {
    /// Reads a code as the plugin shows it. Spaces and line breaks a copy
    /// may add are ignored.
    pub fn parse(code: &str) -> Result<Link> {
        let text: String = code.split_whitespace().collect();
        let body = text.strip_prefix(PREFIX).ok_or_else(|| {
            fail("this is not a Stackyard Connect code; copy it from Tools › Stackyard Connect on the live site")
        })?;
        let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(body.trim_end_matches('='))
            .map_err(|_| fail("the code is cut short or changed; copy it again"))?;
        let code: Code = serde_json::from_slice(&bytes)
            .map_err(|_| fail("the code is cut short or changed; copy it again"))?;
        if code.v != 1 {
            return Err(fail(
                "this code is from a newer Stackyard Connect; update Stackyard",
            ));
        }
        let web = |s: &str| s.starts_with("https://") || s.starts_with("http://");
        if !web(&code.url) || !web(&code.api) || !(code.site.is_empty() || web(&code.site)) {
            return Err(fail("the code doesn't hold a web address"));
        }
        if code.key.len() < 32 || !code.key.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(fail("the code is cut short or changed; copy it again"));
        }
        let url = code.url.trim_end_matches('/').to_string();
        Ok(Link {
            site: match code.site.trim_end_matches('/') {
                "" => url.clone(),
                site => site.to_string(),
            },
            url,
            api: code.api,
            write: code.write,
            key: code.key,
            code: text,
        })
    }

    /// Whether requests go over plain HTTP, where the key and the export
    /// can be read on the way.
    pub fn plain_http(&self) -> bool {
        self.api.starts_with("http://")
    }

    /// The code as it was pasted.
    pub fn code(&self) -> &str {
        &self.code
    }

    /// The plugin's address for `op` with `params` in the query.
    fn address(&self, op: &str, params: &[(&str, &str)]) -> String {
        let mut url = format!(
            "{}{}action=stackyard_connect_api&op={op}",
            self.api,
            if self.api.contains('?') { '&' } else { '?' }
        );
        for (key, value) in params {
            url.push('&');
            url.push_str(key);
            url.push('=');
            url.push_str(&percent(value));
        }
        url
    }

    fn call<T: DeserializeOwned>(&self, agent: &ureq::Agent, op: &str) -> Result<T> {
        self.request(agent, op, &[])
    }

    /// Asks the plugin to do `op` and reads its answer.
    pub(crate) fn request<T: DeserializeOwned>(
        &self,
        agent: &ureq::Agent,
        op: &str,
        params: &[(&str, &str)],
    ) -> Result<T> {
        let mut form = vec![("action", "stackyard_connect_api"), ("op", op)];
        form.extend_from_slice(params);
        let response = agent
            .post(&self.api)
            .header("X-Stackyard-Key", &self.key)
            .send_form(form)
            .map_err(|e| fail(format!("could not reach {}: {e}", self.url)))?;
        self.answer(response)
    }

    /// Sends `body` to be written as `params` say (`op=put`).
    pub(crate) fn put<T: DeserializeOwned>(
        &self,
        agent: &ureq::Agent,
        params: &[(&str, &str)],
        body: &[u8],
    ) -> Result<T> {
        let response = agent
            .post(&self.address("put", params))
            .header("X-Stackyard-Key", &self.key)
            .header("Content-Type", "application/octet-stream")
            .send(body)
            .map_err(|e| fail(format!("could not reach {}: {e}", self.url)))?;
        self.answer(response)
    }

    /// The live site's folder `path` (relative to WordPress's folder,
    /// empty for WordPress's folder itself): its folders and files with
    /// their sizes (plugin 0.4.0 and later).
    pub fn tree(&self, path: &str) -> Result<Folder> {
        self.request(&super::live::agent(), "tree", &[("path", path)])
            .map_err(|e| match e {
                Error::WordPress(m) if m.contains("Unknown request") => fail(format!(
                    "the Stackyard Connect plugin on the live site is too old to list its folders; update it to {TREE_VERSION} or newer"
                )),
                other => other,
            })
    }

    /// Starts a download from the plugin at byte `from`. The answer is
    /// 206 when it starts there, or 200 when it sends the whole file.
    pub(crate) fn get(
        &self,
        agent: &ureq::Agent,
        op: &str,
        params: &[(&str, &str)],
        from: u64,
    ) -> Result<ureq::http::Response<ureq::Body>> {
        let response = agent
            .get(&self.address(op, params))
            .header("X-Stackyard-Key", &self.key)
            .header("Range", &format!("bytes={from}-"))
            .call()
            .map_err(|e| fail(format!("could not reach {}: {e}", self.url)))?;
        match response.status().as_u16() {
            200 | 206 => Ok(response),
            _ => Err(self
                .answer::<serde_json::Value>(response)
                .err()
                .unwrap_or_else(|| fail("the live site answered something unexpected"))),
        }
    }

    fn answer<T: DeserializeOwned>(
        &self,
        mut response: ureq::http::Response<ureq::Body>,
    ) -> Result<T> {
        let status = response.status().as_u16();
        let text = response
            .body_mut()
            .with_config()
            .limit(4 * 1024 * 1024)
            .read_to_string()
            .map_err(|e| fail(format!("reading {}'s answer: {e}", self.url)))?;
        // admin-ajax answers a bare `0` when no plugin took the request.
        if text.trim() == "0" || text.trim() == "-1" {
            return Err(fail(format!(
                "{} doesn't have Stackyard Connect switched on",
                self.url
            )));
        }
        let answer: Answer<serde_json::Value> = serde_json::from_str(&text).map_err(|_| {
            fail(format!(
                "{} answered {status} with something other than Stackyard Connect: {}",
                self.url,
                text.chars().take(200).collect::<String>()
            ))
        })?;
        let data = answer.data.unwrap_or(serde_json::Value::Null);
        if !answer.success {
            return Err(fail(match data.as_str() {
                Some(said) => format!("the live site said: {said}"),
                None => format!("the live site answered {status}"),
            }));
        }
        if !(200..300).contains(&status) {
            return Err(fail(format!("the live site answered {status}")));
        }
        serde_json::from_value(data)
            .map_err(|e| fail(format!("Stackyard Connect's answer could not be read: {e}")))
    }

    /// WordPress's folder on the server.
    pub fn root(&self) -> Result<String> {
        #[derive(Deserialize)]
        struct Root {
            root: String,
        }
        let root: Root = self.call(&super::live::agent(), "root")?;
        Ok(root.root)
    }

    /// What the plugin says about the site.
    pub fn info(&self) -> Result<Info> {
        self.call(&super::live::agent(), "info")
    }

    /// The site's files by part and each theme and plugin, as the plugin
    /// counts them (0.3.0 and later).
    pub fn sizes(&self) -> Result<super::helper::Sizes> {
        self.call(&super::live::agent(), "sizes")
    }

    /// Removes the export from the live site. It holds everything, so it
    /// isn't left there once copied.
    pub fn delete_export(&self) -> Result<()> {
        self.call::<serde_json::Value>(&super::live::agent(), "delete")
            .map(|_| ())
    }

    /// Has the live site make an export, or goes on with one it was
    /// making, and downloads it into `dir`. A download that broke off is
    /// continued. Returns the archive, and what the plugin said about the
    /// site.
    pub fn download(
        &self,
        dir: &Path,
        choice: &Choice,
        progress: OnProgress,
    ) -> Result<(PathBuf, Info)> {
        std::fs::create_dir_all(dir).map_err(io(dir))?;
        let agent = super::live::agent();
        say(progress, "Asking the live site for an export…".into(), 0, 0)?;
        let mine = dir.join("export.json");
        let kept: Option<Export> = std::fs::read(&mine)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok());
        let chosen = dir.join("choice.json");
        let kept_choice: Choice = std::fs::read(&chosen)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        let info: Info = self.call(&agent, "info")?;
        let new_plugin = version_at_least(&info.plugin, TREE_VERSION);
        if !choice.skip.is_empty() && !new_plugin {
            return Err(fail(format!(
                "the Stackyard Connect plugin on the live site ({}) can't leave folders out; update it to {TREE_VERSION} or newer, or tick everything",
                if info.plugin.is_empty() { "an old version" } else { &info.plugin }
            )));
        }
        // Older plugins write one file whatever is asked.
        let choice = Choice {
            piece: if new_plugin { choice.piece } else { 0 },
            ..choice.clone()
        };
        // An export this copy asked for, with the same choice, goes on; a
        // finished one someone else made may be old, so a new one is made.
        let same = kept_choice == choice;
        let mut export = match (info.export.clone(), kept) {
            (Some(e), Some(k)) if same && e.started == k.started && e.name == k.name => e,
            (Some(e), None) if !e.finished && choice == Choice::default() => e,
            _ => {
                for file in list_files(dir) {
                    let _ = std::fs::remove_file(file);
                }
                let skip = serde_json::to_string(&choice.skip).unwrap_or_default();
                let piece = choice.piece.to_string();
                let mut params: Vec<(&str, &str)> = Vec::new();
                if !choice.skip.is_empty() {
                    params.push(("skip", &skip));
                }
                if choice.piece > 0 {
                    params.push(("piece", &piece));
                }
                let started = self.request(&agent, "start", &params)?;
                let text = serde_json::to_vec(&choice).unwrap_or_default();
                std::fs::write(&chosen, text).map_err(io(&chosen))?;
                started
            }
        };
        let note = |export: &Export| {
            let text = serde_json::to_vec(export).unwrap_or_default();
            std::fs::write(&mine, text).map_err(io(&mine))
        };
        note(&export)?;
        let mut failures = 0;
        while !export.finished {
            let (message, done, total) = match export.phase.as_str() {
                "database" => (
                    format!(
                        "The live site is exporting its database: table {} of {}",
                        export.table.min(export.tables),
                        export.tables
                    ),
                    export.table,
                    export.tables,
                ),
                "files" => (
                    format!(
                        "The live site is packing its files: {} of {}",
                        export.files, export.total
                    ),
                    export.files,
                    export.total,
                ),
                "scan" => ("The live site is listing its files…".into(), 0, 0),
                _ => ("The live site is finishing the export…".into(), 0, 0),
            };
            say(progress, message, done, total)?;
            match self.call::<Export>(&agent, "step") {
                Ok(next) => {
                    export = next;
                    failures = 0;
                }
                Err(e) => {
                    failures += 1;
                    if failures >= STEP_TRIES {
                        return Err(e);
                    }
                    std::thread::sleep(Duration::from_secs(3));
                }
            }
        }
        note(&export)?;

        let name = safe_name(&export.name);
        let archive = dir.join(&name);
        if archive.is_file() && file_len(&archive) == export.size {
            return Ok((archive, info));
        }
        let part = dir.join(format!("{name}.part"));
        let mut failures = 0;
        loop {
            match self.fetch(&agent, &part, &export, progress) {
                Ok(()) => break,
                Err(e) if matches!(&e, Error::WordPress(m) if m.starts_with("stopped")) => {
                    return Err(e)
                }
                Err(e) => {
                    failures += 1;
                    if failures >= STEP_TRIES {
                        return Err(e);
                    }
                    std::thread::sleep(Duration::from_secs(3));
                }
            }
        }
        std::fs::rename(&part, &archive).map_err(io(&archive))?;
        Ok((archive, info))
    }

    /// Downloads the finished export into `part`, from where it stopped:
    /// its one file, or its pieces one after another.
    fn fetch(
        &self,
        agent: &ureq::Agent,
        part: &Path,
        export: &Export,
        progress: OnProgress,
    ) -> Result<()> {
        let size = export.size;
        let mut have = file_len(part);
        if have > size {
            let _ = std::fs::remove_file(part);
            have = 0;
        }
        if export.pieces.is_empty() {
            return self.fetch_piece(agent, part, None, have, (0, size), size, progress);
        }
        if export.pieces.iter().sum::<u64>() != size {
            return Err(fail(
                "the live site's export pieces don't add up to its size",
            ));
        }
        let mut start = 0u64;
        for (i, &len) in export.pieces.iter().enumerate() {
            let end = start + len;
            if have < end {
                self.fetch_piece(
                    agent,
                    part,
                    Some(i + 1),
                    have - start,
                    (start, end),
                    size,
                    progress,
                )?;
                have = end;
            }
            start = end;
        }
        Ok(())
    }

    /// Downloads piece `piece` (or the export's one file) from byte
    /// `from` of it into `part`, which holds everything before the piece
    /// (`span.0` bytes) and `from` bytes of it; the piece ends at byte
    /// `span.1` of the whole export, `size` bytes.
    #[allow(clippy::too_many_arguments)]
    fn fetch_piece(
        &self,
        agent: &ureq::Agent,
        part: &Path,
        piece: Option<usize>,
        from: u64,
        span: (u64, u64),
        size: u64,
        progress: OnProgress,
    ) -> Result<()> {
        let (before, end) = span;
        let mut have = before + from;
        if have == end {
            return Ok(());
        }
        let number = piece.map(|n| n.to_string()).unwrap_or_default();
        let params: Vec<(&str, &str)> = match piece {
            Some(_) => vec![("piece", &number)],
            None => Vec::new(),
        };
        let mut response = self.get(agent, "download", &params, from)?;
        // 200: the server sent all of the piece, so it starts over.
        if response.status().as_u16() != 206 {
            have = before;
        }
        // The file is cut back to where this piece (or byte) starts.
        {
            let file = std::fs::OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(false)
                .open(part)
                .map_err(io(part))?;
            file.set_len(have).map_err(io(part))?;
        }
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(part)
            .map_err(io(part))?;
        let mut reader = response.body_mut().with_config().limit(u64::MAX).reader();
        let mut buf = vec![0u8; 256 * 1024];
        let mut since = 0u64;
        loop {
            let n = reader
                .read(&mut buf)
                .map_err(|e| fail(format!("downloading the export: {e}")))?;
            if n == 0 {
                break;
            }
            file.write_all(&buf[..n]).map_err(io(part))?;
            have += n as u64;
            since += n as u64;
            if since >= 1024 * 1024 || have == size {
                since = 0;
                say(progress, "Downloading the export…".into(), have, size)?;
            }
        }
        file.flush().map_err(io(part))?;
        if have != end {
            return Err(fail(format!(
                "the download broke off at {have} of {size} bytes"
            )));
        }
        Ok(())
    }
}

/// The archive's name as a plain file name, whatever the server sent.
fn safe_name(name: &str) -> String {
    let clean: String = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
        .collect();
    let clean = clean.trim_start_matches('.');
    if clean.is_empty() {
        "export.tar".into()
    } else if clean.ends_with(".tar") {
        clean.into()
    } else {
        format!("{clean}.tar")
    }
}

/// Percent-encodes a query value.
fn percent(value: &str) -> String {
    let mut out = String::new();
    for b in value.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~' | b'/') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn say(progress: OnProgress, message: String, done: u64, total: u64) -> Result<()> {
    if progress(Progress {
        stage: Stage::Files,
        message,
        done,
        total,
    }) {
        Ok(())
    } else {
        Err(stopped())
    }
}

fn file_len(path: &Path) -> u64 {
    std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

fn list_files(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.is_file())
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn code(json: &str) -> String {
        format!(
            "{PREFIX}{}",
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(json)
        )
    }

    const KEY: &str = "0123456789abcdef0123456789abcdef0123456789abcdef";

    #[test]
    fn reads_a_code_and_keeps_its_key_out_of_logs() {
        let text = code(&format!(
            r#"{{"v":1,"url":"https://shop.example/","api":"https://shop.example/wp-admin/admin-ajax.php","key":"{KEY}"}}"#
        ));
        // A copy from a narrow box may come with line breaks in it.
        let wrapped = format!("  {}\n{}  ", &text[..30], &text[30..]);
        let link = Link::parse(&wrapped).unwrap();
        assert_eq!(link.url, "https://shop.example");
        assert_eq!(link.api, "https://shop.example/wp-admin/admin-ajax.php");
        assert_eq!(link.site, "https://shop.example");
        assert!(!link.write);
        assert_eq!(Link::parse(link.code()).unwrap(), link);
        assert!(!link.plain_http());
        assert!(!format!("{link:?}").contains(KEY));
    }

    #[test]
    fn refuses_what_is_not_a_code() {
        for bad in [
            String::new(),
            "hello".into(),
            format!("{PREFIX}!!!"),
            code(r#"{"v":1,"url":"https://a","api":"https://a/x"}"#),
            code(&format!(
                r#"{{"v":2,"url":"https://a","api":"https://a/x","key":"{KEY}"}}"#
            )),
            code(&format!(
                r#"{{"v":1,"url":"file:///etc","api":"https://a/x","key":"{KEY}"}}"#
            )),
            code(r#"{"v":1,"url":"https://a","api":"https://a/x","key":"short"}"#),
        ] {
            assert!(Link::parse(&bad).is_err(), "{bad}");
        }
    }

    /// PHP's own web server running the plugin in a fake WordPress (its
    /// `tests/fake-wordpress.php`); stopped when dropped.
    struct FakeSite {
        server: std::process::Child,
        api: String,
        options: PathBuf,
    }

    impl Drop for FakeSite {
        fn drop(&mut self) {
            let _ = self.server.kill();
            let _ = self.server.wait();
        }
    }

    impl FakeSite {
        /// None when this computer has no PHP, or no copy of the plugin's
        /// repository: the folder in STACKYARD_CONNECT_DIR, else
        /// `stackyard-connect` beside this repository's folder.
        fn start(dir: &Path) -> Option<FakeSite> {
            let plugin = std::env::var_os("STACKYARD_CONNECT_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| {
                    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../stackyard-connect")
                });
            let router = plugin.join("tests/fake-wordpress.php");
            if !router.is_file() {
                return None;
            }
            let site = dir.join("site");
            for (rel, text) in [
                ("wp-load.php", "<?php // load"),
                ("wp-includes/version.php", "<?php $wp_version = '6.8.3';"),
                ("wp-content/themes/t/style.css", "/* Theme Name: T */"),
            ] {
                let path = site.join(rel);
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(path, text).unwrap();
            }
            std::fs::create_dir_all(site.join("wp-content/uploads")).unwrap();
            std::fs::write(
                dir.join("wp-config.php"),
                "<?php define('DB_NAME', 'shop'); $table_prefix = 'wp_';",
            )
            .unwrap();
            let port = std::net::TcpListener::bind("127.0.0.1:0")
                .unwrap()
                .local_addr()
                .unwrap()
                .port();
            let options = dir.join("options.json");
            let server = std::process::Command::new("php")
                .current_dir(dir)
                .env("STACKYARD_FAKE_SITE", &site)
                .env("STACKYARD_FAKE_OPTIONS", &options)
                .arg("-S")
                .arg(format!("127.0.0.1:{port}"))
                .arg(&router)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .ok()?;
            let fake = FakeSite {
                server,
                api: format!("http://127.0.0.1:{port}/wp-admin/admin-ajax.php"),
                options,
            };
            for _ in 0..100 {
                if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
                    return Some(fake);
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            None
        }

        /// A code the fake site takes, as Tools › Stackyard Connect makes it.
        fn link(&self) -> Link {
            self.link_with(false)
        }

        /// A code that may change files (`write`) or only copy them.
        fn link_with(&self, write: bool) -> Link {
            use sha2::Digest;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();
            let hash: String = sha2::Sha256::digest(KEY.as_bytes())
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect();
            std::fs::write(
                &self.options,
                format!(
                    r#"{{"stackyard_connect_link":{{"hash":"{hash}","created":{now},"used":{now},"write":{write}}}}}"#
                ),
            )
            .unwrap();
            Link::parse(&code(&format!(
                r#"{{"v":1,"url":"https://shop.example.com","api":"{}","key":"{KEY}","write":{write}}}"#,
                self.api
            )))
            .unwrap()
        }
    }

    #[test]
    fn copies_a_site_through_the_plugin_with_real_php() {
        let dir = crate::testutil::tempdir();
        let Some(site) = FakeSite::start(&dir) else {
            return; // no PHP here
        };
        let link = site.link();

        let wrong = Link {
            key: "f".repeat(48),
            ..link.clone()
        };
        let refused = wrong.info().unwrap_err().to_string();
        assert!(refused.contains("not valid"), "{refused}");

        let info = link.info().unwrap();
        assert_eq!(info.url, "https://shop.example.com");
        assert_eq!(info.wordpress, "6.8.3");
        assert_eq!(info.tables, 2);
        assert_eq!(info.export, None);

        let local = dir.join("local");
        let mut seen = Vec::new();
        let (archive, _) = link
            .download(&local, &Choice::default(), &mut |p| {
                seen.push(p.message);
                true
            })
            .unwrap();
        assert!(
            seen.iter().any(|m| m.starts_with("Downloading")),
            "{seen:?}"
        );
        let mut names: Vec<String> = tar::Archive::new(std::fs::File::open(&archive).unwrap())
            .entries()
            .unwrap()
            .map(|e| e.unwrap().path().unwrap().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(
            names,
            [
                "site/wp-content/themes/t/style.css",
                "site/wp-includes/version.php",
                "site/wp-load.php",
                "stackyard-database.sql",
                "stackyard-export.json",
                "wp-config.php",
            ]
        );
        let whole = std::fs::read(&archive).unwrap();

        // A download that broke off halfway goes on from where it stopped.
        let part = local.join(format!(
            "{}.part",
            archive.file_name().unwrap().to_string_lossy()
        ));
        std::fs::write(&part, &whole[..1000]).unwrap();
        std::fs::remove_file(&archive).unwrap();
        let (again, _) = link
            .download(&local, &Choice::default(), &mut |_| true)
            .unwrap();
        assert_eq!(again, archive);
        assert_eq!(std::fs::read(&archive).unwrap(), whole);

        link.delete_export().unwrap();
        assert_eq!(link.info().unwrap().export, None);
    }

    #[test]
    fn the_plugin_keeps_pieces_and_links_out_of_reach_with_real_php() {
        let dir = crate::testutil::tempdir();
        let Some(site) = FakeSite::start(&dir) else {
            return; // no PHP here
        };
        let link = site.link_with(true);
        let agent = crate::wordpress::live::agent();
        let root = link.root().unwrap();

        // A piece of an upload waits in the plugin's protected folder, not
        // beside the file, where the web could fetch a half-sent file.
        let target = format!("{root}/wp-config-sample.php");
        link.put::<serde_json::Value>(
            &agent,
            &[("path", target.as_str()), ("offset", "0")],
            b"<?php // half",
        )
        .unwrap();
        let site_dir = dir.join("site");
        assert!(!site_dir.join("wp-config-sample.php").exists());
        assert!(!site_dir
            .join("wp-config-sample.php.stackyard-part")
            .exists());
        let parts = site_dir.join("wp-content/uploads/stackyard-exports/uploads");
        assert!(parts.join(".htaccess").is_file(), "the folder is protected");
        assert_eq!(std::fs::read_dir(&parts).unwrap().count(), 4, "one piece");
        link.put::<serde_json::Value>(
            &agent,
            &[("path", target.as_str()), ("offset", "13"), ("last", "1")],
            b" and the rest",
        )
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(site_dir.join("wp-config-sample.php")).unwrap(),
            "<?php // half and the rest"
        );
        assert_eq!(std::fs::read_dir(&parts).unwrap().count(), 3, "piece gone");

        // A link out of WordPress's folder is neither listed nor read.
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&dir, site_dir.join("wp-content/out")).unwrap();
            std::os::unix::fs::symlink(
                dir.join("wp-config.php"),
                site_dir.join("wp-content/config-link.php"),
            )
            .unwrap();
            let listed: Vec<serde_json::Value> = link
                .request(&agent, "ls", &[("path", &format!("{root}/wp-content"))])
                .unwrap();
            let names: Vec<&str> = listed.iter().filter_map(|e| e["name"].as_str()).collect();
            assert!(
                !names.contains(&"out") && !names.contains(&"config-link.php"),
                "{names:?}"
            );
            for path in ["wp-content/out/options.json", "wp-content/config-link.php"] {
                let read = link.get(&agent, "get", &[("path", &format!("{root}/{path}"))], 0);
                assert!(read.is_err(), "{path} was read");
            }
        }

        // The wp-config.php above is reachable only while WordPress's
        // folder has none of its own.
        let above = format!("{}/wp-config.php", root.trim_end_matches("/site"));
        assert!(link.get(&agent, "get", &[("path", &above)], 0).is_ok());
        std::fs::write(site_dir.join("wp-config.php"), "<?php // own").unwrap();
        assert!(link.get(&agent, "get", &[("path", &above)], 0).is_err());
    }

    #[test]
    fn the_plugin_stands_in_for_ftp_with_real_php() {
        use crate::remote::{self, ConnectionInput, Protocol, Sessions};
        let dir = crate::testutil::tempdir();
        let Some(site) = FakeSite::start(&dir) else {
            return; // no PHP here
        };
        let connect = |link: &Link| {
            let draft = remote::draft(ConnectionInput {
                protocol: Some(Protocol::Plugin),
                password: Some(link.code().to_string()),
                ..ConnectionInput::default()
            })
            .unwrap();
            assert_eq!(draft.host, "https://shop.example.com");
            let sessions = Sessions::default();
            sessions
                .login(&draft, Some(link.code().to_string()))
                .unwrap();
            (sessions, draft)
        };
        let (sessions, live) = connect(&site.link_with(true));
        let root = sessions.root(&live).unwrap();
        assert!(root.ends_with("/site"), "{root}");
        let mut names: Vec<String> = sessions
            .list(&live, &root)
            .unwrap()
            .into_iter()
            .map(|e| e.name)
            .collect();
        names.sort();
        assert_eq!(names, ["wp-content", "wp-includes", "wp-load.php"]);

        // An upload bigger than one piece, into a folder not there yet.
        let big: Vec<u8> = (0..5_000_000u32).map(|i| (i % 251) as u8).collect();
        let local = dir.join("big.bin");
        std::fs::write(&local, &big).unwrap();
        let remote_file = format!("{root}/wp-content/uploads/2026/big.bin");
        sessions.upload(&live, &local, &remote_file).unwrap();
        assert_eq!(
            std::fs::read(dir.join("site/wp-content/uploads/2026/big.bin")).unwrap(),
            big
        );
        let back = dir.join("back/big.bin");
        sessions
            .download(&live, &remote_file, &back, &mut |_| true)
            .unwrap();
        assert_eq!(std::fs::read(&back).unwrap(), big);
        // The live file keeps the local file's date, so sync sees them as one.
        // To the second, as servers keep it.
        let date = |p: &Path| crate::remote::mtime_of(&std::fs::metadata(p).unwrap());
        assert_eq!(date(&back), date(&local));

        // An empty file, then deleting it.
        std::fs::write(dir.join("empty.txt"), "").unwrap();
        let empty = format!("{root}/empty.txt");
        sessions
            .upload(&live, &dir.join("empty.txt"), &empty)
            .unwrap();
        assert!(dir.join("site/empty.txt").is_file());
        sessions.remove_file(&live, &empty).unwrap();
        assert!(!dir.join("site/empty.txt").exists());

        // Nothing outside WordPress's folder, but its wp-config.php above.
        let outside = format!("{}/options.json", root.trim_end_matches("/site"));
        assert!(sessions
            .download(&live, &outside, &dir.join("x"), &mut |_| true)
            .is_err());
        assert!(sessions
            .upload(&live, &local, &format!("{root}/../evil.php"))
            .is_err());
        assert!(!dir.join("evil.php").exists());
        sessions
            .download(
                &live,
                &format!("{}/wp-config.php", root.trim_end_matches("/site")),
                &dir.join("config.php"),
                &mut |_| true,
            )
            .unwrap();

        // A code that only copies can read but not change.
        let (sessions, live) = connect(&site.link_with(false));
        sessions
            .download(&live, &remote_file, &dir.join("again.bin"), &mut |_| true)
            .unwrap();
        let refused = sessions
            .remove_file(&live, &remote_file)
            .unwrap_err()
            .to_string();
        assert!(refused.contains("not change"), "{refused}");
        assert!(dir.join("site/wp-content/uploads/2026/big.bin").is_file());
    }

    #[test]
    fn leaves_chosen_folders_out_and_downloads_the_export_in_pieces_with_real_php() {
        let dir = crate::testutil::tempdir();
        let Some(site) = FakeSite::start(&dir) else {
            return; // no PHP here
        };
        let videos = dir.join("site/wp-content/uploads/videos");
        std::fs::create_dir_all(&videos).unwrap();
        // Random-ish bytes, so the pieces are of a size worth splitting.
        let big: Vec<u8> = (0..3_000_000u32)
            .map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8)
            .collect();
        std::fs::write(videos.join("intro.mp4"), &big).unwrap();
        let keep: Vec<u8> = big.iter().rev().take(2_500_000).copied().collect();
        std::fs::write(dir.join("site/wp-content/uploads/keep.jpg"), &keep).unwrap();
        let link = site.link();

        let top = link.tree("").unwrap();
        let content = top.entries.iter().find(|e| e.path == "wp-content").unwrap();
        assert!(content.dir);
        assert!(content.bytes >= 5_500_000, "{content:?}");
        let uploads = link.tree("wp-content/uploads").unwrap();
        assert_eq!(uploads.entries[0].path, "wp-content/uploads/videos");
        assert_eq!(uploads.entries[0].bytes, 3_000_000);
        assert!(link.tree("../etc").is_err());

        let local = dir.join("local");
        let choice = Choice {
            skip: vec!["wp-content/uploads/videos".into()],
            piece: 1024 * 1024,
        };
        let (archive, _) = link.download(&local, &choice, &mut |_| true).unwrap();
        let export = link.info().unwrap().export.unwrap();
        assert_eq!(export.pieces.len(), 3, "{export:?}");
        assert_eq!(export.left_out, ["wp-content/uploads/videos"]);
        let names: Vec<String> = tar::Archive::new(std::fs::File::open(&archive).unwrap())
            .entries()
            .unwrap()
            .map(|e| e.unwrap().path().unwrap().to_string_lossy().into_owned())
            .collect();
        assert!(
            names.contains(&"site/wp-content/uploads/keep.jpg".to_string()),
            "{names:?}"
        );
        assert!(!names.iter().any(|n| n.contains("videos")), "{names:?}");
        let whole = std::fs::read(&archive).unwrap();
        assert_eq!(whole.len() as u64, export.size);

        // Broken off in the second piece: it goes on from there.
        let part = local.join(format!(
            "{}.part",
            archive.file_name().unwrap().to_string_lossy()
        ));
        std::fs::write(&part, &whole[..1_500_000]).unwrap();
        std::fs::remove_file(&archive).unwrap();
        let (again, _) = link.download(&local, &choice, &mut |_| true).unwrap();
        assert_eq!(std::fs::read(&again).unwrap(), whole);

        // Another choice makes a new export.
        let (all, _) = link
            .download(&local, &Choice::default(), &mut |_| true)
            .unwrap();
        assert!(link.info().unwrap().export.unwrap().pieces.is_empty());
        let names: Vec<String> = tar::Archive::new(std::fs::File::open(&all).unwrap())
            .entries()
            .unwrap()
            .map(|e| e.unwrap().path().unwrap().to_string_lossy().into_owned())
            .collect();
        assert!(
            names.iter().any(|n| n.contains("videos/intro.mp4")),
            "{names:?}"
        );
    }

    #[test]
    fn compares_plugin_versions() {
        assert!(version_at_least("0.4.0", TREE_VERSION));
        assert!(version_at_least("0.10.1", TREE_VERSION));
        assert!(version_at_least("1.0", TREE_VERSION));
        assert!(!version_at_least("0.3.0", TREE_VERSION));
        assert!(!version_at_least("", TREE_VERSION));
    }

    #[test]
    fn archive_names_stay_plain_file_names() {
        assert_eq!(safe_name("shop.example-2026.tar"), "shop.example-2026.tar");
        assert_eq!(safe_name("../../evil"), "evil.tar");
        assert_eq!(safe_name("..\\a/b.tar"), "ab.tar");
        assert_eq!(safe_name(""), "export.tar");
    }
}
