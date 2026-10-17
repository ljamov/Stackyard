//! Opens a terminal window in a folder, running one program.
//!
//! Windows: a new console window running `cmd /k`, which Windows 11 shows
//! in Windows Terminal when that is the default. macOS: a Terminal window.
//! Linux: the first terminal program found. In each the window stays open
//! at a prompt in the folder after the program ends.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::{Error, Result};

pub struct Window {
    pub title: String,
    pub folder: PathBuf,
    pub program: PathBuf,
    pub args: Vec<OsString>,
    /// The whole `PATH` for the window (Windows, Linux).
    pub path: OsString,
    /// Folders to put first on the `PATH` the window's shell already has
    /// (macOS and Linux, where the shell's start-up files may set their own).
    pub path_first: Vec<PathBuf>,
}

pub fn open(window: &Window) -> Result<()> {
    #[cfg(windows)]
    return open_windows(window);
    #[cfg(target_os = "macos")]
    return open_macos(window);
    #[cfg(all(unix, not(target_os = "macos")))]
    return open_linux(window);
}

fn spawn_error(program: &str) -> impl FnOnce(std::io::Error) -> Error + '_ {
    move |source| Error::Spawn {
        program: program.to_string(),
        source,
    }
}

#[cfg(windows)]
fn open_windows(window: &Window) -> Result<()> {
    use std::os::windows::process::CommandExt;
    const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;
    Command::new("cmd.exe")
        .raw_arg(cmd_line(window))
        .current_dir(&window.folder)
        .env("PATH", &window.path)
        .creation_flags(CREATE_NEW_CONSOLE)
        .spawn()
        .map(drop)
        .map_err(spawn_error("cmd.exe"))
}

/// Arguments for `cmd.exe`: sets the window title, then runs the program.
/// cmd drops the first and last quote of what follows `/k`, which leaves
/// each path in its own quotes.
#[cfg_attr(not(windows), allow(dead_code))]
fn cmd_line(window: &Window) -> String {
    let title: String = window
        .title
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == ' ')
        .collect();
    let mut line = format!("/d /k \"title {title}& \"{}\"", window.program.display());
    for arg in &window.args {
        line.push_str(&format!(" \"{}\"", arg.to_string_lossy()));
    }
    line.push('"');
    line
}

/// A script that runs the program in the folder; with `stay`, it then
/// starts the user's shell so the window stays open.
#[cfg_attr(windows, allow(dead_code))]
fn script(window: &Window, stay: bool) -> String {
    let mut text = String::from("#!/bin/sh\n");
    text.push_str(&format!("cd {} || exit 1\n", sh_quote(&window.folder)));
    if !window.path_first.is_empty() {
        let first: Vec<String> = window.path_first.iter().map(|p| sh_quote(p)).collect();
        text.push_str(&format!("export PATH={}:\"$PATH\"\n", first.join(":")));
    }
    text.push_str(&sh_quote(&window.program));
    for arg in &window.args {
        text.push(' ');
        text.push_str(&sh_quote(Path::new(arg)));
    }
    text.push('\n');
    if stay {
        text.push_str("exec \"${SHELL:-/bin/sh}\" -i\n");
    }
    text
}

/// `'text'` for sh (and zsh and fish), with any `'` inside kept.
#[cfg_attr(windows, allow(dead_code))]
fn sh_quote(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "'\\''"))
}

/// Writes the script to a file the terminal can run.
#[cfg(unix)]
fn script_file(window: &Window, stay: bool) -> Result<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir().join("stackyard-ai");
    let io = |path: &Path| {
        let path = path.to_owned();
        move |source| Error::Io { path, source }
    };
    std::fs::create_dir_all(&dir).map_err(io(&dir))?;
    let name: String = window
        .title
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect();
    let file = dir.join(format!("open-{}.sh", name.to_ascii_lowercase()));
    std::fs::write(&file, script(window, stay)).map_err(io(&file))?;
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).map_err(io(&file))?;
    Ok(file)
}

#[cfg(target_os = "macos")]
fn open_macos(window: &Window) -> Result<()> {
    let file = script_file(window, false)?;
    // Terminal runs this in the user's own shell, which then stays at a
    // prompt in the folder.
    let line = format!("cd {} && {}", sh_quote(&window.folder), sh_quote(&file));
    let line = line.replace('\\', "\\\\").replace('"', "\\\"");
    let status = Command::new("osascript")
        .args(["-e", "tell application \"Terminal\""])
        .args(["-e", &format!("do script \"{line}\"")])
        .args(["-e", "activate", "-e", "end tell"])
        .status()
        .map_err(spawn_error("osascript"))?;
    if status.success() {
        return Ok(());
    }
    Err(Error::Ai(
        "could not open Terminal; allow Stackyard to control Terminal in System Settings › Privacy & Security › Automation".into(),
    ))
}

/// Terminal programs to try, with what each needs before the command.
#[cfg_attr(not(all(unix, not(target_os = "macos"))), allow(dead_code))]
const LINUX_TERMINALS: &[(&str, &[&str])] = &[
    ("x-terminal-emulator", &["-e"]),
    ("gnome-terminal", &["--"]),
    ("konsole", &["-e"]),
    ("xfce4-terminal", &["-x"]),
    ("kitty", &[]),
    ("alacritty", &["-e"]),
    ("wezterm", &["start", "--"]),
    ("foot", &[]),
    ("tilix", &["-e"]),
    ("xterm", &["-e"]),
];

#[cfg(all(unix, not(target_os = "macos")))]
fn open_linux(window: &Window) -> Result<()> {
    let file = script_file(window, true)?;
    for (program, before) in LINUX_TERMINALS {
        let started = Command::new(program)
            .args(*before)
            .arg("/bin/sh")
            .arg(&file)
            .current_dir(&window.folder)
            .env("PATH", &window.path)
            .spawn();
        match started {
            Ok(_) => return Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(spawn_error(program)(e)),
        }
    }
    Err(Error::Ai(format!(
        "no terminal program found; open a terminal in {} and run {}",
        window.folder.display(),
        file.display()
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window() -> Window {
        Window {
            title: "Claude Code".into(),
            folder: PathBuf::from("/code/it's mine"),
            program: PathBuf::from("/s/runtimes/node/24/bin/node"),
            args: vec!["/s/tools/ai/gemini/bundle/gemini.js".into()],
            path: OsString::from("/s/bin:/usr/bin"),
            path_first: vec![PathBuf::from("/s/bin"), PathBuf::from("/s/node bin")],
        }
    }

    #[test]
    fn script_runs_the_program_in_the_folder() {
        let text = script(&window(), true);
        assert_eq!(
            text,
            "#!/bin/sh\n\
             cd '/code/it'\\''s mine' || exit 1\n\
             export PATH='/s/bin':'/s/node bin':\"$PATH\"\n\
             '/s/runtimes/node/24/bin/node' '/s/tools/ai/gemini/bundle/gemini.js'\n\
             exec \"${SHELL:-/bin/sh}\" -i\n"
        );
        assert!(!script(&window(), false).contains("exec"));
    }

    #[test]
    fn cmd_line_quotes_each_path() {
        let mut w = window();
        w.title = "Claude Code & more".into();
        w.program = PathBuf::from(r"C:\stackyard\runtimes\node\24\node.exe");
        w.args = vec![r"C:\stackyard\tools\ai\gemini\bundle\gemini.js".into()];
        assert_eq!(
            cmd_line(&w),
            r#"/d /k "title Claude Code  more& "C:\stackyard\runtimes\node\24\node.exe" "C:\stackyard\tools\ai\gemini\bundle\gemini.js"""#
        );
        w.args.clear();
        assert!(cmd_line(&w).ends_with(r#"node.exe"""#));
    }
}
