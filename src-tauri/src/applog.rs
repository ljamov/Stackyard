//! The app's own log, `stackyard.log`: each start-up step, errors, panics
//! and errors from the window's page, so a start that goes wrong leaves a
//! trace. On Windows it sits next to `stackyard-app.exe` (the install
//! folder, by the uninstaller); elsewhere, or when that folder can't be
//! written, in the data home's `logs` folder. The file starts over once it
//! passes 1 MB, keeping the two before it as `stackyard.log.1` and `.2`.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

const NAME: &str = "stackyard.log";
const MAX: u64 = 1024 * 1024;
const KEEP: u32 = 2;

struct Log {
    path: PathBuf,
    file: Option<File>,
    size: u64,
}

static LOG: OnceLock<Mutex<Log>> = OnceLock::new();

/// Where the log goes: the install folder on Windows when it can be
/// written, else the data home's `logs` folder. (A macOS app bundle and
/// Linux's /usr/bin are not places for it.)
fn choose(home: &Path) -> PathBuf {
    #[cfg(windows)]
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
    {
        let path = dir.join(NAME);
        if OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .is_ok()
        {
            return path;
        }
    }
    let dir = home.join("logs");
    let _ = std::fs::create_dir_all(&dir);
    dir.join(NAME)
}

fn rotated(path: &Path, n: u32) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(format!(".{n}"));
    PathBuf::from(name)
}

fn rotate(path: &Path) {
    for n in (1..KEEP).rev() {
        let _ = std::fs::rename(rotated(path, n), rotated(path, n + 1));
    }
    let _ = std::fs::rename(path, rotated(path, 1));
}

fn open(path: &Path) -> (Option<File>, u64) {
    let size = std::fs::metadata(path).map_or(0, |m| m.len());
    if size >= MAX {
        rotate(path);
        return open(path);
    }
    let file = OpenOptions::new().create(true).append(true).open(path).ok();
    (file, size)
}

/// Opens the log and catches panics in it. Call once, first thing.
pub fn init(home: &Path) {
    let path = choose(home);
    let (file, size) = open(&path);
    let _ = LOG.set(Mutex::new(Log { path, file, size }));
    let before = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        let place = info
            .location()
            .map(|l| format!(" at {}:{}", l.file(), l.line()))
            .unwrap_or_default();
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "(no message)".into());
        let trace = std::backtrace::Backtrace::force_capture();
        write(
            "PANIC",
            &format!(
                "thread '{}' panicked{place}: {message}\n{trace}",
                thread.name().unwrap_or("unnamed")
            ),
        );
        before(info);
    }));
    write(
        "INFO",
        &format!(
            "Stackyard {} started ({} {}), data in {}",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS,
            std::env::consts::ARCH,
            home.display()
        ),
    );
}

/// The log file, for Open log file.
pub fn path() -> Option<PathBuf> {
    LOG.get()
        .map(|log| log.lock().unwrap_or_else(|e| e.into_inner()).path.clone())
}

/// Adds a line (or a few) to the log, with the time in UTC.
pub fn write(level: &str, message: &str) {
    let Some(log) = LOG.get() else {
        return;
    };
    let mut log = log.lock().unwrap_or_else(|e| e.into_inner());
    if log.size >= MAX {
        log.file = None;
        rotate(&log.path);
        let (file, size) = open(&log.path);
        log.file = file;
        log.size = size;
    }
    let line = format!("{} {level:<5} {}\n", now(), message.trim_end());
    if let Some(file) = log.file.as_mut() {
        if file.write_all(line.as_bytes()).is_ok() {
            let _ = file.flush();
            log.size += line.len() as u64;
        }
    }
}

pub fn info(message: &str) {
    write("INFO", message);
}

/// Logs an error and prints it, for a run from a terminal.
pub fn error(message: &str) {
    eprintln!("{message}");
    write("ERROR", message);
}

/// `2026-10-27 07:53:20.123Z`.
fn now() -> String {
    let since = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = since.as_secs() as i64;
    let (y, m, d) = civil(secs.div_euclid(86_400));
    let t = secs.rem_euclid(86_400);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}.{:03}Z",
        t / 3600,
        t % 3600 / 60,
        t % 60,
        since.subsec_millis()
    )
}

/// Days since 1970-01-01 → year, month, day (Howard Hinnant's algorithm).
fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

/// An error, a warning or a crash from the window's page.
#[tauri::command]
pub fn app_log(level: String, message: String) {
    let level = match level.as_str() {
        "error" => "ERROR",
        "warn" => "WARN",
        _ => "INFO",
    };
    // A runaway page can't fill the disk with one line.
    let message: String = message.chars().take(8000).collect();
    write(level, &format!("window: {message}"));
}

/// Opens the log in the system's text viewer.
#[tauri::command]
pub fn app_log_open() -> crate::CmdResult<String> {
    let path = path().ok_or("the log is not open")?;
    if !path.exists() {
        File::create(&path).map_err(|e| format!("could not make {}: {e}", path.display()))?;
    }
    crate::sitetools::open_with_system(path.as_os_str())?;
    Ok(path.display().to_string())
}

/// Where the log is, for Settings.
#[tauri::command]
pub fn app_log_path() -> Option<String> {
    path().map(|p| p.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates() {
        assert_eq!(civil(0), (1970, 1, 1));
        assert_eq!(civil(20_753), (2026, 10, 27));
        assert_eq!(civil(11_016), (2000, 2, 29));
    }

    #[test]
    fn rotates_and_keeps_two() {
        let dir = std::env::temp_dir().join(format!("stackyard-applog-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(NAME);
        for n in 0..4 {
            std::fs::write(&path, vec![b'x'; MAX as usize + n]).unwrap();
            let (file, size) = open(&path);
            assert!(file.is_some());
            assert_eq!(size, 0);
        }
        assert!(rotated(&path, 1).exists());
        assert!(rotated(&path, 2).exists());
        assert!(!rotated(&path, 3).exists());
        assert_eq!(std::fs::metadata(rotated(&path, 1)).unwrap().len(), MAX + 3);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
