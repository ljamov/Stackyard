import { api, type Service, type ServiceProgress } from "../lib/api";
import type { ServicesHook } from "../lib/useServices";
import { useT } from "../lib/i18n";

/** What each service state is called; translate with t(). */
export const SERVICE_STATE: Record<Service["state"], string> = {
  stopped: "Stopped",
  running: "Running",
  restarting: "Restarting",
  crashed: "Crashed",
};

export function ServiceCard({
  service: s,
  hook,
  onShowLog,
  compact = false,
}: {
  service: Service;
  hook: ServicesHook;
  onShowLog?: (id: string) => void;
  compact?: boolean;
}) {
  const t = useT();
  const running = s.state === "running" || s.state === "restarting";
  const busy = hook.busy[s.id] || s.installing;
  const error = hook.errors[s.id];
  const status = t(s.installing ? "Installing" : s.installed ? SERVICE_STATE[s.state] : "Not installed");
  const tone = s.state === "running" ? "ok" : s.state === "crashed" ? "error" : s.state === "restarting" ? "warn" : "";

  return (
    <div className="card service">
      <div className="row between">
        <div>
          <div className="strong">{s.name}</div>
          <div className="mono muted small">
            {s.installed ?? s.version} · {t("port {ports}", { ports: s.ports.join(", ") })}
          </div>
        </div>
        <span className={`status ${tone}`}>
          <span className={`dot ${tone}`} aria-hidden="true" />
          {status}
        </span>
      </div>
      <div className="muted small">{t(s.role)}</div>
      {!compact && s.connect && (
        <div className="mono small connect" title={t("How apps connect")}>
          {s.connect}
        </div>
      )}
      {s.installing && <InstallProgressBar name={s.name} progress={hook.progress[s.id]} />}
      {error && (
        <p role="alert" className="error small">
          {error}
        </p>
      )}
      <div className="row wrap">
        {!s.installed && (
          <button
            type="button"
            className="btn primary small"
            disabled={busy}
            onClick={() => hook.install(s.id)}
            aria-label={t("Install {name}", { name: s.name })}
          >
            {s.installing ? t("Installing…") : t("Install")}
          </button>
        )}
        {s.installed && !running && (
          <button
            type="button"
            className="btn primary small"
            disabled={busy}
            onClick={() => hook.start(s.id)}
            aria-label={t("Start {name}", { name: s.name })}
          >
            {busy ? t("Starting…") : t("Start")}
          </button>
        )}
        {s.installed && running && (
          <button
            type="button"
            className="btn small"
            disabled={busy}
            onClick={() => hook.stop(s.id)}
            aria-label={t("Stop {name}", { name: s.name })}
          >
            {t("Stop")}
          </button>
        )}
        {s.web && s.state === "running" && (
          <button
            type="button"
            className="btn ghost small"
            onClick={() => api.openLocalPage(s.web!)}
            aria-label={t("Open {name} in your browser", { name: s.name })}
          >
            {t("Open")}
          </button>
        )}
        {!compact && s.installed && onShowLog && (
          <button type="button" className="btn ghost small" onClick={() => onShowLog(s.id)} aria-label={t("{name} log", { name: s.name })}>
            {t("Log")}
          </button>
        )}
        {!compact && s.installed && !running && (
          <button
            type="button"
            className="btn ghost small"
            disabled={busy}
            onClick={() => hook.uninstall(s.id)}
            aria-label={t("Remove {name}", { name: s.name })}
          >
            {t("Remove")}
          </button>
        )}
      </div>
    </div>
  );
}

function InstallProgressBar({ name, progress }: { name: string; progress?: ServiceProgress }) {
  const t = useT();
  const pct =
    progress?.step === "unpacking"
      ? 100
      : progress?.step === "downloading" && progress.total
        ? Math.floor(((progress.done ?? 0) * 100) / progress.total)
        : null;
  return (
    <div
      className="progress"
      role="progressbar"
      aria-label={t("Installing {name}", { name })}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={pct ?? undefined}
    >
      <div className={pct === null ? "progress-fill busy" : "progress-fill"} style={{ width: `${pct ?? 30}%` }} />
    </div>
  );
}
