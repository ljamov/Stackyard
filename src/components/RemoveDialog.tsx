import { useEffect, useRef, useState } from "react";
import { api, errorText, type Site } from "../lib/api";
import { useT } from "../lib/i18n";
import { useBusy } from "../lib/tabLife";

/**
 * Asks before a project, app or site goes: take it off Stackyard's lists
 * and keep its files, or delete its folder too. Deleting is picked on
 * purpose (it is not the default) and says which folders go.
 */
export function RemoveDialog({
  site,
  onClose,
  onRemoved,
}: {
  site: Site;
  onClose: () => void;
  /** After it is removed, even when deleting the files then failed. */
  onRemoved: (error: string | null) => void;
}) {
  const t = useT();
  const [deleteFiles, setDeleteFiles] = useState(false);
  const [busy, setBusy] = useState(false);
  useBusy(busy);
  const [error, setError] = useState<string | null>(null);
  const cancelRef = useRef<HTMLButtonElement>(null);
  useEffect(() => cancelRef.current?.focus(), []);
  // A theme or plugin is built in its own folder, apart from the site's.
  const folders = [site.path, ...(site.project.folder && site.project.folder !== site.path ? [site.project.folder] : [])];

  async function remove() {
    setBusy(true);
    setError(null);
    try {
      await api.removeSite(site.domain, deleteFiles);
      onRemoved(null);
    } catch (e) {
      const text = errorText(e);
      // Refused before anything changed: the dialog stays to say why.
      if (/was not deleted/.test(text)) setError(text);
      else onRemoved(text);
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="palette-backdrop dialog-backdrop" onMouseDown={(e) => e.target === e.currentTarget && !busy && onClose()}>
      <div
        className="dialog card stack"
        role="alertdialog"
        aria-modal="true"
        aria-labelledby="remove-h"
        onKeyDown={(e) => e.key === "Escape" && !busy && onClose()}
      >
        <h2 id="remove-h">{t("Remove {name}?", { name: site.name })}</h2>
        <fieldset className="stack radio-list">
          <legend className="sr-only">{t("What happens to the files")}</legend>
          <label className="check">
            <input type="radio" name="remove-files" checked={!deleteFiles} onChange={() => setDeleteFiles(false)} />
            <span>
              <strong>{t("Keep the files")}</strong>
              <span className="muted small block">{t("Only takes it off Stackyard's lists. The folder stays as it is.")}</span>
            </span>
          </label>
          <label className="check">
            <input type="radio" name="remove-files" checked={deleteFiles} onChange={() => setDeleteFiles(true)} />
            <span>
              <strong>{t("Delete the folder and everything in it")}</strong>
              <span className="muted small block">{t("This can't be undone. The database stays; remove it under Databases.")}</span>
            </span>
          </label>
        </fieldset>
        <div className="stack">
          {folders.map((f) => (
            <span key={f} className={`mono small ${deleteFiles ? "error" : "muted"}`}>
              {f}
            </span>
          ))}
        </div>
        {error && (
          <p className="error small" role="alert">
            {error}
          </p>
        )}
        <div className="row between centered">
          <button ref={cancelRef} type="button" className="btn ghost" onClick={onClose} disabled={busy}>
            {t("Cancel")}
          </button>
          <button type="button" className={`btn ${deleteFiles ? "danger" : "primary"}`} onClick={remove} disabled={busy}>
            {busy ? t("Removing…") : deleteFiles ? t("Remove and delete files") : t("Remove")}
          </button>
        </div>
      </div>
    </div>
  );
}
