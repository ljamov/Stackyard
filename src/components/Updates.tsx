import { useEffect, useState } from "react";
import { api, errorText, type UpdateProgress } from "../lib/api";
import { useModules } from "../lib/modules";
import { useUpdate } from "../lib/updates";

/** Stackyard's version, and installing a newer one when there is one. */
export function Updates() {
  const [status, setStatus] = useUpdate();
  const { isOn } = useModules();
  const [progress, setProgress] = useState<UpdateProgress | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let stop = () => {};
    let gone = false;
    api.onUpdateProgress(setProgress).then((unlisten) => {
      if (gone) unlisten();
      else stop = unlisten;
    });
    return () => {
      gone = true;
      stop();
    };
  }, []);

  if (!status) return null;

  async function check() {
    setError(null);
    setStatus({ ...status!, checking: true });
    setStatus(await api.checkForUpdate());
  }

  async function install() {
    setError(null);
    setStatus({ ...status!, installing: true });
    try {
      // Stackyard restarts into the new version when this succeeds.
      await api.installUpdate();
    } catch (e) {
      setError(errorText(e));
      setProgress(null);
      setStatus(await api.updateStatus());
    }
  }

  const { available } = status;
  return (
    <section aria-labelledby="upd-h" className="card stack">
      <div className="row wrap between centered">
        <div>
          <h2 id="upd-h">Stackyard {status.current}</h2>
          <p className="muted small">
            {!status.configured
              ? "This build can't check for updates."
              : available
                ? `Version ${available.version} is available.`
                : status.checked_at
                  ? `Up to date, checked ${new Date(status.checked_at).toLocaleString()}.`
                  : isOn("updates")
                    ? "Checks for updates twice a day. Nothing installs until you say so."
                    : "Automatic checks are off (Updates, under Features)."}
          </p>
        </div>
        {status.configured && !available && (
          <button type="button" className="btn small" onClick={check} disabled={status.checking}>
            {status.checking ? "Checking…" : "Check for updates"}
          </button>
        )}
      </div>

      {available && (
        <div className="stack tight">
          {available.notes && <p className="small update-notes">{available.notes}</p>}
          <div className="row wrap">
            <button type="button" className="btn primary" onClick={install} disabled={status.installing}>
              {status.installing ? "Installing…" : `Install ${available.version} and restart`}
            </button>
            <span className="muted small">
              {status.installing && progress
                ? progress.total
                  ? `Downloaded ${Math.round((progress.done / progress.total) * 100)}%`
                  : `Downloaded ${(progress.done / 1048576).toFixed(1)} MB`
                : "Running services stop for a moment and start again in the new version."}
            </span>
          </div>
        </div>
      )}

      {(error ?? status.problem) && (
        <p className="error small" role="alert">
          {error ?? status.problem}
        </p>
      )}
    </section>
  );
}
