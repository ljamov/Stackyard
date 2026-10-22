//! Search and replace for WordPress databases, done on a SQL dump as it
//! streams past, so it works the same on a live site's dump, a staging's
//! and the local copy's.
//!
//! WordPress and its plugins keep addresses in three shapes, and all three
//! are handled:
//!
//! - plain text (`https://example.com/about`);
//! - JSON, where `/` may be written `\/` (`https:\/\/example.com`), and
//!   URL-encoded text (`https%3A%2F%2Fexample.com`);
//! - PHP serialized data (`s:19:"https://example.com";`), whose string
//!   lengths are worked out again after a change, so widgets, theme
//!   settings and page-builder layouts keep loading. Serialized data inside
//!   serialized strings is handled too.
//!
//! Values it can't change safely are counted per table instead: serialized
//! data whose lengths were already wrong, and base64 text that holds the
//! old address once decoded.

use std::collections::BTreeMap;
use std::io::{self, BufRead, BufReader, Read, Write};

use base64::Engine as _;
use serde::Serialize;

/// One text to find and what it becomes.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Pair {
    from: Vec<u8>,
    to: Vec<u8>,
    /// The match must not run on into a longer address: `example.com` is
    /// not found in `example.community` or `example.com.au`.
    boundary: bool,
}

/// What to replace, in every form it is stored in.
#[derive(Debug, Clone, Default)]
pub struct Rules {
    /// Longest first, so `https://www.example.com` wins over `//www.example.com`.
    pairs: Vec<Pair>,
    /// Texts at least one of which every match contains; a value with none
    /// of them is passed over quickly.
    needles: Vec<Vec<u8>>,
    /// Decoded base64 holding one of these is reported, not changed.
    watch: Vec<Vec<u8>>,
}

impl Rules {
    /// Exact text, as typed, plus its JSON form when it has a `/`.
    pub fn text(from: &str, to: &str) -> Self {
        let mut rules = Rules::default();
        rules.add_forms(from, to, false);
        rules.watch.push(from.as_bytes().to_vec());
        rules.finish()
    }

    /// A site moving from one address to another, e.g. `https://example.com`
    /// to `https://mysite.stack`. `http://`, `https://`, `www.` and
    /// protocol-relative (`//example.com`) forms of the old address all
    /// become the new one; e-mail addresses at the domain are left alone.
    pub fn site_move(from_url: &str, to_url: &str) -> Self {
        let mut rules = Rules::default();
        rules.add_site_move(from_url, to_url);
        rules.finish()
    }

    /// Adds another address move, e.g. a second domain the site answered on.
    pub fn and_site_move(mut self, from_url: &str, to_url: &str) -> Self {
        self.add_site_move(from_url, to_url);
        self.finish()
    }

    /// Also changes a folder path, e.g. `/home/user/public_html` to
    /// `C:/stackyard/www/mysite`, which some plugins store.
    pub fn and_path(mut self, from: &str, to: &str) -> Self {
        let from = from.trim_end_matches(['/', '\\']);
        let to = to.trim_end_matches(['/', '\\']);
        if from.len() > 1 && from != to {
            self.add_forms(from, to, true);
            self.finish()
        } else {
            self
        }
    }

    fn add_site_move(&mut self, from_url: &str, to_url: &str) {
        let (_, from_rest) = split_url(from_url);
        let (to_scheme, to_rest) = split_url(to_url);
        let to_rest = to_rest.trim_end_matches('/');
        let from_rest = from_rest.trim_end_matches('/');
        let bare = from_rest.strip_prefix("www.").unwrap_or(from_rest);
        if bare.is_empty() || bare == to_rest {
            return;
        }
        let to_scheme = to_scheme.unwrap_or("https");
        for www in ["www.", ""] {
            for scheme in ["https", "http"] {
                self.add_forms(
                    &format!("{scheme}://{www}{bare}"),
                    &format!("{to_scheme}://{to_rest}"),
                    true,
                );
            }
            self.add_forms(&format!("//{www}{bare}"), &format!("//{to_rest}"), true);
        }
        self.watch.push(bare.as_bytes().to_vec());
    }

    /// Plain, JSON-escaped and URL-encoded forms of one pair.
    fn add_forms(&mut self, from: &str, to: &str, boundary: bool) {
        if from.is_empty() || from == to {
            return;
        }
        let mut forms = vec![(from.to_string(), to.to_string())];
        if from.contains('/') {
            forms.push((from.replace('/', "\\/"), to.replace('/', "\\/")));
            forms.push((url_encode(from), url_encode(to)));
        }
        for (from, to) in forms {
            if !self.pairs.iter().any(|p| p.from == from.as_bytes()) {
                self.pairs.push(Pair {
                    from: from.into_bytes(),
                    to: to.into_bytes(),
                    boundary,
                });
            }
        }
    }

    fn finish(mut self) -> Self {
        self.pairs.sort_by_key(|p| std::cmp::Reverse(p.from.len()));
        // Every URL form contains the bare host; plain texts are their own needle.
        let mut needles: Vec<Vec<u8>> = Vec::new();
        for pair in &self.pairs {
            let covered = needles.iter().any(|n| contains(&pair.from, n));
            if !covered {
                needles.retain(|n| !contains(n, &pair.from));
                needles.push(pair.from.clone());
            }
        }
        // A shorter common part makes one check out of many.
        for watch in &self.watch {
            if self
                .pairs
                .iter()
                .all(|p| contains(&p.from, watch) || contains(&p.from, &url_encode_bytes(watch)))
                && !watch.is_empty()
            {
                needles = vec![watch.clone()];
                break;
            }
        }
        self.needles = needles;
        self
    }

    pub fn is_empty(&self) -> bool {
        self.pairs.is_empty()
    }
}

/// `https://example.com/blog` → (`https`, `example.com/blog`).
fn split_url(url: &str) -> (Option<&str>, &str) {
    let url = url.trim();
    match url.split_once("://") {
        Some((scheme, rest)) => (Some(scheme), rest),
        None => (None, url.trim_start_matches('/')),
    }
}

fn url_encode(text: &str) -> String {
    String::from_utf8(url_encode_bytes(text.as_bytes())).unwrap_or_default()
}

fn url_encode_bytes(text: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    for &b in text {
        match b {
            b':' => out.extend_from_slice(b"%3A"),
            b'/' => out.extend_from_slice(b"%2F"),
            b => out.push(b),
        }
    }
    out
}

pub(crate) fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    find(haystack, needle, 0).is_some()
}

fn find(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    let first = *needle.first()?;
    let mut at = from;
    while at + needle.len() <= haystack.len() {
        let next = haystack[at..=haystack.len() - needle.len()]
            .iter()
            .position(|&b| b == first)?;
        at += next;
        if haystack[at..].starts_with(needle) {
            return Some(at);
        }
        at += 1;
    }
    None
}

/// What a run found and changed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Report {
    /// Places changed in all.
    pub replaced: u64,
    /// Places changed, by table.
    pub tables: BTreeMap<String, u64>,
    /// Serialized values holding the old text whose lengths were already
    /// wrong, by table; they were changed as plain text.
    pub damaged: BTreeMap<String, u64>,
    /// Base64 values that hold the old text once decoded, by table; left as
    /// they are.
    pub encoded: BTreeMap<String, u64>,
}

impl Report {
    fn add(&mut self, table: &str, counts: Counts) {
        if counts.replaced > 0 {
            self.replaced += counts.replaced;
            *self.tables.entry(table.to_string()).or_default() += counts.replaced;
        }
        if counts.damaged > 0 {
            *self.damaged.entry(table.to_string()).or_default() += counts.damaged;
        }
        if counts.encoded > 0 {
            *self.encoded.entry(table.to_string()).or_default() += counts.encoded;
        }
    }

    /// Adds another run's numbers to these.
    pub fn merge(&mut self, other: &Report) {
        self.replaced += other.replaced;
        for (map, from) in [
            (&mut self.tables, &other.tables),
            (&mut self.damaged, &other.damaged),
            (&mut self.encoded, &other.encoded),
        ] {
            for (table, n) in from {
                *map.entry(table.clone()).or_default() += n;
            }
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct Counts {
    replaced: u64,
    damaged: u64,
    encoded: u64,
}

/// One stored value with the rules applied: serialized data rewritten with
/// correct lengths, anything else as text. `None` when nothing changed.
pub fn replace_value(value: &[u8], rules: &Rules) -> Option<Vec<u8>> {
    let mut counts = Counts::default();
    replace_counted(value, rules, &mut counts)
}

fn replace_counted(value: &[u8], rules: &Rules, counts: &mut Counts) -> Option<Vec<u8>> {
    if !rules.needles.iter().any(|n| contains(value, n)) {
        if looks_base64(value) {
            if let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(value) {
                if rules.watch.iter().any(|w| contains(&decoded, w)) {
                    counts.encoded += 1;
                }
            }
        }
        return None;
    }
    if looks_serialized(value) {
        let mut out = Vec::with_capacity(value.len() + 32);
        let mut inner = Counts::default();
        if let Some(end) = serialized(value, 0, &mut out, rules, &mut inner, 0) {
            if end == value.len() {
                counts.replaced += inner.replaced;
                counts.encoded += inner.encoded;
                counts.damaged += inner.damaged;
                return (inner.replaced > 0).then_some(out);
            }
        }
        counts.damaged += 1;
    }
    text(value, rules, counts)
}

/// Plain replacement, left to right, longest match first.
fn text(value: &[u8], rules: &Rules, counts: &mut Counts) -> Option<Vec<u8>> {
    let mut out: Option<Vec<u8>> = None;
    let mut copied = 0;
    let mut at = 0;
    while at < value.len() {
        let hit = rules.pairs.iter().find(|p| {
            value[at..].starts_with(&p.from)
                && (!p.boundary || ends_cleanly(value, at + p.from.len()))
        });
        match hit {
            Some(pair) => {
                let buf = out.get_or_insert_with(|| Vec::with_capacity(value.len() + 32));
                buf.extend_from_slice(&value[copied..at]);
                buf.extend_from_slice(&pair.to);
                at += pair.from.len();
                copied = at;
                counts.replaced += 1;
            }
            None => at += 1,
        }
    }
    let mut buf = out?;
    buf.extend_from_slice(&value[copied..]);
    Some(buf)
}

/// After an address: not more of a host name (`example.com.au`,
/// `example.community`) or of a folder name (`/blog` vs `/blogging`).
fn ends_cleanly(value: &[u8], end: usize) -> bool {
    let host = |b: u8| b.is_ascii_alphanumeric() || b == b'-' || b == b'_';
    match value.get(end) {
        None => true,
        Some(&b) if host(b) => false,
        Some(b'.') => !value.get(end + 1).is_some_and(|&b| host(b)),
        Some(_) => true,
    }
}

fn looks_base64(value: &[u8]) -> bool {
    value.len() >= 24
        && value.len().is_multiple_of(4)
        && value
            .iter()
            .all(|&b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'='))
}

fn looks_serialized(value: &[u8]) -> bool {
    matches!(value, [b'a' | b'O' | b's' | b'C', b':', ..])
}

/// Deeper than any real data; stops a crafted value from using up the stack.
const MAX_DEPTH: usize = 512;

/// Copies one serialized value starting at `at` into `out`, with the rules
/// applied to its strings. Returns where it ends, or `None` if it isn't
/// well-formed serialized data.
fn serialized(
    data: &[u8],
    at: usize,
    out: &mut Vec<u8>,
    rules: &Rules,
    counts: &mut Counts,
    depth: usize,
) -> Option<usize> {
    if depth > MAX_DEPTH {
        return None;
    }
    let kind = *data.get(at)?;
    match kind {
        b'N' => {
            expect(data, at + 1, b";")?;
            out.extend_from_slice(b"N;");
            Some(at + 2)
        }
        b'b' | b'i' | b'd' | b'r' | b'R' => {
            expect(data, at + 1, b":")?;
            let end = at + 2 + data[at + 2..].iter().position(|&b| b == b';')?;
            let number = &data[at + 2..end];
            let ok = !number.is_empty()
                && number.iter().all(|&b| {
                    b.is_ascii_digit()
                        || matches!(b, b'-' | b'+' | b'.' | b'E' | b'e')
                        || (kind == b'd' && matches!(b, b'I' | b'N' | b'F' | b'A'))
                });
            if !ok {
                return None;
            }
            out.extend_from_slice(&data[at..=end]);
            Some(end + 1)
        }
        b's' => {
            let (len, start) = length(data, at + 1)?;
            expect(data, start, b"\"")?;
            let body = data.get(start + 1..start + 1 + len)?;
            let end = start + 1 + len;
            expect(data, end, b"\";")?;
            let new = replace_counted(body, rules, counts);
            let body = new.as_deref().unwrap_or(body);
            out.extend_from_slice(format!("s:{}:\"", body.len()).as_bytes());
            out.extend_from_slice(body);
            out.extend_from_slice(b"\";");
            Some(end + 2)
        }
        b'S' => {
            // Escaped strings are rare and their lengths are of the decoded
            // text; copy them as they are.
            let (len, start) = length(data, at + 1)?;
            expect(data, start, b"\"")?;
            let close = start
                + 1
                + data
                    .get(start + 1..)?
                    .windows(2)
                    .position(|w| w == b"\";")?;
            let _ = len;
            out.extend_from_slice(&data[at..close + 2]);
            Some(close + 2)
        }
        b'E' => {
            let (len, start) = length(data, at + 1)?;
            expect(data, start, b"\"")?;
            let end = start + 1 + len;
            expect(data, end, b"\";")?;
            out.extend_from_slice(&data[at..end + 2]);
            Some(end + 2)
        }
        b'a' => {
            let (count, start) = length(data, at + 1)?;
            expect(data, start, b"{")?;
            out.extend_from_slice(&data[at..=start]);
            let mut pos = start + 1;
            for _ in 0..count.checked_mul(2)? {
                pos = serialized(data, pos, out, rules, counts, depth + 1)?;
            }
            expect(data, pos, b"}")?;
            out.push(b'}');
            Some(pos + 1)
        }
        b'O' => {
            let (len, start) = length(data, at + 1)?;
            expect(data, start, b"\"")?;
            let class_end = start + 1 + len;
            expect(data, class_end, b"\":")?;
            let (count, brace) = number_until(data, class_end + 2, b':')?;
            expect(data, brace, b":{")?;
            out.extend_from_slice(&data[at..brace + 2]);
            let mut pos = brace + 2;
            for _ in 0..count.checked_mul(2)? {
                pos = serialized(data, pos, out, rules, counts, depth + 1)?;
            }
            expect(data, pos, b"}")?;
            out.push(b'}');
            Some(pos + 1)
        }
        b'C' => {
            // An object that serializes itself: its data is the class's own
            // format, so it is copied unchanged. If it holds the old text,
            // that is reported.
            let (len, start) = length(data, at + 1)?;
            expect(data, start, b"\"")?;
            let class_end = start + 1 + len;
            expect(data, class_end, b"\":")?;
            let (size, brace) = number_until(data, class_end + 2, b':')?;
            expect(data, brace, b":{")?;
            let body_end = brace + 2 + size;
            expect(data, body_end, b"}")?;
            if rules
                .needles
                .iter()
                .any(|n| contains(&data[brace + 2..body_end], n))
            {
                counts.damaged += 1;
            }
            out.extend_from_slice(&data[at..=body_end]);
            Some(body_end + 1)
        }
        _ => None,
    }
}

fn expect(data: &[u8], at: usize, what: &[u8]) -> Option<()> {
    data.get(at..)?.starts_with(what).then_some(())
}

/// `:12:` at `at` → (12, position of the byte after the second `:`).
fn length(data: &[u8], at: usize) -> Option<(usize, usize)> {
    expect(data, at, b":")?;
    let (n, end) = number_until(data, at + 1, b':')?;
    Some((n, end + 1))
}

/// Digits from `at` up to `stop`; returns the number and where `stop` is.
fn number_until(data: &[u8], at: usize, stop: u8) -> Option<(usize, usize)> {
    let len = data.get(at..)?.iter().take(20).position(|&b| b == stop)?;
    let digits = &data[at..at + len];
    if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let n = std::str::from_utf8(digits).ok()?.parse().ok()?;
    Some((n, at + len))
}

/// Copies the serialized value at `at` into `out` with the rules applied;
/// returns where it ends.
pub(crate) fn copy_serialized(
    data: &[u8],
    at: usize,
    out: &mut Vec<u8>,
    rules: &Rules,
) -> Option<usize> {
    let mut counts = Counts::default();
    serialized(data, at, out, rules, &mut counts, 0)
}

/// Copies a SQL dump from `input` to `output` with the rules applied to
/// every quoted value. Anything else (statements, comments, numbers, hex
/// blobs) is copied byte for byte.
pub fn rewrite(input: impl Read, mut output: impl Write, rules: &Rules) -> io::Result<Report> {
    let mut report = Report::default();
    let mut reader = BufReader::with_capacity(256 * 1024, input);
    let mut out = io::BufWriter::with_capacity(256 * 1024, &mut output);
    let mut scanner = Scanner::default();
    let mut value = Vec::new();
    // The value as written, kept so an unchanged one is copied byte for byte.
    let mut raw = Vec::new();
    loop {
        let chunk = reader.fill_buf()?;
        if chunk.is_empty() {
            break;
        }
        let len = chunk.len();
        let mut plain_from = 0;
        let mut i = 0;
        while i < len {
            let b = chunk[i];
            match scanner.state {
                State::Code => {
                    if b == b'\'' {
                        out.write_all(&chunk[plain_from..=i])?;
                        scanner.state = State::Str;
                        value.clear();
                        raw.clear();
                        plain_from = i + 1;
                    } else {
                        scanner.code_byte(b);
                    }
                }
                State::LineComment => {
                    if b == b'\n' {
                        scanner.state = State::Code;
                    }
                }
                State::BlockComment { quote } => {
                    if let Some(q) = quote {
                        if b == b'\\' {
                            scanner.state = State::BlockEscape { quote: q };
                        } else if b == q {
                            scanner.state = State::BlockComment { quote: None };
                        }
                    } else if b == b'\'' || b == b'"' {
                        scanner.state = State::BlockComment { quote: Some(b) };
                    } else if b == b'/' && scanner.last == b'*' {
                        scanner.state = State::Code;
                    }
                    scanner.last = b;
                }
                State::BlockEscape { quote } => {
                    scanner.state = State::BlockComment { quote: Some(quote) };
                    scanner.last = 0;
                }
                State::Str => {
                    raw.push(b);
                    match b {
                        b'\\' => scanner.state = State::Escape,
                        b'\'' => scanner.state = State::MaybeEnd,
                        _ => value.push(b),
                    }
                    plain_from = i + 1;
                }
                State::Escape => {
                    raw.push(b);
                    value.extend_from_slice(&unescape(b));
                    scanner.state = State::Str;
                    plain_from = i + 1;
                }
                State::MaybeEnd => {
                    if b == b'\'' {
                        // `''` inside a string is one quote.
                        raw.push(b);
                        value.push(b'\'');
                        scanner.state = State::Str;
                        plain_from = i + 1;
                    } else {
                        let mut counts = Counts::default();
                        match replace_counted(&value, rules, &mut counts) {
                            Some(new) => escape_into(&new, &mut out)?,
                            // `raw` ends with the closing quote.
                            None => out.write_all(&raw[..raw.len() - 1])?,
                        }
                        report.add(&scanner.table, counts);
                        out.write_all(b"'")?;
                        scanner.state = State::Code;
                        plain_from = i;
                        scanner.code_byte(b);
                    }
                }
            }
            i += 1;
        }
        if matches!(
            scanner.state,
            State::Code
                | State::LineComment
                | State::BlockComment { .. }
                | State::BlockEscape { .. }
        ) {
            out.write_all(&chunk[plain_from..len])?;
        }
        reader.consume(len);
    }
    if scanner.state == State::MaybeEnd {
        let mut counts = Counts::default();
        match replace_counted(&value, rules, &mut counts) {
            Some(new) => escape_into(&new, &mut out)?,
            None => out.write_all(&raw[..raw.len() - 1])?,
        }
        report.add(&scanner.table, counts);
        out.write_all(b"'")?;
    } else if matches!(scanner.state, State::Str | State::Escape) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "the SQL file ends inside a quoted value",
        ));
    }
    out.flush()?;
    Ok(report)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Code,
    LineComment,
    BlockComment {
        quote: Option<u8>,
    },
    BlockEscape {
        quote: u8,
    },
    Str,
    Escape,
    /// A `'` was read inside a string: its end, or the first of `''`.
    MaybeEnd,
}

/// Follows the statements outside quoted values: comments, and which table
/// an `INSERT` writes to, for the report.
struct Scanner {
    state: State,
    last: u8,
    /// The last bytes of code, to spot `INSERT INTO ` and `--`.
    recent: Vec<u8>,
    table: String,
    in_name: Option<Vec<u8>>,
}

impl Default for Scanner {
    fn default() -> Self {
        Scanner {
            state: State::Code,
            last: 0,
            recent: Vec::with_capacity(32),
            table: String::new(),
            in_name: None,
        }
    }
}

impl Scanner {
    fn code_byte(&mut self, b: u8) {
        if let Some(name) = &mut self.in_name {
            if b == b'`' {
                self.table = String::from_utf8_lossy(name).into_owned();
                self.in_name = None;
            } else if name.len() < 128 {
                name.push(b);
            }
            return;
        }
        if b == b'`' && self.recent.ends_with(b"INSERT INTO ") {
            self.in_name = Some(Vec::new());
        }
        if b == b'-' && self.last == b'-' {
            // `--` at the start of a line (or of the file) begins a comment.
            let before = self.recent.len().checked_sub(2).map(|i| self.recent[i]);
            if matches!(before, None | Some(b'\n' | b';')) {
                self.state = State::LineComment;
            }
        } else if b == b'*' && self.last == b'/' {
            self.state = State::BlockComment { quote: None };
        }
        if self.recent.len() == 32 {
            self.recent.remove(0);
        }
        self.recent.push(b);
        self.last = b;
    }
}

fn unescape(b: u8) -> Vec<u8> {
    match b {
        b'0' => vec![0],
        b'b' => vec![8],
        b'n' => vec![b'\n'],
        b'r' => vec![b'\r'],
        b't' => vec![b'\t'],
        b'Z' => vec![26],
        // MySQL keeps the backslash before % and _ (they matter in LIKE).
        b'%' | b'_' => vec![b'\\', b],
        b => vec![b],
    }
}

/// Writes a value with MySQL's escapes, as `mysqldump` does.
pub(crate) fn escape_into(value: &[u8], out: &mut impl Write) -> io::Result<()> {
    let mut from = 0;
    for (i, &b) in value.iter().enumerate() {
        let escaped: &[u8] = match b {
            0 => b"\\0",
            b'\n' => b"\\n",
            b'\r' => b"\\r",
            26 => b"\\Z",
            b'\\' => b"\\\\",
            b'\'' => b"\\'",
            b'"' => b"\\\"",
            _ => continue,
        };
        out.write_all(&value[from..i])?;
        out.write_all(escaped)?;
        from = i + 1;
    }
    out.write_all(&value[from..])
}

/// Counts what [`rewrite`] would change, without writing anything.
pub fn preview(input: impl Read, rules: &Rules) -> io::Result<Report> {
    rewrite(input, io::sink(), rules)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn site() -> Rules {
        Rules::site_move("https://example.com", "https://mysite.stack")
    }

    fn apply(value: &str, rules: &Rules) -> String {
        replace_value(value.as_bytes(), rules)
            .map(|v| String::from_utf8(v).unwrap())
            .unwrap_or_else(|| value.to_string())
    }

    #[test]
    fn plain_addresses_in_every_form() {
        let rules = site();
        assert_eq!(
            apply(
                "see https://example.com/about and http://www.example.com",
                &rules
            ),
            "see https://mysite.stack/about and https://mysite.stack"
        );
        assert_eq!(
            apply(r#"{"url":"https:\/\/example.com\/shop"}"#, &rules),
            r#"{"url":"https:\/\/mysite.stack\/shop"}"#
        );
        assert_eq!(
            apply("next=https%3A%2F%2Fexample.com%2Fcart", &rules),
            "next=https%3A%2F%2Fmysite.stack%2Fcart"
        );
        assert_eq!(
            apply("<img src=\"//example.com/a.png\">", &rules),
            "<img src=\"//mysite.stack/a.png\">"
        );
        // Other domains and e-mail addresses stay.
        for same in [
            "https://example.community",
            "https://example.com.au/x",
            "info@example.com",
            "https://shop.example.com",
        ] {
            assert_eq!(apply(same, &rules), same);
        }
        assert_eq!(
            apply("Visit https://example.com.", &rules),
            "Visit https://mysite.stack."
        );
    }

    #[test]
    fn serialized_lengths_are_recounted() {
        let rules = site();
        let value = r#"a:2:{s:3:"url";s:19:"https://example.com";s:4:"list";a:1:{i:0;s:25:"https://example.com/a.png";}}"#;
        assert_eq!(
            apply(value, &rules),
            r#"a:2:{s:3:"url";s:20:"https://mysite.stack";s:4:"list";a:1:{i:0;s:26:"https://mysite.stack/a.png";}}"#
        );
        let object = r#"O:8:"stdClass":1:{s:4:"home";s:20:"http://example.com/x";}"#;
        assert_eq!(
            apply(object, &rules),
            r#"O:8:"stdClass":1:{s:4:"home";s:22:"https://mysite.stack/x";}"#
        );
        // Serialized inside serialized.
        let inner = r#"a:1:{i:0;s:19:"https://example.com";}"#;
        let outer = format!("s:{}:\"{}\";", inner.len(), inner);
        let done = apply(&outer, &rules);
        let new_inner = r#"a:1:{i:0;s:20:"https://mysite.stack";}"#;
        assert_eq!(done, format!("s:{}:\"{}\";", new_inner.len(), new_inner));
        // Multibyte text: lengths are in bytes.
        let multi = "a:1:{s:5:\"titel\";s:25:\"Über https://example.com\";}";
        assert_eq!(
            apply(multi, &rules),
            "a:1:{s:5:\"titel\";s:26:\"Über https://mysite.stack\";}"
        );
    }

    #[test]
    fn damaged_serialized_data_is_reported() {
        let rules = site();
        let mut counts = Counts::default();
        let broken = r#"a:1:{i:0;s:99:"https://example.com";}"#;
        let out = replace_counted(broken.as_bytes(), &rules, &mut counts).unwrap();
        assert_eq!(counts.damaged, 1);
        assert!(String::from_utf8(out).unwrap().contains("mysite.stack"));
        let encoded =
            base64::engine::general_purpose::STANDARD.encode("{\"home\":\"https://example.com\"}");
        let mut counts = Counts::default();
        assert!(replace_counted(encoded.as_bytes(), &rules, &mut counts).is_none());
        assert_eq!(counts.encoded, 1);
    }

    #[test]
    fn rewrites_a_dump() {
        let dump = "-- MySQL dump, don't touch 'this'\n\
            /*!40101 SET NAMES utf8mb4 */;\n\
            DROP TABLE IF EXISTS `wp_options`;\n\
            INSERT INTO `wp_options` VALUES (1,'siteurl','https://example.com','yes'),\
            (2,'widget','a:1:{s:3:\\\"url\\\";s:19:\\\"https://example.com\\\";}','yes'),\
            (3,'note','It\\'s at https://example.com\\nnew line','no'),(4,'o''k','','no');\n\
            INSERT INTO `wp_posts` VALUES (9,'<a href=\\\"https://example.com/x\\\">x</a>',0x00FF);\n";
        let mut out = Vec::new();
        let report = rewrite(dump.as_bytes(), &mut out, &site()).unwrap();
        let out = String::from_utf8(out).unwrap();
        assert_eq!(report.replaced, 4, "{out}");
        assert_eq!(report.tables["wp_options"], 3);
        assert_eq!(report.tables["wp_posts"], 1);
        assert!(
            out.contains("(1,'siteurl','https://mysite.stack','yes')"),
            "{out}"
        );
        assert!(
            out.contains(r#"'a:1:{s:3:\"url\";s:20:\"https://mysite.stack\";}'"#),
            "{out}"
        );
        assert!(
            out.contains(r"'It\'s at https://mysite.stack\nnew line'"),
            "{out}"
        );
        assert!(out.contains("(4,'o''k','','no')"), "{out}");
        assert!(
            out.starts_with("-- MySQL dump, don't touch 'this'\n/*!40101 SET NAMES utf8mb4 */;")
        );
        assert!(out.ends_with("0x00FF);\n"));
        // Nothing to change: byte for byte the same.
        let none = Rules::site_move("https://other.org", "https://x.stack");
        let mut same = Vec::new();
        rewrite(dump.as_bytes(), &mut same, &none).unwrap();
        assert_eq!(same, dump.as_bytes());
        assert_eq!(preview(dump.as_bytes(), &site()).unwrap().replaced, 4);
    }

    #[test]
    fn plain_text_and_paths() {
        let rules = Rules::text("Old Name", "New Name");
        assert_eq!(
            apply("a:1:{i:0;s:8:\"Old Name\";}", &rules),
            "a:1:{i:0;s:8:\"New Name\";}"
        );
        let rules = site().and_path("/home/u/public_html", "C:/stackyard/www/mysite");
        assert_eq!(
            apply("/home/u/public_html/wp-content/x.log", &rules),
            "C:/stackyard/www/mysite/wp-content/x.log"
        );
        // A subfolder site.
        let rules = Rules::site_move("https://example.com/blog/", "https://blog.stack");
        assert_eq!(
            apply("https://example.com/blog/hello", &rules),
            "https://blog.stack/hello"
        );
        assert_eq!(
            apply("https://example.com/blogging", &rules),
            "https://example.com/blogging"
        );
        assert_eq!(
            apply("https://example.com/shop", &rules),
            "https://example.com/shop"
        );
    }

    #[test]
    fn going_back_to_live_is_the_same_rule_reversed() {
        let there = apply("a:1:{i:0;s:19:\"https://example.com\";}", &site());
        let back = apply(
            &there,
            &Rules::site_move("https://mysite.stack", "https://example.com"),
        );
        assert_eq!(back, "a:1:{i:0;s:19:\"https://example.com\";}");
    }
}
