import { useEffect, useState } from "react";
import { api, errorText, type DefenderStatus } from "../lib/api";
import { useT } from "../lib/i18n";
import { useBusy } from "../lib/tabLife";

/**
 * Windows Defender scans every file PHP opens, which makes the first load of
 * a big site take minutes on a slow disk. This card shows whether the sites'
 * folders, Stackyard's data and its servers are left out of the scans, and
 * adds what is missing behind one administrator prompt.
 */
export function DefenderExclusions() {
  const t = useT();
  const [status, setStatus] = useState<DefenderStatus | null>(null);
  const [adding, setAdding] = useState(false);
  useBusy(adding);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api
      .defenderStatus()
      .then(setStatus)
      .catch(() => setStatus(null));
  }, []);

  if (!status?.supported) return null;
  const missing = [...status.missing.paths, ...status.missing.processes];
  const done = missing.length === 0;

  async function add() {
    setAdding(true);
    setError(null);
    try {
      setStatus(await api.addDefenderExclusions());
    } catch (e) {
      setError(errorText(e));
    } finally {
      setAdding(false);
    }
  }

  return (
    <section aria-labelledby="defender-h" className="card stack">
      <h2 id="defender-h">{t("Windows Defender exclusions")}</h2>
      <p className="muted">
        {t("Defender scans every file PHP and the databases open, so the first load of a big site can take minutes. Leaving these folders and programs out of the scans makes sites load much faster. Windows asks for administrator rights once.")}
      </p>
      <div className="row wrap centered">
        <span className={`dot ${done ? "ok" : "warn"}`} aria-hidden="true" />
        <span className="small">
          {done
            ? status.read_from_defender
              ? t("All set: Defender leaves these alone.")
              : t("Added by Stackyard. (Windows only shows the list to administrators.)")
            : t("{count} not excluded yet", { count: missing.length })}
        </span>
      </div>
      <ul className="small mono">
        {[...status.wanted.paths, ...status.wanted.processes].map((item) => (
          <li key={item}>
            {item}
            {missing.includes(item) && <span className="muted"> · {t("not excluded")}</span>}
          </li>
        ))}
      </ul>
      {!done && (
        <div className="row wrap">
          <button type="button" className="btn primary" disabled={adding} onClick={add}>
            {adding ? t("Waiting for Windows…") : t("Add exclusions")}
          </button>
        </div>
      )}
      {error && (
        <p className="error small" role="alert">
          {error}
        </p>
      )}
    </section>
  );
}
