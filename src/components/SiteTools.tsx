import { useCallback, useEffect, useState, type FormEvent } from "react";
import {
  api,
  errorText,
  type Packed,
  type ShareStatus,
  type Site,
  type SiteBackup,
  type SiteSettingsInfo,
} from "../lib/api";
import { useModules } from "../lib/modules";
import { SiteRemote } from "./SiteRemote";
import { WordPressTools } from "./wordpress/WordPressTools";

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

/** The tools under a site's row on the Sites screen: its settings, backups, public link, servers and Git. */
export function SiteTools({ site, onChanged }: { site: Site; onChanged: Changed }) {
  const { isOn } = useModules();
  const wordpress = site.framework === "WordPress" || site.framework === "WooCommerce";
  const tabs = [
    wordpress && isOn("wordpress") && "WordPress",
    isOn("sitesettings") && "Settings",
    (isOn("sitebackups") || isOn("sitetransfer")) && "Backups",
    isOn("share") && "Public link",
    (isOn("remote") || isOn("git")) && "Servers & Git",
  ].filter(Boolean) as string[];
  const [tab, setTab] = useState(tabs[0] ?? "Settings");
  if (tabs.length === 0) {
    return <p className="muted small">Turn on site tools in Settings › Modules.</p>;
  }
  const shown = tabs.includes(tab) ? tab : tabs[0];
  return (
    <div className="stack remote-panel">
      {tabs.length > 1 && (
        <div className="row wrap" role="group" aria-label={`Tools for ${site.domain}`}>
          {tabs.map((t) => (
            <button key={t} type="button" className="tab" aria-pressed={shown === t} onClick={() => setTab(t)}>
              {t}
            </button>
          ))}
        </div>
      )}
      {shown === "WordPress" && <WordPressTools site={site} />}
      {shown === "Settings" && <SettingsTab site={site} onChanged={onChanged} />}
      {shown === "Backups" && <BackupsTab site={site} onChanged={onChanged} />}
      {shown === "Public link" && <ShareTab site={site} />}
      {shown === "Servers & Git" && <SiteRemote site={site} />}
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
  );
}

function BackupsTab({ site, onChanged }: { site: Site; onChanged: Changed }) {
  const { isOn } = useModules();
  const [list, setList] = useState<SiteBackup[]>([]);
  const [busy, setBusy] = useState<string | null>(null);
  const [done, setDone] = useState<string | null>(null);
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

  async function run(label: string, action: () => Promise<Packed | null>, message: (p: Packed) => string) {
    setBusy(label);
    setDone(null);
    setWarning(null);
    setError(null);
    try {
      const packed = await action();
      if (packed) {
        setDone(message(packed));
        setWarning(packed.warning);
      }
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(null);
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
            onClick={() => run("Backing up…", () => api.backupSite(site.domain), (p) => `Backed up: ${what(p)}.`)}
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
      {done && (
        <p className="notice small" role="status">
          {done}
        </p>
      )}
      {warning && <p className="warn small">{warning}</p>}
      {busy && busy !== "Backing up…" && busy !== "Exporting…" && (
        <p className="muted small" role="status">
          {busy}
        </p>
      )}
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
                        "Restoring… (backing up the site as it is now first)",
                        () => api.restoreSite(site.domain, b.file),
                        () => `Restored from the backup of ${when(b.created)}. How it was before is the newest backup.`,
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
  const [status, setStatus] = useState<ShareStatus | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    api
      .shareStatus()
      .then(setStatus)
      .catch((e) => setError(errorText(e)));
  }, []);

  const link = status?.links.find((l) => l.site === site.domain);

  async function start() {
    setBusy(status?.cloudflared ? "Asking Cloudflare for an address…" : "Downloading cloudflared, then asking Cloudflare for an address…");
    setError(null);
    try {
      await api.shareStart(site.domain);
      setStatus(await api.shareStatus());
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(null);
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
        <p className="muted small" role="status">
          {busy}
        </p>
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
