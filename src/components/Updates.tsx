import { checkForUpdate, installUpdate, progressText, useUpdate } from "../lib/updates";
import { useModules } from "../lib/modules";
import { ReleaseNotes } from "./ReleaseNotes";
import { useT } from "../lib/i18n";

/** Stackyard's version, and installing a newer one when there is one. */
export function Updates() {
  const t = useT();
  const { status, progress, error } = useUpdate();
  const { isOn } = useModules();
  if (!status) return null;

  const { available } = status;
  return (
    <section aria-labelledby="upd-h" className="card stack">
      <div className="row wrap between centered">
        <div>
          <h2 id="upd-h">Stackyard {status.current}</h2>
          <p className="muted small">
            {!status.configured
              ? t("This build can't check for updates.")
              : available
                ? t("Version {version} is available.", { version: available.version })
                : status.checked_at
                  ? t("Up to date, checked {time}.", { time: new Date(status.checked_at).toLocaleString() })
                  : isOn("updates")
                    ? t(
                        "Checks for updates every half hour and when you open the window, and tells you when one is out. Nothing installs until you say so.",
                      )
                    : t("Automatic checks are off (Updates, under Features).")}
          </p>
        </div>
        {status.configured && !available && (
          <button type="button" className="btn small" onClick={checkForUpdate} disabled={status.checking}>
            {status.checking ? t("Checking…") : t("Check for updates")}
          </button>
        )}
      </div>

      {available && (
        <div className="stack tight">
          {available.notes && (
            <>
              <h3 className="small">{t("What's new in {version}", { version: available.version })}</h3>
              <ReleaseNotes text={available.notes} />
            </>
          )}
          <div className="row wrap">
            <button type="button" className="btn primary" onClick={installUpdate} disabled={status.installing}>
              {status.installing ? t("Installing…") : t("Install {version} and restart", { version: available.version })}
            </button>
            <span className="muted small">
              {status.installing
                ? progressText(progress)
                : t("Running services stop for a moment and start again in the new version.")}
            </span>
          </div>
        </div>
      )}

      {status.whats_new && (
        <details>
          <summary className="small">{t("What's new in {version}", { version: status.current })}</summary>
          <ReleaseNotes text={status.whats_new} />
        </details>
      )}

      {(error ?? status.problem) && (
        <p className="error small" role="alert">
          {error ?? status.problem}
        </p>
      )}
    </section>
  );
}
