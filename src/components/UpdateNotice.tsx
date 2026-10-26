import { useState } from "react";
import { installUpdate, progressText, seenWhatsNew, useUpdate } from "../lib/updates";
import { ReleaseNotes } from "./ReleaseNotes";
import { useClones } from "../lib/clones";
import { useT } from "../lib/i18n";

/**
 * The notice at the top of the window: a new version with its notes and an
 * Update button, or what the version Stackyard just updated to brought.
 */
export function UpdateNotice() {
  const t = useT();
  const { status, progress, error } = useUpdate();
  const [later, setLater] = useState<string | null>(null);
  const [open, setOpen] = useState(false);
  const cloning = useClones().some((c) => c.state === "running");
  if (!status) return null;
  const { available } = status;

  if (available && later !== available.version) {
    return (
      <section className="update-notice stack tight" role="status" aria-label={t("Update available")}>
        <div className="row wrap between centered">
          <div>
            <strong>{t("Stackyard {version} is available", { version: available.version })}</strong>
            <p className="muted small">
              {status.installing
                ? progressText(progress)
                : t("You have {version}. Running services stop for a moment and start again in the new version.", {
                    version: status.current,
                  })}
              {!status.installing && cloning && ` ${t("Clones from a live server pause for the update and go on after it.")}`}
            </p>
          </div>
          <div className="row wrap">
            {available.notes && (
              <button type="button" className="btn small" onClick={() => setOpen(!open)} aria-expanded={open}>
                {open ? t("Hide what's new") : t("What's new")}
              </button>
            )}
            {!status.installing && (
              <button type="button" className="btn small" onClick={() => setLater(available.version)}>
                {t("Later")}
              </button>
            )}
            <button type="button" className="btn small primary" onClick={installUpdate} disabled={status.installing}>
              {status.installing ? t("Updating…") : t("Update now")}
            </button>
          </div>
        </div>
        {open && available.notes && <ReleaseNotes text={available.notes} />}
        {(error ?? status.problem) && (
          <p className="error small" role="alert">
            {error ?? status.problem}
          </p>
        )}
      </section>
    );
  }

  if (status.just_updated) {
    return (
      <section className="update-notice stack tight" role="status" aria-label={t("Stackyard updated")}>
        <div className="row wrap between centered">
          <strong>{t("Stackyard is updated to {version}", { version: status.current })}</strong>
          <button type="button" className="btn small" onClick={seenWhatsNew}>
            {t("OK")}
          </button>
        </div>
        {status.whats_new && <ReleaseNotes text={status.whats_new} />}
      </section>
    );
  }

  return null;
}
