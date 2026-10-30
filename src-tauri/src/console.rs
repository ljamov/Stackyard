//! The Terminal tab in the window's panel: a shell in a pseudo-terminal,
//! started in a site's folder with Stackyard's commands (`php`, `node`,
//! `composer`…) first on its PATH, so they run the versions that folder asks
//! for. What the shell prints arrives as `console-output` events; when it
//! ends, `console-exit`.

use std::collections::HashMap;
use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;

use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use serde::Serialize;
use stackyard_core::paths;
use stackyard_core::platform::Platform;
use stackyard_core::settings::Settings;
use stackyard_core::shell::{self, shims};
use tauri::{AppHandle, Emitter, State};

use crate::{err, CmdResult};

struct Session {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
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

/// The shell to start: PowerShell on Windows, else the user's own login shell.
fn shell() -> CommandBuilder {
    if cfg!(windows) {
        let mut cmd = CommandBuilder::new("powershell.exe");
        cmd.arg("-NoLogo");
        cmd
    } else {
        let shell = std::env::var_os("SHELL")
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| OsString::from("/bin/sh"));
        let mut cmd = CommandBuilder::new(shell);
        cmd.arg("-l");
        cmd
    }
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

/// Starts a shell in `folder` (the home folder when empty) and returns its number.
#[tauri::command]
pub fn console_open(
    app: AppHandle,
    consoles: State<'_, Consoles>,
    folder: Option<String>,
    cols: u16,
    rows: u16,
) -> CmdResult<u32> {
    let settings = Settings::load().map_err(err)?;
    settings.require_module("terminal").map_err(err)?;
    let dir = folder
        .map(PathBuf::from)
        .filter(|d| d.is_dir())
        .or_else(|| {
            std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from)
        })
        .unwrap_or_else(|| PathBuf::from("."));

    let pair = native_pty_system()
        .openpty(size(cols, rows))
        .map_err(|e| format!("could not open a terminal: {e}"))?;
    let mut cmd = shell();
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
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
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
    consoles.open.lock().unwrap().insert(
        id,
        Session {
            master: pair.master,
            writer,
            child,
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
                        let _ = app.emit("console-output", Output { id, data });
                    }
                }
            }
        }
        let _ = app.emit("console-exit", Exit { id });
    });
    Ok(id)
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
    use super::take_text;

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
