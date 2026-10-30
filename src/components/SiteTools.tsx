import { useCallback, useEffect, useState, type FormEvent } from "react";
import {
  api,
  errorText,
  inWordPress,
  projectKinds,
  type PackProgress,
  type ProjectKind,
  type Packed,
  type ShareProgress,
  type ShareStatus,
  type Site,
  type SiteBackup,
  type SiteSettingsInfo,
} from "../lib/api";
import { useModules } from "../lib/modules";
import { useT } from "../lib/i18n";
import { Progress, packState, shareState } from "./Progress";
import { SiteRemote } from "./SiteRemote";
import { Icon, type IconName } from "./Icon";
import { BRAND_PATHS } from "../lib/brands";
import { rovingGroup, rovingKeys } from "../lib/focus";
import { WordPressTools } from "./wordpress/WordPressTools";
import type { LiveAsk } from "./wordpress/Live";
import { wp, type WpEnvironment } from "../lib/wordpress";
import { Keep, useBusy, useRememberedTab } from "../lib/tabLife";

function size(bytes: number) {
  if (bytes < 1024 ** 2) return `${Math.max(1, Math.round(bytes / 1024))} KB`;
  if (bytes < 1024 ** 3) return `${(bytes / 1024 ** 2).toFixed(1)} MB`;
  return `${(bytes / 1024 ** 3).toFixed(2)} GB`;
}

function when(ms: number) {
  return new Date(ms).toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });
}

/** `KEY=value` lines, as typed in a box, to pairs; blank lines and `#` notes are skipped. */
function parseLines(text: string, separator: string): Record<string, string> {
  const out: Record<string, string> = {};
  for (const raw of text.split(/\r?\n/)) {
    const line = raw.trim();
    if (!line || line.startsWith("#") || line.startsWith(";")) continue;
    const at = line.indexOf(separator);
    if (at < 1) throw `"${line}" needs a name, then ${separator}, then the value`;
    out[line.slice(0, at).trim()] = line
      .slice(at + 1)
      .trim()
      .replace(/^"(.*)"$/, "$1");
  }
  return out;
}

function joinLines(pairs: Record<string, string> | undefined, separator: string) {
  return Object.entries(pairs ?? {})
    .map(([k, v]) => `${k}${separator}${v}`)
    .join("\n");
}

/** What a site's tools tab panel needs to know after a change. */
type Changed = (domain?: string) => void;

type Section = { id: string; label: string; icon: IconName | "wordpress" };

/**
 * A site's sections in its own tab (WordPress, Settings, Backups, Public link,
 * Servers & Git) as a row of tabs over one card, so picking a section reads
 * differently from the buttons inside it. The arrow keys move between them.
 */
export function SiteTools({ site, onChanged }: { site: Site; onChanged: Changed }) {
  const t = useT();
  const { isOn } = useModules();
  const wordpress = site.framework === "WordPress" || site.framework === "WooCommerce";
  // A copy of a live WordPress site pushes and pulls in Servers & Git.
  const [live, setLive] = useState<WpEnvironment["live"]>(null);
  const [liveAsk, setLiveAsk] = useState<LiveAsk | null>(null);
  const [linked, setLinked] = useState(0);
  useEffect(() => {
    setLive(null);
    if (!wordpress || !isOn("wordpress")) return;
    wp.environment(site.domain)
      .then((env) => setLive(env?.live ?? null))
      .catch(() => setLive(null));
  }, [site.domain, wordpress, isOn, linked]);
  const sections = [
    wordpress && isOn("wordpress") && { id: "WordPress", label: "WordPress", icon: "wordpress" },
    isOn("sitesettings") && { id: "Settings", label: t("Settings"), icon: "manage" },
    (isOn("sitebackups") || isOn("sitetransfer")) && { id: "Backups", label: t("Backups"), icon: "archive" },
    isOn("share") && { id: "Public link", label: t("Public link"), icon: "link" },
    (isOn("remote") || isOn("git") || live) && { id: "Servers & Git", label: t("Servers & Git"), icon: "git" },
  ].filter(Boolean) as Section[];
  const [tab, setTab] = useRememberedTab(`sections:${site.domain}`, "");
  if (sections.length === 0) {
    return <p className="muted small">Turn on site tools in Settings › Modules.</p>;
  }
  const shown = sections.some((x) => x.id === tab) ? tab : sections[0].id;
  const has = (id: string) => sections.some((x) => x.id === id);
  const tabId = (id: string) => `site-section-${site.domain}-${id}`.replace(/\W+/g, "-").toLowerCase();
  return (
    <div className="stack remote-panel">
      <div
        ref={rovingGroup}
        className="site-sections"
        role="tablist"
        aria-label={t("Sections for {site}", { site: site.domain })}
        onKeyDown={(e) => rovingKeys(e, "horizontal")}
      >
        {sections.map((x) => (
          <button
            key={x.id}
            id={tabId(x.id)}
            type="button"
            role="tab"
            data-roving
            className="site-section-tab"
            aria-selected={shown === x.id}
            aria-controls={`site-section-panel-${site.domain}`}
            onClick={() => {
              setTab(x.id);
              setLiveAsk(null);
            }}
          >
            {x.icon === "wordpress" ? (
              <svg viewBox="0 0 24 24" className="filled" aria-hidden="true">
                <path d={BRAND_PATHS.wordpress} />
              </svg>
            ) : (
              <Icon name={x.icon} />
            )}
            {x.label}
          </button>
        ))}
      </div>
      <section id={`site-section-panel-${site.domain}`} role="tabpanel" aria-labelledby={tabId(shown)} className="card stack site-section-panel">
        {has("WordPress") && (
          <Keep shown={shown === "WordPress"}>
            <WordPressTools
              site={site}
              onLive={(way) => {
                setLiveAsk({ way, at: Date.now() });
                setTab("Servers & Git");
              }}
            />
          </Keep>
        )}
        {has("Settings") && (
          <Keep shown={shown === "Settings"}>
            <SettingsTab site={site} onChanged={onChanged} />
          </Keep>
        )}
        {has("Backups") && (
          <Keep shown={shown === "Backups"}>
            <BackupsTab site={site} onChanged={onChanged} />
          </Keep>
        )}
        {has("Public link") && (
          <Keep shown={shown === "Public link"}>
            <ShareTab site={site} />
          </Keep>
        )}
        {has("Servers & Git") && (
          <Keep shown={shown === "Servers & Git"}>
            <SiteRemote site={site} live={live} liveAsk={liveAsk} onLinked={() => setLinked((n) => n + 1)} />
          </Keep>
        )}
      </section>
    </div>
  );
}

/** What the site is for. Changing it makes, finds or unlinks the WordPress
 * a theme or plugin is tried in; it is its own step, apart from Save. */
function ProjectType({ site, onChanged }: { site: Site; onChanged: Changed }) {
  const t = useT();
  const from = site.project.kind;
  const [kind, setKind] = useState<ProjectKind>(from);
  const [changing, setChanging] = useState(false);
  useBusy(changing);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => setKind(from), [from]);

  async function change() {
    setChanging(true);
    try {
      await api.setProjectKind(site.domain, kind);
      setError(null);
      onChanged(site.domain);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setChanging(false);
    }
  }

  const note =
    kind === from
      ? null
      : inWordPress(kind) && !inWordPress(from)
        ? t("Stackyard makes a WordPress to try it in and links this folder into it. The folder itself is not changed. MySQL or MariaDB must be installed.")
        : inWordPress(from) && !inWordPress(kind)
          ? t("The folder is unlinked from its WordPress and served on its own again. The WordPress is kept, in case you change back.")
          : inWordPress(from) && inWordPress(kind)
            ? t("The folder moves from WordPress's themes to its plugins, or the other way, and is switched on there.")
            : null;
  return (
    <div className="stack project-type">
      <div className="form-grid">
        <div className="field">
          <label htmlFor={`st-kind-${site.domain}`}>{t("Project type")}</label>
          <div className="row">
            <select
              id={`st-kind-${site.domain}`}
              className="grow"
              value={kind}
              disabled={changing}
              onChange={(e) => setKind(e.target.value as ProjectKind)}
            >
              {projectKinds.map((k) => (
                <option key={k.id} value={k.id}>
                  {t(k.name)}
                </option>
              ))}
            </select>
            <button type="button" className="btn" disabled={changing || kind === from} onClick={change}>
              {changing ? t("Changing…") : t("Change")}
            </button>
          </div>
          {note && <span className="muted small">{note}</span>}
        </div>
        <div className="field">
          <span className="label">{t("Project folder")}</span>
          <span className="mono small">{site.project.folder}</span>
          {inWordPress(from) ? (
            <span className="muted small">{t("The editor, the terminal and Git open this folder.")}</span>
          ) : (
            <SiteFolder site={site} onChanged={onChanged} />
          )}
        </div>
      </div>
      {error && <p className="error small">{error}</p>}
    </div>
  );
}

/**
 * Changing where a site's files are: point it at another folder (one already
 * copied there), or move the files, e.g. to a bigger drive.
 */
function SiteFolder({ site, onChanged }: { site: Site; onChanged: Changed }) {
  const t = useT();
  const [busy, setBusy] = useState<"change" | "move" | null>(null);
  useBusy(busy !== null);
  const [moved, setMoved] = useState<{ done: number; total: number } | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const parent = site.path.replace(/[\\/][^\\/]*[\\/]?$/, "");

  useEffect(() => {
    let stop: (() => void) | undefined;
    api
      .onSiteMoveProgress((p) => p.site === site.domain && setMoved({ done: p.done, total: p.total }))
      .then((s) => (stop = s));
    return () => stop?.();
  }, [site.domain]);

  async function change() {
    const folder = await api.pickFolder(t("Select the site's folder"), parent);
    if (!folder) return;
    setBusy("change");
    try {
      await api.changeSiteFolder(site.domain, folder);
      setError(null);
      setNote(t("The site now uses {folder}.", { folder }));
      onChanged(site.domain);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(null);
    }
  }

  async function move() {
    const into = await api.pickFolder(t("Move the site into this folder"), parent);
    if (!into) return;
    setBusy("move");
    setMoved(null);
    try {
      const result = await api.moveSite(site.domain, into);
      setError(null);
      setNote(
        result.left_behind
          ? t("Moved to {path}. Some files in {old} were in use and stayed; you can delete that folder later.", {
              path: result.path,
              old: result.left_behind,
            })
          : t("Moved to {path}.", { path: result.path }),
      );
      onChanged(site.domain);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(null);
      setMoved(null);
    }
  }

  const percent = moved && moved.total > 0 ? Math.round((moved.done / moved.total) * 100) : null;
  return (
    <div className="stack site-folder">
      <div className="row">
        <button type="button" className="btn small" disabled={busy !== null} onClick={move}>
          {busy === "move" ? t("Moving…") : t("Move to another folder…")}
        </button>
        <button type="button" className="btn ghost small" disabled={busy !== null} onClick={change}>
          {t("Change folder…")}
        </button>
      </div>
      {busy === "move" && (
        <Progress
          label={t("Moving {site}", { site: site.domain })}
          state={{
            text: moved ? `${size(moved.done)} / ${size(moved.total)}` : t("Moving…"),
            percent,
          }}
        />
      )}
      <span className="muted small">
        {t("Move copies the files to the new place (another drive works too), then removes the old folder. Change folder points the site at a folder you already copied. Settings, database and connections stay.")}
      </span>
      {note && <span className="small" role="status">{note}</span>}
      {error && (
        <span className="error small" role="alert">
          {error}
        </span>
      )}
    </div>
  );
}

function SettingsTab({ site, onChanged }: { site: Site; onChanged: Changed }) {
  const [info, setInfo] = useState<SiteSettingsInfo | null>(null);
  const [tld, setTld] = useState("");
  const [https, setHttps] = useState(true);
  const [root, setRoot] = useState("");
  const [env, setEnv] = useState("");
  const [php, setPhp] = useState("");
  const [command, setCommand] = useState("");
  const [dbName, setDbName] = useState("");
  const [dbServer, setDbServer] = useState<"mysql" | "postgres">("mysql");
  const [saving, setSaving] = useState(false);
  useBusy(saving);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);

  useEffect(() => {
    api
      .siteSettings(site.domain)
      .then((i) => {
        setInfo(i);
        setTld(i.tld);
        setHttps(i.https);
        setRoot(i.settings.root ?? "");
        setEnv(joinLines(i.settings.env, "="));
        setPhp(joinLines(i.settings.php, " = "));
        setCommand(i.settings.command ?? "");
        setDbName(i.settings.database?.name ?? "");
        setDbServer(i.settings.database?.server ?? i.detected_database?.server ?? "mysql");
      })
      .catch((e) => setError(errorText(e)));
  }, [site.domain]);

  async function save(e: FormEvent) {
    e.preventDefault();
    setSaving(true);
    setSaved(false);
    try {
      const settings = {
        root: root.trim() || null,
        env: parseLines(env, "="),
        php: parseLines(php, "="),
        command: command.trim() || null,
        database: dbName.trim() ? { server: dbServer, name: dbName.trim() } : null,
      };
      const result = await api.saveSiteSettings(site.domain, tld, https, settings);
      setError(null);
      setSaved(true);
      onChanged(result.domain);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setSaving(false);
    }
  }

  if (!info) return error ? <p className="error small">{error}</p> : <p className="muted small">Loading…</p>;
  if (info.parked) {
    return (
      <p className="muted small">
        This site comes from a parked folder, so it uses the usual settings. To change them, add its folder as a site
        above (New site › Project folder).
      </p>
    );
  }
  const name = site.domain.slice(0, site.domain.length - info.tld.length);
  const ownServer = info.port !== null;
  return (
    <>
      <ProjectType site={site} onChanged={onChanged} />
      <form className="stack form" onSubmit={save} aria-label={`Settings for ${site.domain}`}>
        <div className="form-grid">
          <div className="field">
            <label htmlFor={`st-tld-${site.domain}`}>Domain ending</label>
            <div className="input-affix">
              <span className="mono muted">{name}</span>
              <input
                id={`st-tld-${site.domain}`}
                className="mono"
                value={tld}
                onChange={(e) => setTld(e.target.value)}
                spellCheck={false}
              />
            </div>
            <span className="muted small">Just for this site, e.g. .test. Other sites keep theirs.</span>
          </div>
          <div className="field">
            <label htmlFor={`st-root-${site.domain}`}>Folder served</label>
            <input
              id={`st-root-${site.domain}`}
              className="mono"
              value={root}
              placeholder={info.detected_root || "(the project folder)"}
              onChange={(e) => setRoot(e.target.value)}
              spellCheck={false}
              disabled={ownServer}
            />
            <span className="muted small">
              {ownServer
                ? "This site's own server decides what it serves."
                : "Inside the project folder, e.g. public. Leave it empty to let Stackyard pick."}
            </span>
          </div>
          <div className="field">
            <label htmlFor={`st-db-${site.domain}`}>Database</label>
            <div className="row">
              <select
                aria-label="Database server"
                value={dbServer}
                onChange={(e) => setDbServer(e.target.value as "mysql" | "postgres")}
              >
                <option value="mysql">MySQL or MariaDB</option>
                <option value="postgres">PostgreSQL</option>
              </select>
              <input
                id={`st-db-${site.domain}`}
                className="mono grow"
                value={dbName}
                placeholder={info.detected_database?.name ?? "none"}
                onChange={(e) => setDbName(e.target.value)}
                spellCheck={false}
              />
            </div>
            <span className="muted small">
              Goes into the site's backups and exports.
              {info.detected_database && !dbName && ` Found ${info.detected_database.name} in the project.`}
            </span>
          </div>
          <div className="field">
            <label htmlFor={`st-cmd-${site.domain}`}>Start command</label>
            <input
              id={`st-cmd-${site.domain}`}
              className="mono"
              value={command}
              placeholder={info.php ? "e.g. npm run dev, for Vite" : "e.g. npm run start"}
              onChange={(e) => setCommand(e.target.value)}
              spellCheck={false}
            />
            <span className="muted small">
              Runs in the project folder while the web server runs; its output is in Logs as dev-{site.name}.
              {info.port !== null && ` It replaces the usual server and gets port ${info.port} in PORT.`}
            </span>
          </div>
        </div>
        <div className="form-grid">
          <div className="field">
            <label htmlFor={`st-env-${site.domain}`}>Environment variables</label>
            <textarea
              id={`st-env-${site.domain}`}
              className="mono"
              rows={4}
              value={env}
              placeholder={"APP_ENV=local\nAPI_URL=https://api.test"}
              onChange={(e) => setEnv(e.target.value)}
              spellCheck={false}
            />
            <span className="muted small">One NAME=value per line. PHP sees them in getenv() and $_SERVER.</span>
          </div>
          {info.php && (
            <div className="field">
              <label htmlFor={`st-php-${site.domain}`}>PHP settings</label>
              <textarea
                id={`st-php-${site.domain}`}
                className="mono"
                rows={4}
                value={php}
                placeholder={"memory_limit = 1G\nupload_max_filesize = 512M"}
                onChange={(e) => setPhp(e.target.value)}
                spellCheck={false}
              />
              <span className="muted small">php.ini lines for this site only; it gets a PHP process of its own. Other sites keep the version's php.ini.</span>
            </div>
          )}
        </div>
        {error && (
          <p className="error small" role="alert">
            {error}
          </p>
        )}
        <div className="row between centered">
          <label className="check">
            <input type="checkbox" checked={https} onChange={(e) => setHttps(e.target.checked)} />
            Secure with local HTTPS (SSL) certificate
          </label>
          <div className="row centered">
            {saved && (
              <span className="muted small" role="status">
                Saved. The site restarted with the new settings.
              </span>
            )}
            <button type="submit" className="btn primary" disabled={saving}>
              {saving ? "Saving…" : "Save settings"}
            </button>
          </div>
        </div>
      </form>
    </>
  );
}

/** Follows a site's backup, export, restore or import; `null` until the first step. */
function usePackProgress(site: string) {
  const [progress, setProgress] = useState<PackProgress | null>(null);
  useEffect(() => {
    const off = api.onSitePackProgress((p) => {
      if (p.site === site) setProgress(p);
    });
    return () => void off.then((f) => f());
  }, [site]);
  return [progress, setProgress] as const;
}

const RESTORING = "Restoring… (backing up the site as it is now first)";

function BackupsTab({ site, onChanged }: { site: Site; onChanged: Changed }) {
  const t = useT();
  const { isOn } = useModules();
  const [list, setList] = useState<SiteBackup[]>([]);
  const [busy, setBusy] = useState<string | null>(null);
  useBusy(busy !== null);
  const [progress, setProgress] = usePackProgress(site.domain);
  /** What finished, and the file it made when it is in the backups folder. */
  const [done, setDone] = useState<{ text: string; saved: string | null } | null>(null);
  const [warning, setWarning] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [confirm, setConfirm] = useState<string | null>(null);

  const reload = useCallback(() => {
    api
      .siteBackups(site.domain)
      .then(setList)
      .catch((e) => setError(errorText(e)));
  }, [site.domain]);
  useEffect(reload, [reload]);

  async function run(
    label: string,
    action: () => Promise<Packed | null>,
    message: (p: Packed) => string,
    inBackups = false,
  ) {
    setBusy(label);
    setProgress(null);
    setDone(null);
    setWarning(null);
    setError(null);
    try {
      const packed = await action();
      if (packed) {
        setDone({ text: message(packed), saved: inBackups ? packed.path : null });
        setWarning(packed.warning);
      }
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(null);
      setProgress(null);
      reload();
    }
  }

  const what = (p: Packed) =>
    `${size(p.size)}, ${p.files.toLocaleString()} files${p.database ? ` and the database ${p.database}` : ""}`;

  return (
    <div className="stack">
      <p className="muted small">
        A backup is one zip with the site's files (without node_modules), its database and its settings.
      </p>
      <div className="row wrap">
        {isOn("sitebackups") && (
          <button
            type="button"
            className="btn primary"
            disabled={busy !== null}
            onClick={() => run("Backing up…", () => api.backupSite(site.domain), (p) => `Backed up: ${what(p)}.`, true)}
          >
            {busy === "Backing up…" ? busy : "Back up now"}
          </button>
        )}
        {isOn("sitetransfer") && (
          <button
            type="button"
            className="btn"
            disabled={busy !== null}
            onClick={() =>
              run(
                "Exporting…",
                async () => {
                  const dest = await api.pickSaveFile(`Export ${site.domain}`, `${site.domain}.zip`);
                  return dest ? api.exportSite(site.domain, dest) : null;
                },
                (p) => `Exported to ${p.path} (${what(p)}). Open it on another computer with Import site on the Sites screen.`,
              )
            }
          >
            {busy === "Exporting…" ? busy : "Export to a file…"}
          </button>
        )}
        <button type="button" className="btn ghost" onClick={() => api.openSiteBackups(site.domain)}>
          Show backups folder
        </button>
      </div>
      {busy && <Progress label={busy} state={packState(t, progress, busy === RESTORING)} />}
      {done && (
        <div className="notice small stack tight" role="status">
          <span>{done.text}</span>
          {done.saved && (
            <span className="row wrap centered">
              <span>
                {t("Saved to")} <span className="mono">{done.saved}</span>
              </span>
              <button type="button" className="btn small ghost" onClick={() => api.openSiteBackups(site.domain)}>
                {t("Show in folder")}
              </button>
            </span>
          )}
        </div>
      )}
      {warning && <p className="warn small">{warning}</p>}
      {error && (
        <p className="error small" role="alert">
          {error}
        </p>
      )}
      {list.length === 0 ? (
        <p className="muted small">No backups yet.</p>
      ) : (
        list.map((b) => (
          <div key={b.file} className="list-row">
            <span className="small">
              {when(b.created)} <span className="muted">· {size(b.size)}</span>
              {b.database && <span className="muted"> · database {b.database}</span>}
            </span>
            <span className="row">
              {confirm === b.file ? (
                <>
                  <span className="warn small">
                    Replace the site's files and database with this backup? Stackyard backs up how it is now first.
                  </span>
                  <button
                    type="button"
                    className="btn small primary"
                    disabled={busy !== null}
                    onClick={() => {
                      setConfirm(null);
                      run(
                        RESTORING,
                        () => api.restoreSite(site.domain, b.file),
                        () => `Restored from the backup of ${when(b.created)}. How it was before is the newest backup.`,
                        true,
                      ).then(() => onChanged());
                    }}
                  >
                    Restore
                  </button>
                  <button type="button" className="btn small ghost" onClick={() => setConfirm(null)}>
                    Cancel
                  </button>
                </>
              ) : (
                <>
                  {isOn("sitebackups") && (
                    <button
                      type="button"
                      className="btn small"
                      disabled={busy !== null}
                      onClick={() => setConfirm(b.file)}
                    >
                      Restore…
                    </button>
                  )}
                  <button
                    type="button"
                    className="btn small ghost"
                    aria-label={`Delete the backup of ${when(b.created)}`}
                    disabled={busy !== null}
                    onClick={() =>
                      api
                        .removeSiteBackup(site.domain, b.file)
                        .then(setList)
                        .catch((e) => setError(errorText(e)))
                    }
                  >
                    Delete
                  </button>
                </>
              )}
            </span>
          </div>
        ))
      )}
    </div>
  );
}

function ShareTab({ site }: { site: Site }) {
  const t = useT();
  const [status, setStatus] = useState<ShareStatus | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  useBusy(busy !== null);
  const [progress, setProgress] = useState<ShareProgress | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    api
      .shareStatus()
      .then(setStatus)
      .catch((e) => setError(errorText(e)));
  }, []);
  useEffect(() => {
    const off = api.onShareProgress((p) => {
      if (p.site === site.domain) setProgress(p);
    });
    return () => void off.then((f) => f());
  }, [site.domain]);

  const link = status?.links.find((l) => l.site === site.domain);

  async function start() {
    setBusy(status?.cloudflared ? "Asking Cloudflare for an address…" : "Downloading cloudflared, then asking Cloudflare for an address…");
    setProgress(status?.cloudflared ? { site: site.domain, step: "connecting" } : null);
    setError(null);
    try {
      await api.shareStart(site.domain);
      setStatus(await api.shareStatus());
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(null);
      setProgress(null);
    }
  }

  async function stop() {
    try {
      setStatus(await api.shareStop(site.domain));
    } catch (e) {
      setError(errorText(e));
    }
  }

  async function copy(url: string) {
    try {
      await navigator.clipboard.writeText(url);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch {
      setError("Could not copy; select the address and copy it by hand.");
    }
  }

  return (
    <div className="stack">
      <p className="muted small">
        Gives this site a temporary public address, so someone elsewhere can see it. Anyone with the address can open
        the site while the link is on. It uses Cloudflare's free quick tunnels (no account needed): good for showing
        work, not for hosting. Cloudflare's terms of use apply.
      </p>
      {link?.url ? (
        <div className="list-row">
          <span className="mono small">{link.url}</span>
          <span className="row">
            <button type="button" className="btn small" onClick={() => copy(link.url!)}>
              {copied ? "Copied" : "Copy"}
            </button>
            <button type="button" className="btn small" onClick={() => api.openShareLink(site.domain)}>
              Open
            </button>
            <button type="button" className="btn small ghost" onClick={stop}>
              Stop link
            </button>
          </span>
        </div>
      ) : (
        <div className="row centered wrap">
          <button
            type="button"
            className="btn primary"
            disabled={busy !== null || status?.web_running === false}
            onClick={start}
          >
            {busy ? "Making the link…" : "Make a public link"}
          </button>
          {status?.web_running === false && (
            <span className="muted small">Start the web server first (Services › Caddy).</span>
          )}
        </div>
      )}
      {busy && (
        <>
          <p className="muted small">{busy}</p>
          <Progress label={busy} state={shareState(t, progress)} />
        </>
      )}
      {link?.url && (
        <p className="muted small">
          Apps that write their own address into pages (WordPress's Site Address, Laravel's APP_URL) may still link to{" "}
          <span className="mono">{site.domain}</span>; those links only work on this computer.
        </p>
      )}
      {error && (
        <p className="error small" role="alert">
          {error}
        </p>
      )}
      {status?.cloudflared && (
        <p className="muted small">
          cloudflared {status.cloudflared} is downloaded.{" "}
          <button
            type="button"
            className="link small"
            onClick={() =>
              api
                .shareUninstall()
                .then(setStatus)
                .catch((e) => setError(errorText(e)))
            }
          >
            Remove it
          </button>
        </p>
      )}
    </div>
  );
}
