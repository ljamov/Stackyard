import { useEffect, useState } from "react";
import { api, errorText, type PhpMyAdminStatus, type ServiceProgress } from "../lib/api";
import { useT } from "../lib/i18n";
import { Progress, downloadState } from "./Progress";

/** Installs and opens phpMyAdmin, the full database manager. */
export function PhpMyAdminCard() {
  const t = useT();
  const [status, setStatus] = useState<PhpMyAdminStatus | null>(null);
  const [busy, setBusy] = useState<"install" | "open" | "remove" | null>(null);
  const [progress, setProgress] = useState<ServiceProgress | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api
      .phpMyAdminStatus()
      .then(setStatus)
      .catch((e) => setError(errorText(e)));
    const off = api.onServiceProgress((p) => {
      if (p.id === "phpmyadmin") setProgress(p);
    });
    return () => void off.then((f) => f());
  }, []);

  async function act(kind: "install" | "open" | "remove", action: () => Promise<PhpMyAdminStatus | void>) {
    setBusy(kind);
    setError(null);
    try {
      const next = await action();
      if (next) setStatus(next);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(null);
      setProgress(null);
    }
  }

  if (!status) return null;
  const installing = busy === "install" || status.installing;
  const pct =
    progress?.step === "downloading" && progress.total
      ? Math.floor(((progress.done ?? 0) * 100) / progress.total)
      : null;

  return (
    <section className="card stack" aria-labelledby="pma-h">
      <div className="row wrap between centered">
        <div className="stack tight">
          <h2 id="pma-h">phpMyAdmin</h2>
          <p className="muted small">
            {status.installed
              ? `Version ${status.installed}. Import and export SQL files, create and edit tables and rows. Opens logged in as root.`
              : "The full database manager in your browser: import and export SQL files, create and edit tables and rows."}
          </p>
        </div>
        <div className="row">
          {status.installed ? (
            <>
              <button
                type="button"
                className="btn small"
                disabled={!!busy}
                onClick={() => act("remove", api.uninstallPhpMyAdmin)}
              >
                {busy === "remove" ? "Removing…" : "Remove"}
              </button>
              <button
                type="button"
                className="btn primary small"
                disabled={!!busy}
                onClick={() => act("open", api.openPhpMyAdmin)}
              >
                {busy === "open" ? "Starting…" : "Open phpMyAdmin"}
              </button>
            </>
          ) : (
            <button
              type="button"
              className="btn primary small"
              disabled={installing}
              onClick={() => act("install", api.installPhpMyAdmin)}
            >
              {installing
                ? progress?.step === "unpacking"
                  ? "Unpacking…"
                  : pct !== null
                    ? `Downloading ${pct}%`
                    : "Installing…"
                : "Install phpMyAdmin"}
            </button>
          )}
        </div>
      </div>
      {status.installed && status.url && (
        <p className="small muted">
          Address: <span className="mono">{status.url}</span>
          {status.local_url && (
            <>
              {" "}
              or <span className="mono">{status.local_url}</span>
            </>
          )}
        </p>
      )}
      {installing && <Progress label={t("Installing {name}", { name: "phpMyAdmin" })} state={downloadState(t, progress)} />}
      {status.problem && <p className="warn small">{status.problem}. Install a PHP version under Runtimes.</p>}
      {error && (
        <p className="error small" role="alert">
          {error}
        </p>
      )}
    </section>
  );
}
