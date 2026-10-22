//! WP-CLI, WordPress's own command-line tool, which Stackyard uses for
//! plugins, themes, users and WooCommerce. It is one PHP file, downloaded
//! the first time it is needed and checked against the SHA-512 its makers
//! publish next to it, and it runs with the site's own PHP version.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::download::Http;
use crate::install::Step;
use crate::shell::{Context, Tool};
use crate::{Error, Result};

const PHAR: &str = "https://raw.githubusercontent.com/wp-cli/builds/gh-pages/phar/wp-cli.phar";

pub fn dir(home: &Path) -> PathBuf {
    home.join("tools").join("wp-cli")
}

pub fn phar(home: &Path) -> PathBuf {
    dir(home).join("wp-cli.phar")
}

pub fn installed(home: &Path) -> bool {
    phar(home).is_file()
}

/// Downloads WP-CLI, or a newer one over it. `STACKYARD_WP_CLI_MIRROR`
/// points at another copy (with its `.sha512` next to it).
pub fn install(home: &Path, progress: &mut dyn FnMut(Step)) -> Result<PathBuf> {
    let url = std::env::var("STACKYARD_WP_CLI_MIRROR").unwrap_or_else(|_| PHAR.into());
    let http = Http::new();
    progress(Step::Resolving);
    let sums_url = format!("{url}.sha512");
    let checksum = http
        .get_text(&sums_url)?
        .split_whitespace()
        .next()
        .filter(|c| c.len() == 128 && c.bytes().all(|b| b.is_ascii_hexdigit()))
        .map(str::to_ascii_lowercase)
        .ok_or_else(|| Error::Download {
            url: sums_url.clone(),
            message: "no SHA-512 checksum in it".into(),
        })?;
    let dest = phar(home);
    http.download(&url, &dest, &checksum, &mut |done, total| {
        progress(Step::Downloading { done, total })
    })?;
    Ok(dest)
}

pub fn uninstall(home: &Path) -> Result<()> {
    let dir = dir(home);
    match std::fs::remove_dir_all(&dir) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(Error::Io {
            path: dir,
            source: e,
        }),
        _ => Ok(()),
    }
}

/// How WP-CLI loads the site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Load {
    /// WordPress with every plugin and the theme, for commands that need
    /// them (WooCommerce, scheduled actions).
    Everything,
    /// WordPress without plugins and themes: a broken plugin can't stop
    /// the command, and it starts faster.
    Core,
}

/// What a command printed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    pub stdout: String,
    pub stderr: String,
}

/// Runs `wp <args>` for the WordPress in `root` (the folder with
/// `wp-load.php`), downloading WP-CLI first if needed. Fails with WP-CLI's
/// own error message.
pub fn run(home: &Path, root: &Path, load: Load, args: &[String]) -> Result<Output> {
    let output = run_raw(home, root, load, args, false)?;
    if output.0 {
        Ok(output.1)
    } else {
        Err(Error::WordPress(error_text(&output.1)))
    }
}

/// Runs WP-CLI and keeps going whatever it says; `.0` is whether it succeeded.
pub fn run_raw(
    home: &Path,
    root: &Path,
    load: Load,
    args: &[String],
    interactive: bool,
) -> Result<(bool, Output)> {
    if !installed(home) {
        install(home, &mut |_| {})?;
    }
    let mut command = command(home, root, load, args)?;
    if interactive {
        let status = command.status().map_err(|source| Error::Spawn {
            program: "php".into(),
            source,
        })?;
        return Ok((
            status.success(),
            Output {
                stdout: String::new(),
                stderr: String::new(),
            },
        ));
    }
    let output = command
        .stdin(Stdio::null())
        .output()
        .map_err(|source| Error::Spawn {
            program: "php".into(),
            source,
        })?;
    Ok((
        output.status.success(),
        Output {
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        },
    ))
}

/// The command that runs WP-CLI with the site's PHP.
pub fn command(home: &Path, root: &Path, load: Load, args: &[String]) -> Result<Command> {
    let context = Context::load(home)?;
    let mut full: Vec<OsString> = vec![
        "-d".into(),
        "memory_limit=1024M".into(),
        phar(home).into_os_string(),
    ];
    let mut path = OsString::from("--path=");
    path.push(root.as_os_str());
    full.push(path);
    full.push("--no-color".into());
    // WP-CLI refuses to run as root unless told; Stackyard run as root
    // (some Linux setups, containers) would otherwise get nothing done.
    #[cfg(unix)]
    if unsafe { libc::geteuid() } == 0 {
        full.push("--allow-root".into());
    }
    if load == Load::Core {
        full.push("--skip-plugins".into());
        full.push("--skip-themes".into());
    }
    full.extend(args.iter().map(OsString::from));
    let launch = context.launch(Tool::Php, root, full)?;
    let mut command = Command::new(&launch.program);
    let cache = dir(home).join("cache");
    command
        .args(&launch.args)
        .current_dir(root)
        .env("PATH", launch.path_var())
        .envs(launch.env.iter().map(|(k, v)| (k, v)))
        .env("WP_CLI_CACHE_DIR", &cache)
        .env("WP_CLI_PACKAGES_DIR", dir(home).join("packages"))
        .env("WP_CLI_DISABLE_AUTO_CHECK_UPDATE", "1");
    crate::platform::hide_console(&mut command);
    Ok(command)
}

/// WP-CLI's `Error: …` line, in plain words where we know them.
fn error_text(output: &Output) -> String {
    let all = format!("{}\n{}", output.stderr, output.stdout);
    if all.contains("Error establishing a database connection")
        || all.contains("establish a database connection")
    {
        return "WordPress can't reach its database. Start MySQL or MariaDB on the Services screen, and check DB_HOST in wp-config.php.".into();
    }
    let line = all
        .lines()
        .map(str::trim)
        .find(|l| l.starts_with("Error:"))
        .or_else(|| all.lines().map(str::trim).rfind(|l| !l.is_empty()))
        .unwrap_or("WP-CLI failed");
    let line = line.strip_prefix("Error:").unwrap_or(line).trim();
    // PHP's fatal errors carry HTML tags when display_errors is on.
    let mut plain = String::with_capacity(line.len());
    let mut in_tag = false;
    for c in line.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            c if !in_tag => plain.push(c),
            _ => {}
        }
    }
    plain
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explains_errors() {
        let out = |stderr: &str| Output {
            stdout: String::new(),
            stderr: stderr.into(),
        };
        assert_eq!(
            error_text(&out(
                "Warning: x\nError: The 'nope' plugin could not be found.\n"
            )),
            "The 'nope' plugin could not be found."
        );
        assert!(
            error_text(&out("Error: Error establishing a database connection."))
                .contains("Start MySQL")
        );
        assert_eq!(
            error_text(&out("PHP Fatal error: <b>boom</b> in x.php")),
            "PHP Fatal error: boom in x.php"
        );
    }
}
