//! AI coding tools: command-line coding assistants such as Claude Code,
//! Codex and Gemini CLI, installed with npm on Stackyard's own Node.js.
//!
//! Each tool lives in its own folder, `tools/ai/<id>/`, as a plain npm
//! project, so it never touches a Node.js version's global packages and
//! removing it is one folder delete. A receipt there records the Node.js
//! version it was installed with; the tool keeps running on that version
//! whatever version a project pins, since these tools need a recent one.
//!
//! The tool's command (`claude`, `codex`...) is a shim in `<home>/bin` like
//! `php` and `node`, leading back to `stackyard`, which runs the tool.

pub mod terminal;

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};

use crate::catalog::RuntimeKind;
use crate::download::Http;
use crate::install::Step;
use crate::platform::{Os, Platform};
use crate::runtimes::RuntimeManager;
use crate::settings::{read_toml, write_toml, Settings};
use crate::shell::{shims, Launch};
use crate::{Error, Result};

const RECEIPT: &str = ".stackyard-ai.toml";
const REGISTRY: &str = "https://registry.npmjs.org";

/// The Node.js version installed for a tool when no installed version is
/// new enough: the newest long-term support line.
pub const NODE_FOR_TOOLS: &str = "24";

#[derive(Debug, Clone, Serialize)]
pub struct AiTool {
    pub id: &'static str,
    pub name: &'static str,
    pub maker: &'static str,
    pub about: &'static str,
    /// The npm package its maker publishes.
    pub package: &'static str,
    /// What you type to start it.
    pub command: &'static str,
    /// Oldest Node.js major version it supports.
    pub min_node: u32,
    /// The maker's install and sign-in guide.
    pub docs: &'static str,
}

/// Every tool here is installed with npm, as its maker's guide shows.
pub const TOOLS: &[AiTool] = &[
    AiTool {
        id: "claude",
        name: "Claude Code",
        maker: "Anthropic",
        about: "Anthropic's coding agent. Sign in with a Claude account or an API key.",
        package: "@anthropic-ai/claude-code",
        command: "claude",
        min_node: 22,
        docs: "https://code.claude.com/docs/en/setup",
    },
    AiTool {
        id: "codex",
        name: "Codex CLI",
        maker: "OpenAI",
        about: "OpenAI's coding agent. Sign in with ChatGPT or an API key.",
        package: "@openai/codex",
        command: "codex",
        min_node: 18,
        docs: "https://github.com/openai/codex",
    },
    AiTool {
        id: "gemini",
        name: "Gemini CLI",
        maker: "Google",
        about: "Google's coding agent. Sign in with a Google account or an API key.",
        package: "@google/gemini-cli",
        command: "gemini",
        min_node: 20,
        docs: "https://github.com/google-gemini/gemini-cli",
    },
    AiTool {
        id: "copilot",
        name: "GitHub Copilot CLI",
        maker: "GitHub",
        about: "GitHub's coding agent. Needs a Copilot plan.",
        package: "@github/copilot",
        command: "copilot",
        min_node: 22,
        docs: "https://github.com/github/copilot-cli",
    },
    AiTool {
        id: "qwen",
        name: "Qwen Code",
        maker: "Qwen (Alibaba)",
        about: "Coding agent for Qwen models. Sign in with Qwen or use an API key.",
        package: "@qwen-code/qwen-code",
        command: "qwen",
        min_node: 22,
        docs: "https://github.com/QwenLM/qwen-code",
    },
    AiTool {
        id: "opencode",
        name: "OpenCode",
        maker: "SST",
        about: "Open-source coding agent that works with many model providers.",
        package: "@opencode/cli",
        command: "opencode",
        min_node: 20,
        docs: "https://github.com/sst/opencode",
    },
];

pub fn find(id: &str) -> Result<&'static AiTool> {
    TOOLS.iter().find(|t| t.id == id).ok_or_else(|| {
        let ids: Vec<&str> = TOOLS.iter().map(|t| t.id).collect();
        Error::Ai(format!("no AI tool {id:?}; pick one of {}", ids.join(", ")))
    })
}

/// `claude`, `CLAUDE` or `claude.exe` → Claude Code.
pub fn from_command(name: &str) -> Option<&'static AiTool> {
    let name = name.to_ascii_lowercase();
    let name = name.strip_suffix(".exe").unwrap_or(&name);
    TOOLS.iter().find(|t| t.command == name)
}

/// One tool and whether it is installed, for the screen and `stackyard ai`.
#[derive(Debug, Clone, Serialize)]
pub struct Status {
    #[serde(flatten)]
    pub tool: AiTool,
    /// Installed version, e.g. `2.1.302`.
    pub installed: Option<String>,
    /// The Node.js version it runs on.
    pub node: Option<String>,
    /// Newest version on npm, once checked.
    pub latest: Option<String>,
    /// `latest` is newer than `installed`.
    pub update: bool,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Receipt {
    node: String,
}

pub fn dir(home: &Path, tool: &AiTool) -> PathBuf {
    home.join("tools/ai").join(tool.id)
}

fn package_dir(dir: &Path, tool: &AiTool) -> PathBuf {
    tool.package
        .split('/')
        .fold(dir.join("node_modules"), |p, part| p.join(part))
}

/// The installed version, read from the package itself.
pub fn installed_version(home: &Path, tool: &AiTool) -> Option<String> {
    let dir = dir(home, tool);
    read_toml::<Receipt>(&dir.join(RECEIPT)).ok()??;
    let text = std::fs::read_to_string(package_dir(&dir, tool).join("package.json")).ok()?;
    let json: serde_json::Value = serde_json::from_str(&text).ok()?;
    json["version"].as_str().map(str::to_string)
}

pub fn status(home: &Path, tool: &AiTool) -> Status {
    let installed = installed_version(home, tool);
    let node = installed
        .as_ref()
        .and_then(|_| read_toml::<Receipt>(&dir(home, tool).join(RECEIPT)).ok()?)
        .map(|r| r.node);
    Status {
        tool: tool.clone(),
        installed,
        node,
        latest: None,
        update: false,
    }
}

pub fn list(home: &Path) -> Vec<Status> {
    TOOLS.iter().map(|t| status(home, t)).collect()
}

/// Adds the newest version on npm to each status. A tool whose lookup fails
/// keeps `latest` empty; the others still get theirs.
pub fn check_updates(statuses: &mut [Status]) {
    let http = Http::new();
    let registry = registry();
    std::thread::scope(|scope| {
        for status in statuses.iter_mut() {
            let (http, registry) = (&http, &registry);
            scope.spawn(move || {
                status.latest = latest(http, registry, status.tool.package).ok();
                status.update = match (&status.installed, &status.latest) {
                    (Some(have), Some(newest)) => is_newer(newest, have),
                    _ => false,
                };
            });
        }
    });
}

fn registry() -> String {
    std::env::var("STACKYARD_NPM_REGISTRY")
        .unwrap_or_else(|_| REGISTRY.into())
        .trim_end_matches('/')
        .to_string()
}

/// The version npm's `latest` tag points at.
fn latest(http: &Http, registry: &str, package: &str) -> Result<String> {
    let url = format!("{registry}/{package}/latest");
    let text = http.get_text(&url)?;
    serde_json::from_str::<serde_json::Value>(&text)
        .ok()
        .and_then(|v| v["version"].as_str().map(str::to_string))
        .ok_or_else(|| Error::Download {
            url,
            message: "no version in the answer".into(),
        })
}

/// Whether version `a` comes after `b`, comparing the numbers in order:
/// `0.10.0` after `0.9.3`. A pre-release (`1.0.0-beta`) counts as its numbers.
pub fn is_newer(a: &str, b: &str) -> bool {
    let numbers = |v: &str| -> Vec<u64> {
        v.split(['.', '-', '+'])
            .map_while(|part| part.parse().ok())
            .collect()
    };
    let (na, nb) = (numbers(a), numbers(b));
    if na.is_empty() || nb.is_empty() {
        return a != b;
    }
    na > nb
}

fn major(version: &str) -> u32 {
    version
        .split('.')
        .next()
        .and_then(|m| m.parse().ok())
        .unwrap_or(0)
}

/// An installed Node.js version new enough for `tool`: the default one if
/// it is, else the newest.
fn node_for(home: &Path, settings: &Settings, tool: &AiTool) -> Option<String> {
    let runtimes = RuntimeManager::new(home);
    let fits = |v: &&str| major(v) >= tool.min_node && runtimes.get(RuntimeKind::Node, v).is_some();
    settings
        .default_runtime(RuntimeKind::Node)
        .filter(fits)
        .or_else(|| {
            crate::catalog::runtime(RuntimeKind::Node)
                .versions
                .iter()
                .copied()
                .find(fits)
        })
        .map(str::to_string)
}

/// Installs the newest version of `tool`, or updates it. Installs Node.js
/// first when no installed version is new enough. Progress comes as short
/// sentences.
pub fn install(home: &Path, tool: &AiTool, progress: &mut dyn FnMut(String)) -> Result<Status> {
    let settings = Settings::load_from(&home.join("settings.toml"))?;
    settings.require_module("ai")?;
    settings.require_module("node")?;
    let node = match node_for(home, &settings, tool) {
        Some(version) => version,
        None => {
            let runtimes = RuntimeManager::new(home);
            runtimes.install(RuntimeKind::Node, NODE_FOR_TOOLS, &mut |step| {
                progress(node_step(&step))
            })?;
            NODE_FOR_TOOLS.to_string()
        }
    };
    let runtimes = RuntimeManager::new(home);
    let installed = runtimes
        .get(RuntimeKind::Node, &node)
        .ok_or_else(|| Error::NotInstalled {
            name: "Node.js".into(),
            version: node.clone(),
        })?;
    let dest = dir(home, tool);
    let parent = dest.parent().expect("tools/ai/<id>");
    let staging = parent.join(format!(".{}.new", tool.id));
    let io = |path: &Path| {
        let path = path.to_owned();
        move |source| Error::Io { path, source }
    };
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging).map_err(io(&staging))?;
    // A package.json of its own stops npm from looking in parent folders.
    std::fs::write(
        staging.join("package.json"),
        "{\"name\":\"stackyard-ai-tool\",\"private\":true}\n",
    )
    .map_err(io(&staging))?;

    progress(format!("Installing {} with npm…", tool.name));
    let result = npm_install(home, &installed.path, &installed.binary, &staging, tool);
    let result = result.and_then(|()| {
        write_toml(&staging.join(RECEIPT), &Receipt { node: node.clone() })?;
        // Catch a package that changed shape before it replaces a working one.
        program(&staging, tool).map(|_| ())
    });
    if let Err(e) = result {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(e);
    }
    if dest.exists() {
        if let Err(e) = set_aside(&dest, tool, "updated") {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(e);
        }
    }
    std::fs::rename(&staging, &dest).map_err(io(&dest))?;
    link_shim(home, tool)?;
    Ok(status(home, tool))
}

fn node_step(step: &Step) -> String {
    let what = format!("Node.js {NODE_FOR_TOOLS}");
    match step {
        Step::Resolving => format!("Finding {what}…"),
        Step::Downloading {
            done,
            total: Some(total),
        } if *total > 0 => format!("Downloading {what}… {}%", done * 100 / total),
        Step::Downloading { .. } => format!("Downloading {what}…"),
        Step::Unpacking => format!("Unpacking {what}…"),
    }
}

/// Runs npm from the Node.js folder `node_dir` to install `tool` into `into`.
fn npm_install(
    home: &Path,
    node_dir: &Path,
    node: &Path,
    into: &Path,
    tool: &AiTool,
) -> Result<()> {
    let os = Platform::current().os;
    let modules = match os {
        Os::Windows => node_dir.join("node_modules"),
        _ => node_dir.join("lib/node_modules"),
    };
    let npm = modules.join("npm/bin/npm-cli.js");
    let mut command = Command::new(node);
    command
        .arg(&npm)
        .args(["install", "--no-fund", "--no-audit", "--loglevel=error"])
        .arg(format!("--registry={}/", registry()))
        .arg(format!("{}@latest", tool.package))
        .current_dir(into)
        .env("PATH", path_with(&[node.parent().unwrap_or(node_dir)]))
        .env("npm_config_update_notifier", "false")
        .stdin(Stdio::null());
    crate::platform::hide_console(&mut command);
    let output = command.output().map_err(|source| Error::Spawn {
        program: node.display().to_string(),
        source,
    })?;
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    let log = home.join("logs").join("ai-tools.log");
    let _ = std::fs::create_dir_all(home.join("logs"));
    let _ = std::fs::write(&log, &text);
    if output.status.success() {
        return Ok(());
    }
    let last: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    let last = &last[last.len().saturating_sub(4)..];
    Err(Error::Ai(format!(
        "npm could not install {}: {}. The full output is in {}",
        tool.name,
        last.join(" / "),
        log.display()
    )))
}

/// `PATH` with `first` in front.
fn path_with(first: &[&Path]) -> OsString {
    let rest = std::env::var_os("PATH").unwrap_or_default();
    let first: Vec<PathBuf> = first.iter().map(|p| p.to_path_buf()).collect();
    let others = std::env::split_paths(&rest).filter(|p| !first.contains(p));
    let parts: Vec<PathBuf> = first.iter().cloned().chain(others).collect();
    std::env::join_paths(parts).unwrap_or(rest)
}

pub fn uninstall(home: &Path, tool: &AiTool) -> Result<()> {
    let dest = dir(home, tool);
    if !dest.exists() {
        return Err(Error::Ai(format!("{} is not installed", tool.name)));
    }
    set_aside(&dest, tool, "removed")?;
    let shim = shim_path(home, tool);
    remove_file(&shim).map_err(|source| Error::Io { path: shim, source })
}

/// Moves an installed tool out of the way and deletes it. Windows refuses
/// the move while the tool runs, which leaves it whole.
fn set_aside(dest: &Path, tool: &AiTool, verb: &str) -> Result<()> {
    let trash = dest.with_file_name(format!(".{}.old-{}", tool.id, std::process::id()));
    std::fs::rename(dest, &trash).map_err(|_| {
        Error::Ai(format!(
            "{} is running, so it cannot be {verb}; close it and try again",
            tool.name
        ))
    })?;
    let _ = std::fs::remove_dir_all(&trash);
    Ok(())
}

/// What the package says to run: a JavaScript file for Node.js, or a
/// native program (Claude Code and OpenCode ship one).
#[derive(Debug, PartialEq, Eq)]
enum Program {
    Script(PathBuf),
    Native(PathBuf),
}

fn program(dir: &Path, tool: &AiTool) -> Result<Program> {
    let package = package_dir(dir, tool);
    let broken = |why: &str| {
        Error::Ai(format!(
            "{} is not installed properly ({why}); install it again",
            tool.name
        ))
    };
    let text = std::fs::read_to_string(package.join("package.json"))
        .map_err(|_| broken("its package.json is missing"))?;
    let json: serde_json::Value =
        serde_json::from_str(&text).map_err(|_| broken("its package.json is damaged"))?;
    let bin = match &json["bin"] {
        serde_json::Value::String(path) => Some(path.as_str()),
        serde_json::Value::Object(map) => map
            .get(tool.command)
            .or_else(|| map.values().next())
            .and_then(|v| v.as_str()),
        _ => None,
    }
    .ok_or_else(|| broken("it names no program"))?;
    let path = package.join(bin);
    if !path.is_file() {
        return Err(broken(&format!("{bin} is missing")));
    }
    let script = matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("js" | "mjs" | "cjs")
    );
    Ok(if script {
        Program::Script(path)
    } else {
        Program::Native(path)
    })
}

/// How to run `tool` with `args`.
pub fn launch(home: &Path, tool: &AiTool, args: Vec<OsString>) -> Result<Launch> {
    let settings = Settings::load_from(&home.join("settings.toml"))?;
    settings.require_module("ai")?;
    let dest = dir(home, tool);
    let version = installed_version(home, tool).ok_or_else(|| {
        Error::Ai(format!(
            "{} is not installed; install it on Stackyard's AI tools screen or with `stackyard ai install {}`",
            tool.name, tool.id
        ))
    })?;
    let receipt: Receipt = read_toml(&dest.join(RECEIPT))?.unwrap_or_default();
    // The Node.js it was installed with, or another new enough one if that
    // version was removed since.
    let runtimes = RuntimeManager::new(home);
    let node = runtimes
        .get(RuntimeKind::Node, &receipt.node)
        .or_else(|| {
            node_for(home, &settings, tool).and_then(|v| runtimes.get(RuntimeKind::Node, &v))
        })
        .ok_or_else(|| {
            Error::Ai(format!(
                "{} needs Node.js {} or newer; install it on the Runtimes screen",
                tool.name, tool.min_node
            ))
        })?;
    let node_bin = node
        .binary
        .parent()
        .map_or_else(|| node.path.clone(), Path::to_path_buf);
    let (program, args) = match program(&dest, tool)? {
        Program::Script(script) => {
            let mut full = vec![script.into_os_string()];
            full.extend(args);
            (node.binary.clone(), full)
        }
        Program::Native(exe) => (exe, args),
    };
    Ok(Launch {
        program,
        args,
        path_first: node_bin,
        env: Vec::new(),
        summary: format!("{} {version} (Node.js {})", tool.name, node.version),
    })
}

/// Opens a terminal window in `folder` running `tool`. The window keeps
/// Stackyard's commands (`php`, `node`, the AI tools) on its PATH and stays
/// open after the tool ends.
pub fn open(home: &Path, tool: &AiTool, folder: &Path) -> Result<()> {
    if !folder.is_dir() {
        return Err(Error::NotAFolder(folder.to_owned()));
    }
    let run = launch(home, tool, Vec::new())?;
    let mut path_first = Vec::new();
    if shims::program_path(home).exists() {
        path_first.push(shims::bin_dir(home));
    }
    path_first.push(run.path_first.clone());
    let refs: Vec<&Path> = path_first.iter().map(PathBuf::as_path).collect();
    terminal::open(&terminal::Window {
        title: tool.name.to_string(),
        folder: folder.to_owned(),
        program: run.program,
        args: run.args,
        path: path_with(&refs),
        path_first,
    })
}

pub fn shim_path(home: &Path, tool: &AiTool) -> PathBuf {
    shims::bin_dir(home).join(format!("{}{}", tool.command, std::env::consts::EXE_SUFFIX))
}

/// Adds the command for `tool` next to `php` and `node`, when those are
/// set up (Stackyard Shell is on).
fn link_shim(home: &Path, tool: &AiTool) -> Result<()> {
    let program = shims::program_path(home);
    if !program.exists() {
        return Ok(());
    }
    let shim = shim_path(home, tool);
    let io = |source| Error::Io {
        path: shim.clone(),
        source,
    };
    remove_file(&shim).map_err(io)?;
    link(&program, &shim).map_err(io)
}

/// Re-creates the commands of installed tools, after the `stackyard`
/// program they lead to was replaced.
pub fn link_shims(home: &Path) -> Result<()> {
    for tool in TOOLS {
        if installed_version(home, tool).is_some() {
            link_shim(home, tool)?;
        }
    }
    Ok(())
}

/// Removes every tool's command; the tools stay installed.
pub fn remove_shims(home: &Path) -> Result<()> {
    for tool in TOOLS {
        let shim = shim_path(home, tool);
        remove_file(&shim).map_err(|source| Error::Io { path: shim, source })?;
    }
    Ok(())
}

fn remove_file(path: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

#[cfg(unix)]
fn link(program: &Path, shim: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(program, shim)
}

#[cfg(windows)]
fn link(program: &Path, shim: &Path) -> std::io::Result<()> {
    std::fs::hard_link(program, shim).or_else(|_| std::fs::copy(program, shim).map(|_| ()))
}

#[cfg(test)]
mod tests;
