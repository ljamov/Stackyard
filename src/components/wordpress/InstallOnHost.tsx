import { useEffect, useState, type FormEvent } from "react";
import { api, errorText, type RemoteConnection, type Site } from "../../lib/api";
import { useT } from "../../lib/i18n";
import { useBusy } from "../../lib/tabLife";
import { wp, type WpInstalled } from "../../lib/wordpress";
import { Progress, useSyncProgress } from "./Live";

/** The host's name in an address, e.g. `example.com`. */
function hostOf(url: string) {
  const m = /^https?:\/\/([^/?#\s]+)/i.exec(url.trim());
  return m ? m[1] : "";
}

/**
 * The Live site part of a WordPress site that isn't a copy of one yet: puts
 * it on a host through one of its servers, into a WordPress already there.
 * Afterwards it is a copy of the host's site, to push and pull.
 */
export function InstallOnHost({ site, onInstalled }: { site: Site; onInstalled: () => void }) {
  const t = useT();
  const [servers, setServers] = useState<RemoteConnection[] | null>(null);
  const [server, setServer] = useState("");
  const [url, setUrl] = useState("");
  const [confirm, setConfirm] = useState("");
  const [running, setRunning] = useState(false);
  useBusy(running);
  const [error, setError] = useState<string | null>(null);
  const [done, setDone] = useState<WpInstalled | null>(null);
  const [progress, setProgress] = useSyncProgress(site.domain);
  const host = hostOf(url);

  useEffect(() => {
    api
      .remoteConnections(site.domain)
      .then((d) => {
        setServers(d.connections);
        if (d.connections.length) setServer(d.connections[0].id);
      })
      .catch((e) => setError(errorText(e)));
  }, [site.domain]);

  async function install(e: FormEvent) {
    e.preventDefault();
    setRunning(true);
    setError(null);
    setProgress(null);
    try {
      setDone(await wp.install(site.domain, server, url.trim(), confirm.trim()));
    } catch (err) {
      setError(errorText(err));
    } finally {
      setRunning(false);
    }
  }

  if (done) {
    return (
      <div className="stack">
        <p className="notice small">
          {t("{domain} is on {url}. {count} files were sent; the host's database was backed up first.", {
            domain: site.domain,
            url: url.trim(),
            count: done.files,
          })}
        </p>
        {done.notes.map((n) => (
          <p key={n} className="muted small">
            {n}
          </p>
        ))}
        <div className="row">
          <button type="button" className="btn primary" onClick={onInstalled}>
            {t("Open Live site")}
          </button>
        </div>
      </div>
    );
  }

  return (
    <form className="stack form" onSubmit={install}>
      <p className="muted small">
        {t(
          "This site isn't linked to a live site yet. Put it on a host: Stackyard uploads every file and the database into a WordPress already installed there, with every address changed to the host's. After that, push and pull work as for a cloned site.",
        )}
      </p>
      {servers && servers.length === 0 ? (
        <p className="notice small">
          {t(
            "Add the host under Servers first: SFTP, FTP, or the Stackyard Connect plugin with a code that allows changes.",
          )}
        </p>
      ) : (
        <>
          <div className="form-grid">
            <div className="field">
              <label htmlFor={`in-server-${site.domain}`}>{t("Server")}</label>
              <select id={`in-server-${site.domain}`} value={server} onChange={(e) => setServer(e.target.value)}>
                {(servers ?? []).map((c) => (
                  <option key={c.id} value={c.id}>
                    {c.name} · {c.host}
                  </option>
                ))}
              </select>
            </div>
            <div className="field">
              <label htmlFor={`in-url-${site.domain}`}>{t("Live site's address")}</label>
              <input
                id={`in-url-${site.domain}`}
                className="mono"
                value={url}
                placeholder="https://example.com"
                onChange={(e) => setUrl(e.target.value)}
              />
            </div>
          </div>
          {host && (
            <div className="field">
              <label htmlFor={`in-confirm-${site.domain}`}>
                {t("Type {host} to confirm: the site there is replaced", { host })}
              </label>
              <input
                id={`in-confirm-${site.domain}`}
                className="mono"
                value={confirm}
                autoComplete="off"
                onChange={(e) => setConfirm(e.target.value)}
              />
            </div>
          )}
          {running ? (
            <Progress progress={progress} onStop={() => void wp.syncStop()} />
          ) : (
            <div className="row">
              <button type="submit" className="btn danger" disabled={!server || !host || confirm.trim() !== host}>
                {t("Put it on the host")}
              </button>
            </div>
          )}
        </>
      )}
      {error && (
        <p className="error small" role="alert">
          {error}
        </p>
      )}
    </form>
  );
}
