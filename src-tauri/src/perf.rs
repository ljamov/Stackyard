//! Numbers for the performance checks in CI. With `STACKYARD_PERF_REPORT`
//! set to a file, the app writes `ready_ms=` there once the window's page
//! has loaded, closes the window as a person would (dropping the web view,
//! leaving the tray), and writes `tray=1` once that has settled, so the
//! check can measure tray-only memory. Without the variable this does
//! nothing.

use std::io::Write;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use tauri::{Manager, Runtime, Webview};

static STARTED: OnceLock<Instant> = OnceLock::new();
static REPORTED: OnceLock<()> = OnceLock::new();

/// How long the closed window gets to let go of its memory.
const SETTLE: Duration = Duration::from_secs(8);

fn report_file() -> Option<PathBuf> {
    std::env::var_os("STACKYARD_PERF_REPORT").map(PathBuf::from)
}

/// Called first thing in `run`.
pub fn started() {
    STARTED.get_or_init(Instant::now);
}

fn append(file: &std::path::Path, line: &str) {
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(file)
    {
        let _ = writeln!(f, "{line}");
    }
}

/// The main window's page finished loading.
pub fn page_loaded<R: Runtime>(webview: &Webview<R>) {
    let Some(file) = report_file() else {
        return;
    };
    if REPORTED.set(()).is_err() {
        return;
    }
    let ms = STARTED.get().map_or(0, |at| at.elapsed().as_millis());
    append(&file, &format!("ready_ms={ms}"));
    let app = webview.app_handle().clone();
    std::thread::spawn(move || {
        // Let the screen's first data arrive, as it would for a person.
        std::thread::sleep(Duration::from_secs(2));
        let closer = app.clone();
        let _ = app.run_on_main_thread(move || {
            if let Some(window) = closer.get_webview_window("main") {
                let _ = window.destroy();
            }
        });
        std::thread::sleep(SETTLE);
        append(&file, "tray=1");
    });
}
