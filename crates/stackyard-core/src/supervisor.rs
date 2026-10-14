//! Starts, stops and watches service processes.
//!
//! One background thread polls every managed process a few times a second.
//! A process that exits on its own is restarted with growing delays if its
//! spec asks for that; otherwise it is marked as crashed. Dropping the
//! supervisor stops everything it started, so quitting Stackyard never
//! leaves services running behind the user's back.

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;

use crate::{ports, Error, Result};

const POLL_INTERVAL: Duration = Duration::from_millis(250);
const STOP_GRACE: Duration = Duration::from_secs(5);
const MAX_RESTARTS: u32 = 5;
/// A process that stayed up this long gets a fresh set of restart attempts.
const STABLE_AFTER: Duration = Duration::from_secs(60);
const MAX_LOG_BYTES: u64 = 5 * 1024 * 1024;
const TAIL_WINDOW: u64 = 64 * 1024;

#[derive(Debug, Clone)]
pub struct ProcessSpec {
    /// Stable id, e.g. `mariadb`. Also names the log file.
    pub id: String,
    /// Shown in messages, e.g. `MariaDB`.
    pub name: String,
    pub program: PathBuf,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub cwd: Option<PathBuf>,
    /// Ports that must be free before starting.
    pub ports: Vec<u16>,
    pub restart_on_crash: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "lowercase")]
pub enum ProcessState {
    Stopped,
    Running { pid: u32 },
    Restarting { attempt: u32 },
    Crashed { code: Option<i32> },
}

struct Entry {
    spec: ProcessSpec,
    child: Option<Child>,
    state: ProcessState,
    restarts: u32,
    started_at: Instant,
    restart_at: Option<Instant>,
}

struct Shared {
    entries: Mutex<HashMap<String, Entry>>,
    log_dir: PathBuf,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, HashMap<String, Entry>> {
        // A panic while holding the lock leaves the map usable; keep going.
        self.entries.lock().unwrap_or_else(|e| e.into_inner())
    }
}

pub struct Supervisor {
    shared: Arc<Shared>,
}

impl Supervisor {
    pub fn new(log_dir: impl Into<PathBuf>) -> Self {
        let shared = Arc::new(Shared {
            entries: Mutex::new(HashMap::new()),
            log_dir: log_dir.into(),
        });
        let weak = Arc::downgrade(&shared);
        std::thread::Builder::new()
            .name("stackyard-supervisor".into())
            .spawn(move || watch(weak))
            .expect("could not start the supervisor thread");
        Self { shared }
    }

    /// Starts a process, or returns the pid if it is already running.
    pub fn start(&self, spec: ProcessSpec) -> Result<u32> {
        let mut entries = self.shared.lock();
        if let Some(Entry {
            state: ProcessState::Running { pid },
            ..
        }) = entries.get(&spec.id)
        {
            return Ok(*pid);
        }
        if let Some(port) = spec.ports.iter().copied().find(|p| !ports::is_free(*p)) {
            return Err(Error::PortInUse {
                name: spec.name.clone(),
                port,
            });
        }
        let child = spawn(&spec, &self.shared.log_dir).map_err(|source| Error::Spawn {
            program: spec.program.display().to_string(),
            source,
        })?;
        let pid = child.id();
        entries.insert(
            spec.id.clone(),
            Entry {
                spec,
                child: Some(child),
                state: ProcessState::Running { pid },
                restarts: 0,
                started_at: Instant::now(),
                restart_at: None,
            },
        );
        Ok(pid)
    }

    /// Stops a process: a polite request first, then a hard kill after a grace period.
    pub fn stop(&self, id: &str) -> Result<()> {
        let child = {
            let mut entries = self.shared.lock();
            let entry = entries
                .get_mut(id)
                .ok_or_else(|| Error::UnknownProcess(id.to_string()))?;
            entry.state = ProcessState::Stopped;
            entry.restart_at = None;
            entry.child.take()
        };
        if let Some(child) = child {
            terminate(child);
        }
        Ok(())
    }

    pub fn stop_all(&self) {
        let children: Vec<Child> = {
            let mut entries = self.shared.lock();
            entries
                .values_mut()
                .filter_map(|entry| {
                    entry.state = ProcessState::Stopped;
                    entry.restart_at = None;
                    entry.child.take()
                })
                .collect()
        };
        for child in children {
            terminate(child);
        }
    }

    pub fn state(&self, id: &str) -> ProcessState {
        self.shared
            .lock()
            .get(id)
            .map_or(ProcessState::Stopped, |e| e.state.clone())
    }

    pub fn states(&self) -> Vec<(String, ProcessState)> {
        let mut states: Vec<_> = self
            .shared
            .lock()
            .iter()
            .map(|(id, e)| (id.clone(), e.state.clone()))
            .collect();
        states.sort_by(|a, b| a.0.cmp(&b.0));
        states
    }

    pub fn log_path(&self, id: &str) -> PathBuf {
        log_path(&self.shared.log_dir, id)
    }

    /// The last `lines` lines of a process's log (empty if it never ran).
    pub fn tail_log(&self, id: &str, lines: usize) -> Result<Vec<String>> {
        let path = self.log_path(id);
        let io = |source| Error::Io {
            path: path.clone(),
            source,
        };
        let mut file = match File::open(&path) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(io(e)),
        };
        let len = file.metadata().map_err(io)?.len();
        file.seek(SeekFrom::Start(len.saturating_sub(TAIL_WINDOW)))
            .map_err(io)?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).map_err(io)?;
        let text = String::from_utf8_lossy(&bytes);
        let all: Vec<&str> = text.lines().collect();
        let skip = all.len().saturating_sub(lines);
        Ok(all[skip..].iter().map(|l| l.to_string()).collect())
    }
}

impl Drop for Supervisor {
    fn drop(&mut self) {
        self.stop_all();
    }
}

fn watch(shared: Weak<Shared>) {
    loop {
        std::thread::sleep(POLL_INTERVAL);
        let Some(shared) = shared.upgrade() else {
            return;
        };
        let now = Instant::now();
        let mut entries = shared.lock();
        for entry in entries.values_mut() {
            check_exit(entry, now);
            if entry.restart_at.is_some_and(|at| now >= at) {
                entry.restart_at = None;
                entry.restarts += 1;
                match spawn(&entry.spec, &shared.log_dir) {
                    Ok(child) => {
                        entry.state = ProcessState::Running { pid: child.id() };
                        entry.started_at = now;
                        entry.child = Some(child);
                    }
                    Err(_) => entry.state = ProcessState::Crashed { code: None },
                }
            }
        }
    }
}

/// Notices a process that exited on its own and decides what happens next.
fn check_exit(entry: &mut Entry, now: Instant) {
    let Some(child) = entry.child.as_mut() else {
        return;
    };
    let code = match child.try_wait() {
        Ok(Some(status)) => status.code(),
        Ok(None) => return,
        Err(_) => None,
    };
    entry.child = None;
    if now.duration_since(entry.started_at) >= STABLE_AFTER {
        entry.restarts = 0;
    }
    if entry.spec.restart_on_crash && entry.restarts < MAX_RESTARTS {
        entry.state = ProcessState::Restarting {
            attempt: entry.restarts + 1,
        };
        entry.restart_at = Some(now + backoff(entry.restarts));
    } else {
        entry.state = ProcessState::Crashed { code };
    }
}

/// 1 s, 2 s, 4 s, 8 s, 16 s.
fn backoff(restarts: u32) -> Duration {
    Duration::from_secs(1u64 << restarts.min(4))
}

fn log_path(dir: &Path, id: &str) -> PathBuf {
    dir.join(format!("{id}.log"))
}

fn spawn(spec: &ProcessSpec, log_dir: &Path) -> std::io::Result<Child> {
    std::fs::create_dir_all(log_dir)?;
    let path = log_path(log_dir, &spec.id);
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > MAX_LOG_BYTES) {
        std::fs::rename(&path, path.with_extension("log.1"))?;
    }
    let mut log = OpenOptions::new().create(true).append(true).open(&path)?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    writeln!(
        log,
        "--- Stackyard started {} (unix time {stamp}) ---",
        spec.name
    )?;

    let mut command = Command::new(&spec.program);
    command
        .args(&spec.args)
        .envs(spec.env.iter().map(|(k, v)| (k, v)))
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log);
    if let Some(cwd) = &spec.cwd {
        command.current_dir(cwd);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Own process group, so stopping it also stops its worker processes.
        command.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command.spawn()
}

#[cfg(unix)]
fn terminate(mut child: Child) {
    let group = -(child.id() as i32);
    // SAFETY: kill(2) with a negative pid signals the process group we created.
    unsafe { libc::kill(group, libc::SIGTERM) };
    let deadline = Instant::now() + STOP_GRACE;
    while Instant::now() < deadline {
        if let Ok(Some(_)) = child.try_wait() {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    // SAFETY: as above; harmless if the group is already gone.
    unsafe { libc::kill(group, libc::SIGKILL) };
    let _ = child.wait();
}

#[cfg(not(unix))]
fn terminate(mut child: Child) {
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A process that prints a line, then waits about 30 seconds.
    fn sleeper(id: &str) -> ProcessSpec {
        #[cfg(unix)]
        let (program, args) = ("sh", vec!["-c", "echo hello; sleep 30"]);
        #[cfg(windows)]
        let (program, args) = ("cmd", vec!["/C", "echo hello & ping -n 30 127.0.0.1 >NUL"]);
        spec(id, program, &args)
    }

    /// A process that prints a line and exits with code 3.
    fn crasher(id: &str) -> ProcessSpec {
        #[cfg(unix)]
        let (program, args) = ("sh", vec!["-c", "echo boom; exit 3"]);
        #[cfg(windows)]
        let (program, args) = ("cmd", vec!["/C", "echo boom & exit 3"]);
        spec(id, program, &args)
    }

    fn spec(id: &str, program: &str, args: &[&str]) -> ProcessSpec {
        ProcessSpec {
            id: id.into(),
            name: id.into(),
            program: program.into(),
            args: args.iter().map(|a| a.to_string()).collect(),
            env: Vec::new(),
            cwd: None,
            ports: Vec::new(),
            restart_on_crash: false,
        }
    }

    fn wait_for(mut done: impl FnMut() -> bool) -> bool {
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if done() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        false
    }

    #[test]
    fn starts_logs_and_stops() {
        let sup = Supervisor::new(crate::testutil::tempdir());
        let pid = sup.start(sleeper("svc")).unwrap();
        assert_eq!(sup.state("svc"), ProcessState::Running { pid });
        assert_eq!(
            sup.start(sleeper("svc")).unwrap(),
            pid,
            "second start is a no-op"
        );
        assert!(wait_for(|| sup
            .tail_log("svc", 5)
            .unwrap()
            .iter()
            .any(|l| l.trim() == "hello")));

        sup.stop("svc").unwrap();
        assert_eq!(sup.state("svc"), ProcessState::Stopped);
        assert!(matches!(sup.stop("nope"), Err(Error::UnknownProcess(_))));
    }

    #[test]
    fn marks_crashes() {
        let sup = Supervisor::new(crate::testutil::tempdir());
        sup.start(crasher("bad")).unwrap();
        assert!(wait_for(
            || sup.state("bad") == ProcessState::Crashed { code: Some(3) }
        ));
    }

    #[test]
    fn restarts_after_a_crash() {
        let sup = Supervisor::new(crate::testutil::tempdir());
        sup.start(ProcessSpec {
            restart_on_crash: true,
            ..crasher("flaky")
        })
        .unwrap();
        // First retry comes after 1 s, so a second "boom" proves a restart.
        assert!(wait_for(|| sup
            .tail_log("flaky", 50)
            .unwrap()
            .iter()
            .filter(|l| l.trim() == "boom")
            .count()
            >= 2));
        sup.stop("flaky").unwrap();
        assert_eq!(sup.state("flaky"), ProcessState::Stopped);
    }

    #[test]
    fn refuses_a_busy_port() {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let sup = Supervisor::new(crate::testutil::tempdir());
        let err = sup
            .start(ProcessSpec {
                ports: vec![port],
                ..sleeper("web")
            })
            .unwrap_err();
        assert!(matches!(err, Error::PortInUse { port: p, .. } if p == port));
    }

    #[test]
    fn backoff_grows_and_caps() {
        assert_eq!(backoff(0), Duration::from_secs(1));
        assert_eq!(backoff(3), Duration::from_secs(8));
        assert_eq!(backoff(9), Duration::from_secs(16));
    }
}
