//! What AI tools did through the MCP server, one JSON line per tool call in
//! `logs/mcp.jsonl`, for the MCP server screen and `stackyard mcp log`.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::supervisor::tail_file;
use crate::{Error, Result};

/// The file is trimmed to the newest entries once it grows past this.
const MAX_BYTES: u64 = 256 * 1024;
const KEEP: usize = 500;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// Milliseconds since 1970 (UTC).
    pub at: u64,
    /// The AI tool, as it named itself, e.g. `claude-code`.
    pub client: String,
    pub tool: String,
    /// What it asked for, e.g. `caddy` or `myapp.stack → PHP 8.3`.
    pub summary: String,
    pub ok: bool,
    /// Why it failed, when it did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

pub fn file(home: &Path) -> PathBuf {
    home.join("logs").join("mcp.jsonl")
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Adds an entry, trimming old ones when the file gets large.
pub fn record(home: &Path, entry: &Entry) -> Result<()> {
    let path = file(home);
    let io = |source| Error::Io {
        path: path.clone(),
        source,
    };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(io)?;
    }
    let mut line = serde_json::to_string(entry).map_err(|e| Error::Mcp(e.to_string()))?;
    line.push('\n');
    let mut out = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(io)?;
    // One write per line, so several AI tools at once do not mix lines.
    out.write_all(line.as_bytes()).map_err(io)?;
    let size = out.metadata().map(|m| m.len()).unwrap_or(0);
    drop(out);
    if size > MAX_BYTES {
        let kept = tail_file(&path, KEEP)?;
        let tmp = path.with_extension("jsonl.tmp");
        std::fs::write(&tmp, kept.join("\n") + "\n").map_err(io)?;
        std::fs::rename(&tmp, &path).map_err(io)?;
    }
    Ok(())
}

/// The newest `count` entries, newest first.
pub fn recent(home: &Path, count: usize) -> Vec<Entry> {
    let lines = tail_file(&file(home), count).unwrap_or_default();
    lines
        .iter()
        .rev()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(n: u64) -> Entry {
        Entry {
            at: n,
            client: "test".into(),
            tool: "list_sites".into(),
            summary: String::new(),
            ok: n.is_multiple_of(2),
            error: (!n.is_multiple_of(2)).then(|| "failed".into()),
        }
    }

    #[test]
    fn records_newest_first_and_stays_small() {
        let home = crate::testutil::tempdir();
        assert!(recent(&home, 10).is_empty());
        for n in 0..3 {
            record(&home, &entry(n)).unwrap();
        }
        let got = recent(&home, 2);
        assert_eq!(got, vec![entry(2), entry(1)]);

        let big = Entry {
            summary: "x".repeat(1000),
            ..entry(0)
        };
        for _ in 0..300 {
            record(&home, &big).unwrap();
        }
        let size = std::fs::metadata(file(&home)).unwrap().len();
        assert!(size <= MAX_BYTES + 2000, "{size}");
        assert_eq!(recent(&home, 1), vec![big]);
    }
}
