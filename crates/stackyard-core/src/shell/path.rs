//! Puts the shims folder on the user's `PATH`, so new terminals and editors
//! such as VS Code find `php`, `node` and the rest.
//!
//! macOS and Linux: a marked block in the shell start-up files (`.zshrc`,
//! `.bashrc`, `.bash_profile`, `.profile`) and a file in fish's `conf.d`.
//! Windows: the user's `Path` variable. No administrator rights are needed.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::platform::{Os, Platform};
use crate::{Error, Result};

const BEGIN: &str = "# >>> stackyard >>>";
const END: &str = "# <<< stackyard <<<";
const ABOUT: &str =
    "Stackyard Shell: php, node, npm, composer and mysql use each project's versions.";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PathStatus {
    /// New terminals will find the shims.
    pub on_path: bool,
    /// What carries the setting: start-up files, or the Windows `Path`.
    pub places: Vec<String>,
}

pub fn status(bin: &Path) -> PathStatus {
    if Platform::current().os == Os::Windows {
        let on_path =
            windows_user_path().is_some_and(|p| p.split(';').any(|part| same_dir(part, bin)));
        return PathStatus {
            on_path,
            places: on_path
                .then(|| "your user Path".to_string())
                .into_iter()
                .collect(),
        };
    }
    let Some(home) = dirs::home_dir() else {
        return PathStatus {
            on_path: false,
            places: Vec::new(),
        };
    };
    let places: Vec<String> = candidates(&home)
        .into_iter()
        .chain([fish_file(&home)])
        .filter(|f| std::fs::read_to_string(f).is_ok_and(|t| t.contains(BEGIN)))
        .map(|f| f.display().to_string())
        .collect();
    PathStatus {
        on_path: !places.is_empty(),
        places,
    }
}

/// Adds `bin` to the PATH of new terminals; returns what was changed.
pub fn add(bin: &Path) -> Result<Vec<String>> {
    if Platform::current().os == Os::Windows {
        run_powershell(&windows_script(bin, true))?;
        return Ok(vec!["your user Path".into()]);
    }
    let home = user_home()?;
    let shell = std::env::var("SHELL").unwrap_or_default();
    let mut changed = Vec::new();
    for file in targets(&home, &shell, Platform::current().os) {
        let text = read(&file)?;
        let new = with_block(&text, &sh_block(bin));
        if new != text {
            write(&file, &new)?;
        }
        changed.push(file.display().to_string());
    }
    if shell.ends_with("/fish") || home.join(".config/fish").is_dir() {
        let file = fish_file(&home);
        write(&file, &fish_script(bin))?;
        changed.push(file.display().to_string());
    }
    Ok(changed)
}

/// Undoes [`add`]; returns what was changed.
pub fn remove(bin: &Path) -> Result<Vec<String>> {
    if Platform::current().os == Os::Windows {
        run_powershell(&windows_script(bin, false))?;
        return Ok(vec!["your user Path".into()]);
    }
    let home = user_home()?;
    let mut changed = Vec::new();
    for file in candidates(&home) {
        let text = read(&file)?;
        let new = without_block(&text);
        if new != text {
            write(&file, &new)?;
            changed.push(file.display().to_string());
        }
    }
    let fish = fish_file(&home);
    if fish.exists() {
        std::fs::remove_file(&fish).map_err(|source| Error::Io {
            path: fish.clone(),
            source,
        })?;
        changed.push(fish.display().to_string());
    }
    Ok(changed)
}

/// What [`add`] would do, for `--print`.
pub fn preview(bin: &Path) -> String {
    match Platform::current().os {
        Os::Windows => windows_script(bin, true),
        _ => sh_block(bin),
    }
}

/// Start-up files that may hold the block.
fn candidates(home: &Path) -> Vec<PathBuf> {
    [".zshrc", ".bashrc", ".bash_profile", ".profile"]
        .iter()
        .map(|f| home.join(f))
        .collect()
}

/// The start-up files that exist, plus the one the user's shell reads.
fn targets(home: &Path, shell: &str, os: Os) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = candidates(home)
        .into_iter()
        .filter(|f| f.exists())
        .collect();
    let own: &[&str] = match shell.rsplit('/').next().unwrap_or_default() {
        "zsh" => &[".zshrc"],
        // Terminal on macOS starts bash as a login shell; editors may not.
        "bash" if os == Os::Macos => &[".bash_profile", ".bashrc"],
        "bash" => &[".bashrc"],
        "fish" => &[],
        _ => &[".profile"],
    };
    for file in own.iter().map(|f| home.join(f)) {
        if !files.contains(&file) {
            files.push(file);
        }
    }
    files
}

fn fish_file(home: &Path) -> PathBuf {
    home.join(".config/fish/conf.d/stackyard.fish")
}

fn sh_block(bin: &Path) -> String {
    let dir = sh_quote(&bin.display().to_string());
    format!(
        "{BEGIN}\n# {ABOUT}\ncase \":$PATH:\" in\n  *:\"{dir}\":*) ;;\n  *) export PATH=\"{dir}:$PATH\" ;;\nesac\n{END}\n"
    )
}

fn fish_script(bin: &Path) -> String {
    let dir = bin
        .display()
        .to_string()
        .replace('\\', "\\\\")
        .replace('\'', "\\'");
    format!(
        "{BEGIN}\n# {ABOUT}\nif not contains -- '{dir}' $PATH\n    set -gx PATH '{dir}' $PATH\nend\n{END}\n"
    )
}

/// Escapes text for use inside double quotes in sh.
fn sh_quote(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if matches!(c, '\\' | '"' | '$' | '`') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// `text` with the block at its end, replacing any older one.
fn with_block(text: &str, block: &str) -> String {
    let mut out = without_block(text);
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    if !out.is_empty() && !out.ends_with("\n\n") {
        out.push('\n');
    }
    out.push_str(block);
    out
}

fn without_block(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut inside = false;
    for line in text.split_inclusive('\n') {
        match line.trim_end() {
            BEGIN => inside = true,
            END if inside => inside = false,
            _ if !inside => out.push_str(line),
            _ => {}
        }
    }
    // Drop the blank line `with_block` put before the block.
    if out.ends_with("\n\n") && text.trim_end().ends_with(END) {
        out.pop();
    }
    out
}

fn windows_script(bin: &Path, add: bool) -> String {
    let dir = bin.display().to_string().replace('\'', "''");
    let first = if add { "@($bin) + " } else { "" };
    format!(
        "$ErrorActionPreference = 'Stop'\n\
         $bin = '{dir}'\n\
         $key = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey('Environment')\n\
         $old = [string]$key.GetValue('Path', '', 'DoNotExpandEnvironmentNames')\n\
         $parts = @($old -split ';' | Where-Object {{ $_ -and ($_.TrimEnd('\\') -ne $bin.TrimEnd('\\')) }})\n\
         $key.SetValue('Path', (({first}$parts) -join ';'), 'ExpandString')\n\
         $key.Close()\n\
         # Setting any user variable tells open programs that the environment changed.\n\
         [Environment]::SetEnvironmentVariable('STACKYARD_PATH_CHANGED', '1', 'User')\n\
         [Environment]::SetEnvironmentVariable('STACKYARD_PATH_CHANGED', $null, 'User')\n"
    )
}

fn windows_user_path() -> Option<String> {
    let script = "$key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Environment')\n\
                  if ($key) { [Console]::Out.Write([string]$key.GetValue('Path', '', 'DoNotExpandEnvironmentNames')) }";
    let output = crate::system::powershell_command(script).output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

fn run_powershell(script: &str) -> Result<()> {
    let output = crate::system::powershell_command(script)
        .output()
        .map_err(|e| Error::Shell(format!("could not run PowerShell: {e}")))?;
    if output.status.success() {
        return Ok(());
    }
    Err(Error::Shell(format!(
        "changing your Path failed: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    )))
}

fn same_dir(a: &str, b: &Path) -> bool {
    let b = b.display().to_string();
    a.trim_end_matches(['\\', '/'])
        .eq_ignore_ascii_case(b.trim_end_matches(['\\', '/']))
}

fn user_home() -> Result<PathBuf> {
    dirs::home_dir().ok_or_else(|| Error::Shell("could not find your home folder".into()))
}

fn read(file: &Path) -> Result<String> {
    match std::fs::read_to_string(file) {
        Ok(text) => Ok(text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(source) => Err(Error::Io {
            path: file.to_owned(),
            source,
        }),
    }
}

/// A plain write, not a rename, so start-up files that are links into a
/// dotfiles folder stay links.
fn write(file: &Path, text: &str) -> Result<()> {
    let io = |source| Error::Io {
        path: file.to_owned(),
        source,
    };
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent).map_err(io)?;
    }
    std::fs::write(file, text).map_err(io)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adds_and_removes_one_block() {
        let bin = Path::new("/Users/me/Library/Application Support/Stackyard/bin");
        let block = sh_block(bin);
        assert!(block
            .contains("export PATH=\"/Users/me/Library/Application Support/Stackyard/bin:$PATH\""));

        let original = "export EDITOR=vim\nalias ll='ls -l'";
        let added = with_block(original, &block);
        assert!(added.starts_with("export EDITOR=vim\nalias ll='ls -l'\n\n# >>> stackyard"));
        assert_eq!(
            with_block(&added, &block),
            added,
            "adding twice changes nothing"
        );
        assert_eq!(
            without_block(&added),
            "export EDITOR=vim\nalias ll='ls -l'\n"
        );
        assert_eq!(with_block("", &block), block);
        assert_eq!(without_block(&block), "");

        // Text after the block survives.
        let middle = format!("{added}export B=1\n");
        assert_eq!(
            without_block(&middle),
            "export EDITOR=vim\nalias ll='ls -l'\n\nexport B=1\n"
        );
    }

    #[test]
    fn quotes_awkward_folder_names() {
        let block = sh_block(Path::new("/tmp/a \"$b`"));
        assert!(
            block.contains(r#"export PATH="/tmp/a \"\$b\`:$PATH""#),
            "{block}"
        );
        let fish = fish_script(Path::new("/tmp/it's"));
        assert!(fish.contains(r"'/tmp/it\'s'"), "{fish}");
    }

    #[test]
    fn writes_to_the_files_the_shell_reads() {
        let home = crate::testutil::tempdir();
        let names = |files: Vec<PathBuf>| -> Vec<String> {
            files
                .iter()
                .map(|f| f.file_name().unwrap().to_string_lossy().into_owned())
                .collect()
        };
        assert_eq!(names(targets(&home, "/bin/zsh", Os::Macos)), [".zshrc"]);
        assert_eq!(
            names(targets(&home, "/bin/bash", Os::Macos)),
            [".bash_profile", ".bashrc"]
        );
        assert_eq!(
            names(targets(&home, "/usr/bin/bash", Os::Linux)),
            [".bashrc"]
        );
        assert_eq!(names(targets(&home, "", Os::Linux)), [".profile"]);
        assert!(targets(&home, "/usr/bin/fish", Os::Linux).is_empty());

        std::fs::write(home.join(".bashrc"), "").unwrap();
        std::fs::write(home.join(".profile"), "").unwrap();
        assert_eq!(
            names(targets(&home, "/bin/zsh", Os::Linux)),
            [".bashrc", ".profile", ".zshrc"]
        );
    }

    #[test]
    fn windows_script_keeps_other_entries() {
        let script = windows_script(
            Path::new(r"C:\Users\o'neil\AppData\Roaming\Stackyard\bin"),
            true,
        );
        assert!(script.contains(r"$bin = 'C:\Users\o''neil\AppData\Roaming\Stackyard\bin'"));
        assert!(script.contains("(@($bin) + $parts) -join ';'"));
        assert!(script.contains("'ExpandString'"));
        let undo = windows_script(Path::new(r"C:\x"), false);
        assert!(undo.contains("(($parts) -join ';')"));
        assert!(same_dir(r"c:\X\", Path::new(r"C:\x")));
    }
}
