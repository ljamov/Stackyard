import { api } from "../lib/api";
import { useServices } from "../lib/useServices";
import { useT } from "../lib/i18n";
import { BrandIcon } from "../components/BrandIcon";
import { SERVICE_STATE } from "../components/ServiceCard";
import { ServiceLog } from "./Services";

/** One service in a tab of its own: its state, start and stop, how to connect, and its log. */
export function ServicePage({ id }: { id: string }) {
  const t = useT();
  const hook = useServices();
  const s = hook.services.find((x) => x.id === id);
  if (!s) {
    return hook.services.length ? <p className="muted">{t("This service isn't in Stackyard any more.")}</p> : null;
  }
  const running = s.state === "running" || s.state === "restarting";
  const busy = hook.busy[s.id] || s.installing;
  const error = hook.errors[s.id];
  const status = t(s.installing ? "Installing" : s.installed ? SERVICE_STATE[s.state] : "Not installed");
  const tone = s.state === "running" ? "ok" : s.state === "crashed" ? "error" : s.state === "restarting" ? "warn" : "";

  return (
    <>
      <header className="page-header">
        <div className="page-title">
          <BrandIcon id={s.id} />
          <div>
            <h1>{s.name}</h1>
            <p className={`status ${tone}`}>
              <span className={`dot ${tone}`} aria-hidden="true" />
              {status} · {t(s.role)}
              {s.installed && ` · ${s.installed}`}
            </p>
          </div>
        </div>
        <div className="row-actions">
          {s.web && s.state === "running" && (
            <button type="button" className="btn" onClick={() => api.openLocalPage(s.web!)}>
              {t("Open")}
            </button>
          )}
          {s.installed && !running && (
            <button type="button" className="btn ghost" disabled={busy} onClick={() => hook.uninstall(s.id)}>
              {t("Remove")}
            </button>
          )}
          {s.installed && running && (
            <button
              type="button"
              className="btn"
              disabled={busy}
              onClick={async () => {
                await hook.stop(s.id);
                await hook.start(s.id);
              }}
            >
              {t("Restart")}
            </button>
          )}
          {!s.installed ? (
            <button type="button" className="btn primary" disabled={busy} onClick={() => hook.install(s.id)}>
              {s.installing ? t("Installing…") : t("Install")}
            </button>
          ) : running ? (
            <button type="button" className="btn" disabled={busy} onClick={() => hook.stop(s.id)}>
              {t("Stop")}
            </button>
          ) : (
            <button type="button" className="btn primary" disabled={busy} onClick={() => hook.start(s.id)}>
              {busy ? t("Starting…") : t("Start")}
            </button>
          )}
        </div>
      </header>

      {error && (
        <p role="alert" className="error">
          {error}
        </p>
      )}

      <section className="card facts-card" aria-label={t("Details")}>
        <dl className="facts-grid">
          <dt>{t("Version")}</dt>
          <dd className="mono">{s.installed ?? s.version}</dd>
          {s.ports.length > 0 && (
            <>
              <dt>{t("Ports")}</dt>
              <dd className="mono">{s.ports.join(", ")}</dd>
            </>
          )}
          {s.connect && (
            <>
              <dt>{t("How apps connect")}</dt>
              <dd className="mono connect">{s.connect}</dd>
            </>
          )}
        </dl>
      </section>

      {s.installed && <ServiceLog id={s.id} name={s.name} />}
    </>
  );
}
