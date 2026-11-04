//! Live-server paths in a copied site.
//!
//! A site copied from a host often brings PHP settings that name folders
//! only that host has. Wordfence's firewall, for one, adds
//! `auto_prepend_file = '/home/…/wordfence-waf.php'` to `.user.ini`; on
//! this computer the file isn't there and PHP stops before any of the
//! site's code runs, with "Failed opening required … in Unknown on line 0".
//!
//! [`scan`] finds such settings in `.user.ini`, `php.ini` and `.htaccess`
//! and notes other leftovers that are harmless here. [`fix`] keeps each
//! file it changes as `<name>.live` and switches the lines off with a note;
//! [`undo`] puts the kept files back byte for byte. [`check`] asks the
//! site's PHP for the home page the way the web server does, so a startup
//! error that only shows in the page is seen too.
//!
//! Files changed here never go to the live server: see [`stays_here`].

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::services::ServiceManager;
use crate::supervisor::Supervisor;
use crate::web::{Backend, ServedSite};
use crate::{Error, Result};

/// Starts the note on every line Stackyard switches off.
pub const MARKER: &str = "[Stackyard] disabled for local dev";

/// The files PHP and Apache read settings from, in a site's folders.
const FILES: [&str; 3] = [".user.ini", "php.ini", ".htaccess"];

/// Settings that name a file or folder.
const DIRECTIVES: [&str; 7] = [
    "auto_prepend_file",
    "auto_append_file",
    "include_path",
    "open_basedir",
    "error_log",
    "session.save_path",
    "upload_tmp_dir",
];

/// Where hosts keep sites; a quoted path starting so in a leftover file is
/// looked up.
const HOST_FOLDERS: [&str; 7] = [
    "/home/",
    "/var/www/",
    "/srv/",
    "/usr/",
    "/opt/",
    "/data/",
    "/mnt/",
];

/// A setting naming a path this computer doesn't have.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Finding {
    /// Relative to the site's folder, with `/`.
    pub file: String,
    /// Counted from 1.
    pub line: usize,
    pub directive: String,
    pub value: String,
    /// The part of the value that isn't on this computer.
    pub missing: String,
    /// `.htaccess` lines only matter to Apache; the web server here skips them.
    pub apache_only: bool,
}

/// Something from the live server that does no harm here, worth knowing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Leftover {
    pub file: String,
    pub text: String,
}

/// A file [`fix`] changed, and where the file as it was is kept.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Changed {
    pub file: String,
    pub backup: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Report {
    pub findings: Vec<Finding>,
    pub leftovers: Vec<Leftover>,
    /// Files switched off by an earlier fix, which [`undo`] puts back.
    pub changed: Vec<Changed>,
}

/// The folders a site's settings files are read from: its own folder and
/// the one the web server serves, when that is another.
fn folders(site: &Path, root: &Path) -> Vec<PathBuf> {
    let mut out = vec![site.to_path_buf()];
    if root != site && root.is_dir() {
        out.push(root.to_path_buf());
    }
    out
}

fn rel(site: &Path, path: &Path) -> String {
    path.strip_prefix(site)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn io(path: &Path) -> impl FnOnce(std::io::Error) -> Error + '_ {
    move |source| Error::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// Looks without changing anything.
pub fn scan(site: &Path, root: &Path) -> Report {
    let mut report = Report::default();
    for dir in folders(site, root) {
        for name in FILES {
            let path = dir.join(name);
            let Ok(bytes) = std::fs::read(&path) else {
                continue;
            };
            let text = String::from_utf8_lossy(&bytes);
            let file = rel(site, &path);
            let htaccess = name == ".htaccess";
            for (i, line) in text.lines().enumerate() {
                let setting = if htaccess {
                    htaccess_setting(line)
                } else {
                    ini_setting(line)
                };
                if let Some((directive, value)) = setting {
                    if let Some(missing) = missing_path(&directive, &value) {
                        report.findings.push(Finding {
                            file: file.clone(),
                            line: i + 1,
                            directive,
                            value,
                            missing,
                            apache_only: htaccess,
                        });
                    }
                } else if htaccess {
                    if let Some(handler) = host_php_handler(line) {
                        report.leftovers.push(Leftover {
                            file: file.clone(),
                            text: format!(
                                "Line {} picks the host's own PHP ({handler}). The web server here skips it, but another Apache server without that PHP would not.",
                                i + 1
                            ),
                        });
                    }
                }
            }
            if text.contains(MARKER) {
                if let Some(backup) = backups(&path).pop() {
                    report.changed.push(Changed {
                        file,
                        backup: rel(site, &backup),
                    });
                }
            }
        }
        report.leftovers.extend(leftover_files(site, &dir));
    }
    report.leftovers.dedup();
    report
}

/// `key = value` from a `php.ini` line, the value unquoted; `None` for
/// comments, sections and other settings.
fn ini_setting(line: &str) -> Option<(String, String)> {
    let line = line.trim();
    if line.starts_with([';', '#', '[']) {
        return None;
    }
    let (key, value) = line.split_once('=')?;
    let key = key.trim().to_ascii_lowercase();
    DIRECTIVES
        .contains(&key.as_str())
        .then(|| (key, ini_value(value)))
}

/// A `php.ini` value: what is inside its quotes, or up to a `;` comment.
fn ini_value(raw: &str) -> String {
    let raw = raw.trim();
    for quote in ['"', '\''] {
        if let Some(rest) = raw.strip_prefix(quote) {
            return rest.split(quote).next().unwrap_or(rest).to_string();
        }
    }
    raw.split(';').next().unwrap_or(raw).trim().to_string()
}

/// `php_value key value` (or `php_admin_value`) from an `.htaccess` line.
fn htaccess_setting(line: &str) -> Option<(String, String)> {
    let line = line.trim();
    let mut words = line.splitn(3, char::is_whitespace);
    let command = words.next()?.to_ascii_lowercase();
    if command != "php_value" && command != "php_admin_value" {
        return None;
    }
    let key = words.next()?.trim().to_ascii_lowercase();
    let value = words.next().unwrap_or("").trim();
    DIRECTIVES
        .contains(&key.as_str())
        .then(|| (key, ini_value(value)))
}

/// cPanel and other hosts pick their PHP build with a handler line such
/// as `AddHandler application/x-httpd-ea-php74___lsphp .php`.
fn host_php_handler(line: &str) -> Option<String> {
    let line = line.trim();
    let lower = line.to_ascii_lowercase();
    let command = lower.split_whitespace().next()?;
    let handler = matches!(command, "addhandler" | "sethandler" | "addtype")
        && lower.contains("x-httpd-")
        && lower.contains("php");
    handler.then(|| line.to_string())
}

/// Whether `path` is absolute on any system: `/home/…`, `C:\…` or `\\server\…`.
fn absolute(path: &str) -> bool {
    let b = path.as_bytes();
    path.starts_with(['/', '\\'])
        || (b.len() >= 3
            && b[0].is_ascii_alphabetic()
            && b[1] == b':'
            && matches!(b[2], b'/' | b'\\'))
}

/// A path list as `include_path` and `open_basedir` take it: split at `;`
/// (Windows) and at `:` (Linux), but not at a drive letter's colon.
fn path_list(value: &str) -> Vec<String> {
    let mut out = Vec::new();
    for part in value.split(';') {
        let pieces: Vec<&str> = part.split(':').collect();
        let mut i = 0;
        while i < pieces.len() {
            let piece = pieces[i];
            let drive = piece.len() == 1
                && piece.as_bytes()[0].is_ascii_alphabetic()
                && pieces
                    .get(i + 1)
                    .is_some_and(|next| next.starts_with(['/', '\\']));
            if drive {
                out.push(format!("{piece}:{}", pieces[i + 1]));
                i += 2;
            } else {
                out.push(piece.to_string());
                i += 1;
            }
        }
    }
    out.into_iter()
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .collect()
}

fn exists(path: &str) -> bool {
    Path::new(path).exists()
}

/// The part of a setting's value that names a place this computer doesn't
/// have, if any.
fn missing_path(directive: &str, value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() || value.eq_ignore_ascii_case("none") {
        return None;
    }
    match directive {
        "include_path" | "open_basedir" => path_list(value)
            .into_iter()
            .find(|p| absolute(p) && !exists(p)),
        "error_log" => {
            if value.eq_ignore_ascii_case("syslog") || !absolute(value) {
                return None;
            }
            // The log file is made when first written; its folder must be there.
            let parent = Path::new(value).parent()?;
            (!parent.as_os_str().is_empty() && !parent.exists()).then(|| value.to_string())
        }
        "session.save_path" => {
            // `N;/path` or `N;MODE;/path`: the folder comes last.
            let dir = value.rsplit(';').next().unwrap_or(value).trim();
            (absolute(dir) && !exists(dir)).then(|| dir.to_string())
        }
        _ => (absolute(value) && !exists(value)).then(|| value.to_string()),
    }
}

/// Files in a site's folder that name a host's folders: a firewall's start
/// file (`wordfence-waf.php`, `malcare-waf.php`) and a page cache's
/// `advanced-cache.php`.
fn leftover_files(site: &Path, dir: &Path) -> Vec<Leftover> {
    let mut candidates: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| {
                    p.file_name()
                        .and_then(|n| n.to_str())
                        .map(str::to_ascii_lowercase)
                        .is_some_and(|n| n.ends_with(".php") && n.contains("waf"))
                })
                .collect()
        })
        .unwrap_or_default();
    candidates.sort();
    candidates.push(dir.join("wp-content").join("advanced-cache.php"));
    let mut out = Vec::new();
    for path in candidates {
        let Some(missing) = read_head(&path).and_then(|text| host_path_in(&text)) else {
            continue;
        };
        let cache = path.ends_with("advanced-cache.php");
        let text = if cache {
            format!("Uses {missing}, which only the live server has. A page cache drop-in: harmless while WP_CACHE is off in wp-config.php.")
        } else {
            format!("Loads {missing}, which only the live server has. A firewall's start file: harmless as long as no setting runs it first (auto_prepend_file).")
        };
        out.push(Leftover {
            file: rel(site, &path),
            text,
        });
    }
    out
}

/// The first 256 KB of a file, as text.
fn read_head(path: &Path) -> Option<String> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .ok()?
        .take(256 * 1024)
        .read_to_end(&mut bytes)
        .ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// The first quoted host path in `text` this computer doesn't have.
fn host_path_in(text: &str) -> Option<String> {
    for quote in ['\'', '"'] {
        for start in HOST_FOLDERS {
            let needle = format!("{quote}{start}");
            let mut from = 0;
            while let Some(at) = text[from..].find(&needle) {
                let begin = from + at + 1;
                let end = text[begin..]
                    .find([quote, '\n', '\r'])
                    .map_or(text.len(), |n| begin + n);
                let path = &text[begin..end];
                if !exists(path) {
                    return Some(path.to_string());
                }
                from = end;
            }
        }
    }
    None
}

/// Whether a file name is a copy [`fix`] kept: `.user.ini.live`,
/// `.htaccess.live.2`…
pub fn is_backup_name(name: &str) -> bool {
    FILES.iter().any(|base| {
        name.strip_prefix(base)
            .and_then(|rest| rest.strip_prefix(".live"))
            .is_some_and(|n| {
                n.is_empty()
                    || n.strip_prefix('.')
                        .is_some_and(|d| !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit()))
            })
    })
}

/// The kept copies of `file`, oldest first: `.live`, `.live.1`, `.live.2`…
fn backups(file: &Path) -> Vec<PathBuf> {
    let Some(name) = file.file_name().and_then(|n| n.to_str()) else {
        return Vec::new();
    };
    let dir = file.parent().unwrap_or(Path::new("."));
    let mut found: Vec<(u32, PathBuf)> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|e| {
                    let n = e.file_name().into_string().ok()?;
                    let rest = n.strip_prefix(name)?.strip_prefix(".live")?;
                    let number = match rest {
                        "" => 0,
                        _ => rest.strip_prefix('.')?.parse::<u32>().ok()?,
                    };
                    Some((number, e.path()))
                })
                .collect()
        })
        .unwrap_or_default();
    found.sort();
    found.into_iter().map(|(_, p)| p).collect()
}

/// Where the next copy of `file` goes: `.live`, or past the newest one.
fn next_backup(file: &Path) -> PathBuf {
    let name = file
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let newest = backups(file).last().and_then(|p| {
        let n = p.file_name()?.to_str()?.to_string();
        let rest = n.strip_prefix(&name)?.strip_prefix(".live")?.to_string();
        Some(
            rest.strip_prefix('.')
                .and_then(|d| d.parse::<u32>().ok())
                .unwrap_or(0),
        )
    });
    let suffix = match newest {
        None => ".live".to_string(),
        Some(n) => format!(".live.{}", n + 1),
    };
    file.with_file_name(format!("{name}{suffix}"))
}

/// `text` with the given lines (counted from 1) switched off, each behind
/// a note naming `backup`. In a `php.ini` an emptied file setting follows,
/// so a value set elsewhere doesn't come back.
fn switch_off(text: &str, lines: &[(usize, &str)], htaccess: bool, backup: &str) -> String {
    let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let comment = if htaccess { "#" } else { ";" };
    let mut out = String::with_capacity(text.len() + 256);
    for (i, line) in text.split_inclusive('\n').enumerate() {
        let Some((_, directive)) = lines.iter().find(|(n, _)| *n == i + 1) else {
            out.push_str(line);
            continue;
        };
        let body = line.trim_end_matches(['\r', '\n']);
        let ending = &line[body.len()..];
        let indent = &body[..body.len() - body.trim_start().len()];
        out.push_str(&format!(
            "{indent}{comment} {MARKER}: path only exists on the live server. Original kept in {backup}{eol}"
        ));
        let empty = !htaccess && matches!(*directive, "auto_prepend_file" | "auto_append_file");
        if empty {
            out.push_str(&format!("{indent}{comment} {}{eol}", body.trim_start()));
            out.push_str(&format!("{indent}{directive} ={ending}"));
        } else {
            out.push_str(&format!("{indent}{comment} {}{ending}", body.trim_start()));
        }
    }
    out
}

/// Switches off every setting [`scan`] found, keeping each file as it was
/// next to it. Returns the files changed.
pub fn fix(site: &Path, root: &Path) -> Result<Vec<Changed>> {
    let report = scan(site, root);
    let mut files: Vec<&str> = report.findings.iter().map(|f| f.file.as_str()).collect();
    files.dedup();
    let mut changed = Vec::new();
    for file in files {
        let path = site.join(file);
        let bytes = std::fs::read(&path).map_err(io(&path))?;
        let text = String::from_utf8_lossy(&bytes);
        let lines: Vec<(usize, &str)> = report
            .findings
            .iter()
            .filter(|f| f.file == file)
            .map(|f| (f.line, f.directive.as_str()))
            .collect();
        let backup = next_backup(&path);
        let backup_name = backup
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let htaccess = file.ends_with(".htaccess");
        let patched = switch_off(&text, &lines, htaccess, &backup_name);
        std::fs::write(&backup, &bytes).map_err(io(&backup))?;
        std::fs::write(&path, patched).map_err(io(&path))?;
        changed.push(Changed {
            file: file.to_string(),
            backup: rel(site, &backup),
        });
    }
    Ok(changed)
}

/// Puts back what [`fix`] changed, from the newest kept copy of each file,
/// and removes that copy. Returns the files put back.
pub fn undo(site: &Path, root: &Path) -> Result<Vec<String>> {
    let mut restored = Vec::new();
    for changed in scan(site, root).changed {
        let path = site.join(&changed.file);
        let backup = site.join(&changed.backup);
        std::fs::copy(&backup, &path).map_err(io(&path))?;
        std::fs::remove_file(&backup).map_err(io(&backup))?;
        restored.push(changed.file);
    }
    Ok(restored)
}

/// Files of a site that stay on this computer when it is pushed to (or
/// pulled from) its server, by path relative to `site` with `/`: kept
/// copies, and settings files with lines switched off by [`fix`], so the
/// server keeps its own, firewall and all.
pub fn stays_here(site: &Path, rel: &str) -> bool {
    let name = rel.rsplit('/').next().unwrap_or(rel);
    if is_backup_name(name) {
        return true;
    }
    FILES.contains(&name)
        && std::fs::read(site.join(rel))
            .is_ok_and(|bytes| String::from_utf8_lossy(&bytes).contains(MARKER))
}

/// The files [`stays_here`] keeps that a fix changed, for a note.
pub fn changed_here(site: &Path, root: &Path) -> Vec<String> {
    scan(site, root)
        .changed
        .into_iter()
        .map(|c| c.file)
        .collect()
}

// ---------------------------------------------------------------------------
// A site, as the app and the command line see it

/// What [`fix_site`] did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Fixed {
    pub changed: Vec<Changed>,
    /// The site's PHP was restarted, so the change counts at once.
    pub restarted: bool,
    /// The home page afterwards; `None` with `check_error` when it couldn't
    /// be asked for (the web server is stopped, no index.php).
    pub check: Option<Check>,
    pub check_error: Option<String>,
}

/// What [`undo_site`] put back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Undone {
    pub restored: Vec<String>,
    pub restarted: bool,
}

fn served(manager: &ServiceManager, domain: &str) -> Result<ServedSite> {
    manager
        .sites()?
        .into_iter()
        .find(|s| s.domain() == domain)
        .ok_or_else(|| Error::SiteNotFound(domain.to_string()))
}

/// [`scan`] for a site by its domain.
pub fn scan_site(manager: &ServiceManager, domain: &str) -> Result<Report> {
    let served = served(manager, domain)?;
    Ok(scan(&served.site.path, &served.root))
}

/// Asks for the site's home page; a PHP error found also goes in the log
/// of the PHP version serving it.
pub fn check_site(manager: &ServiceManager, domain: &str) -> Result<Check> {
    let served = served(manager, domain)?;
    let check = check(&served, CHECK_TIMEOUT)?;
    if let Some(error) = &check.error {
        log_error(manager.home(), &served, error);
    }
    Ok(check)
}

/// [`fix`], then a restart of the site's PHP so `.user.ini` is read again
/// (PHP otherwise keeps it for `user_ini.cache_ttl`, five minutes), then a
/// look at the home page.
pub fn fix_site(manager: &ServiceManager, supervisor: &Supervisor, domain: &str) -> Result<Fixed> {
    let served = served(manager, domain)?;
    let changed = fix(&served.site.path, &served.root)?;
    let restarted = !changed.is_empty() && manager.restart_site_php(supervisor, domain)?;
    let (check, check_error) = match check(&served, CHECK_TIMEOUT) {
        Ok(check) => {
            if let Some(error) = &check.error {
                log_error(manager.home(), &served, error);
            }
            (Some(check), None)
        }
        Err(e) => (None, Some(e.to_string())),
    };
    Ok(Fixed {
        changed,
        restarted,
        check,
        check_error,
    })
}

/// [`undo`], then a restart of the site's PHP.
pub fn undo_site(
    manager: &ServiceManager,
    supervisor: &Supervisor,
    domain: &str,
) -> Result<Undone> {
    let served = served(manager, domain)?;
    let restored = undo(&served.site.path, &served.root)?;
    let restarted = !restored.is_empty() && manager.restart_site_php(supervisor, domain)?;
    Ok(Undone {
        restored,
        restarted,
    })
}

// ---------------------------------------------------------------------------
// Asking PHP for the home page

/// What the site's PHP answered for the home page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Check {
    /// The HTTP status PHP gave.
    pub status: u16,
    /// 2xx or 3xx with no PHP error.
    pub ok: bool,
    /// The PHP error, as plain text.
    pub error: Option<String>,
    /// A file PHP couldn't open (`Failed opening required '…'`) that this
    /// computer doesn't have: a live-server path.
    pub missing: Option<String>,
    pub millis: u64,
}

/// How long the home page may take.
pub const CHECK_TIMEOUT: Duration = Duration::from_secs(90);

/// Asks the PHP process serving the site for its home page, the way the
/// web server does (FastCGI), so what PHP says before the site runs is
/// seen even when the page looks blank.
pub fn check(served: &ServedSite, timeout: Duration) -> Result<Check> {
    let Backend::Php { port, .. } = &served.backend else {
        return Err(Error::Project(format!(
            "{} isn't served by PHP",
            served.domain()
        )));
    };
    let script = served.root.join("index.php");
    if !script.is_file() {
        return Err(Error::Project(format!(
            "{} has no index.php to ask for",
            served.root.display()
        )));
    }
    let domain = served.domain();
    let slash = |p: &Path| p.to_string_lossy().replace('\\', "/");
    let https = served.site.https;
    let mut params: Vec<(String, String)> = [
        ("GATEWAY_INTERFACE", "CGI/1.1".to_string()),
        ("SERVER_SOFTWARE", "Stackyard".to_string()),
        ("SERVER_PROTOCOL", "HTTP/1.1".to_string()),
        ("REQUEST_METHOD", "GET".to_string()),
        (
            "REQUEST_SCHEME",
            if https { "https" } else { "http" }.to_string(),
        ),
        ("REQUEST_URI", "/".to_string()),
        ("QUERY_STRING", String::new()),
        ("DOCUMENT_ROOT", slash(&served.root)),
        ("SCRIPT_FILENAME", slash(&script)),
        ("SCRIPT_NAME", "/index.php".to_string()),
        ("DOCUMENT_URI", "/index.php".to_string()),
        ("SERVER_NAME", domain.clone()),
        ("SERVER_PORT", if https { "443" } else { "80" }.to_string()),
        ("HTTP_HOST", domain.clone()),
        ("REMOTE_ADDR", "127.0.0.1".to_string()),
        ("HTTP_USER_AGENT", "Stackyard site check".to_string()),
        ("HTTP_ACCEPT", "text/html".to_string()),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect();
    if https {
        params.push(("HTTPS".into(), "on".into()));
    }
    for (key, value) in &served.site.settings.env {
        params.push((key.clone(), value.clone()));
    }
    let started = Instant::now();
    let (stdout, stderr) = fastcgi(*port, &params, timeout)?;
    let millis = started.elapsed().as_millis() as u64;
    Ok(read_answer(&stdout, &stderr, millis))
}

/// Makes sense of PHP's FastCGI answer.
fn read_answer(stdout: &[u8], stderr: &[u8], millis: u64) -> Check {
    let text = String::from_utf8_lossy(stdout);
    let (head, body) = text
        .split_once("\r\n\r\n")
        .or_else(|| text.split_once("\n\n"))
        .unwrap_or(("", &text));
    let mut status = None;
    let mut redirect = false;
    for line in head.lines() {
        if let Some((name, value)) = line.split_once(':') {
            match name.trim().to_ascii_lowercase().as_str() {
                "status" => status = value.trim().get(..3).and_then(|s| s.parse().ok()),
                "location" => redirect = true,
                _ => {}
            }
        }
    }
    let status = status.unwrap_or(if redirect { 302 } else { 200 });
    let errors = String::from_utf8_lossy(stderr);
    let error = php_error(&errors).or_else(|| php_error(body));
    let missing = [errors.as_ref(), body]
        .iter()
        .find_map(|t| required_path(t))
        .filter(|p| !exists(p));
    Check {
        status,
        ok: (200..400).contains(&status) && error.is_none(),
        error,
        missing,
        millis,
    }
}

/// The first fatal PHP error in a page or log, as plain text.
fn php_error(text: &str) -> Option<String> {
    let plain = strip_tags(text);
    plain
        .lines()
        .map(str::trim)
        .find(|line| {
            line.contains("Fatal error:")
                || line.contains("Parse error:")
                || line.contains("Failed opening required")
        })
        .map(|line| {
            let line = line.trim_start_matches("PHP ").to_string();
            match line.char_indices().nth(400) {
                Some((at, _)) => format!("{}…", &line[..at]),
                None => line,
            }
        })
}

/// The path in `Failed opening required '…'`.
fn required_path(text: &str) -> Option<String> {
    let at = text.find("Failed opening required '")? + "Failed opening required '".len();
    let rest = &text[at..];
    Some(rest[..rest.find('\'')?].to_string())
}

/// HTML as text: tags dropped, `<br>` as a new line, entities left alone.
fn strip_tags(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(open) = rest.find('<') {
        out.push_str(&rest[..open]);
        let Some(close) = rest[open..].find('>') else {
            rest = &rest[open..];
            break;
        };
        let tag = rest[open + 1..open + close].to_ascii_lowercase();
        if tag.starts_with("br") || tag.starts_with("/p") || tag.starts_with("/div") {
            out.push('\n');
        }
        rest = &rest[open + close + 1..];
    }
    out.push_str(rest);
    out
}

const FCGI_BEGIN_REQUEST: u8 = 1;
const FCGI_END_REQUEST: u8 = 3;
const FCGI_PARAMS: u8 = 4;
const FCGI_STDIN: u8 = 5;
const FCGI_STDOUT: u8 = 6;
const FCGI_STDERR: u8 = 7;
/// The most of a page kept: errors show at its start.
const MAX_ANSWER: usize = 2 * 1024 * 1024;

fn record(kind: u8, content: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(8 + content.len());
    out.extend_from_slice(&[1, kind, 0, 1]);
    out.extend_from_slice(&(content.len() as u16).to_be_bytes());
    out.extend_from_slice(&[0, 0]);
    out.extend_from_slice(content);
    out
}

fn name_value_length(out: &mut Vec<u8>, len: usize) {
    if len < 128 {
        out.push(len as u8);
    } else {
        out.extend_from_slice(&((len as u32) | 0x8000_0000).to_be_bytes());
    }
}

/// One FastCGI request with no body: what PHP wrote to its output and to
/// its error stream.
fn fastcgi(
    port: u16,
    params: &[(String, String)],
    timeout: Duration,
) -> Result<(Vec<u8>, Vec<u8>)> {
    let address = SocketAddr::from(([127, 0, 0, 1], port));
    let not_running = |e: std::io::Error| {
        Error::Project(format!(
            "PHP isn't answering on port {port} ({e}); start the web server first"
        ))
    };
    let mut stream =
        TcpStream::connect_timeout(&address, Duration::from_secs(3)).map_err(not_running)?;
    let deadline = Instant::now() + timeout;
    stream.set_write_timeout(Some(Duration::from_secs(10))).ok();

    let mut request = record(FCGI_BEGIN_REQUEST, &[0, 1, 0, 0, 0, 0, 0, 0]);
    let mut pairs = Vec::new();
    for (name, value) in params {
        name_value_length(&mut pairs, name.len());
        name_value_length(&mut pairs, value.len());
        pairs.extend_from_slice(name.as_bytes());
        pairs.extend_from_slice(value.as_bytes());
    }
    for chunk in pairs.chunks(0xFFFF) {
        request.extend(record(FCGI_PARAMS, chunk));
    }
    request.extend(record(FCGI_PARAMS, &[]));
    request.extend(record(FCGI_STDIN, &[]));
    stream.write_all(&request).map_err(not_running)?;

    let timed_out = || {
        Error::Project(format!(
            "the home page did not answer within {} seconds",
            timeout.as_secs()
        ))
    };
    let mut read_exact = |buf: &mut [u8]| -> Result<bool> {
        let mut filled = 0;
        while filled < buf.len() {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(timed_out());
            }
            stream.set_read_timeout(Some(left)).ok();
            match stream.read(&mut buf[filled..]) {
                Ok(0) => return Ok(false),
                Ok(n) => filled += n,
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    return Err(timed_out())
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(e) => return Err(Error::Project(format!("PHP closed the connection: {e}"))),
            }
        }
        Ok(true)
    };
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    loop {
        let mut header = [0u8; 8];
        if !read_exact(&mut header)? {
            break;
        }
        let len = u16::from_be_bytes([header[4], header[5]]) as usize;
        let mut content = vec![0u8; len + header[6] as usize];
        if !read_exact(&mut content)? {
            break;
        }
        content.truncate(len);
        match header[1] {
            FCGI_STDOUT if stdout.len() < MAX_ANSWER => stdout.extend_from_slice(&content),
            FCGI_STDERR if stderr.len() < MAX_ANSWER => stderr.extend_from_slice(&content),
            FCGI_END_REQUEST => break,
            _ => {}
        }
    }
    Ok((stdout, stderr))
}

/// Adds a PHP error a check found to the log of the PHP version serving
/// the site, since PHP itself only puts it in the page.
pub fn log_error(home: &Path, served: &ServedSite, error: &str) {
    let Backend::Php { version, .. } = &served.backend else {
        return;
    };
    let path = home.join("logs").join(format!("php-{version}.log"));
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(file, "[Stackyard site check] {}: {error}", served.domain());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::tempdir;

    const WAF: &str = "/home/858819.cloudwaysapps.com/hatmbdzgdm/public_html/wordfence-waf.php";

    fn wordfence_site() -> PathBuf {
        let dir = tempdir();
        std::fs::write(
            dir.join(".user.ini"),
            format!("; Wordfence WAF\r\nauto_prepend_file = '{WAF}'\r\n; END Wordfence WAF\r\n"),
        )
        .unwrap();
        std::fs::write(
            dir.join("wordfence-waf.php"),
            "<?php\nif (file_exists('/home/858819.cloudwaysapps.com/hatmbdzgdm/public_html/wp-content/plugins/wordfence/waf/bootstrap.php')) {}\n",
        )
        .unwrap();
        std::fs::write(
            dir.join(".htaccess"),
            "AddHandler application/x-httpd-ea-php74___lsphp .php\n<IfModule mod_php7.c>\n\tphp_value auto_prepend_file '/home/x/public_html/wordfence-waf.php'\n</IfModule>\n",
        )
        .unwrap();
        std::fs::write(dir.join("index.php"), "<?php echo 'hi';").unwrap();
        dir
    }

    #[test]
    fn finds_the_wordfence_line_and_leftovers() {
        let dir = wordfence_site();
        let report = scan(&dir, &dir);
        assert_eq!(report.findings.len(), 2, "{report:?}");
        let ini = &report.findings[0];
        assert_eq!(ini.file, ".user.ini");
        assert_eq!(ini.line, 2);
        assert_eq!(ini.directive, "auto_prepend_file");
        assert_eq!(ini.missing, WAF);
        assert!(!ini.apache_only);
        let htaccess = report
            .findings
            .iter()
            .find(|f| f.file == ".htaccess")
            .unwrap();
        assert_eq!(htaccess.line, 3);
        assert!(htaccess.apache_only);
        assert!(report
            .leftovers
            .iter()
            .any(|l| l.file == "wordfence-waf.php"));
        assert!(report
            .leftovers
            .iter()
            .any(|l| l.file == ".htaccess" && l.text.contains("x-httpd-ea-php74")));
        assert!(report.changed.is_empty());
    }

    #[test]
    fn paths_that_exist_are_fine() {
        let dir = tempdir();
        let here = dir.join("prepend.php");
        std::fs::write(&here, "<?php").unwrap();
        std::fs::write(
            dir.join(".user.ini"),
            format!(
                "auto_prepend_file = \"{}\"\ninclude_path = \".:{}\"\nerror_log = syslog\nmemory_limit = 512M\n",
                here.display(),
                dir.display()
            ),
        )
        .unwrap();
        assert!(scan(&dir, &dir).findings.is_empty());
    }

    #[test]
    fn path_lists_split_on_both_systems() {
        assert_eq!(path_list(".;C:\\php\\pear"), [".", "C:\\php\\pear"]);
        assert_eq!(
            path_list(".:/usr/share/php:/home/a"),
            [".", "/usr/share/php", "/home/a"]
        );
        assert_eq!(
            missing_path("include_path", ".:/home/nowhere-here/lib"),
            Some("/home/nowhere-here/lib".into())
        );
        assert_eq!(
            missing_path("session.save_path", "2;/var/nowhere-here/sessions"),
            Some("/var/nowhere-here/sessions".into())
        );
        assert_eq!(
            missing_path("error_log", "/home/nowhere-here/logs/php.log"),
            Some("/home/nowhere-here/logs/php.log".into())
        );
        assert_eq!(missing_path("auto_prepend_file", "prepend.php"), None);
        assert_eq!(missing_path("auto_prepend_file", "none"), None);
    }

    #[test]
    fn fix_switches_off_keeps_a_copy_and_undo_puts_it_back() {
        let dir = wordfence_site();
        let before = std::fs::read(dir.join(".user.ini")).unwrap();
        let before_htaccess = std::fs::read(dir.join(".htaccess")).unwrap();
        let changed = fix(&dir, &dir).unwrap();
        assert_eq!(changed.len(), 2);
        assert_eq!(changed[0].backup, ".user.ini.live");
        assert_eq!(std::fs::read(dir.join(".user.ini.live")).unwrap(), before);
        let ini = std::fs::read_to_string(dir.join(".user.ini")).unwrap();
        assert_eq!(
            ini,
            format!("; Wordfence WAF\r\n; {MARKER}: path only exists on the live server. Original kept in .user.ini.live\r\n; auto_prepend_file = '{WAF}'\r\nauto_prepend_file =\r\n; END Wordfence WAF\r\n")
        );
        let htaccess = std::fs::read_to_string(dir.join(".htaccess")).unwrap();
        assert!(
            htaccess.contains("\t# php_value auto_prepend_file"),
            "{htaccess}"
        );
        assert!(!htaccess.contains("\nauto_prepend_file ="));

        let report = scan(&dir, &dir);
        assert!(report.findings.is_empty(), "{report:?}");
        assert_eq!(report.changed.len(), 2);
        assert!(stays_here(&dir, ".user.ini"));
        assert!(stays_here(&dir, ".htaccess"));
        assert!(stays_here(&dir, ".user.ini.live"));
        assert!(!stays_here(&dir, "index.php"));

        let restored = undo(&dir, &dir).unwrap();
        assert_eq!(restored.len(), 2);
        assert_eq!(std::fs::read(dir.join(".user.ini")).unwrap(), before);
        assert_eq!(
            std::fs::read(dir.join(".htaccess")).unwrap(),
            before_htaccess
        );
        assert!(!dir.join(".user.ini.live").exists());
        assert!(scan(&dir, &dir).changed.is_empty());
        assert!(!stays_here(&dir, ".user.ini"));
    }

    #[test]
    fn an_earlier_copy_is_never_overwritten() {
        let dir = wordfence_site();
        std::fs::write(dir.join(".user.ini.live"), "older").unwrap();
        std::fs::write(dir.join(".user.ini.live.3"), "older still").unwrap();
        let changed = fix(&dir, &dir).unwrap();
        assert_eq!(changed[0].backup, ".user.ini.live.4");
        assert_eq!(
            std::fs::read_to_string(dir.join(".user.ini.live")).unwrap(),
            "older"
        );
        assert!(std::fs::read_to_string(dir.join(".user.ini"))
            .unwrap()
            .contains(".user.ini.live.4"));
        undo(&dir, &dir).unwrap();
        assert!(!dir.join(".user.ini.live.4").exists());
        assert!(dir.join(".user.ini.live.3").exists());
    }

    #[test]
    fn a_served_folder_inside_the_site_is_scanned_too() {
        let dir = tempdir();
        let public = dir.join("public");
        std::fs::create_dir_all(&public).unwrap();
        std::fs::write(
            public.join(".user.ini"),
            "auto_append_file=/srv/nowhere-here/end.php",
        )
        .unwrap();
        let report = scan(&dir, &public);
        assert_eq!(report.findings[0].file, "public/.user.ini");
        fix(&dir, &public).unwrap();
        let ini = std::fs::read_to_string(public.join(".user.ini")).unwrap();
        assert!(
            ini.ends_with("; auto_append_file=/srv/nowhere-here/end.php\nauto_append_file ="),
            "{ini}"
        );
        assert!(public.join(".user.ini.live").exists());
    }

    #[test]
    fn backup_names() {
        assert!(is_backup_name(".user.ini.live"));
        assert!(is_backup_name(".htaccess.live.12"));
        assert!(is_backup_name("php.ini.live.1"));
        assert!(!is_backup_name(".user.ini"));
        assert!(!is_backup_name(".user.ini.live."));
        assert!(!is_backup_name(".user.ini.lively"));
        assert!(!is_backup_name("wp-config.live.php"));
    }

    #[test]
    fn reads_a_startup_error() {
        let page = format!("Status: 500 Internal Server Error\r\nContent-type: text/html; charset=UTF-8\r\n\r\n<br />\n<b>Warning</b>:  Unknown: Failed to open stream: No such file or directory in <b>Unknown</b> on line <b>0</b><br />\n<br />\n<b>Fatal error</b>:  Failed opening required '{WAF}' (include_path='.;C:\\php\\pear') in <b>Unknown</b> on line <b>0</b><br />\n");
        let check = read_answer(page.as_bytes(), b"", 12);
        assert_eq!(check.status, 500);
        assert!(!check.ok);
        assert_eq!(check.missing.as_deref(), Some(WAF));
        assert!(check
            .error
            .unwrap()
            .starts_with("Fatal error:  Failed opening required"));

        let fine = read_answer(
            b"Content-type: text/html\r\n\r\n<h1>Fatal error: a post title</h1>",
            b"",
            3,
        );
        assert_eq!(fine.status, 200);
        let moved = read_answer(
            b"Location: https://a.stack/\r\n\r\n",
            b"PHP Deprecated:  x in y on line 3\n",
            3,
        );
        assert_eq!(moved.status, 302);
        assert!(moved.ok);
        let logged = read_answer(
            b"Status: 500\r\n\r\n",
            b"PHP Fatal error:  Uncaught Error: boom in /a.php:3\n",
            3,
        );
        assert_eq!(
            logged.error.as_deref(),
            Some("Fatal error:  Uncaught Error: boom in /a.php:3")
        );
    }

    #[test]
    fn talks_fastcgi() {
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut seen = Vec::new();
            // Read records until the empty STDIN one.
            loop {
                let mut header = [0u8; 8];
                stream.read_exact(&mut header).unwrap();
                let len = u16::from_be_bytes([header[4], header[5]]) as usize + header[6] as usize;
                let mut content = vec![0u8; len];
                stream.read_exact(&mut content).unwrap();
                seen.extend_from_slice(&content);
                if header[1] == FCGI_STDIN && len == 0 {
                    break;
                }
            }
            let mut answer = record(FCGI_STDERR, b"PHP Fatal error:  boom\n");
            answer.extend(record(
                FCGI_STDOUT,
                b"Status: 500 Internal Server Error\r\n\r\nx",
            ));
            answer.extend(record(FCGI_END_REQUEST, &[0; 8]));
            stream.write_all(&answer).unwrap();
            String::from_utf8_lossy(&seen).into_owned()
        });
        let params = vec![(
            "SCRIPT_FILENAME".to_string(),
            "C:/www/a/index.php".to_string(),
        )];
        let (out, err) = fastcgi(port, &params, Duration::from_secs(5)).unwrap();
        assert!(server
            .join()
            .unwrap()
            .contains("SCRIPT_FILENAMEC:/www/a/index.php"));
        let check = read_answer(&out, &err, 1);
        assert_eq!(check.status, 500);
        assert_eq!(check.error.as_deref(), Some("Fatal error:  boom"));
    }

    /// The whole repair against a real PHP: `STACKYARD_TEST_PHP_CGI` names
    /// a php-cgi program.
    #[test]
    #[ignore]
    fn fixes_a_real_php() {
        use crate::sites::{Site, SiteRuntime};
        let Ok(program) = std::env::var("STACKYARD_TEST_PHP_CGI") else {
            return;
        };
        let dir = tempdir();
        let ini = "auto_prepend_file = '/home/x/public_html/wordfence-waf.php'\n";
        std::fs::write(dir.join(".user.ini"), ini).unwrap();
        std::fs::write(dir.join("index.php"), "<?php echo 'hello';").unwrap();
        let port = {
            let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            l.local_addr().unwrap().port()
        };
        let start = || {
            let child = std::process::Command::new(&program)
                .args([
                    "-b",
                    &format!("127.0.0.1:{port}"),
                    "-d",
                    "user_ini.cache_ttl=300",
                ])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .unwrap();
            std::thread::sleep(Duration::from_millis(500));
            child
        };
        let served = ServedSite {
            site: Site {
                name: "copy".into(),
                tld: ".stack".into(),
                path: dir.clone(),
                runtime: SiteRuntime::Php {
                    version: "8.4".into(),
                },
                https: true,
                dev_server: false,
                settings: Default::default(),
            },
            parked: false,
            framework: None,
            root: dir.clone(),
            backend: Backend::Php {
                version: "8.4".into(),
                port,
                installed: true,
            },
        };
        let mut php = start();
        let before = check(&served, Duration::from_secs(20)).unwrap();
        assert!(!before.ok, "{before:?}");
        assert_eq!(
            before.missing.as_deref(),
            Some("/home/x/public_html/wordfence-waf.php")
        );

        fix(&dir, &dir).unwrap();
        // PHP keeps .user.ini for its cache time: a restart reads it again.
        php.kill().unwrap();
        php.wait().unwrap();
        let mut php = start();
        let after = check(&served, Duration::from_secs(20)).unwrap();
        assert!(after.ok, "{after:?}");
        assert_eq!(after.status, 200);
        php.kill().unwrap();
        php.wait().unwrap();

        undo(&dir, &dir).unwrap();
        assert_eq!(std::fs::read_to_string(dir.join(".user.ini")).unwrap(), ini);
    }
}
