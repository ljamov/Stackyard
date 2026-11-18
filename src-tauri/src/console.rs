//! The Terminal tab in the window's panel: a shell in a pseudo-terminal,
//! started in a site's folder with Stackyard's commands (`php`, `node`,
//! `composer`…) first on its PATH, so they run the versions that folder asks
//! for. What the shell prints arrives as `console-output` events; when it
//! ends, `console-exit`.
//!
//! Each terminal keeps the end of what it printed, so a window opened later
//! (the main window again, or a terminal popped out into its own window)
//! shows the same screen and the lines above it.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use serde::Serialize;
use stackyard_core::paths;
use stackyard_core::platform::Platform;
use stackyard_core::settings::Settings;
use stackyard_core::shell::{self, shims};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::{err, CmdResult};

struct Session {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
    info: ConsoleInfo,
    history: Arc<Mutex<History>>,
}

/// An open terminal, as the window lists it.
#[derive(Clone, Serialize)]
pub struct ConsoleInfo {
    id: u32,
    folder: Option<String>,
    label: Option<String>,
    shell: String,
}

/// What `console_open` gives back: the terminal's number, and the AI tools
/// installed with npm that no longer start and need installing again.
#[derive(Serialize)]
pub struct Opened {
    id: u32,
    broken: Vec<stackyard_core::ai::Broken>,
}

/// A shell the terminal can start.
#[derive(Clone, Serialize)]
pub struct ShellChoice {
    id: String,
    name: String,
}

/// The end of what a terminal printed, at most [`History::LIMIT`] bytes,
/// cut at a line break so no line or colour code starts half way.
#[derive(Default)]
struct History {
    text: String,
    seq: u64,
}

/// What a terminal printed so far, and the `seq` of the last output in it.
#[derive(Serialize)]
pub struct Printed {
    text: String,
    seq: u64,
}

impl History {
    const LIMIT: usize = 2 * 1024 * 1024;

    fn push(&mut self, data: &str) -> u64 {
        self.seq += 1;
        self.text.push_str(data);
        if self.text.len() > Self::LIMIT {
            let mut cut = self.text.len() - Self::LIMIT * 3 / 4;
            while !self.text.is_char_boundary(cut) {
                cut += 1;
            }
            let cut = self.text[cut..].find('\n').map_or(cut, |at| cut + at + 1);
            self.text.drain(..cut);
        }
        self.seq
    }
}

/// The open terminals, by the number the window knows them by.
#[derive(Default)]
pub struct Consoles {
    next: AtomicU32,
    open: Mutex<HashMap<u32, Session>>,
}

impl Drop for Consoles {
    fn drop(&mut self) {
        if let Ok(open) = self.open.get_mut() {
            for session in open.values_mut() {
                let _ = session.child.kill();
            }
        }
    }
}

#[derive(Clone, Serialize)]
struct Output {
    id: u32,
    data: String,
    /// Counts this terminal's output events, so a window that read the
    /// history can skip the ones it holds.
    seq: u64,
}

#[derive(Clone, Serialize)]
struct Exit {
    id: u32,
}

fn size(cols: u16, rows: u16) -> PtySize {
    PtySize {
        rows: rows.max(2),
        cols: cols.max(10),
        pixel_width: 0,
        pixel_height: 0,
    }
}

/// The shells on this computer, the default first.
fn shells() -> Vec<(ShellChoice, PathBuf, Vec<&'static str>)> {
    let choice = |id: &str, name: &str| ShellChoice {
        id: id.to_string(),
        name: name.to_string(),
    };
    let mut found = Vec::new();
    if cfg!(windows) {
        let system = std::env::var_os("SystemRoot")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\Windows"))
            .join("System32");
        found.push((
            choice("powershell", "Windows PowerShell"),
            system.join(r"WindowsPowerShell\v1.0\powershell.exe"),
            vec!["-NoLogo"],
        ));
        if let Some(pwsh) = on_path("pwsh.exe").or_else(|| program_files(r"PowerShell\7\pwsh.exe"))
        {
            found.push((choice("pwsh", "PowerShell 7"), pwsh, vec!["-NoLogo"]));
        }
        found.push((
            choice("cmd", "Command Prompt"),
            system.join("cmd.exe"),
            vec![],
        ));
        if let Some(bash) = git_bash() {
            found.push((choice("gitbash", "Git Bash"), bash, vec!["--login", "-i"]));
        }
        if system.join("wsl.exe").is_file() {
            found.push((choice("wsl", "WSL"), system.join("wsl.exe"), vec![]));
        }
    } else {
        let own = std::env::var_os("SHELL")
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/bin/sh"));
        let name = own
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "sh".into());
        found.push((choice("default", &name), own.clone(), vec!["-l"]));
        for (id, path) in [
            ("bash", "/bin/bash"),
            ("zsh", "/bin/zsh"),
            ("fish", "/usr/bin/fish"),
        ] {
            if Path::new(path).is_file() && Path::new(path) != own {
                found.push((choice(id, id), PathBuf::from(path), vec!["-l"]));
            }
        }
    }
    found
}

fn on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|p| p.is_file())
}

fn program_files(rest: &str) -> Option<PathBuf> {
    ["ProgramFiles", "ProgramW6432", "LOCALAPPDATA"]
        .iter()
        .filter_map(std::env::var_os)
        .map(|dir| PathBuf::from(dir).join(rest))
        .find(|p| p.is_file())
}

/// Git for Windows' bash, the one its "Git Bash" shortcut starts.
fn git_bash() -> Option<PathBuf> {
    program_files(r"Git\bin\bash.exe")
        .or_else(|| program_files(r"Programs\Git\bin\bash.exe"))
        .or_else(|| {
            // git.exe is in <Git>\cmd; bash is in <Git>\bin.
            let git = on_path("git.exe")?;
            let bash = git.parent()?.parent()?.join(r"bin\bash.exe");
            bash.is_file().then_some(bash)
        })
}

/// The shell to start: the one picked, else the default (PowerShell on
/// Windows, the user's own login shell elsewhere).
fn shell(wanted: Option<&str>) -> (CommandBuilder, String) {
    let all = shells();
    let (choice, program, args) = wanted
        .and_then(|w| all.iter().find(|(c, _, _)| c.id == w))
        .or_else(|| all.first())
        .cloned()
        .expect("there is always a default shell");
    let mut cmd = CommandBuilder::new(program);
    for arg in args {
        cmd.arg(arg);
    }
    (cmd, choice.id)
}

/// The shells the terminal can start, the default first.
#[tauri::command]
pub fn console_shells() -> Vec<ShellChoice> {
    shells().into_iter().map(|(c, _, _)| c).collect()
}

/// Splits off the text that is complete UTF-8, keeping a character cut in
/// half at the end for the next read. Bytes that can never be UTF-8 become �.
fn take_text(pending: &mut Vec<u8>) -> String {
    let mut text = String::new();
    loop {
        match std::str::from_utf8(pending) {
            Ok(s) => {
                text.push_str(s);
                pending.clear();
                return text;
            }
            Err(e) => {
                let good = e.valid_up_to();
                text.push_str(std::str::from_utf8(&pending[..good]).unwrap_or_default());
                match e.error_len() {
                    // A character that continues in the next read.
                    None => {
                        pending.drain(..good);
                        return text;
                    }
                    Some(bad) => {
                        text.push('\u{fffd}');
                        pending.drain(..good + bad);
                    }
                }
            }
        }
    }
}

/// Starts a shell in `folder` (the home folder when empty) and returns its
/// number. `label` is what the window calls the folder; `shell` is one of
/// [`console_shells`], or the default when empty.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn console_open(
    app: AppHandle,
    consoles: State<'_, Consoles>,
    folder: Option<String>,
    label: Option<String>,
    shell: Option<String>,
    cols: u16,
    rows: u16,
) -> CmdResult<Opened> {
    let settings = Settings::load().map_err(err)?;
    settings.require_module("terminal").map_err(err)?;
    let dir = folder
        .clone()
        .map(PathBuf::from)
        .filter(|d| d.is_dir())
        .or_else(|| {
            std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from)
        })
        .unwrap_or_else(|| PathBuf::from("."));

    let pair = native_pty_system()
        .openpty(size(cols, rows))
        .map_err(|e| format!("could not open a terminal: {e}"))?;
    let (mut cmd, shell_id) = self::shell(shell.as_deref());
    cmd.cwd(&dir);
    let home = paths::home();
    // Stackyard Shell's commands, even before they are added to the computer's own PATH.
    if settings.module_enabled("shell") && !shims::installed(&home) {
        if let Some(cli) = crate::cli_program() {
            let _ = shims::install(&home, &cli);
        }
    }
    // An AI tool an update left without its program (Claude Code when its
    // updater was cut off) gets it back before anyone types its name.
    for name in stackyard_core::ai::repair(&home, &settings) {
        crate::applog::info(&format!(
            "put back {name}'s program after an unfinished update"
        ));
    }
    // Then the commands of global npm packages (`npm install -g` goes to a
    // folder of Stackyard's own, which Node.js updates leave alone).
    let mut path = shell::terminal_path(&home, &settings, Platform::current().os);
    let rest = std::env::var_os("PATH").unwrap_or_default();
    let others: Vec<PathBuf> = std::env::split_paths(&rest)
        .filter(|p| !path.contains(p))
        .collect();
    path.extend(others);
    if let Ok(joined) = std::env::join_paths(path) {
        cmd.env("PATH", joined);
    }
    if let Some((name, value)) = shell::npm_prefix_env(&home) {
        cmd.env(name, value);
    }
    // Claude Code updates itself while it runs, by replacing its program.
    // Restarting or closing this terminal, quitting Stackyard or shutting
    // the computer down in the middle leaves it without one, so in here it
    // doesn't; `claude update` still updates it, in plain sight.
    for tool in stackyard_core::ai::TOOLS {
        for (name, value) in tool.env {
            if std::env::var_os(name).is_none() {
                cmd.env(name, value);
            }
        }
    }
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    cmd.env("TERM_PROGRAM", "Stackyard");
    let child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| format!("could not start the shell: {e}"))?;
    drop(pair.slave);
    let mut reader = pair
        .master
        .try_clone_reader()
        .map_err(|e| format!("could not read the terminal: {e}"))?;
    let writer = pair
        .master
        .take_writer()
        .map_err(|e| format!("could not write to the terminal: {e}"))?;

    let id = consoles.next.fetch_add(1, Ordering::Relaxed) + 1;
    let history = Arc::new(Mutex::new(History::default()));
    consoles.open.lock().unwrap().insert(
        id,
        Session {
            master: pair.master,
            writer,
            child,
            info: ConsoleInfo {
                id,
                folder,
                label,
                shell: shell_id,
            },
            history: history.clone(),
        },
    );

    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        let mut pending = Vec::new();
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    pending.extend_from_slice(&buf[..n]);
                    let data = take_text(&mut pending);
                    if !data.is_empty() {
                        // Sent while the history is held, so a window reading
                        // it sees each output either there or as an event after.
                        let mut kept = history.lock().unwrap();
                        let seq = kept.push(&data);
                        let _ = app.emit("console-output", Output { id, data, seq });
                    }
                }
            }
        }
        // A shell that ended by itself leaves the list; one closed from the
        // window has left it already.
        if let Some(consoles) = app.try_state::<Consoles>() {
            consoles.open.lock().unwrap().remove(&id);
        }
        let _ = app.emit("console-exit", Exit { id });
    });
    Ok(Opened {
        id,
        broken: stackyard_core::ai::broken(&home, &settings),
    })
}

/// The terminals still open, oldest first, for a window opened again.
#[tauri::command]
pub fn console_list(consoles: State<'_, Consoles>) -> Vec<ConsoleInfo> {
    let open = consoles.open.lock().unwrap();
    let mut list: Vec<ConsoleInfo> = open.values().map(|s| s.info.clone()).collect();
    list.sort_by_key(|i| i.id);
    list
}

/// What a terminal printed so far, its last 2 MB.
#[tauri::command]
pub fn console_history(consoles: State<'_, Consoles>, id: u32) -> Printed {
    let history = consoles
        .open
        .lock()
        .unwrap()
        .get(&id)
        .map(|s| s.history.clone());
    match history {
        Some(h) => {
            let h = h.lock().unwrap();
            Printed {
                text: h.text.clone(),
                seq: h.seq,
            }
        }
        None => Printed {
            text: String::new(),
            seq: 0,
        },
    }
}

/// The program the shell is running, when it runs one (`claude`, `npm`),
/// so the window can ask before ending it.
#[tauri::command]
pub fn console_busy(consoles: State<'_, Consoles>, id: u32) -> Option<String> {
    let pid = consoles.open.lock().unwrap().get(&id)?.child.process_id()?;
    running_under(pid)
}

/// The name of a program `pid` started, skipping the console's own helpers.
#[cfg(windows)]
fn running_under(pid: u32) -> Option<String> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    let helpers = ["conhost.exe", "openconsole.exe"];
    // SAFETY: a snapshot handle that is checked, read with a correctly sized
    // entry and closed.
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == INVALID_HANDLE_VALUE {
            return None;
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut found = None;
        let mut more = Process32FirstW(snap, &mut entry) != 0;
        while more {
            if entry.th32ParentProcessID == pid {
                let len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(0);
                let name = String::from_utf16_lossy(&entry.szExeFile[..len]);
                if !helpers.contains(&name.to_ascii_lowercase().as_str()) {
                    found = Some(name.trim_end_matches(".exe").to_string());
                    break;
                }
            }
            more = Process32NextW(snap, &mut entry) != 0;
        }
        CloseHandle(snap);
        found
    }
}

#[cfg(not(windows))]
fn running_under(pid: u32) -> Option<String> {
    let parent = pid.to_string();
    std::fs::read_dir("/proc").ok()?.flatten().find_map(|e| {
        let stat = std::fs::read_to_string(e.path().join("stat")).ok()?;
        // pid (name) state ppid ...
        let (head, rest) = stat.rsplit_once(") ")?;
        let name = head.split_once(" (")?.1;
        (rest.split(' ').nth(1)? == parent).then(|| name.to_string())
    })
}

/// Opens a web address clicked in a terminal in the browser. Only http and
/// https addresses: whatever a program prints must not start programs.
#[tauri::command]
pub fn open_link(url: String) -> CmdResult<()> {
    if !is_web_link(&url) {
        return Err(format!("not a web address: {url}"));
    }
    #[cfg(target_os = "macos")]
    let mut cmd = std::process::Command::new("open");
    #[cfg(target_os = "windows")]
    let mut cmd = {
        let mut c = std::process::Command::new("rundll32");
        c.arg("url.dll,FileProtocolHandler");
        c
    };
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut cmd = std::process::Command::new("xdg-open");
    cmd.arg(&url)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("could not open {url}: {e}"))
}

fn is_web_link(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    let rest = lower
        .strip_prefix("https://")
        .or_else(|| lower.strip_prefix("http://"));
    rest.is_some_and(|r| !r.is_empty() && !r.starts_with('/'))
        && url.len() <= 4096
        && !url
            .chars()
            .any(|c| c.is_control() || c.is_whitespace() || matches!(c, '"' | '<' | '>' | '`'))
}

/// Shows terminal `id` in a window of its own; when that window closes, the
/// terminal goes back to the panel (`console-docked`).
#[tauri::command]
pub fn console_window(app: AppHandle, id: u32, title: String) -> CmdResult<()> {
    let label = format!("terminal-{id}");
    if let Some(window) = app.get_webview_window(&label) {
        let _ = window.unminimize();
        let _ = window.set_focus();
        return Ok(());
    }
    let url = tauri::WebviewUrl::App(format!("index.html?terminal={id}").into());
    let window = tauri::WebviewWindowBuilder::new(&app, &label, url)
        .title(title)
        .inner_size(960.0, 600.0)
        .min_inner_size(420.0, 240.0)
        .build()
        .map_err(|e| format!("could not open the window: {e}"))?;
    let handle = app.clone();
    window.on_window_event(move |event| {
        if let tauri::WindowEvent::Destroyed = event {
            let _ = handle.emit("console-docked", Exit { id });
        }
    });
    Ok(())
}

/// Sends typed keys, or pasted text, to a terminal.
#[tauri::command]
pub fn console_write(consoles: State<'_, Consoles>, id: u32, data: String) -> CmdResult<()> {
    let mut open = consoles.open.lock().unwrap();
    let session = open.get_mut(&id).ok_or("that terminal has closed")?;
    session
        .writer
        .write_all(data.as_bytes())
        .and_then(|_| session.writer.flush())
        .map_err(|e| format!("could not write to the terminal: {e}"))
}

/// Tells a terminal how many columns and rows it now has.
#[tauri::command]
pub fn console_resize(
    consoles: State<'_, Consoles>,
    id: u32,
    cols: u16,
    rows: u16,
) -> CmdResult<()> {
    let open = consoles.open.lock().unwrap();
    if let Some(session) = open.get(&id) {
        session
            .master
            .resize(size(cols, rows))
            .map_err(|e| format!("could not resize the terminal: {e}"))?;
    }
    Ok(())
}

/// Ends a terminal's shell and everything it runs.
#[tauri::command]
pub fn console_close(consoles: State<'_, Consoles>, id: u32) {
    let session = consoles.open.lock().unwrap().remove(&id);
    if let Some(mut session) = session {
        let _ = session.child.kill();
    }
}

#[cfg(test)]
mod tests {
    use super::{is_web_link, take_text, History};

    #[test]
    fn opens_only_web_addresses() {
        assert!(is_web_link("https://claude.ai/code?x=1&y=2"));
        assert!(is_web_link("http://localhost:5173/"));
        assert!(!is_web_link("file:///C:/Windows/System32/calc.exe"));
        assert!(!is_web_link("javascript:alert(1)"));
        assert!(!is_web_link("https://"));
        assert!(!is_web_link("https:///etc/passwd"));
        assert!(!is_web_link("https://a.b/\" & calc"));
    }

    #[test]
    fn history_keeps_the_end_cut_at_a_line() {
        let mut history = History::default();
        let line = format!("{}\r\n", "é".repeat(500));
        // Three times what it keeps.
        for _ in 0..(3 * History::LIMIT / line.len()) {
            history.push(&line);
        }
        history.push("last");
        assert!(history.text.len() <= History::LIMIT);
        assert!(history.text.starts_with('é'), "starts at a line");
        assert!(history.text.ends_with("last"));
    }

    #[test]
    fn keeps_a_character_cut_in_half_for_the_next_read() {
        let euro = "€".as_bytes();
        let mut pending = b"a".to_vec();
        pending.extend_from_slice(&euro[..2]);
        assert_eq!(take_text(&mut pending), "a");
        assert_eq!(pending, &euro[..2]);
        pending.extend_from_slice(&euro[2..]);
        assert_eq!(take_text(&mut pending), "€");
        assert!(pending.is_empty());
    }

    #[test]
    fn replaces_bytes_that_are_never_text() {
        let mut pending = vec![b'a', 0xff, b'b'];
        assert_eq!(take_text(&mut pending), "a\u{fffd}b");
        assert!(pending.is_empty());
    }
}
