//! The MCP server behind `stackyard mcp`: AI coding tools such as Claude
//! Code or Cursor start it and talk the Model Context Protocol to it,
//! JSON-RPC 2.0 messages one per line on stdin and stdout. It offers tools
//! that read sites, services, runtimes, database names and logs, and, once
//! the user allows each one, tools that start, stop and restart services,
//! add or change sites, and query or change MySQL and MariaDB databases.

pub mod activity;
mod sql;
mod tools;

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use serde_json::{json, Map, Value};

use crate::settings::Settings;
use crate::shell::shims;
use crate::{Error, Result};

pub use tools::{find, input_schema, ToolInfo, TOOLS};

/// Protocol versions this server speaks, newest first.
pub const PROTOCOL_VERSIONS: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

/// How often the server checks whether the user allowed or blocked tools.
const WATCH_EVERY: Duration = if cfg!(test) {
    Duration::from_millis(50)
} else {
    Duration::from_secs(2)
};

const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;

const INSTRUCTIONS: &str = "Stackyard runs this computer's local development stack: \
sites at addresses such as https://myapp.stack, PHP and Node.js versions, and services \
such as the Caddy web server, MySQL, MariaDB and Mailpit. Use the list tools to see what \
is set up and read_logs to find out why something fails. Tools that change things, and \
querying databases, appear only after the user allows them in Stackyard.";

/// One connection with an AI tool.
pub struct Server {
    home: PathBuf,
    program: PathBuf,
    /// The AI tool, as it named itself when it connected.
    client: String,
    version: &'static str,
    initialized: bool,
}

impl Server {
    /// `program` is the `stackyard` program, started in the background
    /// when a tool starts a service and nothing runs services yet.
    pub fn new(home: &Path, program: &Path) -> Self {
        Self {
            home: home.to_owned(),
            program: program.to_owned(),
            client: "unknown".into(),
            version: PROTOCOL_VERSIONS[0],
            initialized: false,
        }
    }

    /// Answers one line of input; `None` when it needs no answer.
    pub fn handle_line(&mut self, line: &str) -> Option<String> {
        let line = line.trim();
        if line.is_empty() {
            return None;
        }
        let answer = match serde_json::from_str::<Value>(line) {
            Err(_) => Some(error(Value::Null, PARSE_ERROR, "parse error")),
            // Batches were part of earlier protocol versions.
            Ok(Value::Array(batch)) if batch.is_empty() => {
                Some(error(Value::Null, INVALID_REQUEST, "empty batch"))
            }
            Ok(Value::Array(batch)) => {
                let answers: Vec<Value> =
                    batch.into_iter().filter_map(|m| self.handle(m)).collect();
                (!answers.is_empty()).then_some(Value::Array(answers))
            }
            Ok(message) => self.handle(message),
        };
        answer.map(|a| a.to_string())
    }

    fn handle(&mut self, message: Value) -> Option<Value> {
        let Value::Object(message) = message else {
            return Some(error(Value::Null, INVALID_REQUEST, "expected an object"));
        };
        let id = message.get("id").cloned();
        let Some(method) = message.get("method").and_then(Value::as_str) else {
            // An answer to a request this server never sends, or junk.
            let answer = message.contains_key("result") || message.contains_key("error");
            return (!answer)
                .then(|| error(id.unwrap_or(Value::Null), INVALID_REQUEST, "missing method"));
        };
        let params = message.get("params").cloned().unwrap_or(Value::Null);
        // Notifications (initialized, cancelled...) need no answer.
        let id = id?;
        Some(match self.request(method, &params) {
            Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
            Err((code, message)) => error(id, code, &message),
        })
    }

    fn request(
        &mut self,
        method: &str,
        params: &Value,
    ) -> std::result::Result<Value, (i64, String)> {
        match method {
            "initialize" => Ok(self.initialize(params)),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({ "tools": self.tool_list() })),
            "tools/call" => self.call_tool(params),
            _ => Err((METHOD_NOT_FOUND, format!("unknown method {method}"))),
        }
    }

    fn initialize(&mut self, params: &Value) -> Value {
        let asked = params.get("protocolVersion").and_then(Value::as_str);
        self.version = PROTOCOL_VERSIONS
            .iter()
            .find(|v| Some(**v) == asked)
            .copied()
            .unwrap_or(PROTOCOL_VERSIONS[0]);
        if let Some(name) = params.pointer("/clientInfo/name").and_then(Value::as_str) {
            self.client = name.chars().filter(|c| !c.is_control()).take(64).collect();
        }
        self.initialized = true;
        json!({
            "protocolVersion": self.version,
            "capabilities": { "tools": { "listChanged": true } },
            "serverInfo": {
                "name": "stackyard",
                "title": "Stackyard",
                "version": env!("CARGO_PKG_VERSION"),
            },
            "instructions": INSTRUCTIONS,
        })
    }

    fn tool_list(&self) -> Vec<Value> {
        visible_tools(&self.home)
            .into_iter()
            .map(|tool| {
                json!({
                    "name": tool.name,
                    "title": tool.title,
                    "description": tool.description,
                    "inputSchema": input_schema(tool.name),
                    "annotations": {
                        "title": tool.title,
                        "readOnlyHint": tool.read_only,
                        "destructiveHint": tool.destructive,
                        "idempotentHint": !matches!(tool.name, "create_site" | "change_database"),
                        "openWorldHint": false,
                    },
                })
            })
            .collect()
    }

    fn call_tool(&mut self, params: &Value) -> std::result::Result<Value, (i64, String)> {
        let name = params
            .get("name")
            .and_then(Value::as_str)
            .ok_or((INVALID_PARAMS, "missing tool name".to_string()))?;
        let tool = find(name).ok_or_else(|| (INVALID_PARAMS, format!("unknown tool {name}")))?;
        let args = match params.get("arguments") {
            None | Some(Value::Null) => Map::new(),
            Some(Value::Object(args)) => args.clone(),
            Some(_) => return Err((INVALID_PARAMS, "arguments must be an object".into())),
        };
        let outcome = match blocked(&self.home, tool) {
            Some(reason) => tools::Outcome {
                summary: tools::summarize(tool.name, &args),
                result: Err(reason),
            },
            None => tools::call(
                &tools::Ctx {
                    home: &self.home,
                    program: &self.program,
                },
                tool,
                &args,
            ),
        };
        let entry = activity::Entry {
            at: activity::now_ms(),
            client: self.client.clone(),
            tool: tool.name.into(),
            summary: outcome.summary,
            ok: outcome.result.is_ok(),
            error: outcome.result.as_ref().err().cloned(),
        };
        if let Err(e) = activity::record(&self.home, &entry) {
            eprintln!("stackyard mcp: could not record activity: {e}");
        }
        Ok(match outcome.result {
            Ok(value) => {
                let text = serde_json::to_string_pretty(&value).unwrap_or_default();
                let mut result = json!({
                    "content": [{ "type": "text", "text": text }],
                    "isError": false,
                });
                // Structured results arrived in 2025-06-18.
                if self.version >= "2025-06-18" {
                    result["structuredContent"] = value;
                }
                result
            }
            Err(message) => json!({
                "content": [{ "type": "text", "text": message }],
                "isError": true,
            }),
        })
    }
}

fn error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn load_settings(home: &Path) -> Settings {
    Settings::load_from(&home.join("settings.toml")).unwrap_or_default()
}

/// Why a tool cannot be used right now, if it cannot.
fn blocked(home: &Path, tool: &ToolInfo) -> Option<String> {
    let settings = load_settings(home);
    if !settings.module_enabled("mcp") {
        return Some("The MCP server is switched off in Stackyard (Settings › Modules).".into());
    }
    (tool.action && !settings.mcp.allow.contains(tool.name)).then(|| {
        format!(
            "{} is turned off. The user can allow it in Stackyard › Settings › AI tools, or with `stackyard mcp allow {}`.",
            tool.title, tool.name
        )
    })
}

/// The tools an AI tool sees: none while the module is off, otherwise the
/// tools that are always on plus the ones the user allowed.
pub fn visible_tools(home: &Path) -> Vec<&'static ToolInfo> {
    TOOLS
        .iter()
        .filter(|t| blocked(home, t).is_none())
        .collect()
}

/// Allows or blocks an action tool, or every one with `all`.
pub fn set_allowed(settings: &mut Settings, tool: &str, allowed: bool) -> Result<()> {
    let names: Vec<&str> = match tool {
        "all" => TOOLS.iter().filter(|t| t.action).map(|t| t.name).collect(),
        name => match find(name) {
            Some(t) if t.action => vec![t.name],
            Some(t) => {
                return Err(Error::Mcp(format!(
                    "{} only reads, so it is always allowed",
                    t.name
                )))
            }
            None => {
                let actions: Vec<&str> =
                    TOOLS.iter().filter(|t| t.action).map(|t| t.name).collect();
                return Err(Error::Mcp(format!(
                    "no action tool named {name:?}; use one of {}, or all",
                    actions.join(", ")
                )));
            }
        },
    };
    for name in names {
        if allowed {
            settings.mcp.allow.insert(name.into());
        } else {
            settings.mcp.allow.remove(name);
        }
    }
    Ok(())
}

/// How to add Stackyard to AI tools.
#[derive(Debug, Clone, Serialize)]
pub struct ClientSetup {
    /// The program AI tools start.
    pub program: PathBuf,
    /// The command that adds it to Claude Code.
    pub claude_code: String,
    /// The `mcpServers` entry Cursor, Claude Desktop, Windsurf and others read.
    pub json: String,
}

/// Prefers the copy in `<home>/bin` that the app keeps up to date, so the
/// setup survives the app moving or updating; else `fallback`.
pub fn client_setup(home: &Path, fallback: &Path) -> ClientSetup {
    let copy = shims::program_path(home);
    let program = if copy.is_file() {
        copy
    } else {
        fallback.to_owned()
    };
    let shown = program.display().to_string();
    let quoted = if shown.contains([' ', '\'', '"']) {
        format!("\"{shown}\"")
    } else {
        shown.clone()
    };
    let json = json!({ "mcpServers": { "stackyard": { "command": shown, "args": ["mcp"] } } });
    ClientSetup {
        claude_code: format!("claude mcp add --scope user stackyard -- {quoted} mcp"),
        json: serde_json::to_string_pretty(&json).unwrap_or_default(),
        program,
    }
}

/// Serves one AI tool until its input ends. While it runs, the tool is
/// told when the user allows or blocks tools, so it sees the change.
pub fn serve(
    home: &Path,
    program: &Path,
    input: impl BufRead,
    output: impl Write + Send + 'static,
) -> Result<()> {
    let output = Arc::new(Mutex::new(output));
    let ready = Arc::new(AtomicBool::new(false));
    let (stop, stopped) = mpsc::channel::<()>();
    let watcher = {
        let (home, output, ready) = (home.to_owned(), output.clone(), ready.clone());
        std::thread::spawn(move || {
            let names =
                |home: &Path| -> Vec<&str> { visible_tools(home).iter().map(|t| t.name).collect() };
            let mut last = names(&home);
            while let Err(RecvTimeoutError::Timeout) = stopped.recv_timeout(WATCH_EVERY) {
                let now = names(&home);
                if now != last {
                    last = now;
                    if ready.load(Ordering::Relaxed) {
                        let note = json!({ "jsonrpc": "2.0", "method": "notifications/tools/list_changed" });
                        let _ = write_line(&output, &note.to_string());
                    }
                }
            }
        })
    };
    let io = |source| Error::Io {
        path: PathBuf::from("<stdio>"),
        source,
    };
    let mut server = Server::new(home, program);
    let result = (|| {
        for line in input.lines() {
            if let Some(answer) = server.handle_line(&line.map_err(io)?) {
                write_line(&output, &answer).map_err(io)?;
            }
            ready.store(server.initialized, Ordering::Relaxed);
        }
        Ok(())
    })();
    drop(stop);
    let _ = watcher.join();
    result
}

fn write_line<W: Write>(output: &Mutex<W>, line: &str) -> std::io::Result<()> {
    let mut out = output.lock().unwrap_or_else(|e| e.into_inner());
    out.write_all(line.as_bytes())?;
    out.write_all(b"\n")?;
    out.flush()
}

#[cfg(test)]
pub(crate) mod tests;
