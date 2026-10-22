//! Reading and changing `wp-config.php` without running it: the database
//! settings, the table prefix and `define()`d constants such as `WP_DEBUG`.
//!
//! A small scanner skips comments and quoted text, so a `define` that is
//! commented out is not taken for a real one.

use std::collections::BTreeMap;
use std::ops::Range;

/// What Stackyard needs from a `wp-config.php`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WpConfig {
    /// Constants by name, with their values when they are plain literals
    /// (`'text'`, `true`, `42`); `None` for anything computed, such as
    /// `getenv('DB_NAME')`.
    pub defines: BTreeMap<String, Option<String>>,
    pub table_prefix: Option<String>,
}

impl WpConfig {
    pub fn read(text: &str) -> Self {
        let mut config = WpConfig::default();
        for define in defines(text) {
            config.defines.insert(define.name, literal(&define.value));
        }
        config.table_prefix = table_prefix(text);
        config
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.defines.get(name).and_then(|v| v.as_deref())
    }

    /// Whether a constant is defined as `true` (or `1`).
    pub fn flag(&self, name: &str) -> bool {
        matches!(self.get(name), Some("true" | "1"))
    }

    pub fn prefix(&self) -> &str {
        self.table_prefix.as_deref().unwrap_or("wp_")
    }
}

/// One `define( 'NAME', value );` call.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Define {
    name: String,
    /// The value as written, e.g. `'secret'` or `true`.
    value: String,
    /// From `define` to its `;` (or `)` when there is none).
    span: Range<usize>,
}

/// The parts of `text` that are code: comments and quoted strings are
/// marked so searches can skip them.
fn code_mask(text: &str) -> Vec<bool> {
    let bytes = text.as_bytes();
    let mut mask = vec![true; bytes.len()];
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'/' if bytes.get(i + 1) == Some(&b'/') => i = skip_line(bytes, &mut mask, i),
            b'#' => i = skip_line(bytes, &mut mask, i),
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                let end = text[i + 2..]
                    .find("*/")
                    .map_or(bytes.len(), |e| i + 2 + e + 2);
                mask[i..end].fill(false);
                i = end;
            }
            q @ (b'\'' | b'"') => {
                let end = string_end(bytes, i, q);
                // The quotes stay code; what is between them is not.
                mask[i + 1..end.saturating_sub(1).max(i + 1)].fill(false);
                i = end;
            }
            _ => i += 1,
        }
    }
    mask
}

fn skip_line(bytes: &[u8], mask: &mut [bool], from: usize) -> usize {
    let end = bytes[from..]
        .iter()
        .position(|&b| b == b'\n')
        .map_or(bytes.len(), |e| from + e);
    mask[from..end].fill(false);
    end
}

/// Index just past the closing quote of a string opened at `start`.
fn string_end(bytes: &[u8], start: usize, quote: u8) -> usize {
    let mut i = start + 1;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            b if b == quote => return i + 1,
            _ => i += 1,
        }
    }
    bytes.len()
}

fn defines(text: &str) -> Vec<Define> {
    let mask = code_mask(text);
    let bytes = text.as_bytes();
    let lower = text.to_ascii_lowercase();
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(at) = lower[from..].find("define") {
        let start = from + at;
        from = start + 6;
        let word_start = start == 0 || !is_ident(bytes[start - 1]);
        if !mask[start] || !word_start || bytes.get(start + 6).is_some_and(|&b| is_ident(b)) {
            continue;
        }
        let mut i = skip_space(bytes, start + 6);
        if bytes.get(i) != Some(&b'(') {
            continue;
        }
        i = skip_space(bytes, i + 1);
        let Some(&quote) = bytes.get(i).filter(|q| **q == b'\'' || **q == b'"') else {
            continue;
        };
        let name_end = string_end(bytes, i, quote);
        let name = text[i + 1..name_end - 1].to_string();
        i = skip_space(bytes, name_end);
        if bytes.get(i) != Some(&b',') {
            continue;
        }
        let value_start = skip_space(bytes, i + 1);
        // The value runs to the `)` that closes `define(`.
        let mut depth = 0;
        let mut j = value_start;
        let mut close = None;
        while j < bytes.len() {
            match bytes[j] {
                q @ (b'\'' | b'"') => {
                    j = string_end(bytes, j, q);
                    continue;
                }
                b'(' => depth += 1,
                b')' if depth == 0 => {
                    close = Some(j);
                    break;
                }
                b')' => depth -= 1,
                // A third argument (case-insensitive flag) ends the value too.
                b',' if depth == 0 => {}
                _ => {}
            }
            j += 1;
        }
        let Some(close) = close else { break };
        let mut value = text[value_start..close].trim().to_string();
        if let Some((first, _)) = split_top_comma(&value) {
            value = first;
        }
        let after = skip_space(bytes, close + 1);
        let end = if bytes.get(after) == Some(&b';') {
            after + 1
        } else {
            close + 1
        };
        found.push(Define {
            name,
            value,
            span: start..end,
        });
        from = end;
    }
    found
}

/// `'a', true` → (`'a'`, ` true`); `None` without a comma outside quotes and brackets.
fn split_top_comma(value: &str) -> Option<(String, String)> {
    let bytes = value.as_bytes();
    let mut depth = 0;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            q @ (b'\'' | b'"') => {
                i = string_end(bytes, i, q);
                continue;
            }
            b'(' | b'[' => depth += 1,
            b')' | b']' => depth -= 1,
            b',' if depth == 0 => {
                return Some((value[..i].trim().to_string(), value[i + 1..].to_string()))
            }
            _ => {}
        }
        i += 1;
    }
    None
}

fn is_ident(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'$' || b == b'>' || b == b':'
}

fn skip_space(bytes: &[u8], mut i: usize) -> usize {
    while bytes.get(i).is_some_and(u8::is_ascii_whitespace) {
        i += 1;
    }
    i
}

/// A PHP literal's value: `'it\'s'` → `it's`, `true` → `true`, `42` → `42`.
pub fn literal(raw: &str) -> Option<String> {
    let raw = raw.trim();
    let bytes = raw.as_bytes();
    match bytes.first()? {
        q @ (b'\'' | b'"') => {
            if raw.len() < 2 || string_end(bytes, 0, *q) != raw.len() {
                return None;
            }
            let inner = &raw[1..raw.len() - 1];
            if *q == b'"' && inner.contains('$') {
                return None;
            }
            let mut out = String::new();
            let mut chars = inner.chars();
            while let Some(c) = chars.next() {
                if c == '\\' {
                    match chars.next() {
                        Some('\\') => out.push('\\'),
                        Some(c) if c as u32 == u32::from(*q) => out.push(c),
                        Some('n') if *q == b'"' => out.push('\n'),
                        Some('t') if *q == b'"' => out.push('\t'),
                        Some(c) => {
                            out.push('\\');
                            out.push(c);
                        }
                        None => out.push('\\'),
                    }
                } else {
                    out.push(c);
                }
            }
            Some(out)
        }
        _ => {
            let lower = raw.to_ascii_lowercase();
            if lower == "true" || lower == "false" || lower == "null" {
                Some(lower)
            } else if raw.parse::<f64>().is_ok() {
                Some(raw.to_string())
            } else {
                None
            }
        }
    }
}

/// A text as a single-quoted PHP string.
pub fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\\', "\\\\").replace('\'', "\\'"))
}

fn table_prefix(text: &str) -> Option<String> {
    let mask = code_mask(text);
    let bytes = text.as_bytes();
    let mut from = 0;
    while let Some(at) = text[from..].find("$table_prefix") {
        let start = from + at;
        from = start + 13;
        if !mask[start] || bytes.get(start + 13).is_some_and(|&b| is_ident(b)) {
            continue;
        }
        let i = skip_space(bytes, start + 13);
        if bytes.get(i) != Some(&b'=') || bytes.get(i + 1) == Some(&b'=') {
            continue;
        }
        let i = skip_space(bytes, i + 1);
        let q = *bytes.get(i)?;
        if q != b'\'' && q != b'"' {
            return None;
        }
        let end = string_end(bytes, i, q);
        return literal(&text[i..end]);
    }
    None
}

/// Sets a constant: changes its `define` where it is, or adds one where
/// WordPress asks for custom values to go. `value` is PHP code, e.g. `true`
/// or `'staging'` (see [`quote`]).
pub fn set_define(text: &str, name: &str, value: &str) -> String {
    let line = format!("define( '{name}', {value} );");
    let all = defines(text);
    if let Some(found) = all.iter().find(|d| d.name == name) {
        let mut out = String::with_capacity(text.len() + 16);
        out.push_str(&text[..found.span.start]);
        out.push_str(&line);
        out.push_str(&text[found.span.end..]);
        return out;
    }
    let at = insert_point(text);
    let mut out = String::with_capacity(text.len() + line.len() + 2);
    out.push_str(&text[..at]);
    out.push_str(&line);
    out.push('\n');
    out.push_str(&text[at..]);
    out
}

/// Removes a constant's `define`, if there is one.
pub fn remove_define(text: &str, name: &str) -> String {
    let all = defines(text);
    let Some(found) = all.iter().find(|d| d.name == name) else {
        return text.to_string();
    };
    let mut end = found.span.end;
    if text[end..].starts_with("\r\n") {
        end += 2;
    } else if text[end..].starts_with('\n') {
        end += 1;
    }
    format!("{}{}", &text[..found.span.start], &text[end..])
}

/// Sets `$table_prefix`.
pub fn set_table_prefix(text: &str, prefix: &str) -> String {
    let mask = code_mask(text);
    let bytes = text.as_bytes();
    if let Some(at) = text
        .match_indices("$table_prefix")
        .map(|(i, _)| i)
        .find(|&i| mask[i])
    {
        let i = skip_space(bytes, at + 13);
        if bytes.get(i) == Some(&b'=') {
            let start = skip_space(bytes, i + 1);
            if let Some(&q) = bytes.get(start).filter(|q| **q == b'\'' || **q == b'"') {
                let end = string_end(bytes, start, q);
                return format!("{}{}{}", &text[..start], quote(prefix), &text[end..]);
            }
        }
    }
    let at = insert_point(text);
    format!(
        "{}$table_prefix = {};\n{}",
        &text[..at],
        quote(prefix),
        &text[at..]
    )
}

/// Where new settings go: above "That's all, stop editing!", else above
/// the lines that load WordPress, else at the end.
fn insert_point(text: &str) -> usize {
    let mask = code_mask(text);
    let line_start = |at: usize| text[..at].rfind('\n').map_or(0, |i| i + 1);
    if let Some(at) = text.find("That's all, stop editing") {
        return line_start(at);
    }
    for marker in [
        "if ( ! defined( 'ABSPATH' )",
        "if ( !defined('ABSPATH') )",
        "require_once",
    ] {
        if let Some(at) = text
            .match_indices(marker)
            .map(|(i, _)| i)
            .find(|&i| mask[i])
        {
            return line_start(at);
        }
    }
    let lower = text.to_ascii_lowercase();
    if let Some(at) = lower.find("abspath") {
        if let Some(at) = text[..at].rfind("if") {
            return line_start(at);
        }
    }
    text.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"<?php
/**
 * The base configuration for WordPress
 * define( 'DB_NAME', 'not_this_one' );
 */
// define('WP_DEBUG', true);
define( 'DB_NAME', 'shop_live' );
define('DB_USER', "shop");
define( 'DB_PASSWORD', 'p@ss\'word;)' );
define( 'DB_HOST', 'localhost:3306' );
define( 'WP_HOME', getenv( 'HOME_URL' ) );
define('DISALLOW_FILE_EDIT', true);
# define('SCRIPT_DEBUG', true);
$table_prefix = 'shop_';

/* That's all, stop editing! Happy publishing. */
if ( ! defined( 'ABSPATH' ) ) {
	define( 'ABSPATH', __DIR__ . '/' );
}
require_once ABSPATH . 'wp-settings.php';
"#;

    #[test]
    fn reads_defines_and_prefix() {
        let config = WpConfig::read(SAMPLE);
        assert_eq!(config.get("DB_NAME"), Some("shop_live"));
        assert_eq!(config.get("DB_USER"), Some("shop"));
        assert_eq!(config.get("DB_PASSWORD"), Some("p@ss'word;)"));
        assert_eq!(config.get("DB_HOST"), Some("localhost:3306"));
        assert_eq!(config.defines.get("WP_HOME"), Some(&None));
        assert!(config.flag("DISALLOW_FILE_EDIT"));
        assert!(!config.flag("WP_DEBUG"));
        assert!(!config.defines.contains_key("SCRIPT_DEBUG"));
        assert_eq!(config.prefix(), "shop_");
        assert_eq!(config.defines.get("ABSPATH"), Some(&None));
    }

    #[test]
    fn changes_and_adds_defines() {
        let text = set_define(SAMPLE, "DB_NAME", &quote("shop"));
        let text = set_define(&text, "WP_DEBUG", "true");
        let text = set_define(&text, "WP_ENVIRONMENT_TYPE", &quote("staging"));
        let text = set_table_prefix(&text, "wp_");
        let config = WpConfig::read(&text);
        assert_eq!(config.get("DB_NAME"), Some("shop"));
        assert!(config.flag("WP_DEBUG"));
        assert_eq!(config.get("WP_ENVIRONMENT_TYPE"), Some("staging"));
        assert_eq!(config.prefix(), "wp_");
        // New lines go above "stop editing", the commented-out ones stay.
        let added = text.find("define( 'WP_DEBUG', true );").unwrap();
        assert!(added < text.find("That's all").unwrap(), "{text}");
        assert!(text.contains("// define('WP_DEBUG', true);"));
        assert!(text.contains("define( 'DB_PASSWORD', 'p@ss\\'word;)' );"));
        let removed = remove_define(&text, "WP_DEBUG");
        assert!(!WpConfig::read(&removed).defines.contains_key("WP_DEBUG"));
        assert_eq!(
            removed.len(),
            text.len() - "define( 'WP_DEBUG', true );\n".len()
        );
    }

    #[test]
    fn literals() {
        assert_eq!(literal("'a\\\\b'").as_deref(), Some("a\\b"));
        assert_eq!(literal("\"x\\\"y\"").as_deref(), Some("x\"y"));
        assert_eq!(literal("TRUE").as_deref(), Some("true"));
        assert_eq!(literal("256").as_deref(), Some("256"));
        assert_eq!(literal("\"$x\""), None);
        assert_eq!(literal("__DIR__ . '/'"), None);
        assert_eq!(quote("it's"), "'it\\'s'");
    }
}
