import { useEffect, useRef, useState, type FormEvent } from "react";
import { api, errorText, type Site } from "../../lib/api";
import { useT } from "../../lib/i18n";
import {
  wp,
  type WpCloneProgress,
  type WpCloneSource,
  type WpFound,
  type WpMedia,
  type WpProtocol,
  type WpRoute,
  type WpSavedConnection,
  type WpSizes,
} from "../../lib/wordpress";
import { size } from "./WordPressTools";
import { closeClone, editClone, minimizeClone, retryClone, startClone, stopClone, useClones } from "../../lib/clones";
import { CloneProgress } from "./Clones";

type T = ReturnType<typeof useT>;
type From = "saved" | "new" | "files";

/** The core's errors end with this when it needs the live site's address typed in. */
const NEED_ADDRESS = "Type it in the Live site's address box";

/** The site this name would take, e.g. `shop` → `shop.stack` (stagings have more dots). */
function takenBy(sites: Site[], name: string) {
  return sites.find((s) => s.domain.startsWith(`${name}.`) && s.domain.split(".").length === 2);
}

/** `shop` → `shop-2`, or the next number no site uses. */
function freeName(sites: Site[], name: string) {
  const base = name.replace(/-\d+$/, "");
  for (let n = 2; ; n++) if (!takenBy(sites, `${base}-${n}`)) return `${base}-${n}`;
}

/** `https://www.shop.example/` → `shop`. */
function nameFrom(url: string) {
  const host = url.replace(/^[a-z]+:\/\//i, "").split(/[/:]/)[0].replace(/^www\./, "");
  return (host.split(".")[0] || "")
    .toLowerCase()
    .replace(/[^a-z0-9-]+/g, "-")
    .replace(/^-+|-+$/g, "");
}

function routeText(t: T, route: WpRoute | null) {
  if (route === "ssh") return t("SSH: files over SFTP, database exported on the server");
  if (route === "helper") return t("Files over SFTP or FTP, database through Stackyard's helper");
  return t("Backup files from this computer");
}

export function stageText(t: T, stage: WpCloneProgress["stage"]) {
  return {
    connecting: t("Connecting"),
    files: t("Files"),
    database: t("Database"),
    loading: t("Loading"),
    finishing: t("Finishing"),
  }[stage];
}

/** How much the live site's files hold: WordPress core, themes, plugins, media and all of them. */
function FileSizes({ sizes }: { sizes: WpSizes }) {
  const t = useT();
  const n = (count: number) => count.toLocaleString();
  const all = ["core", "themes", "plugins", "uploads", "other"] as const;
  const total = all.reduce((sum, k) => ({ files: sum.files + sizes[k].files, bytes: sum.bytes + sizes[k].bytes }), { files: 0, bytes: 0 });
  const more = sizes.complete ? "" : `${t("at least")} `;
  const rows: [string, string][] = [
    [t("WordPress core"), `${t("{count} files", { count: n(sizes.core.files) })} · ${size(sizes.core.bytes)}`],
    [t("Themes"), `${t("{count} themes", { count: n(sizes.themes.items) })} · ${size(sizes.themes.bytes)}`],
    [t("Plugins"), `${t("{count} plugins", { count: n(sizes.plugins.items) })} · ${size(sizes.plugins.bytes)}`],
    [t("Media (uploads)"), `${t("{count} files", { count: n(sizes.uploads.files) })} · ${size(sizes.uploads.bytes)}`],
  ];
  if (sizes.other.files > 0) rows.push([t("Other files"), `${t("{count} files", { count: n(sizes.other.files) })} · ${size(sizes.other.bytes)}`]);
  return (
    <>
      {rows.map(([label, value]) => (
        <div key={label} className="list-row">
          <dt className="muted small">{label}</dt>
          <dd className="small">
            {more}
            {value}
          </dd>
        </div>
      ))}
      <div className="list-row">
        <dt className="small">{t("All files")}</dt>
        <dd className="small">
          <strong>
            {more}
            {t("{count} files", { count: n(total.files) })} · {size(total.bytes)}
          </strong>
          {sizes.skipped.files > 0 && (
            <span className="muted">
              {" "}
              · {t("{size} of caches and backups inside the site are left out", { size: size(sizes.skipped.bytes) })}
            </span>
          )}
        </dd>
      </div>
    </>
  );
}

/** New site › Clone from a live server: find WordPress, choose, clone. Each form is one clone of many. */
export function CloneSite({ id, onCloned }: { id: string; onCloned: (domain: string) => void }) {
  const t = useT();
  const job = useClones().find((c) => c.id === id);
  const [from, setFrom] = useState<From>("new");
  const [saved, setSaved] = useState<WpSavedConnection[]>([]);
  const [savedId, setSavedId] = useState("");
  const [protocol, setProtocol] = useState<WpProtocol>("sftp");
  const [host, setHost] = useState("");
  const [port, setPort] = useState("");
  const [user, setUser] = useState("");
  const [password, setPassword] = useState("");
  const [keyFile, setKeyFile] = useState("");
  const [remoteDir, setRemoteDir] = useState("");
  const [files, setFiles] = useState<string[]>([]);
  const [url, setUrl] = useState("");
  const [found, setFound] = useState<WpFound | null>(null);
  const [root, setRoot] = useState("");
  const [name, setName] = useState("");
  const [media, setMedia] = useState<WpMedia>("download");
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [unfinished, setUnfinished] = useState(false);
  const [sites, setSites] = useState<Site[]>([]);
  const [replace, setReplace] = useState(false);
  const urlInput = useRef<HTMLInputElement>(null);
  const needsAddress = !!error?.includes(NEED_ADDRESS);
  const existing = name ? takenBy(sites, name) : undefined;

  useEffect(() => {
    api
      .listSites()
      .then(setSites)
      .catch(() => setSites([]));
  }, []);
  useEffect(() => setReplace(false), [name]);
  useEffect(() => {
    if (needsAddress) urlInput.current?.focus();
  }, [needsAddress]);

  useEffect(() => {
    wp.savedConnections()
      .then((list) => {
        setSaved(list);
        if (list.length) setSavedId(list[0].id);
      })
      .catch(() => setSaved([]));
  }, []);
  useEffect(() => {
    if (!name) return;
    wp.cloneUnfinished(name)
      .then(setUnfinished)
      .catch(() => setUnfinished(false));
  }, [name, error]);

  function source(): WpCloneSource {
    if (from === "saved") return { kind: "saved", id: savedId };
    if (from === "files") return { kind: "files", files };
    return {
      kind: "new",
      connection: {
        site: "",
        name: "Live",
        protocol,
        host: host.trim(),
        port: port.trim() ? Number(port) : null,
        user: user.trim(),
        password: password || null,
        key_file: keyFile.trim() || null,
        remote_dir: remoteDir.trim(),
      },
    };
  }

  async function look(e: FormEvent) {
    e.preventDefault();
    setBusy(t("Looking for WordPress…"));
    setError(null);
    setFound(null);
    try {
      const result = await wp.cloneInspect(id, source(), root || null, url.trim() || null);
      setFound(result);
      setRoot(result.root);
      if (result.url && !url) setUrl(result.url);
      if (!name) setName(nameFrom(result.url || url));
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(null);
    }
  }

  function clone() {
    setError(null);
    const live = url.trim() || found?.url || "";
    const label =
      live.replace(/^[a-z]+:\/\//i, "").replace(/\/$/, "") ||
      (from === "files" ? files.map((f) => f.split(/[\\/]/).pop()).join(", ") : host.trim() || saved.find((c) => c.id === savedId)?.host || "");
    void startClone(
      id,
      source(),
      { name, folder: null, root: root || null, url: url.trim() || null, media, https: true, replace: !!existing && replace },
      label,
    );
  }

  async function pickFiles() {
    const picked = await api.pickFiles(t("Pick the backup files (archives and .sql)"), undefined, true);
    if (picked.length) {
      setFiles(picked);
      setFound(null);
    }
  }

  async function pickKey() {
    const [picked] = await api.pickFiles(t("Pick the private key"), undefined, false);
    if (picked) setKeyFile(picked);
  }

  if (job && job.state !== "new") return <CloneJob id={id} onCloned={onCloned} />;

  const ready = from === "files" ? files.length > 0 : from === "saved" ? !!savedId : !!host.trim() && !!user.trim();
  return (
    <section className="card stack" aria-labelledby={`${id}-h`}>
      <div className="row between centered">
        <h2 id={`${id}-h`}>{t("Clone from a live server")}</h2>
        <button type="button" className="btn ghost" onClick={() => closeClone(id)} disabled={busy !== null}>
          {t("Cancel")}
        </button>
      </div>
      <p className="muted small">
        {t(
          "Copies a live WordPress or WooCommerce site to this computer: files, database and settings, with every live address changed to the local one. Nothing on the live site is changed.",
        )}
      </p>
      <form className="stack form" onSubmit={look}>
        <div className="row wrap" role="radiogroup" aria-label={t("Where is the live site?")}>
          {saved.length > 0 && (
            <label className="row centered small">
              <input type="radio" checked={from === "saved"} onChange={() => setFrom("saved")} />
              {t("A saved server")}
            </label>
          )}
          <label className="row centered small">
            <input type="radio" checked={from === "new"} onChange={() => setFrom("new")} />
            {t("A server (SFTP or FTP)")}
          </label>
          <label className="row centered small">
            <input type="radio" checked={from === "files"} onChange={() => setFrom("files")} />
            {t("Backup files")}
          </label>
        </div>

        {from === "saved" && (
          <div className="field">
            <label htmlFor="cl-saved">{t("Server")}</label>
            <select id="cl-saved" value={savedId} onChange={(e) => setSavedId(e.target.value)}>
              {saved.map((c) => (
                <option key={c.id} value={c.id}>
                  {c.name} · {c.user}@{c.host} ({c.site})
                </option>
              ))}
            </select>
          </div>
        )}

        {from === "new" && (
          <div className="form-grid">
            <div className="field">
              <label htmlFor="cl-protocol">{t("Connect with")}</label>
              <select id="cl-protocol" value={protocol} onChange={(e) => setProtocol(e.target.value as WpProtocol)}>
                <option value="sftp">{t("SFTP (SSH)")}</option>
                <option value="ftps">FTPS</option>
                <option value="ftp">FTP</option>
              </select>
            </div>
            <div className="field">
              <label htmlFor="cl-host">{t("Server address")}</label>
              <input id="cl-host" className="mono" value={host} placeholder="203.0.113.10" onChange={(e) => setHost(e.target.value)} />
            </div>
            <div className="field">
              <label htmlFor="cl-port">{t("Port")}</label>
              <input
                id="cl-port"
                className="mono"
                inputMode="numeric"
                value={port}
                placeholder={protocol === "sftp" ? "22" : "21"}
                onChange={(e) => setPort(e.target.value.replace(/\D/g, ""))}
              />
            </div>
            <div className="field">
              <label htmlFor="cl-user">{t("User name")}</label>
              <input id="cl-user" className="mono" value={user} onChange={(e) => setUser(e.target.value)} />
            </div>
            <div className="field">
              <label htmlFor="cl-password">{keyFile ? t("Key passphrase") : t("Password")}</label>
              <input id="cl-password" type="password" value={password} onChange={(e) => setPassword(e.target.value)} />
            </div>
            {protocol === "sftp" && (
              <div className="field">
                <label htmlFor="cl-key">{t("Private key (optional)")}</label>
                <div className="row folder-input">
                  <input id="cl-key" className="mono grow" value={keyFile} onChange={(e) => setKeyFile(e.target.value)} />
                  <button type="button" className="btn" onClick={pickKey}>
                    {t("Browse…")}
                  </button>
                </div>
              </div>
            )}
            <div className="field">
              <label htmlFor="cl-dir">{t("Folder on the server (optional)")}</label>
              <input id="cl-dir" className="mono" value={remoteDir} placeholder="/home/user/public_html" onChange={(e) => setRemoteDir(e.target.value)} />
              <span className="muted small">{t("Stackyard looks for WordPress in it and the folders below.")}</span>
            </div>
          </div>
        )}

        {from === "files" && (
          <div className="stack">
            <div className="row">
              <button type="button" className="btn" onClick={pickFiles}>
                {t("Pick backup files…")}
              </button>
              {files.length > 0 && <span className="mono small muted">{files.map((f) => f.split(/[\\/]/).pop()).join(", ")}</span>}
            </div>
            <span className="muted small">
              {t(
                "A full backup from the hosting panel (cPanel, Plesk, DirectAdmin) or a zip of the site's files, plus the database export (.sql or .sql.gz) if it is separate.",
              )}
            </span>
          </div>
        )}

        {from !== "files" && (
          <div className="field">
            <label htmlFor="cl-url">{t("Live site's address")}</label>
            <input
              id="cl-url"
              ref={urlInput}
              className="mono"
              value={url}
              placeholder="https://example.com"
              aria-describedby={needsAddress ? "cl-url-why" : undefined}
              onChange={(e) => setUrl(e.target.value)}
            />
            {needsAddress ? (
              <div id="cl-url-why" className="notice small stack">
                <span>{t("Stackyard found the server, but needs the live site's address to read its database. Type it above, e.g. https://example.com, and press Look for WordPress again.")}</span>
                <span className="muted">{error}</span>
              </div>
            ) : (
              <span className="muted small">
                {t("Needed only when the server has no SSH: the database then comes through the site's own address.")}
              </span>
            )}
          </div>
        )}

        <div className="row">
          <button type="submit" className="btn" disabled={!ready || busy !== null}>
            {busy === t("Looking for WordPress…") ? busy : t("Look for WordPress")}
          </button>
        </div>
      </form>

      {found && (
        <div className="stack">
          <dl className="facts">
            {found.roots.length > 1 ? (
              <div className="list-row">
                <dt className="muted small">{t("WordPress folder")}</dt>
                <dd className="small">
                  <select value={root} onChange={(e) => setRoot(e.target.value)} aria-label={t("WordPress folder")}>
                    {found.roots.map((r) => (
                      <option key={r} value={r}>
                        {r}
                      </option>
                    ))}
                  </select>
                </dd>
              </div>
            ) : (
              <div className="list-row">
                <dt className="muted small">{t("WordPress folder")}</dt>
                <dd className="small mono">{found.root}</dd>
              </div>
            )}
            {found.url && (
              <div className="list-row">
                <dt className="muted small">{t("Address")}</dt>
                <dd className="small mono">{found.url}</dd>
              </div>
            )}
            <div className="list-row">
              <dt className="muted small">{t("Versions")}</dt>
              <dd className="small">
                {[found.wordpress && `WordPress ${found.wordpress}`, found.php && `PHP ${found.php}`, found.mysql]
                  .filter(Boolean)
                  .join(" · ") || t("unknown")}
              </dd>
            </div>
            <div className="list-row">
              <dt className="muted small">{t("Database")}</dt>
              <dd className="small">
                {found.tables > 0 ? `${t("{count} tables", { count: found.tables })} · ` : ""}
                {size(found.database_bytes)}
              </dd>
            </div>
            {found.files && <FileSizes sizes={found.files} />}
            <div className="list-row">
              <dt className="muted small">{t("How")}</dt>
              <dd className="small">{routeText(t, found.route)}</dd>
            </div>
          </dl>
          {found.files && !found.files.complete && (
            <p className="muted small">{t("Counting the files on the server took too long, so the sizes above are what was counted before it stopped.")}</p>
          )}
          {found.notes.map((n) => (
            <p key={n} className="muted small">
              {n}
            </p>
          ))}
          <div className="form-grid">
            <div className="field">
              <label htmlFor="cl-name">{t("Name")}</label>
              <div className="input-affix">
                <input
                  id="cl-name"
                  className="mono"
                  value={name}
                  onChange={(e) => setName(e.target.value.toLowerCase().replace(/[^a-z0-9-]/g, "-"))}
                  required
                />
                <span className="mono muted">.stack</span>
              </div>
            </div>
            {from !== "files" && (
              <div className="field">
                <span className="small">{t("Media (wp-content/uploads)")}</span>
                <label className="row centered small">
                  <input type="radio" checked={media === "download"} onChange={() => setMedia("download")} />
                  {t("Download it all")}
                  {found.files ? ` (${size(found.files.uploads.bytes)})` : ""}
                </label>
                <label className="row centered small">
                  <input type="radio" checked={media === "from-live"} onChange={() => setMedia("from-live")} />
                  {t("Leave it on the live site; load each file when a page needs it")}
                </label>
              </div>
            )}
          </div>
          {existing && !unfinished && (
            <div className="notice small stack">
              <span>{t("A site named {domain} already exists.", { domain: existing.domain })}</span>
              <label className="row centered">
                <input type="checkbox" checked={replace} onChange={(e) => setReplace(e.target.checked)} />
                {t("Replace it with the copy. Stackyard backs it up first (files and database), so you can restore it under Manage › Backups.")}
              </label>
              <div className="row">
                <button type="button" className="btn" onClick={() => setName(freeName(sites, name))}>
                  {t("Use {name} instead", { name: freeName(sites, name) })}
                </button>
              </div>
            </div>
          )}
          {unfinished && !busy && (
            <p className="notice small">{t("An earlier clone with this name stopped halfway; cloning again continues where it stopped.")}</p>
          )}
          <div className="row">
            <button
              type="button"
              className={`btn ${existing && replace ? "danger" : "primary"}`}
              disabled={!name || busy !== null || (!!existing && !unfinished && !replace)}
              onClick={clone}
            >
              {unfinished ? t("Continue the clone") : existing && replace ? t("Replace and clone") : t("Clone")}
            </button>
          </div>
        </div>
      )}
      {error && !needsAddress && (
        <p className="error small" role="alert">
          {error}
        </p>
      )}
    </section>
  );
}

/** A clone that was started: how far it is, then the new site or what went wrong. */
function CloneJob({ id, onCloned }: { id: string; onCloned: (domain: string) => void }) {
  const t = useT();
  const job = useClones().find((c) => c.id === id);
  if (!job) return null;
  const domain = job.result ? `${job.result.site.name}${job.result.site.tld}` : `${job.name}.stack`;
  const minimize = (
    <button type="button" className="btn ghost" onClick={() => minimizeClone(id)} title={t("Keeps cloning; the status bar shows every clone")}>
      {t("Minimize")}
    </button>
  );

  if (job.state === "done" && job.result) {
    const cloned = job.result;
    return (
      <section className="card stack" aria-labelledby={`${id}-h`}>
        <div className="row between centered">
          <h2 id={`${id}-h`}>{t("{domain} is ready", { domain })}</h2>
          {minimize}
        </div>
        <p className="muted small">
          {t("Cloned from {live}; the address was changed in {count} places.", {
            live: cloned.live,
            count: cloned.replaced,
          })}{" "}
          {t("Mail goes to Mailpit, search engines are kept away, and scheduled tasks can't call outside services.")}
        </p>
        {cloned.notes.map((n) => (
          <p key={n} className="warn small">
            {n}
          </p>
        ))}
        <div className="row">
          <button type="button" className="btn" onClick={() => api.openLocalPage(cloned.url)}>
            {t("Open site")}
          </button>
          <button
            type="button"
            className="btn primary"
            onClick={() => {
              closeClone(id);
              onCloned(domain);
            }}
          >
            {t("Done")}
          </button>
        </div>
      </section>
    );
  }

  return (
    <section className="card stack" aria-labelledby={`${id}-h`}>
      <div className="row between centered">
        <h2 id={`${id}-h`}>
          {job.state === "running" ? t("Cloning {domain}", { domain }) : t("{domain} was not cloned", { domain })}
        </h2>
        {minimize}
      </div>
      {job.from && <p className="muted small mono">{t("From {live}", { live: job.from })}</p>}
      {job.state === "running" && <CloneProgress clone={job} />}
      {job.error && (
        <p className="error small" role="alert">
          {job.error}
        </p>
      )}
      <div className="row">
        {job.state === "running" ? (
          <button type="button" className="btn" onClick={() => stopClone(id)}>
            {t("Stop")}
          </button>
        ) : (
          <>
            <button type="button" className="btn primary" onClick={() => retryClone(id)}>
              {t("Try again")}
            </button>
            <button type="button" className="btn" onClick={() => editClone(id)}>
              {t("Change settings")}
            </button>
            <button type="button" className="btn ghost" onClick={() => closeClone(id)}>
              {t("Close")}
            </button>
          </>
        )}
      </div>
      {job.state === "failed" && (
        <p className="muted small">{t("Trying again continues where it stopped; files already downloaded are kept.")}</p>
      )}
    </section>
  );
}
