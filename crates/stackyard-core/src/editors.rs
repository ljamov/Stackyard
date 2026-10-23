//! Code editors a site's folder can be opened in: the popular ones, found
//! where their installers put them or on PATH. The one picked in Settings
//! is kept as `editor` in settings.toml; with none picked VS Code is used,
//! or the first one installed when it isn't.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;

use crate::platform::{Os, Platform};
use crate::{Error, Result};

/// The editor used when none is picked.
pub const DEFAULT: &str = "vscode";

struct Def {
    id: &'static str,
    name: &'static str,
    /// Commands their installers put on PATH.
    commands: &'static [&'static str],
    /// The app's name in /Applications on macOS.
    mac_app: &'static str,
    /// Places on Windows, under %LOCALAPPDATA% (`L`), %ProgramFiles% (`P`)
    /// or %ProgramFiles(x86)% (`X`); `*` matches one folder name, for
    /// JetBrains' versioned folders.
    windows: &'static [&'static str],
    /// Options before the folder, e.g. Notepad++'s folder-as-workspace.
    args: &'static [&'static str],
}

const EDITORS: &[Def] = &[
    Def {
        id: "vscode",
        name: "VS Code",
        commands: &["code"],
        mac_app: "Visual Studio Code",
        windows: &[
            r"L\Programs\Microsoft VS Code\Code.exe",
            r"P\Microsoft VS Code\Code.exe",
        ],
        args: &[],
    },
    Def {
        id: "cursor",
        name: "Cursor",
        commands: &["cursor"],
        mac_app: "Cursor",
        windows: &[r"L\Programs\cursor\Cursor.exe", r"P\Cursor\Cursor.exe"],
        args: &[],
    },
    Def {
        id: "windsurf",
        name: "Windsurf",
        commands: &["windsurf"],
        mac_app: "Windsurf",
        windows: &[
            r"L\Programs\Windsurf\Windsurf.exe",
            r"P\Windsurf\Windsurf.exe",
        ],
        args: &[],
    },
    Def {
        id: "phpstorm",
        name: "PhpStorm",
        commands: &["phpstorm", "phpstorm.sh"],
        mac_app: "PhpStorm",
        windows: &[
            r"L\JetBrains\Toolbox\scripts\phpstorm.cmd",
            r"L\Programs\PhpStorm\bin\phpstorm64.exe",
            r"P\JetBrains\PhpStorm*\bin\phpstorm64.exe",
        ],
        args: &[],
    },
    Def {
        id: "webstorm",
        name: "WebStorm",
        commands: &["webstorm", "webstorm.sh"],
        mac_app: "WebStorm",
        windows: &[
            r"L\JetBrains\Toolbox\scripts\webstorm.cmd",
            r"L\Programs\WebStorm\bin\webstorm64.exe",
            r"P\JetBrains\WebStorm*\bin\webstorm64.exe",
        ],
        args: &[],
    },
    Def {
        id: "sublime",
        name: "Sublime Text",
        commands: &["subl"],
        mac_app: "Sublime Text",
        windows: &[
            r"P\Sublime Text\sublime_text.exe",
            r"P\Sublime Text 3\sublime_text.exe",
        ],
        args: &[],
    },
    Def {
        id: "zed",
        name: "Zed",
        commands: &["zed", "zeditor"],
        mac_app: "Zed",
        windows: &[r"L\Programs\Zed\Zed.exe"],
        args: &[],
    },
    Def {
        id: "notepadpp",
        name: "Notepad++",
        commands: &[],
        mac_app: "",
        windows: &[r"P\Notepad++\notepad++.exe", r"X\Notepad++\notepad++.exe"],
        args: &["-openFoldersAsWorkspace"],
    },
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Editor {
    pub id: String,
    pub name: String,
    pub installed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Editors {
    /// The editor Open in editor uses.
    pub current: String,
    /// What was picked in Settings, if anything.
    pub chosen: Option<String>,
    pub list: Vec<Editor>,
}

/// Where an editor is, and how it is started.
enum Found {
    Program(PathBuf),
    MacApp(&'static str),
}

/// The editors this system can have, which of them are installed, and the
/// one in use given what was picked (`chosen`).
pub fn list(chosen: Option<&str>) -> Editors {
    let os = Platform::current().os;
    let list: Vec<Editor> = EDITORS
        .iter()
        .filter(|d| os == Os::Windows || !d.commands.is_empty())
        .map(|d| Editor {
            id: d.id.into(),
            name: d.name.into(),
            installed: find(d, os).is_some(),
        })
        .collect();
    let chosen = chosen.filter(|c| list.iter().any(|e| e.id == *c));
    let current = current_of(chosen, &list);
    Editors {
        current,
        chosen: chosen.map(str::to_string),
        list,
    }
}

/// The picked editor; else VS Code, or the first installed one when VS
/// Code isn't.
fn current_of(chosen: Option<&str>, list: &[Editor]) -> String {
    if let Some(c) = chosen {
        return c.to_string();
    }
    let installed = |id: &str| list.iter().any(|e| e.id == id && e.installed);
    if installed(DEFAULT) {
        return DEFAULT.into();
    }
    list.iter()
        .find(|e| e.installed)
        .map_or(DEFAULT.into(), |e| e.id.clone())
}

/// Checks that `id` is an editor Stackyard knows.
pub fn check(id: &str) -> Result<()> {
    if EDITORS.iter().any(|d| d.id == id) {
        Ok(())
    } else {
        Err(Error::Shell(format!(
            "{id} isn't an editor Stackyard knows"
        )))
    }
}

/// Opens `folder` in the editor `id`.
pub fn open(id: &str, folder: &Path) -> Result<()> {
    let def = EDITORS
        .iter()
        .find(|d| d.id == id)
        .ok_or_else(|| Error::Shell(format!("{id} isn't an editor Stackyard knows")))?;
    let os = Platform::current().os;
    let found = find(def, os).ok_or_else(|| {
        Error::Shell(format!(
            "{} isn't installed; install it or pick another editor in Settings",
            def.name
        ))
    })?;
    let mut command = match found {
        Found::MacApp(app) => {
            let mut c = Command::new("open");
            c.args(["-a", app]);
            if !def.args.is_empty() {
                c.arg("--args").args(def.args);
            }
            c
        }
        Found::Program(program) => {
            let mut c = Command::new(program);
            c.args(def.args);
            c
        }
    };
    command.arg(folder).current_dir(folder);
    crate::platform::hide_console(&mut command);
    command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| Error::Shell(format!("could not start {}: {e}", def.name)))
}

fn find(def: &Def, os: Os) -> Option<Found> {
    match os {
        Os::Windows => windows_place(def)
            .map(Found::Program)
            .or_else(|| on_path(def, os)),
        Os::Macos => {
            let app = format!("{}.app", def.mac_app);
            let home = std::env::var_os("HOME").map(PathBuf::from);
            let there = !def.mac_app.is_empty()
                && [
                    Some(PathBuf::from("/Applications")),
                    home.map(|h| h.join("Applications")),
                ]
                .into_iter()
                .flatten()
                .any(|dir| dir.join(&app).exists());
            there
                .then_some(Found::MacApp(def.mac_app))
                .or_else(|| on_path(def, os))
        }
        Os::Linux => on_path(def, os).or_else(|| {
            // Snap and Flatpak installs aren't always on PATH yet.
            def.commands
                .iter()
                .map(|c| PathBuf::from("/snap/bin").join(c))
                .find(|p| p.is_file())
                .map(Found::Program)
        }),
    }
}

fn on_path(def: &Def, os: Os) -> Option<Found> {
    let path = std::env::var_os("PATH")?;
    let suffixes: &[&str] = if os == Os::Windows {
        &[".exe", ".cmd"]
    } else {
        &[""]
    };
    std::env::split_paths(&path)
        .flat_map(|dir| {
            def.commands
                .iter()
                .flat_map(|c| suffixes.iter().map(move |s| format!("{c}{s}")))
                .map(move |name| dir.join(name))
        })
        .find(|p| p.is_file())
        .map(Found::Program)
}

fn windows_place(def: &Def) -> Option<PathBuf> {
    def.windows.iter().find_map(|place| {
        let (root, rest) = place.split_once('\\')?;
        let var = match root {
            "L" => "LOCALAPPDATA",
            "P" => "ProgramFiles",
            _ => "ProgramFiles(x86)",
        };
        let base = PathBuf::from(std::env::var_os(var)?);
        expand(&base, rest).into_iter().find(|p| p.is_file())
    })
}

/// `base` joined with `rest`, where a part ending in `*` matches any folder
/// starting with what comes before it (newest name first).
fn expand(base: &Path, rest: &str) -> Vec<PathBuf> {
    let mut paths = vec![base.to_path_buf()];
    for part in rest.split('\\') {
        paths = match part.strip_suffix('*') {
            None => paths.into_iter().map(|p| p.join(part)).collect(),
            Some(start) => paths
                .into_iter()
                .flat_map(|p| {
                    let mut names: Vec<PathBuf> = std::fs::read_dir(&p)
                        .into_iter()
                        .flatten()
                        .flatten()
                        .filter(|e| e.file_name().to_string_lossy().starts_with(start))
                        .map(|e| e.path())
                        .collect();
                    names.sort();
                    names.reverse();
                    names
                })
                .collect(),
        };
    }
    paths
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uses_vs_code_unless_another_is_picked_or_it_is_missing() {
        let e = |id: &str, installed| Editor {
            id: id.into(),
            name: id.into(),
            installed,
        };
        let both = [e("vscode", true), e("zed", true)];
        assert_eq!(current_of(None, &both), "vscode");
        assert_eq!(current_of(Some("zed"), &both), "zed");
        let no_code = [e("vscode", false), e("cursor", false), e("zed", true)];
        assert_eq!(current_of(None, &no_code), "zed");
        let none = [e("vscode", false), e("zed", false)];
        assert_eq!(current_of(None, &none), "vscode");
    }

    #[test]
    fn lists_known_editors_only() {
        let editors = list(Some("nope"));
        assert_eq!(editors.chosen, None);
        assert!(editors.list.iter().any(|e| e.id == "vscode"));
        assert!(check("cursor").is_ok());
        assert!(check("vim").is_err());
    }

    #[test]
    fn expands_versioned_folders() {
        let base = crate::testutil::tempdir();
        for v in ["PhpStorm 2025.2", "PhpStorm 2026.1", "Other"] {
            std::fs::create_dir_all(base.join(v).join("bin")).unwrap();
        }
        let found = expand(&base, r"PhpStorm*\bin");
        assert_eq!(
            found,
            vec![
                base.join("PhpStorm 2026.1").join("bin"),
                base.join("PhpStorm 2025.2").join("bin")
            ]
        );
    }
}
