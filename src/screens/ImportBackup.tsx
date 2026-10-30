import { useEffect, useRef, useState, type FormEvent } from "react";
import { api, errorText, type PackProgress, type SiteFile } from "../lib/api";
import { useT } from "../lib/i18n";
import { useModules } from "../lib/modules";
import { openTab, showScreen, siteTab } from "../lib/layout";
import { isSiteBackup, newClone, useClones } from "../lib/clones";
import { CloneSite } from "../components/wordpress/CloneSite";
import { Icon } from "../components/Icon";
import { Progress, packState } from "../components/Progress";
import { useBusy, useShown } from "../lib/tabLife";
import { siteName } from "./Sites";

// Backups handed over from elsewhere (dropped on the Sites screen), for
// this screen to start on once it is shown.
let pending: string[][] = [];
const waiting = new Set<() => void>();

/** Shows Import backup and starts it on these files. */
export function importBackup(files: string[]) {
  pending = [...pending, files];
  waiting.forEach((w) => w());
  showScreen("Import backup");
}

/** "C:\Downloads\shop.tar" as Windows' Copy as path gives it, quotes and all → the path. */
function cleanPath(typed: string) {
  return typed.trim().replace(/^["']+|["']+$/g, "").trim();
}

const fileName = (path: string) => path.split(/[\\/]/).pop() ?? path;

/**
 * Import backup: a box to drop a backup on, or a path to type, then the
 * site is made from it. A Stackyard Connect export or a hosting panel
 * backup goes through Clone (unpack, find WordPress, load the database);
 * a site file packed by Stackyard is imported as it was packed.
 */
export function ImportBackup() {
  const t = useT();
  const { isOn } = useModules();
  const clones = useClones().filter((c) => c.place === "import" && c.open);
  const [files, setFiles] = useState<string[]>([]);
  const [typed, setTyped] = useState("");
  const [over, setOver] = useState(false);
  const [checking, setChecking] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [importing, setImporting] = useState<{ path: string; file: SiteFile } | null>(null);
  const canClone = isOn("wordpress") && isOn("remote");
  const canImport = isOn("sitetransfer");
  const shown = useShown();
  const shownNow = useRef(shown);
  shownNow.current = shown;
  const startRef = useRef<(files: string[]) => void>(() => {});

  async function start(chosen: string[]) {
    const list = chosen.map(cleanPath).filter(Boolean);
    if (!list.length) return;
    setError(null);
    const odd = list.filter((f) => !isSiteBackup(f));
    if (odd.length) {
      setError(t("{name} is not a backup Stackyard can read; pick a .tar, .tar.gz, .zip, .sql or .sql.gz file", { name: fileName(odd[0]) }));
      return;
    }
    // One .zip may be a site packed by Stackyard; reading its contents list takes a moment.
    if (list.length === 1 && /\.zip$/i.test(list[0]) && canImport) {
      setChecking(true);
      try {
        const file = await api.inspectSiteFile(list[0]);
        setImporting({ path: list[0], file });
        setFiles([]);
        setTyped("");
        return;
      } catch {
        // Not one of Stackyard's: a backup of a WordPress site, below.
      } finally {
        setChecking(false);
      }
    }
    if (!canClone) {
      setError(t("To bring in a WordPress backup, turn on the WordPress and Remote servers modules in Settings › Modules."));
      return;
    }
    newClone(list, "import");
    setFiles([]);
    setTyped("");
  }
  startRef.current = (f) => void start(f);

  // Files dropped on the window while this screen is shown start at once.
  useEffect(() => {
    let offDrop: (() => void) | undefined;
    let offHover: (() => void) | undefined;
    let cancelled = false;
    api
      .onDrop((dropped) => {
        setOver(false);
        if (shownNow.current && dropped.length) startRef.current(dropped);
      })
      .then((off) => (cancelled ? off() : (offDrop = off)));
    api
      .onDragHover((on) => setOver(on && shownNow.current))
      .then((off) => (cancelled ? off() : (offHover = off)));
    return () => {
      cancelled = true;
      offDrop?.();
      offHover?.();
    };
  }, []);

  // Backups dropped on the Sites screen.
  useEffect(() => {
    const take = () => {
      const all = pending;
      pending = [];
      for (const f of all) startRef.current(f);
    };
    waiting.add(take);
    take();
    return () => void waiting.delete(take);
  }, []);

  async function browse() {
    const picked = await api.pickFiles(t("Pick the backup or Stackyard export file"), undefined, true);
    if (picked.length) await start(picked);
  }

  function add(e: FormEvent) {
    e.preventDefault();
    const path = cleanPath(typed);
    if (!path) return;
    void start([...files, path]);
  }

  return (
    <>
      <header className="page-header">
        <div>
          <h1>{t("Import backup")}</h1>
          <p className="muted">
            {t("Make a local site from a backup: an export from the Stackyard Connect plugin, a backup from the hosting panel, or a site packed by Stackyard.")}
          </p>
        </div>
      </header>

      <section className="card stack" aria-labelledby="imp-drop-h">
        <h2 id="imp-drop-h" className="sr-only">
          {t("Pick the backup")}
        </h2>
        <div className="drop-zone import-drop" data-over={over || undefined}>
          <Icon name="import" />
          <p>
            <strong>{t("Drop your backup or Stackyard export file here")}</strong>
          </p>
          <p className="muted small">
            {t("A .tar from Stackyard Connect, a .tar.gz or .zip from cPanel, Plesk or DirectAdmin, a zip of the site's files with its .sql, or a .zip packed by Stackyard (Backups › Export).")}
          </p>
          <button type="button" className="btn" onClick={browse} disabled={checking}>
            {t("Browse…")}
          </button>
        </div>
        <form className="field" onSubmit={add}>
          <label htmlFor="imp-path">{t("Or type the file's path")}</label>
          <div className="row folder-input">
            <input
              id="imp-path"
              className="mono grow"
              value={typed}
              placeholder="C:\Users\you\Downloads\example.com-2026-10-30.tar"
              spellCheck={false}
              onChange={(e) => setTyped(e.target.value)}
            />
            <button type="submit" className="btn primary" disabled={!cleanPath(typed) || checking}>
              {t("Import")}
            </button>
          </div>
          <span className="muted small">
            {t("The file stays where it is: Stackyard reads it from there, nothing is uploaded anywhere. A hosting backup with the database in a separate .sql file: drop both at once.")}
          </span>
        </form>
        {checking && <Progress label={t("Checking the file")} state={{ text: t("Checking the file…"), percent: null }} />}
        {error && (
          <p className="error small" role="alert">
            {error}
          </p>
        )}
      </section>

      {importing && (
        <ImportSite
          path={importing.path}
          file={importing.file}
          onClose={() => setImporting(null)}
          onImported={(domain) => {
            setImporting(null);
            openTab(siteTab(domain));
          }}
        />
      )}
      {clones.map((c) => (
        <CloneSite key={c.id} id={c.id} onCloned={(domain) => openTab(siteTab(domain))} />
      ))}
    </>
  );
}

/** Asks where to put a site from a file, then brings it in. */
function ImportSite({
  path,
  file,
  onClose,
  onImported,
}: {
  path: string;
  file: SiteFile;
  onClose: () => void;
  onImported: (domain: string) => void;
}) {
  const t = useT();
  const [name, setName] = useState(file.name);
  const [folder, setFolder] = useState("");
  const [busy, setBusy] = useState(false);
  useBusy(busy);
  const [error, setError] = useState<string | null>(null);
  const [warning, setWarning] = useState<{ domain: string; text: string } | null>(null);
  const [progress, setProgress] = useState<PackProgress | null>(null);
  const ending = file.domain.slice(file.name.length);

  useEffect(() => {
    const off = api.onSitePackProgress((p) => {
      if (p.site === path) setProgress(p);
    });
    return () => void off.then((f) => f());
  }, [path]);

  async function browse() {
    const picked = await api.pickFolder("Pick a new or empty folder for the site", folder || undefined);
    if (picked) setFolder(picked);
  }

  async function submit(e: FormEvent) {
    e.preventDefault();
    setBusy(true);
    setProgress(null);
    setError(null);
    try {
      const result = await api.importSite(path, siteName(name) || null, folder.trim() || null);
      if (result.warning) setWarning({ domain: result.domain, text: result.warning });
      else onImported(result.domain);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  }

  if (warning) {
    return (
      <section className="card stack" aria-label="Site imported">
        <p className="notice small">
          <span className="mono">{warning.domain}</span> is added.
        </p>
        <p className="warn small">{warning.text}</p>
        <div className="row">
          <button type="button" className="btn primary" onClick={() => onImported(warning.domain)}>
            OK
          </button>
        </div>
      </section>
    );
  }
  return (
    <form className="card stack form" aria-labelledby="import-h" onSubmit={submit}>
      <h2 id="import-h">Import {file.domain}</h2>
      <p className="muted small">
        {file.runtime ?? "Static files"}
        {file.database ? ` with the database ${file.database}` : ""}, packed{" "}
        {new Date(file.created).toLocaleDateString()} by Stackyard {file.made_by}.
        {file.database && " Start MySQL, MariaDB or PostgreSQL first so the database can be loaded."}
      </p>
      <div className="form-grid">
        <div className="field">
          <label htmlFor="imp-name">Name</label>
          <div className="input-affix">
            <input id="imp-name" className="mono" value={name} onChange={(e) => setName(e.target.value)} required />
            <span className="mono muted">{ending}</span>
          </div>
        </div>
        <div className="field">
          <label htmlFor="imp-folder">Project folder</label>
          <div className="row folder-input">
            <input
              id="imp-folder"
              className="mono grow"
              value={folder}
              placeholder={file.folder.replace(new RegExp(`${file.name}$`), siteName(name) || file.name)}
              onChange={(e) => setFolder(e.target.value)}
            />
            <button type="button" className="btn" onClick={browse}>
              Browse…
            </button>
          </div>
          <span className="muted small">A new or empty folder.</span>
        </div>
      </div>
      {busy && <Progress label={t("Importing {name}", { name: file.domain })} state={packState(t, progress)} />}
      {error && (
        <p className="error small" role="alert">
          {error}
        </p>
      )}
      <div className="row between centered">
        <button type="button" className="btn ghost" onClick={onClose}>
          Cancel
        </button>
        <button type="submit" className="btn primary" disabled={busy || !siteName(name)}>
          {busy ? "Importing…" : "Import site"}
        </button>
      </div>
    </form>
  );
}
