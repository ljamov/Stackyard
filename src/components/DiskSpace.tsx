import { useState } from "react";
import { api, errorText, type CleanupOutcome, type CleanupReport } from "../lib/api";

/** How much Stackyard's data folder takes, and cleaning up what can go.
 * Nothing is read from disk until the person asks. */
export function DiskSpace() {
  const [report, setReport] = useState<CleanupReport | null>(null);
  const [picked, setPicked] = useState<Set<string>>(new Set());
  const [busy, setBusy] = useState<"scan" | "clean" | null>(null);
  const [confirming, setConfirming] = useState(false);
  const [outcome, setOutcome] = useState<CleanupOutcome | null>(null);
  const [error, setError] = useState<string | null>(null);

  async function scan() {
    setBusy("scan");
    setError(null);
    try {
      const next = await api.cleanupScan();
      setReport(next);
      setPicked(new Set(next.items.filter((i) => i.recommended).map((i) => i.id)));
    } catch (e) {
      setError(errorText(e));
    }
    setBusy(null);
  }

  const chosen = report?.items.filter((i) => picked.has(i.id)) ?? [];
  const chosenSize = chosen.reduce((n, i) => n + i.size, 0);
  const permanent = chosen.filter((i) => i.permanent);

  async function clean() {
    if (permanent.length > 0 && !confirming) {
      setConfirming(true);
      return;
    }
    setConfirming(false);
    setBusy("clean");
    setError(null);
    try {
      setOutcome(await api.cleanupRun(chosen.map((i) => i.id)));
    } catch (e) {
      setError(errorText(e));
    }
    setBusy(null);
    await scan();
  }

  function toggle(id: string) {
    setConfirming(false);
    setPicked((now) => {
      const next = new Set(now);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }

  const largest = report?.usage[0]?.size ?? 1;

  return (
    <section aria-labelledby="disk-h" className="card stack">
      <div className="row wrap between centered">
        <div>
          <h2 id="disk-h">Disk space</h2>
          <p className="muted small">
            {report
              ? `Stackyard uses ${size(report.total)}. Your project folders are not counted or touched.`
              : "See what Stackyard's own folder takes and clean up what it no longer needs."}
          </p>
        </div>
        <button type="button" className="btn small" onClick={scan} disabled={busy !== null}>
          {busy === "scan" ? "Checking…" : report ? "Check again" : "Check disk space"}
        </button>
      </div>

      {error && (
        <p className="error small" role="alert">
          {error}
        </p>
      )}
      {outcome && (
        <p className="notice small" role="status">
          {outcome.cleaned.length > 0 ? `Freed ${size(outcome.freed)}: ${outcome.cleaned.join(", ")}.` : "Nothing was cleaned."}
          {outcome.problems.map((p) => (
            <span key={p} className="error">
              {" "}
              Not cleaned: {p}.
            </span>
          ))}
        </p>
      )}

      {report && report.usage.length > 0 && (
        <div className="usage" aria-label="What takes space">
          {report.usage.map((part) => (
            <div key={part.name} className="usage-row">
              <span className="small">{part.name}</span>
              <span className="usage-bar" aria-hidden="true">
                <span style={{ width: `${Math.max(2, (part.size / largest) * 100)}%` }} />
              </span>
              <span className="small num">{size(part.size)}</span>
            </div>
          ))}
        </div>
      )}

      {report && report.items.length === 0 && <p className="muted small">Nothing to clean up.</p>}

      {report && report.items.length > 0 && (
        <>
          <h3 className="subhead">Can be cleaned up</h3>
          <div className="stack tight">
            {report.items.map((item) => (
              <label key={item.id} className="clean-item">
                <input type="checkbox" checked={picked.has(item.id)} onChange={() => toggle(item.id)} />
                <span className="stack tight grow">
                  <span className="row wrap between">
                    <span className="strong">
                      {item.title}
                      {item.permanent && <span className="warn small"> · can't be undone</span>}
                    </span>
                    <span className="small num">{size(item.size)}</span>
                  </span>
                  <span className="muted small">{item.detail}</span>
                </span>
              </label>
            ))}
          </div>
          <div className="row wrap">
            <button
              type="button"
              className={`btn ${confirming ? "danger" : "primary"}`}
              onClick={clean}
              disabled={busy !== null || chosen.length === 0}
            >
              {busy === "clean"
                ? "Cleaning up…"
                : confirming
                  ? `Delete ${permanent.map((i) => i.title).join(", ")} for good`
                  : chosen.length > 0
                    ? `Clean up ${size(chosenSize)}`
                    : "Clean up"}
            </button>
            {confirming && (
              <button type="button" className="btn ghost" onClick={() => setConfirming(false)}>
                Cancel
              </button>
            )}
          </div>
        </>
      )}
    </section>
  );
}

function size(bytes: number) {
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let value = bytes / 1024;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit++;
  }
  return `${value < 10 ? value.toFixed(1) : Math.round(value)} ${units[unit]}`;
}
