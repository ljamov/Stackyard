import { useCallback, useEffect, useState, type FormEvent } from "react";
import { api, errorText, type SchedulePreview, type Site, type Task, type TaskInput, type TaskRun } from "../lib/api";

const PRESETS: { label: string; schedule: string }[] = [
  { label: "Every minute", schedule: "* * * * *" },
  { label: "Every 5 minutes", schedule: "*/5 * * * *" },
  { label: "Every 15 minutes", schedule: "*/15 * * * *" },
  { label: "Every hour", schedule: "0 * * * *" },
  { label: "Every day at 02:00", schedule: "0 2 * * *" },
  { label: "Every Monday at 09:00", schedule: "0 9 * * 1" },
];

const EMPTY: TaskInput = { name: "", command: "", folder: "", schedule: "* * * * *", enabled: true };

export function Scheduler() {
  const [tasks, setTasks] = useState<Task[] | null>(null);
  const [sites, setSites] = useState<Site[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [form, setForm] = useState<TaskInput>(EMPTY);
  const [editing, setEditing] = useState<string | null>(null);
  const [formError, setFormError] = useState<string | null>(null);
  const [preview, setPreview] = useState<SchedulePreview | null>(null);
  const [previewError, setPreviewError] = useState<string | null>(null);
  const [logFor, setLogFor] = useState<string | null>(null);
  const [log, setLog] = useState<string[]>([]);
  const [removing, setRemoving] = useState<string | null>(null);

  const reload = useCallback(() => {
    api
      .listTasks()
      .then((t) => {
        setTasks(t);
        setError(null);
      })
      .catch((e) => setError(errorText(e)));
  }, []);

  // Runs start and end on their own; keep what is shown current.
  useEffect(() => {
    reload();
    api.listSites().then(setSites).catch(() => {});
    const timer = setInterval(reload, 3000);
    return () => clearInterval(timer);
  }, [reload]);

  useEffect(() => {
    let current = true;
    api
      .previewSchedule(form.schedule)
      .then((p) => {
        if (!current) return;
        setPreview(p);
        setPreviewError(null);
      })
      .catch((e) => {
        if (!current) return;
        setPreview(null);
        setPreviewError(errorText(e));
      });
    return () => {
      current = false;
    };
  }, [form.schedule]);

  useEffect(() => {
    if (!logFor) return;
    const load = () => api.taskLog(logFor).then(setLog).catch(() => setLog([]));
    load();
    const timer = setInterval(load, 3000);
    return () => clearInterval(timer);
  }, [logFor]);

  async function attempt(action: () => Promise<Task[]>) {
    try {
      setTasks(await action());
      setError(null);
    } catch (e) {
      setError(errorText(e));
    }
  }

  async function save(e: FormEvent) {
    e.preventDefault();
    try {
      setTasks(editing ? await api.updateTask(editing, form) : await api.addTask(form));
      setForm(EMPTY);
      setEditing(null);
      setFormError(null);
    } catch (err) {
      setFormError(errorText(err));
    }
  }

  function edit(task: Task) {
    setEditing(task.id);
    setForm({ name: task.name, command: task.command, folder: task.folder, schedule: task.schedule, enabled: task.enabled });
    setFormError(null);
    document.getElementById("task-form-h")?.scrollIntoView({ behavior: "smooth", block: "start" });
  }

  function cancel() {
    setEditing(null);
    setForm(EMPTY);
    setFormError(null);
  }

  const set = (change: Partial<TaskInput>) => setForm((f) => ({ ...f, ...change }));
  const preset = PRESETS.find((p) => p.schedule === form.schedule.trim())?.schedule ?? "custom";
  const laravel = sites.filter(
    (s) => s.framework === "Laravel" && !(tasks ?? []).some((t) => t.folder === s.path && t.command.includes("schedule:run")),
  );

  return (
    <>
      <header className="page-header">
        <div>
          <h1>Scheduler</h1>
          <p className="muted">
            Run commands on a schedule in a project folder, with the PHP and Node.js versions that project uses. Tasks run
            while Stackyard is running, including from the tray.
          </p>
        </div>
      </header>

      {error && (
        <p className="error" role="alert">
          {error}
        </p>
      )}

      <div className="split">
        <section className="table-wrap stack" aria-label="Tasks">
          {tasks && tasks.length === 0 && (
            <div className="card empty">
              <p>No tasks yet. Add one with the form, for example Laravel's scheduler every minute.</p>
            </div>
          )}
          {tasks?.map((task) => (
            <article key={task.id} className="card stack task" aria-label={task.name}>
              <div className="row between">
                <div className="stack tight grow">
                  <div className="row wrap">
                    <span className="strong">{task.name}</span>
                    {task.running_since !== null && <span className="badge">Running</span>}
                  </div>
                  <code className="mono small">{task.command}</code>
                  <span className="muted small path">{task.folder}</span>
                </div>
                <label className="switch">
                  <input
                    type="checkbox"
                    checked={task.enabled}
                    onChange={() => attempt(() => api.setTaskEnabled(task.id, !task.enabled))}
                    aria-label={`Run ${task.name} on schedule`}
                  />
                  <span>{task.enabled ? "On" : "Off"}</span>
                </label>
              </div>

              <div className="row wrap small">
                <span>{task.description ?? <code className="mono">{task.schedule}</code>}</span>
                <span className="muted">·</span>
                <span className="muted">
                  {task.problem
                    ? "Not running"
                    : !task.enabled
                      ? "Switched off"
                      : task.next_run
                        ? `Next ${when(task.next_run)}`
                        : "Never due"}
                </span>
              </div>
              {task.problem && <p className="error small">{task.problem}</p>}
              {task.last && <LastRun run={task.last} />}

              <div className="row wrap">
                {task.running_since !== null ? (
                  <button type="button" className="btn small" onClick={() => attempt(() => api.stopTask(task.id))}>
                    Stop
                  </button>
                ) : (
                  <button
                    type="button"
                    className="btn small"
                    disabled={task.problem !== null}
                    onClick={() => attempt(() => api.runTask(task.id))}
                  >
                    Run now
                  </button>
                )}
                <button type="button" className="btn ghost small" onClick={() => edit(task)}>
                  Edit
                </button>
                <button
                  type="button"
                  className="btn ghost small"
                  aria-expanded={logFor === task.id}
                  onClick={() => setLogFor(logFor === task.id ? null : task.id)}
                >
                  {logFor === task.id ? "Hide output" : "Output"}
                </button>
                {removing === task.id ? (
                  <>
                    <button
                      type="button"
                      className="btn small danger"
                      onClick={() => {
                        setRemoving(null);
                        if (editing === task.id) cancel();
                        attempt(() => api.removeTask(task.id));
                      }}
                    >
                      Remove with its output
                    </button>
                    <button type="button" className="btn ghost small" onClick={() => setRemoving(null)}>
                      Keep
                    </button>
                  </>
                ) : (
                  <button type="button" className="btn ghost small" onClick={() => setRemoving(task.id)}>
                    Remove
                  </button>
                )}
              </div>

              {logFor === task.id && (
                <pre className="code mono log task-log" aria-label={`${task.name} output`}>
                  {log.length ? log.join("\n") : "No output yet."}
                </pre>
              )}
            </article>
          ))}
        </section>

        <form className="card stack form" aria-labelledby="task-form-h" onSubmit={save}>
          <h2 id="task-form-h">{editing ? "Edit task" : "New task"}</h2>
          {!editing && laravel.length > 0 && (
            <div className="stack tight">
              <span className="muted small">Quick start</span>
              <div className="row wrap">
                {laravel.slice(0, 3).map((site) => (
                  <button
                    key={site.name}
                    type="button"
                    className="btn ghost small"
                    onClick={() =>
                      setForm({
                        name: `Laravel scheduler (${site.name})`,
                        command: "php artisan schedule:run",
                        folder: site.path,
                        schedule: "* * * * *",
                        enabled: true,
                      })
                    }
                  >
                    Laravel scheduler for {site.name}
                  </button>
                ))}
              </div>
            </div>
          )}
          <div className="field">
            <label htmlFor="t-name">Name</label>
            <input id="t-name" value={form.name} placeholder="e.g. Nightly backup" onChange={(e) => set({ name: e.target.value })} />
          </div>
          <div className="field">
            <label htmlFor="t-command">Command</label>
            <input
              id="t-command"
              className="mono"
              value={form.command}
              placeholder="e.g. php artisan schedule:run"
              onChange={(e) => set({ command: e.target.value })}
            />
          </div>
          <div className="field">
            <label htmlFor="t-folder">Run it in</label>
            <input
              id="t-folder"
              className="mono"
              value={form.folder}
              placeholder="Full path of the project folder"
              onChange={(e) => set({ folder: e.target.value })}
            />
          </div>
          <div className="field">
            <label htmlFor="t-preset">When</label>
            <select
              id="t-preset"
              value={preset}
              onChange={(e) => e.target.value !== "custom" && set({ schedule: e.target.value })}
            >
              {PRESETS.map((p) => (
                <option key={p.schedule} value={p.schedule}>
                  {p.label}
                </option>
              ))}
              <option value="custom">Custom schedule</option>
            </select>
          </div>
          <div className="field">
            <label htmlFor="t-schedule">Schedule (minute hour day month weekday)</label>
            <input
              id="t-schedule"
              className="mono"
              value={form.schedule}
              aria-describedby="t-schedule-help"
              onChange={(e) => set({ schedule: e.target.value })}
            />
            <span id="t-schedule-help" className="small" aria-live="polite">
              {previewError ? (
                <span className="error">{previewError}</span>
              ) : preview ? (
                <span className="muted">
                  {preview.description ? `${preview.description}. ` : ""}
                  {preview.runs.length
                    ? `Next: ${preview.runs.map((r) => clock(r)).join(", ")}`
                    : "This date never comes up."}
                </span>
              ) : null}
            </span>
          </div>
          {formError && (
            <p className="error small" role="alert">
              {formError}
            </p>
          )}
          <div className="row wrap">
            <button type="submit" className="btn primary">
              {editing ? "Save changes" : "Add task"}
            </button>
            {editing && (
              <button type="button" className="btn ghost" onClick={cancel}>
                Cancel
              </button>
            )}
          </div>
        </form>
      </div>
    </>
  );
}

function LastRun({ run }: { run: TaskRun }) {
  const ok = run.outcome === "succeeded";
  const text = {
    succeeded: "Succeeded",
    failed: run.exit_code !== null ? `Failed with exit code ${run.exit_code}` : "Failed",
    stopped: "Stopped",
    "could-not-start": "Could not start",
  }[run.outcome];
  return (
    <div className="stack tight">
      <span className={ok ? "status ok" : run.outcome === "stopped" ? "status warn" : "status error"}>
        <span className={ok ? "dot ok" : run.outcome === "stopped" ? "dot warn" : "dot error"} aria-hidden="true" />
        {text} {ago(run.started)}
        {run.took_ms > 0 && ` · took ${duration(run.took_ms)}`}
        {run.by_hand && " · run by hand"}
      </span>
      {run.message && <span className="muted small">{run.message}</span>}
    </div>
  );
}

/** e.g. "today 22:05", "Thu 09:00" or "29 Feb 2028 00:00". */
function clock(ms: number) {
  const d = new Date(ms);
  const time = d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  const now = new Date();
  if (d.toDateString() === now.toDateString()) return time;
  const days = (ms - now.getTime()) / 86_400_000;
  if (days < 6 && days > -1) return `${d.toLocaleDateString([], { weekday: "short" })} ${time}`;
  return `${d.toLocaleDateString([], { day: "numeric", month: "short", year: "numeric" })} ${time}`;
}

function when(ms: number) {
  const minutes = Math.round((ms - Date.now()) / 60_000);
  if (minutes <= 0) return "now";
  if (minutes < 60) return `in ${minutes} min (${clock(ms)})`;
  return clock(ms);
}

function ago(ms: number) {
  const seconds = Math.round((Date.now() - ms) / 1000);
  if (seconds < 60) return "just now";
  if (seconds < 3600) return `${Math.round(seconds / 60)} min ago`;
  const today = new Date(ms).toDateString() === new Date().toDateString();
  return `${today ? "at" : "on"} ${clock(ms)}`;
}

function duration(ms: number) {
  if (ms < 1000) return `${ms} ms`;
  if (ms < 60_000) return `${(ms / 1000).toFixed(1)} s`;
  return `${Math.floor(ms / 60_000)} min ${Math.round((ms % 60_000) / 1000)} s`;
}
