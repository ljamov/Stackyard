use std::time::{Duration, Instant};

use super::*;

fn input(name: &str, command: &str, folder: &Path, schedule: &str) -> TaskInput {
    TaskInput {
        name: name.into(),
        command: command.into(),
        folder: folder.to_owned(),
        schedule: schedule.into(),
        enabled: true,
    }
}

/// Waits until `id` has a finished run.
fn finished(home: &Path, id: &str) -> RunRecord {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let view = views(home)
            .unwrap()
            .into_iter()
            .find(|v| v.task.id == id)
            .unwrap();
        if let (Some(last), None) = (&view.last, view.running_since) {
            return last.clone();
        }
        assert!(
            Instant::now() < deadline,
            "the task did not finish: {view:?}"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn log(home: &Path, id: &str) -> String {
    std::fs::read_to_string(log_path(home, id)).unwrap_or_default()
}

#[test]
fn adds_changes_and_removes_tasks() {
    let home = crate::testutil::tempdir();
    let project = home.join("app");
    std::fs::create_dir_all(&project).unwrap();
    assert!(list(&home).unwrap().is_empty());

    let task = add(
        &home,
        input(
            "  Queue ",
            "php artisan queue:work --once",
            &project,
            " */5  * * * * ",
        ),
    )
    .unwrap();
    assert!(is_id(&task.id), "{}", task.id);
    assert_eq!(task.name, "Queue");
    assert_eq!(task.schedule, "*/5 * * * *");
    assert_eq!(find(&home, "queue").unwrap(), task);
    assert_eq!(find(&home, &task.id).unwrap(), task);

    let err = |e: Error| e.to_string();
    let dup = add(&home, input("QUEUE", "x", &project, "@daily")).unwrap_err();
    assert!(err(dup).contains("already exists"));
    let bad = add(&home, input("b", "x", &project, "61 * * * *")).unwrap_err();
    assert_eq!(err(bad), "minute: 61 is outside 0-59");
    let gone = add(&home, input("c", "x", &home.join("nope"), "@daily")).unwrap_err();
    assert!(err(gone).contains("does not exist"));
    let relative = add(&home, input("d", "x", Path::new("app"), "@daily")).unwrap_err();
    assert!(err(relative).contains("full path"));
    let lines = add(&home, input("e", "a\nb", &project, "@daily")).unwrap_err();
    assert!(err(lines).contains("one line"));
    assert!(add(&home, input(" ", "x", &project, "@daily")).is_err());

    let changed = update(&home, &task.id, input("Queue", "true", &project, "@hourly")).unwrap();
    assert_eq!(changed.id, task.id);
    assert_eq!(changed.command, "true");
    let off = set_enabled(&home, &task.id, false).unwrap();
    assert!(!off.enabled);

    let view = &views(&home).unwrap()[0];
    assert_eq!(view.description.as_deref(), Some("Every hour, on the hour"));
    assert_eq!(view.next_run, None, "switched off");
    assert_eq!(view.problem, None);
    set_enabled(&home, &task.id, true).unwrap();
    let next = views(&home).unwrap()[0].next_run.unwrap();
    assert_eq!(next % 60_000, 0);

    std::fs::remove_dir(&project).unwrap();
    let view = &views(&home).unwrap()[0];
    assert!(view.problem.as_deref().unwrap().contains("not there"));
    assert_eq!(view.next_run, None);

    std::fs::create_dir_all(log_path(&home, &task.id).parent().unwrap()).unwrap();
    std::fs::write(log_path(&home, &task.id), "old output\n").unwrap();
    remove(&home, &task.id).unwrap();
    assert!(list(&home).unwrap().is_empty());
    assert!(!log_path(&home, &task.id).exists());
    assert!(remove(&home, &task.id).is_err());
}

#[test]
fn previews_upcoming_runs() {
    let runs = preview("*/10 * * * *", 3).unwrap();
    assert_eq!(runs.len(), 3);
    assert_eq!(runs[1] - runs[0], 600_000);
    assert!(preview("nope", 3).is_err());
}

#[test]
fn runs_in_the_folder_and_records_how_it_went() {
    let home = crate::testutil::tempdir();
    let project = home.join("my app");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(project.join("marker.txt"), "here").unwrap();
    #[cfg(unix)]
    let (ok, fail) = (
        "cat marker.txt && echo \" and $STACKYARD_HOME\"",
        "echo oops >&2; exit 3",
    );
    #[cfg(windows)]
    let (ok, fail) = (
        "type marker.txt && echo  and %STACKYARD_HOME%",
        "echo oops 1>&2 & exit 3",
    );
    let good = add(&home, input("Good", ok, &project, "@daily")).unwrap();
    let bad = add(&home, input("Bad", fail, &project, "@daily")).unwrap();

    let scheduler = Scheduler::idle(&home);
    scheduler.run_now(&good.id).unwrap();
    let record = finished(&home, &good.id);
    assert_eq!(record.outcome, Outcome::Succeeded);
    assert_eq!(record.exit_code, Some(0));
    assert!(record.by_hand);
    let out = log(&home, &good.id);
    assert!(out.contains("Good started by hand"), "{out}");
    assert!(out.contains("here"), "{out}");
    assert!(out.contains(&home.display().to_string()), "{out}");
    assert!(out.contains("finished with exit code 0"), "{out}");

    scheduler.run_now(&bad.id).unwrap();
    let record = finished(&home, &bad.id);
    assert_eq!(record.outcome, Outcome::Failed);
    assert_eq!(record.exit_code, Some(3));
    assert!(log(&home, &bad.id).contains("oops"));
    assert!(scheduler.run_now("zzzzzzzz").is_err());
}

#[test]
fn a_running_task_is_not_started_twice_and_can_be_stopped() {
    let home = crate::testutil::tempdir();
    #[cfg(unix)]
    let slow = "echo started; sleep 30";
    #[cfg(windows)]
    let slow = "echo started & ping -n 30 127.0.0.1 >NUL";
    let task = add(&home, input("Slow", slow, &home, "* * * * *")).unwrap();
    let scheduler = Scheduler::idle(&home);
    scheduler.run_now(&task.id).unwrap();
    let err = scheduler.run_now(&task.id).unwrap_err();
    assert!(err.to_string().contains("still running"), "{err}");
    assert_eq!(scheduler.running(), std::slice::from_ref(&task.id));
    assert!(views(&home).unwrap()[0].running_since.is_some());

    // Wait for it to really be going, then stop it.
    let deadline = Instant::now() + Duration::from_secs(10);
    // cmd's `echo started &` prints "started " with the space before `&`.
    while !log(&home, &task.id)
        .lines()
        .any(|line| line.trim() == "started")
    {
        assert!(Instant::now() < deadline, "{}", log(&home, &task.id));
        std::thread::sleep(Duration::from_millis(50));
    }
    let asked = Instant::now();
    scheduler.stop(&task.id).unwrap();
    let record = finished(&home, &task.id);
    assert_eq!(record.outcome, Outcome::Stopped);
    assert!(asked.elapsed() < Duration::from_secs(10));
    assert!(scheduler.running().is_empty());
    assert!(scheduler.stop(&task.id).is_err());
}

#[test]
fn the_clock_starts_due_tasks_only_while_the_module_is_on() {
    let home = crate::testutil::tempdir();
    let every = add(
        &home,
        input("Every minute", "echo tick", &home, "* * * * *"),
    )
    .unwrap();
    let never = add(&home, input("Leap", "echo leap", &home, "0 0 31 2 *")).unwrap();
    let off = add(&home, input("Off", "echo off", &home, "* * * * *")).unwrap();
    set_enabled(&home, &off.id, false).unwrap();
    let scheduler = Scheduler::idle(&home);

    scheduler.tick(clock::now() / 60 * 60);
    std::thread::sleep(Duration::from_millis(300));
    assert!(
        views(&home).unwrap().iter().all(|v| v.last.is_none()),
        "module is off"
    );

    let settings_path = home.join("settings.toml");
    let mut settings = Settings::load_from(&settings_path).unwrap();
    settings.set_module("scheduler", true).unwrap();
    settings.save_to(&settings_path).unwrap();
    scheduler.tick(clock::now() / 60 * 60);
    let record = finished(&home, &every.id);
    assert_eq!(record.outcome, Outcome::Succeeded);
    assert!(!record.by_hand);
    assert!(log(&home, &every.id).contains("started on schedule"));
    let views = views(&home).unwrap();
    assert!(views
        .iter()
        .find(|v| v.task.id == never.id)
        .unwrap()
        .last
        .is_none());
    assert!(views
        .iter()
        .find(|v| v.task.id == off.id)
        .unwrap()
        .last
        .is_none());
}

#[test]
fn runs_cut_short_by_quitting_are_recorded() {
    let home = crate::testutil::tempdir();
    let task = add(&home, input("Backup", "echo hi", &home, "@daily")).unwrap();
    runner_state_running(&home, &task.id);
    let _scheduler = Scheduler::idle(&home);
    let view = &views(&home).unwrap()[0];
    assert_eq!(view.running_since, None);
    let last = view.last.as_ref().unwrap();
    assert_eq!(last.outcome, Outcome::Stopped);
    assert!(last.message.as_deref().unwrap().contains("quit"));
}

/// Marks a task as running, as a Stackyard that was killed would leave it.
fn runner_state_running(home: &Path, id: &str) {
    let dir = home.join("data/scheduler");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("state.json"),
        format!(r#"{{"{id}": {{"running_since": 1792015509000}}}}"#),
    )
    .unwrap();
}

use crate::settings::Settings;
