//! The task scheduler: commands that run on a schedule in a project folder,
//! such as Laravel's `php artisan schedule:run` every minute or a nightly
//! backup script.
//!
//! Tasks are kept in `scheduler.toml`. They run while Stackyard runs (the
//! app or the background service), each in its folder with Stackyard's
//! terminal commands first on `PATH`, so `php` and `node` are the versions
//! that project uses. Each task's output goes to `logs/task-<id>.log`, and
//! how its last run went to `data/scheduler/state.json`.

pub mod clock;
pub mod cron;
mod runner;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::settings;
use crate::{Error, Result};

pub use cron::Schedule;
pub use runner::{command, Scheduler};

const FILE: &str = "scheduler.toml";
const MAX_NAME: usize = 80;
const MAX_COMMAND: usize = 4000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Task {
    /// Eight letters and digits, made when the task is added.
    pub id: String,
    pub name: String,
    /// Run by the system shell (`sh` or `cmd`), so `&&` and `>` work.
    pub command: String,
    pub folder: PathBuf,
    pub schedule: String,
    #[serde(default = "yes")]
    pub enabled: bool,
}

fn yes() -> bool {
    true
}

/// What a person fills in to add or change a task.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct TaskInput {
    pub name: String,
    pub command: String,
    pub folder: PathBuf,
    pub schedule: String,
    #[serde(default = "yes")]
    pub enabled: bool,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct TaskFile {
    #[serde(default, rename = "task")]
    tasks: Vec<Task>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Outcome {
    Succeeded,
    Failed,
    /// Stopped by hand, or because Stackyard quit.
    Stopped,
    CouldNotStart,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunRecord {
    /// Milliseconds since 1970.
    pub started: u64,
    pub took_ms: u64,
    pub outcome: Outcome,
    pub exit_code: Option<i32>,
    /// Why it could not start, when it could not.
    #[serde(default)]
    pub message: Option<String>,
    /// Started with "Run now" rather than by its schedule.
    #[serde(default)]
    pub by_hand: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct TaskState {
    #[serde(default)]
    last: Option<RunRecord>,
    /// When the run going on now started, in milliseconds.
    #[serde(default)]
    running_since: Option<u64>,
}

/// A task with what the screens show about it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TaskView {
    #[serde(flatten)]
    pub task: Task,
    /// e.g. "Every day at 02:30"; `None` for schedules shown as written.
    pub description: Option<String>,
    /// Milliseconds since 1970; `None` when switched off or never due.
    pub next_run: Option<u64>,
    pub last: Option<RunRecord>,
    pub running_since: Option<u64>,
    /// Something that stops it from running, e.g. a folder that is gone.
    pub problem: Option<String>,
}

/// Every task, in the order they were added.
pub fn list(home: &Path) -> Result<Vec<Task>> {
    Ok(settings::read_toml::<TaskFile>(&home.join(FILE))?
        .unwrap_or_default()
        .tasks)
}

/// A task by its id or, failing that, its name (any case).
pub fn find(home: &Path, key: &str) -> Result<Task> {
    let tasks = list(home)?;
    tasks
        .iter()
        .find(|t| t.id == key)
        .or_else(|| tasks.iter().find(|t| t.name.eq_ignore_ascii_case(key)))
        .cloned()
        .ok_or_else(|| Error::Task(format!("there is no task called {key}")))
}

pub fn add(home: &Path, input: TaskInput) -> Result<Task> {
    let _guard = lock();
    let input = validate(input)?;
    let mut file = load(home)?;
    if file
        .tasks
        .iter()
        .any(|t| t.name.eq_ignore_ascii_case(&input.name))
    {
        return Err(Error::Task(format!(
            "a task called {} already exists",
            input.name
        )));
    }
    let task = Task {
        id: new_id()?,
        name: input.name,
        command: input.command,
        folder: input.folder,
        schedule: input.schedule,
        enabled: input.enabled,
    };
    file.tasks.push(task.clone());
    save(home, &file)?;
    Ok(task)
}

pub fn update(home: &Path, id: &str, input: TaskInput) -> Result<Task> {
    let _guard = lock();
    let input = validate(input)?;
    let mut file = load(home)?;
    let clash = file
        .tasks
        .iter()
        .any(|t| t.id != id && t.name.eq_ignore_ascii_case(&input.name));
    if clash {
        return Err(Error::Task(format!(
            "a task called {} already exists",
            input.name
        )));
    }
    let task = file
        .tasks
        .iter_mut()
        .find(|t| t.id == id)
        .ok_or_else(|| Error::Task(format!("there is no task {id}")))?;
    task.name = input.name;
    task.command = input.command;
    task.folder = input.folder;
    task.schedule = input.schedule;
    task.enabled = input.enabled;
    let task = task.clone();
    save(home, &file)?;
    Ok(task)
}

pub fn set_enabled(home: &Path, id: &str, enabled: bool) -> Result<Task> {
    let _guard = lock();
    let mut file = load(home)?;
    let task = file
        .tasks
        .iter_mut()
        .find(|t| t.id == id)
        .ok_or_else(|| Error::Task(format!("there is no task {id}")))?;
    task.enabled = enabled;
    let task = task.clone();
    save(home, &file)?;
    Ok(task)
}

/// Removes a task with its log and run history.
pub fn remove(home: &Path, id: &str) -> Result<()> {
    let _guard = lock();
    let mut file = load(home)?;
    let before = file.tasks.len();
    file.tasks.retain(|t| t.id != id);
    if file.tasks.len() == before {
        return Err(Error::Task(format!("there is no task {id}")));
    }
    save(home, &file)?;
    let log = log_path(home, id);
    let _ = std::fs::remove_file(&log);
    let _ = std::fs::remove_file(log.with_extension("log.1"));
    runner::forget(home, id);
    Ok(())
}

/// The tasks with their next and last runs, for the screens.
pub fn views(home: &Path) -> Result<Vec<TaskView>> {
    let states = runner::states(home);
    let now = clock::now();
    Ok(list(home)?
        .into_iter()
        .map(|task| {
            let state = states.get(&task.id).cloned().unwrap_or_default();
            let schedule = Schedule::parse(&task.schedule);
            let problem = match &schedule {
                Err(e) => Some(format!("The schedule is not valid: {e}")),
                Ok(_) if !task.folder.is_dir() => Some(format!(
                    "The folder {} is not there any more",
                    task.folder.display()
                )),
                Ok(_) => None,
            };
            let next_run = match (&schedule, task.enabled && problem.is_none()) {
                (Ok(s), true) => s
                    .next_runs(now, 1, clock::local)
                    .first()
                    .map(|at| *at as u64 * 1000),
                _ => None,
            };
            TaskView {
                description: cron::describe(&task.schedule),
                next_run,
                last: state.last,
                running_since: state.running_since,
                problem,
                task,
            }
        })
        .collect())
}

/// The next few times a schedule runs, in milliseconds, for previews while
/// typing one.
pub fn preview(schedule: &str, count: usize) -> Result<Vec<u64>> {
    let schedule = Schedule::parse(schedule).map_err(Error::Task)?;
    Ok(schedule
        .next_runs(clock::now(), count.min(20), clock::local)
        .into_iter()
        .map(|at| at as u64 * 1000)
        .collect())
}

/// Where a task's output goes.
pub fn log_path(home: &Path, id: &str) -> PathBuf {
    home.join("logs").join(format!("task-{id}.log"))
}

/// Whether `id` looks like a task id (and so is safe in a file name).
pub fn is_id(id: &str) -> bool {
    id.len() == 8
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
}

fn validate(mut input: TaskInput) -> Result<TaskInput> {
    input.name = input.name.trim().to_string();
    input.command = input.command.trim().to_string();
    input.schedule = input
        .schedule
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let fail = |m: &str| Err(Error::Task(m.to_string()));
    if input.name.is_empty() {
        return fail("give the task a name");
    }
    if input.name.chars().count() > MAX_NAME {
        return fail("the name is too long; keep it under 80 characters");
    }
    if input.command.is_empty() {
        return fail("enter the command to run");
    }
    if input.command.len() > MAX_COMMAND {
        return fail("the command is too long");
    }
    if input.command.contains(['\n', '\r', '\0']) {
        return fail("the command must be on one line; put longer scripts in a file and run that");
    }
    if !input.folder.is_absolute() {
        return fail("the folder must be a full path, e.g. /Users/you/Code/app");
    }
    if !input.folder.is_dir() {
        return Err(Error::Task(format!(
            "the folder {} does not exist",
            input.folder.display()
        )));
    }
    Schedule::parse(&input.schedule).map_err(Error::Task)?;
    Ok(input)
}

fn load(home: &Path) -> Result<TaskFile> {
    Ok(settings::read_toml(&home.join(FILE))?.unwrap_or_default())
}

fn save(home: &Path, file: &TaskFile) -> Result<()> {
    settings::write_toml(&home.join(FILE), file)
}

/// Changes to the task list happen one at a time within this process.
fn lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

fn new_id() -> Result<String> {
    const ALPHABET: &[u8] = b"abcdefghijkmnpqrstuvwxyz23456789";
    let mut bytes = [0u8; 8];
    getrandom::fill(&mut bytes).map_err(|e| Error::Task(format!("no randomness: {e}")))?;
    Ok(bytes
        .iter()
        .map(|b| ALPHABET[usize::from(*b) % ALPHABET.len()] as char)
        .collect())
}

/// Run history, by task id.
type States = BTreeMap<String, TaskState>;

#[cfg(test)]
mod tests;
