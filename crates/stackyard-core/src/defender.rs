//! Windows Defender exclusions for Stackyard's folders and programs.
//!
//! Defender's real-time protection scans every file a program opens. A
//! WordPress page opens thousands of PHP files, so on a slow disk the first
//! load of a site waits minutes on the scans. Excluding the sites' folders,
//! Stackyard's own data folder and the servers that read them (PHP, the
//! databases, the web server) takes that wait away. Adding exclusions needs
//! administrator rights, so it runs once behind Windows' own prompt, after
//! the person asks for it on the Settings page.
//!
//! Reading the exclusion list also needs administrator rights on current
//! Windows, so what was added is also recorded in `config/defender.toml`.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::platform::Os;
use crate::settings::{read_toml, write_toml};
use crate::system::{elevated, powershell_command, Elevate};
use crate::{Error, Result};

/// The servers whose file reads are left unscanned.
pub const PROCESSES: [&str; 4] = ["php-cgi.exe", "mariadbd.exe", "mysqld.exe", "caddy.exe"];

/// What Stackyard asks Defender to leave alone.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Exclusions {
    pub paths: Vec<String>,
    pub processes: Vec<String>,
}

/// Whether the exclusions are in place, and which are missing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Status {
    /// Only Windows has Defender.
    pub supported: bool,
    pub wanted: Exclusions,
    pub missing: Exclusions,
    /// `true` when Defender's list could be read; otherwise `missing` is
    /// worked out from what Stackyard added before.
    pub read_from_defender: bool,
}

fn record_file(home: &Path) -> PathBuf {
    home.join("config").join("defender.toml")
}

/// The folders to exclude: Stackyard's data folder and the folders that
/// hold the sites (the folder a site sits in, e.g. `D:\Stackyard`, or the
/// site's own folder when it sits right at the top of a drive). A folder
/// inside another one on the list is left out, since that one covers it.
pub fn wanted(home: &Path, site_folders: &[PathBuf]) -> Exclusions {
    let clean = |p: &Path| {
        p.to_string_lossy()
            .trim_end_matches(['\\', '/'])
            .to_string()
    };
    let mut paths: Vec<String> = vec![clean(home)];
    for site in site_folders {
        let site = clean(site);
        // Worked out on the text, so `D:\Stackyard\site` reads the same on any system.
        let folder = match site.rfind(['\\', '/']) {
            Some(cut)
                if site[..cut].contains(['\\', '/'])
                    || !(site[..cut].is_empty() || site[..cut].ends_with(':')) =>
            {
                site[..cut].to_string()
            }
            _ => site.clone(),
        };
        paths.push(folder);
    }
    paths.retain(|f| !f.is_empty());
    paths.sort_by_key(|p| p.to_lowercase());
    paths.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    let covered = |p: &String, all: &[String]| {
        all.iter().any(|other| {
            other != p && {
                let (p, o) = (p.to_lowercase(), other.to_lowercase());
                p.starts_with(&format!("{o}\\")) || p.starts_with(&format!("{o}/"))
            }
        })
    };
    let all = paths.clone();
    paths.retain(|p| !covered(p, &all));
    Exclusions {
        paths,
        processes: PROCESSES.iter().map(|p| p.to_string()).collect(),
    }
}

/// What is missing from `have`. Paths match without regard to case or a
/// trailing slash, and a path is covered by an excluded folder above it.
pub fn missing(wanted: &Exclusions, have: &Exclusions) -> Exclusions {
    let norm = |p: &str| p.trim_end_matches(['\\', '/']).to_lowercase();
    let have_paths: Vec<String> = have.paths.iter().map(|p| norm(p)).collect();
    let have_procs: Vec<String> = have.processes.iter().map(|p| p.to_lowercase()).collect();
    Exclusions {
        paths: wanted
            .paths
            .iter()
            .filter(|p| {
                let p = norm(p);
                !have_paths
                    .iter()
                    .any(|h| *h == p || p.starts_with(&format!("{h}\\")))
            })
            .cloned()
            .collect(),
        processes: wanted
            .processes
            .iter()
            .filter(|p| !have_procs.contains(&p.to_lowercase()))
            .cloned()
            .collect(),
    }
}

/// Defender's exclusion lists, when this account may read them.
fn read_defender() -> Option<Exclusions> {
    let script = "$p = Get-MpPreference -ErrorAction Stop; \
        $paths = @($p.ExclusionPath | Where-Object { $_ }); \
        $procs = @($p.ExclusionProcess | Where-Object { $_ }); \
        ConvertTo-Json -Compress @{ paths = $paths; processes = $procs }";
    let output = powershell_command(script).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let found: Exclusions = serde_json::from_slice(&output.stdout).ok()?;
    // Without administrator rights Windows answers with a note in place of
    // the list ("N/A: Must be an administrator to view exclusions").
    let hidden = |list: &[String]| list.iter().any(|v| v.starts_with("N/A"));
    if hidden(&found.paths) || hidden(&found.processes) {
        return None;
    }
    Some(found)
}

pub fn status(home: &Path, os: Os, site_folders: &[PathBuf]) -> Status {
    let wanted = wanted(home, site_folders);
    if os != Os::Windows {
        return Status {
            supported: false,
            missing: Exclusions::default(),
            wanted,
            read_from_defender: false,
        };
    }
    let (have, read_from_defender) = match read_defender() {
        Some(found) => (found, true),
        None => (
            read_toml::<Exclusions>(&record_file(home))
                .ok()
                .flatten()
                .unwrap_or_default(),
            false,
        ),
    };
    Status {
        supported: true,
        missing: missing(&wanted, &have),
        wanted,
        read_from_defender,
    }
}

/// A PowerShell string literal: single quotes, with any inside doubled.
fn quoted(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn script(add: &Exclusions) -> String {
    let list = |items: &[String]| {
        items
            .iter()
            .map(|i| quoted(i))
            .collect::<Vec<_>>()
            .join(",")
    };
    let mut lines = vec!["$ErrorActionPreference = 'Stop'".to_string()];
    if !add.paths.is_empty() {
        lines.push(format!(
            "Add-MpPreference -ExclusionPath @({})",
            list(&add.paths)
        ));
    }
    if !add.processes.is_empty() {
        lines.push(format!(
            "Add-MpPreference -ExclusionProcess @({})",
            list(&add.processes)
        ));
    }
    lines.join("; ")
}

/// Adds the missing exclusions behind one administrator prompt, and records
/// them. Nothing else in Defender's settings is changed.
pub fn apply(home: &Path, os: Os, site_folders: &[PathBuf]) -> Result<Status> {
    if os != Os::Windows {
        return Err(Error::SystemSetup(
            "Windows Defender exclusions are for Windows only".into(),
        ));
    }
    let now = status(home, os, site_folders);
    let add = now.missing.clone();
    if !add.paths.is_empty() || !add.processes.is_empty() {
        let output = elevated(os, &script(&add), Elevate::Dialog)
            .output()
            .map_err(|e| {
                Error::SystemSetup(format!("could not ask for administrator rights: {e}"))
            })?;
        if !output.status.success() {
            let mut text = String::from_utf8_lossy(&output.stderr).trim().to_string();
            if text.is_empty() {
                text = format!(
                    "it ended with {} (was the prompt cancelled, or does a policy manage Defender?)",
                    output.status
                );
            }
            return Err(Error::SystemSetup(text));
        }
    }
    let mut recorded = read_toml::<Exclusions>(&record_file(home))
        .ok()
        .flatten()
        .unwrap_or_default();
    for p in &now.wanted.paths {
        if !recorded.paths.iter().any(|r| r.eq_ignore_ascii_case(p)) {
            recorded.paths.push(p.clone());
        }
    }
    for p in &now.wanted.processes {
        if !recorded.processes.iter().any(|r| r.eq_ignore_ascii_case(p)) {
            recorded.processes.push(p.clone());
        }
    }
    write_toml(&record_file(home), &recorded)?;
    Ok(status(home, os, site_folders))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn excludes_the_folders_that_hold_the_sites() {
        let home = PathBuf::from(r"C:\Users\me\AppData\Roaming\Stackyard");
        let sites = [
            PathBuf::from(r"D:\Stackyard\kidstoday"),
            PathBuf::from(r"D:\Stackyard\deksiko"),
            PathBuf::from(r"C:\Users\me\AppData\Roaming\Stackyard\www\shop"),
        ];
        let w = wanted(&home, &sites);
        assert_eq!(
            w.paths,
            vec![
                r"C:\Users\me\AppData\Roaming\Stackyard".to_string(),
                r"D:\Stackyard".to_string()
            ]
        );
        assert!(w.processes.contains(&"php-cgi.exe".to_string()));
    }

    #[test]
    fn a_site_at_the_top_of_a_drive_is_excluded_itself_not_the_drive() {
        let w = wanted(Path::new(r"C:\data"), &[PathBuf::from(r"E:\site")]);
        assert!(w.paths.contains(&r"E:\site".to_string()));
        assert!(!w.paths.iter().any(|p| p == "E:" || p == r"E:\"));
    }

    #[test]
    fn lists_what_is_missing() {
        let wanted = Exclusions {
            paths: vec![r"D:\Stackyard".into(), r"C:\Data\Stackyard".into()],
            processes: vec!["php-cgi.exe".into(), "caddy.exe".into()],
        };
        let have = Exclusions {
            paths: vec![r"d:\stackyard\".into(), r"C:\Data".into()],
            processes: vec!["PHP-CGI.EXE".into()],
        };
        let m = missing(&wanted, &have);
        assert!(m.paths.is_empty(), "{m:?}");
        assert_eq!(m.processes, vec!["caddy.exe".to_string()]);
    }

    #[test]
    fn quotes_paths_for_powershell() {
        let s = script(&Exclusions {
            paths: vec![r"D:\Kris's sites".into()],
            processes: vec!["php-cgi.exe".into()],
        });
        assert!(s.contains(r"-ExclusionPath @('D:\Kris''s sites')"), "{s}");
        assert!(s.contains("-ExclusionProcess @('php-cgi.exe')"), "{s}");
    }
}
