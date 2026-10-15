//! Runs tasks: once a minute it starts the ones that are due, and "Run now"
//! starts one straight away. A task still running when it is due again is
//! not started a second time.

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use super::{clock, list, log_path, Outcome, RunRecord, Schedule, States, Task};
use crate::settings::Settings;
use crate::shell::shims;
use crate::{Error, Result};

const STATE: &str = "data/scheduler/state.json";
/// A task's log starts over (keeping one older file) past this size.
const MAX_LOG_BYTES: u64 = 1024 * 1024;
/// How long a stopped task gets to finish before it is killed.
const STOP_GRACE: Duration = Duration::from_secs(5);
const POLL: Duration = Duration::from_millis(100);

pub struct Scheduler {
    inner: Arc<Inner>,
}

struct Inner {
    home: PathBuf,
    quitting: Mutex<bool>,
    wake: Condvar,
    running: Mutex<HashMap<String, Running>>,
    /// Signalled whenever a run ends.
    ended: Condvar,
}

#[derive(Default)]
struct Running {
    /// 0 until the process has started.
    pid: u32,
    stop_requested: Option<Instant>,
}

impl Scheduler {
    /// Starts the once-a-minute clock. Tasks only run while the scheduler
    /// module is on, which is checked each minute.
    pub fn start(home: &Path) -> Self {
        let scheduler = Self::idle(home);
        let inner = scheduler.inner.clone();
        let _ = std::thread::Builder::new()
            .name("scheduler".into())
            .spawn(move || inner.clock());
        scheduler
    }

    /// A scheduler without its clock, for "Run now" and tests.
    pub(crate) fn idle(home: &Path) -> Self {
        settle_interrupted(home);
        Self {
            inner: Arc::new(Inner {
                home: home.to_owned(),
                quitting: Mutex::new(false),
                wake: Condvar::new(),
                running: Mutex::new(HashMap::new()),
                ended: Condvar::new(),
            }),
        }
    }

    /// Runs a task now, whatever its schedule says and even when it is
    /// switched off; its output goes to its log as usual.
    pub fn run_now(&self, id: &str) -> Result<()> {
        let task = list(&self.inner.home)?
            .into_iter()
            .find(|t| t.id == id)
            .ok_or_else(|| Error::Task(format!("there is no task {id}")))?;
        self.inner.launch(task, true)
    }

    /// Stops a running task: a polite request first, then a kill after a
    /// few seconds.
    pub fn stop(&self, id: &str) -> Result<()> {
        let mut running = self.inner.lock_running();
        let run = running
            .get_mut(id)
            .ok_or_else(|| Error::Task("that task is not running".into()))?;
        run.stop_requested.get_or_insert_with(Instant::now);
        Ok(())
    }

    /// The ids of the tasks running now.
    pub fn running(&self) -> Vec<String> {
        self.inner.lock_running().keys().cloned().collect()
    }

    /// Starts the tasks due at `at` (seconds since 1970, on a minute).
    #[cfg(test)]
    pub(crate) fn tick(&self, at: i64) {
        self.inner.tick(at);
    }

    /// Stops the clock and every running task, waiting for them to end.
    pub fn shutdown(&self) {
        *self
            .inner
            .quitting
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = true;
        self.inner.wake.notify_all();
        let mut running = self.inner.lock_running();
        for run in running.values_mut() {
            run.stop_requested.get_or_insert_with(Instant::now);
        }
        let deadline = Instant::now() + STOP_GRACE + Duration::from_secs(2);
        while !running.is_empty() && Instant::now() < deadline {
            running = self
                .inner
                .ended
                .wait_timeout(running, POLL)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
    }
}

impl Drop for Scheduler {
    fn drop(&mut self) {
        self.shutdown();
    }
}

impl Inner {
    fn lock_running(&self) -> MutexGuard<'_, HashMap<String, Running>> {
        self.running.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Wakes at the start of every minute and starts what is due. A minute
    /// skipped (the computer was asleep) is not caught up on later.
    fn clock(self: Arc<Self>) {
        let mut minute = now_ms() / 60_000;
        loop {
            let wait = ((minute + 1) * 60_000).saturating_sub(now_ms()) + 50;
            let quitting = self.quitting.lock().unwrap_or_else(|e| e.into_inner());
            let (quitting, _) = self
                .wake
                .wait_timeout_while(quitting, Duration::from_millis(wait.min(60_000)), |q| !*q)
                .unwrap_or_else(|e| e.into_inner());
            if *quitting {
                return;
            }
            drop(quitting);
            let now = now_ms() / 60_000;
            if now > minute {
                self.tick(now as i64 * 60);
            }
            // Also follows the clock when it is set back.
            minute = now;
        }
    }

    fn tick(self: &Arc<Self>, at: i64) {
        let settings = Settings::load_from(&self.home.join("settings.toml")).unwrap_or_default();
        if !settings.module_enabled("scheduler") {
            return;
        }
        let moment = clock::local(at);
        for task in list(&self.home).unwrap_or_default() {
            let due = task.enabled
                && Schedule::parse(&task.schedule).is_ok_and(|s| s.matches(&moment))
                && task.folder.is_dir();
            if due {
                // Still running from last time: skip this one.
                let _ = self.launch(task, false);
            }
        }
    }

    fn launch(self: &Arc<Self>, task: Task, by_hand: bool) -> Result<()> {
        {
            let mut running = self.lock_running();
            if running.contains_key(&task.id) {
                return Err(Error::Task(format!("{} is still running", task.name)));
            }
            running.insert(task.id.clone(), Running::default());
        }
        let started = now_ms();
        update_state(&self.home, &task.id, |s| s.running_since = Some(started));
        let inner = self.clone();
        let id = task.id.clone();
        let spawned = std::thread::Builder::new()
            .name(format!("task {}", task.id))
            .spawn(move || inner.run(task, by_hand, started));
        if let Err(e) = spawned {
            self.lock_running().remove(&id);
            update_state(&self.home, &id, |s| s.running_since = None);
            return Err(Error::Task(format!("could not start the task: {e}")));
        }
        Ok(())
    }

    fn run(&self, task: Task, by_hand: bool, started: u64) {
        let log = open_log(&self.home, &task, by_hand);
        let result = match log {
            Ok(log) => spawn(&self.home, &task, log),
            Err(e) => Err(e),
        };
        let (outcome, exit_code, message) = match result {
            Ok(child) => {
                let (status, stopped) = self.wait(&task.id, child);
                let code = status.and_then(|s| s.code());
                let outcome = match status {
                    _ if stopped => Outcome::Stopped,
                    Some(s) if s.success() => Outcome::Succeeded,
                    _ => Outcome::Failed,
                };
                (outcome, code, None)
            }
            Err(e) => (Outcome::CouldNotStart, None, Some(e.to_string())),
        };
        let took_ms = now_ms().saturating_sub(started);
        let ending = match (outcome, exit_code, &message) {
            (Outcome::CouldNotStart, _, Some(m)) => format!("could not start: {m}"),
            (Outcome::Stopped, _, _) => "stopped".to_string(),
            (_, Some(code), _) => format!("finished with exit code {code}"),
            _ => "ended without an exit code".to_string(),
        };
        if let Ok(mut log) = OpenOptions::new()
            .append(true)
            .open(log_path(&self.home, &task.id))
        {
            let _ = writeln!(log, "--- {ending} after {} ---", seconds(took_ms));
        }
        let record = RunRecord {
            started,
            took_ms,
            outcome,
            exit_code,
            message,
            by_hand,
        };
        update_state(&self.home, &task.id, |s| {
            s.running_since = None;
            s.last = Some(record);
        });
        self.lock_running().remove(&task.id);
        self.ended.notify_all();
    }

    /// Waits for a task's process, stopping it when asked. Returns how it
    /// ended and whether it was stopped.
    fn wait(&self, id: &str, mut child: Child) -> (Option<ExitStatus>, bool) {
        let pid = child.id();
        if let Some(run) = self.lock_running().get_mut(id) {
            run.pid = pid;
        }
        let mut asked = false;
        let mut killed = false;
        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    let stopped = self
                        .lock_running()
                        .get(id)
                        .is_some_and(|r| r.stop_requested.is_some());
                    return (Some(status), stopped);
                }
                Ok(None) => {}
                Err(_) => {
                    let _ = child.kill();
                    return (child.wait().ok(), true);
                }
            }
            let requested = self.lock_running().get(id).and_then(|r| r.stop_requested);
            if let Some(at) = requested {
                if !asked {
                    ask_to_stop(pid);
                    asked = true;
                } else if !killed && at.elapsed() >= STOP_GRACE {
                    kill_tree(pid, &mut child);
                    killed = true;
                }
            }
            std::thread::sleep(POLL);
        }
    }
}

/// Opens the task's log for this run, starting it over when it got large,
/// and writes a line saying the run began.
fn open_log(home: &Path, task: &Task, by_hand: bool) -> Result<File> {
    let path = log_path(home, &task.id);
    let io = |source| Error::Io {
        path: path.clone(),
        source,
    };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(io)?;
    }
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > MAX_LOG_BYTES) {
        let _ = std::fs::rename(&path, path.with_extension("log.1"));
    }
    let mut log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(io)?;
    let m = clock::local(clock::now());
    let how = if by_hand { "by hand" } else { "on schedule" };
    writeln!(
        log,
        "--- {}-{:02}-{:02} {:02}:{:02} {} started {how}: {} ---",
        m.year, m.month, m.day, m.hour, m.minute, task.name, task.command
    )
    .map_err(io)?;
    Ok(log)
}

/// The command a task runs: the system shell running it in the task's
/// folder, with Stackyard's terminal commands first on `PATH`.
pub fn command(home: &Path, task: &Task) -> Command {
    #[cfg(unix)]
    let mut command = {
        let mut command = Command::new("/bin/sh");
        command.arg("-c").arg(&task.command);
        command
    };
    #[cfg(windows)]
    let mut command = {
        use std::os::windows::process::CommandExt;
        let shell = std::env::var_os("ComSpec").unwrap_or_else(|| "cmd.exe".into());
        let mut command = Command::new(shell);
        // `/S` with the whole command in quotes: cmd takes it as typed.
        command.raw_arg(format!("/D /S /C \"{}\"", task.command));
        command
    };
    let bin = shims::bin_dir(home);
    let rest = std::env::var_os("PATH").unwrap_or_default();
    let mut parts: Vec<PathBuf> = Vec::new();
    if bin.is_dir() {
        parts.push(bin.clone());
    }
    parts.extend(std::env::split_paths(&rest).filter(|p| *p != bin));
    let path = std::env::join_paths(parts).unwrap_or(rest);
    command
        .current_dir(&task.folder)
        .env("PATH", path)
        .env("STACKYARD_HOME", home);
    command
}

/// Starts a task in the background with its output going to `log`.
fn spawn(home: &Path, task: &Task, log: File) -> Result<Child> {
    let io = |source| Error::Io {
        path: task.folder.clone(),
        source,
    };
    let mut command = command(home, task);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Its own process group, so stopping it also stops what it started.
        command.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
        .stdin(Stdio::null())
        .stdout(log.try_clone().map_err(io)?)
        .stderr(log);
    command.spawn().map_err(io)
}

#[cfg(unix)]
fn ask_to_stop(pid: u32) {
    // SAFETY: kill(2) with a negative pid signals the task's process group,
    // whose leader has not been waited for yet, so the group is still ours.
    unsafe { libc::kill(-(pid as i32), libc::SIGTERM) };
}

#[cfg(unix)]
fn kill_tree(pid: u32, _child: &mut Child) {
    // SAFETY: as above.
    unsafe { libc::kill(-(pid as i32), libc::SIGKILL) };
}

/// Windows console programs have no polite stop; end the whole tree.
#[cfg(windows)]
fn ask_to_stop(pid: u32) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let _ = Command::new("taskkill")
        .args(["/T", "/F", "/PID", &pid.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .status();
}

#[cfg(windows)]
fn kill_tree(_pid: u32, child: &mut Child) {
    let _ = child.kill();
}

/// Run history for every task.
pub(super) fn states(home: &Path) -> States {
    std::fs::read_to_string(home.join(STATE))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// Drops a removed task's history.
pub(super) fn forget(home: &Path, id: &str) {
    let _guard = state_lock();
    let mut all = states(home);
    if all.remove(id).is_some() {
        write_states(home, &all);
    }
}

/// Runs a Stackyard that stopped had going are recorded as stopped.
fn settle_interrupted(home: &Path) {
    let _guard = state_lock();
    let mut all = states(home);
    let mut changed = false;
    for state in all.values_mut() {
        if let Some(started) = state.running_since.take() {
            state.last = Some(RunRecord {
                started,
                took_ms: 0,
                outcome: Outcome::Stopped,
                exit_code: None,
                message: Some("Stackyard quit while it ran".into()),
                by_hand: false,
            });
            changed = true;
        }
    }
    if changed {
        write_states(home, &all);
    }
}

fn update_state(home: &Path, id: &str, change: impl FnOnce(&mut super::TaskState)) {
    let _guard = state_lock();
    let mut all = states(home);
    change(all.entry(id.to_string()).or_default());
    write_states(home, &all);
}

fn write_states(home: &Path, all: &States) {
    let path = home.join(STATE);
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let Ok(text) = serde_json::to_string_pretty(all) else {
        return;
    };
    let tmp = path.with_extension("json.tmp");
    if std::fs::write(&tmp, text).is_ok() {
        let _ = std::fs::rename(&tmp, &path);
    }
}

fn state_lock() -> MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

fn seconds(ms: u64) -> String {
    if ms < 60_000 {
        format!("{:.1} s", ms as f64 / 1000.0)
    } else {
        format!("{} min {} s", ms / 60_000, ms % 60_000 / 1000)
    }
}
