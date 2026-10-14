//! The log viewer: every log Stackyard writes, read from the end and then
//! followed as it grows, the way `tail -f` does.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::time::UNIX_EPOCH;

use serde::Serialize;

use crate::catalog::{self, SERVICES};
use crate::supervisor;
use crate::{Error, Result};

/// The most a single read returns; a follower catches up over several reads.
const MAX_CHUNK: u64 = 1024 * 1024;
/// How far back from the end the first read looks for its lines.
const TAIL_WINDOW: u64 = 256 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LogFile {
    /// `caddy`, `php-8.4`, `daemon`…
    pub id: String,
    /// `Caddy`, `PHP 8.4`, `Background service`…
    pub name: String,
    pub size: u64,
    /// Last written, in milliseconds since 1970.
    pub modified: u64,
}

/// What a read found: whole lines, and where the next read starts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Chunk {
    pub lines: Vec<String>,
    /// Pass this back to read what was written since.
    pub offset: u64,
    /// The file was started over (a new log after it grew too large), so
    /// what was shown before is gone; `lines` is the end of the new one.
    pub reset: bool,
}

/// A readable name for a log id.
pub fn name(id: &str) -> String {
    if let Some(def) = catalog::service(id) {
        return def.name.to_string();
    }
    if let Some(version) = id.strip_prefix("php-") {
        return format!("PHP {version}");
    }
    match id {
        "daemon" => "Background service".into(),
        other => other.into(),
    }
}

/// Every log there is, services first, then PHP versions, then the rest.
pub fn list(home: &Path) -> Vec<LogFile> {
    let Ok(entries) = std::fs::read_dir(home.join("logs")) else {
        return Vec::new();
    };
    let mut logs: Vec<LogFile> = entries
        .flatten()
        .filter_map(|entry| {
            let file_name = entry.file_name();
            let id = file_name.to_str()?.strip_suffix(".log")?;
            supervisor::log_file(home, id).ok()?;
            let meta = entry.metadata().ok()?;
            let modified = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_millis() as u64);
            Some(LogFile {
                id: id.to_string(),
                name: name(id),
                size: meta.len(),
                modified,
            })
        })
        .collect();
    let rank = |id: &str| -> (usize, Vec<u32>) {
        if let Some(i) = SERVICES.iter().position(|s| s.id == id) {
            return (i, Vec::new());
        }
        match id.strip_prefix("php-") {
            // Newest PHP first.
            Some(v) => (
                SERVICES.len(),
                v.split('.')
                    .map(|n| u32::MAX - n.parse::<u32>().unwrap_or(0))
                    .collect(),
            ),
            None => (SERVICES.len() + 1, Vec::new()),
        }
    };
    logs.sort_by_key(|log| rank(&log.id));
    logs
}

/// Reads a log: with `from` as `None`, its last `lines` lines; with the
/// `offset` a previous read returned, whatever was written since. Only
/// whole lines are returned; a line still being written waits for the
/// next read.
pub fn read(home: &Path, id: &str, from: Option<u64>, lines: usize) -> Result<Chunk> {
    let path = supervisor::log_file(home, id)?;
    let io = |source| Error::Io {
        path: path.clone(),
        source,
    };
    let mut file = match File::open(&path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Chunk {
                lines: Vec::new(),
                offset: 0,
                reset: from.is_some_and(|f| f > 0),
            })
        }
        Err(e) => return Err(io(e)),
    };
    let len = file.metadata().map_err(io)?.len();
    match from {
        Some(offset) if offset <= len => {
            let bytes = read_range(&mut file, offset, len.min(offset + MAX_CHUNK)).map_err(io)?;
            let whole = bytes.iter().rposition(|b| *b == b'\n').map_or(0, |i| i + 1);
            Ok(Chunk {
                lines: split(&bytes[..whole]),
                offset: offset + whole as u64,
                reset: false,
            })
        }
        // Shorter than where we were: it was started over.
        reset => {
            let start = len.saturating_sub(TAIL_WINDOW);
            let bytes = read_range(&mut file, start, len).map_err(io)?;
            let whole = bytes.iter().rposition(|b| *b == b'\n').map_or(0, |i| i + 1);
            let mut body = &bytes[..whole];
            if start > 0 {
                // The window began mid-line; skip to the next whole one.
                let first = body
                    .iter()
                    .position(|b| *b == b'\n')
                    .map_or(body.len(), |i| i + 1);
                body = &body[first..];
            }
            let all = split(body);
            let skip = all.len().saturating_sub(lines);
            Ok(Chunk {
                lines: all[skip..].to_vec(),
                offset: start + whole as u64,
                reset: reset.is_some(),
            })
        }
    }
}

fn read_range(file: &mut File, start: u64, end: u64) -> std::io::Result<Vec<u8>> {
    file.seek(SeekFrom::Start(start))?;
    let mut bytes = Vec::with_capacity((end - start) as usize);
    file.take(end - start).read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn split(bytes: &[u8]) -> Vec<String> {
    String::from_utf8_lossy(bytes)
        .lines()
        .map(|l| l.trim_end_matches('\r').to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn append(home: &Path, id: &str, text: &str) {
        let dir = home.join("logs");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join(format!("{id}.log")))
            .unwrap()
            .write_all(text.as_bytes())
            .unwrap();
    }

    #[test]
    fn lists_known_logs_in_order() {
        let home = crate::testutil::tempdir();
        assert!(list(&home).is_empty());
        for id in ["daemon", "php-8.2", "mailpit", "php-8.4", "caddy"] {
            append(&home, id, "x\n");
        }
        append(&home, "notes", "not a Stackyard log\n");
        std::fs::write(home.join("logs/caddy.log.1"), "old").unwrap();
        let ids: Vec<String> = list(&home).into_iter().map(|l| l.id).collect();
        assert_eq!(ids, ["caddy", "mailpit", "php-8.4", "php-8.2", "daemon"]);
        assert_eq!(list(&home)[2].name, "PHP 8.4");
        assert_eq!(list(&home)[4].name, "Background service");
        assert_eq!(list(&home)[0].size, 2);
    }

    #[test]
    fn reads_the_end_then_follows() {
        let home = crate::testutil::tempdir();
        let first = read(&home, "caddy", None, 10).unwrap();
        assert_eq!(
            first,
            Chunk {
                lines: vec![],
                offset: 0,
                reset: false
            }
        );

        append(&home, "caddy", "one\r\ntwo\nthree\nhalf");
        let first = read(&home, "caddy", None, 2).unwrap();
        assert_eq!(first.lines, ["two", "three"]);
        let nothing = read(&home, "caddy", Some(first.offset), 10).unwrap();
        assert!(nothing.lines.is_empty());

        append(&home, "caddy", " done\nfour\n");
        let next = read(&home, "caddy", Some(first.offset), 10).unwrap();
        assert_eq!(next.lines, ["half done", "four"]);
        assert!(!next.reset);

        // Started over after growing too large.
        std::fs::write(home.join("logs/caddy.log"), "fresh\n").unwrap();
        let again = read(&home, "caddy", Some(next.offset), 10).unwrap();
        assert!(again.reset);
        assert_eq!(again.lines, ["fresh"]);
        assert_eq!(again.offset, 6);

        assert!(read(&home, "../secret", None, 10).is_err());
    }

    #[test]
    fn a_long_log_starts_at_a_whole_line() {
        let home = crate::testutil::tempdir();
        let line = format!("{}\n", "y".repeat(99));
        append(&home, "mysql", &line.repeat(5000));
        let chunk = read(&home, "mysql", None, 100_000).unwrap();
        assert!(chunk.lines.len() < 5000);
        assert!(chunk.lines.iter().all(|l| l.len() == 99));
        assert_eq!(chunk.offset, 500_000);
    }
}
