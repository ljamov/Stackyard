import { useState } from "react";
import { installUpdate, progressText, seenWhatsNew, useUpdate } from "../lib/updates";
import { ReleaseNotes } from "./ReleaseNotes";

/**
 * The notice at the top of the window: a new version with its notes and an
 * Update button, or what the version Stackyard just updated to brought.
 */
export function UpdateNotice() {
  const { status, progress, error } = useUpdate();
  const [later, setLater] = useState<string | null>(null);
  const [open, setOpen] = useState(false);
  if (!status) return null;
  const { available } = status;

  if (available && later !== available.version) {
    return (
      <section className="update-notice stack tight" role="status" aria-label="Update available">
        <div className="row wrap between centered">
          <div>
            <strong>Stackyard {available.version} is available</strong>
            <p className="muted small">
              {status.installing
                ? progressText(progress)
                : `You have ${status.current}. Running services stop for a moment and start again in the new version.`}
            </p>
          </div>
          <div className="row wrap">
            {available.notes && (
              <button type="button" className="btn small" onClick={() => setOpen(!open)} aria-expanded={open}>
                {open ? "Hide what's new" : "What's new"}
              </button>
            )}
            {!status.installing && (
              <button type="button" className="btn small" onClick={() => setLater(available.version)}>
                Later
              </button>
            )}
            <button type="button" className="btn small primary" onClick={installUpdate} disabled={status.installing}>
              {status.installing ? "Updating…" : "Update now"}
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
      <section className="update-notice stack tight" role="status" aria-label="Stackyard updated">
        <div className="row wrap between centered">
          <strong>Stackyard is updated to {status.current}</strong>
          <button type="button" className="btn small" onClick={seenWhatsNew}>
            OK
          </button>
        </div>
        {status.whats_new && <ReleaseNotes text={status.whats_new} />}
      </section>
    );
  }

  return null;
}
