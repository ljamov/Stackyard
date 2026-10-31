import { useEffect, useRef, useState } from "react";
import { useT } from "../../lib/i18n";
import type { WpMergeChoice, WpSyncChanges, WpSyncProgress, WpWay } from "../../lib/wordpress";
import { size } from "./WordPressTools";

type T = ReturnType<typeof useT>;

/** One step of a push or pull, and the progress messages that mean it has begun. */
type Step = { id: string; label: string; starts: RegExp };

/** What a push or pull is about to do, worked out from the picked changes. */
export type SyncPlan = {
  way: WpWay;
  level: WpMergeChoice["level"];
  upload: { files: number; bytes: number };
  remove: number;
  switches: number;
  settings: number;
};

export function syncPlan(list: WpSyncChanges, choice: WpMergeChoice): SyncPlan {
  const picked = new Set(choice.files);
  const files = list.files.filter((f) => picked.has(f.path));
  const copied = files.filter((f) => f.change !== "removed");
  return {
    way: list.way,
    level: choice.level,
    upload: { files: copied.length, bytes: copied.reduce((sum, f) => sum + f.bytes, 0) },
    remove: files.length - copied.length,
    switches: choice.switches.length,
    settings: choice.settings.length,
  };
}

/** The steps in the order the core takes them (wordpress/sync.rs push and pull). */
function steps(t: T, plan: SyncPlan, site: string): Step[] {
  const moved = plan.upload.files + plan.remove;
  const database = plan.level === "everything" || plan.settings > 0 || (plan.way === "push" && plan.switches > 0);
  if (plan.way === "push") {
    return [
      { id: "connect", label: t("Connect and compare again"), starts: /^Connecting|^Listing|^Reading/ },
      { id: "backup", label: t("Back up the live site"), starts: /^Backing up/ },
      { id: "on", label: t("Show the maintenance page on live"), starts: /^Switching the live site to maintenance/ },
      ...(moved ? [{ id: "files", label: t("Upload {count} files", { count: moved.toLocaleString() }), starts: /^Uploading / }] : []),
      ...(database
        ? [
            {
              id: "database",
              label: plan.level === "everything" ? t("Replace the live database") : t("Change the live settings"),
              starts: /^Changing the live settings|^Putting together|^Loading the new database/,
            },
          ]
        : []),
      { id: "off", label: t("Take the maintenance page down"), starts: /^Switching maintenance mode off/ },
      { id: "check", label: t("Check the live site answers"), starts: /^Checking the live site|^Noting where/ },
    ];
  }
  return [
    { id: "connect", label: t("Connect and compare again"), starts: /^Connecting|^Listing|^Reading/ },
    { id: "snapshot", label: t("Save a snapshot of {site}", { site }), starts: /^Saving a snapshot/ },
    ...(plan.upload.files ? [{ id: "files", label: t("Download {count} files", { count: plan.upload.files.toLocaleString() }), starts: /^Downloading/ }] : []),
    ...(database
      ? [
          {
            id: "database",
            label: plan.level === "everything" ? t("Bring in the live database") : t("Copy the settings"),
            starts: /^Copying the settings|^Backing up the table|^Changing addresses/,
          },
        ]
      : []),
    { id: "finish", label: t("Note where both sides stand"), starts: /^Noting where/ },
  ];
}

/** Roughly how long, in words. */
export function duration(t: T, seconds: number) {
  if (seconds < 60) return t("{count} s", { count: Math.max(1, Math.round(seconds)) });
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return t("{count} min", { count: minutes });
  return t("{hours} h {minutes} min", { hours: Math.floor(minutes / 60), minutes: minutes % 60 });
}

/** What pressing Push or Pull will do, in order, before it is pressed. */
export function SyncPreview({ plan, site }: { plan: SyncPlan; site: string }) {
  const t = useT();
  const push = plan.way === "push";
  const lines: string[] = [];
  if (push) {
    lines.push(t("Back up the whole live database, and every live file about to change, so the push can be undone."));
    lines.push(t("Show visitors a maintenance page while the live site changes."));
  } else {
    lines.push(t("Save a snapshot of {site}, so the pull can be undone.", { site }));
  }
  if (plan.upload.files)
    lines.push(
      push
        ? t("Upload {count} files ({size}).", { count: plan.upload.files.toLocaleString(), size: size(plan.upload.bytes) })
        : t("Download {count} files ({size}).", { count: plan.upload.files.toLocaleString(), size: size(plan.upload.bytes) }),
    );
  if (plan.remove)
    lines.push(push ? t("Delete {count} files on the live site.", { count: plan.remove.toLocaleString() }) : t("Delete {count} files here.", { count: plan.remove.toLocaleString() }));
  if (plan.level !== "everything" && plan.switches)
    lines.push(plan.switches === 1 ? t("Switch 1 plugin or theme on or off.") : t("Switch {count} plugins or themes on or off.", { count: plan.switches }));
  if (plan.settings) lines.push(plan.settings === 1 ? t("Change 1 setting.") : t("Change {count} settings.", { count: plan.settings }));
  if (plan.level === "everything")
    lines.push(
      push
        ? t("Replace the live database with this copy's, keeping the live users, orders, customers and payment settings.")
        : t("Replace {site}'s database with the live one, with local addresses and payments in test mode.", { site }),
    );
  else if (!plan.settings) lines.push(push ? t("Leave the live database as it is.") : t("Leave {site}'s database as it is.", { site }));
  if (push) lines.push(t("Take the maintenance page down and check that the live home page and login page answer. If anything fails on the way, the backup is put back by itself."));
  return (
    <div className="notice small stack tight">
      <strong>{push ? t("Pressing Push to live will:") : t("Pressing Pull will:")}</strong>
      <ol className="sync-preview">
        {lines.map((l) => (
          <li key={l}>{l}</li>
        ))}
      </ol>
    </div>
  );
}

/**
 * A push or pull that is running: every step with a tick once done, the
 * current one with its bar and time left, and how long it has taken.
 */
export function SyncSteps({ plan, site, progress, onStop }: { plan: SyncPlan; site: string; progress: WpSyncProgress | null; onStop: () => void }) {
  const t = useT();
  const list = steps(t, plan, site);
  // Steps only move forward: a message seen again later (listing files
  // after the push) doesn't send the list back.
  const [at, setAt] = useState(0);
  const started = useRef(Date.now());
  const pace = useRef<{ step: number; since: number; from: number } | null>(null);
  const [, tick] = useState(0);
  useEffect(() => {
    const timer = setInterval(() => tick((n) => n + 1), 1000);
    return () => clearInterval(timer);
  }, []);
  useEffect(() => {
    if (!progress) return;
    const found = list.findIndex((s) => s.starts.test(progress.message));
    if (found > at) setAt(found);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [progress]);
  const rollingBack = !!progress?.message.startsWith("Something went wrong");
  const total = progress?.total ?? 0;
  const done = progress?.done ?? 0;
  if (!pace.current || pace.current.step !== at) pace.current = { step: at, since: Date.now(), from: done };
  const percent = total > 0 ? Math.min(100, Math.floor((done * 100) / total)) : null;
  const spent = (Date.now() - pace.current.since) / 1000;
  const rate = spent > 3 ? (done - pace.current.from) / spent : 0;
  const left = total > 0 && rate > 0 ? (total - done) / rate : null;
  const elapsed = (Date.now() - started.current) / 1000;
  return (
    <div className="stack tight" role="status">
      <ol className="sync-steps">
        {list.map((s, i) => (
          <li key={s.id} className={i < at ? "done" : i === at ? "now" : "next"}>
            <span className="sync-step-mark" aria-hidden="true">
              {i < at ? "✓" : i === at ? "●" : "○"}
            </span>
            <span className="small">{s.label}</span>
          </li>
        ))}
      </ol>
      <div className="progress" aria-hidden="true">
        <div className={`progress-fill${percent === null ? " busy" : ""}`} style={{ width: `${percent ?? 30}%` }} />
      </div>
      <p className={`small ${rollingBack ? "warn" : "muted"} clone-progress-file`} title={progress?.message}>
        {progress?.message ?? t("Starting…")}
      </p>
      <p className="muted small">
        {[
          t("Step {n} of {count}", { n: at + 1, count: list.length }),
          percent !== null && `${percent}%`,
          left !== null && t("about {time} left in this step", { time: duration(t, left) }),
          t("{time} so far", { time: duration(t, elapsed) }),
        ]
          .filter(Boolean)
          .join(" · ")}
      </p>
      <div className="row">
        <button type="button" className="btn small" onClick={onStop} disabled={rollingBack}>
          {t("Stop")}
        </button>
      </div>
    </div>
  );
}
