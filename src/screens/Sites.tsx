import { useEffect, useState, type FormEvent } from "react";
import { api, errorText } from "../lib/api";
import { useLoad } from "../lib/useLoad";

const RUNTIME_OPTIONS = ["PHP 8.4", "PHP 8.3", "PHP 8.2", "PHP 8.1", "Node 24", "Node 22", "Node 20", "Static"];

export function Sites() {
  const sites = useLoad(api.listSites, []);
  const tldInfo = useLoad(api.getTld, { tld: ".stack", warning: null });
  const [tldDraft, setTldDraft] = useState(".stack");
  const [name, setName] = useState("");
  const [path, setPath] = useState("");
  const [runtime, setRuntime] = useState("PHP 8.3");
  const [https, setHttps] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => setTldDraft(tldInfo.data.tld), [tldInfo.data.tld]);

  async function saveTld() {
    if (tldDraft === tldInfo.data.tld) return;
    try {
      await api.setTld(tldDraft);
      setError(null);
    } catch (e) {
      setError(errorText(e));
    }
    tldInfo.reload();
  }

  async function create(e: FormEvent) {
    e.preventDefault();
    try {
      await saveTld();
      await api.addSite({ name, path: path || `~/Code/${name}`, runtime, https });
      setName("");
      setPath("");
      setError(null);
      sites.reload();
    } catch (err) {
      setError(errorText(err));
    }
  }

  async function remove(siteName: string) {
    try {
      await api.removeSite(siteName);
      sites.reload();
    } catch (err) {
      setError(errorText(err));
    }
  }

  return (
    <>
      <header className="page-header">
        <div>
          <h1>Sites</h1>
          <p className="muted">Local domains with HTTPS, each with its own runtime version.</p>
        </div>
      </header>

      <div className="split">
        <section aria-label="Site list" className="card table-wrap">
          <table>
            <thead>
              <tr>
                <th>Domain</th>
                <th>Folder</th>
                <th>Runtime</th>
                <th>HTTPS</th>
                <th>
                  <span className="sr-only">Actions</span>
                </th>
              </tr>
            </thead>
            <tbody>
              {sites.data.length === 0 && (
                <tr>
                  <td colSpan={5} className="muted">
                    No sites yet. Create one with the form.
                  </td>
                </tr>
              )}
              {sites.data.map((site) => (
                <tr key={site.domain}>
                  <td className="mono">{site.url}</td>
                  <td className="mono muted small">{site.path}</td>
                  <td>{site.runtime}</td>
                  <td className={site.https ? "ok" : "muted"}>{site.https ? "On" : "Off"}</td>
                  <td className="right">
                    <button
                      type="button"
                      className="btn ghost small"
                      aria-label={`Remove ${site.domain}`}
                      onClick={() => remove(site.domain)}
                    >
                      Remove
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </section>

        <form className="card stack form" aria-labelledby="new-h" onSubmit={create}>
          <h2 id="new-h">New site</h2>
          <div className="field">
            <label htmlFor="f-name">Name</label>
            <div className="input-affix">
              <input
                id="f-name"
                className="mono"
                value={name}
                placeholder="myapp"
                required
                onChange={(e) => setName(e.target.value.toLowerCase().replace(/[^a-z0-9-]/g, ""))}
              />
              <span className="mono muted">{tldDraft}</span>
            </div>
          </div>
          <div className="field">
            <label htmlFor="f-folder">Project folder</label>
            <input
              id="f-folder"
              className="mono"
              value={path}
              placeholder="~/Code/myapp"
              onChange={(e) => setPath(e.target.value)}
            />
          </div>
          <div className="field">
            <label htmlFor="f-rt">Runtime</label>
            <select id="f-rt" value={runtime} onChange={(e) => setRuntime(e.target.value)}>
              {RUNTIME_OPTIONS.map((r) => (
                <option key={r} value={r}>
                  {r === "Static" ? "Static files" : r}
                </option>
              ))}
            </select>
          </div>
          <div className="field">
            <label htmlFor="f-tld">Domain ending</label>
            <input
              id="f-tld"
              className="mono"
              value={tldDraft}
              onChange={(e) => setTldDraft(e.target.value)}
              onBlur={saveTld}
            />
            <span className="muted small">Default is .stack. Applies to new sites.</span>
            {tldInfo.data.warning && <span className="warn small">{tldInfo.data.warning}</span>}
          </div>
          <label className="check">
            <input type="checkbox" checked={https} onChange={(e) => setHttps(e.target.checked)} />
            Secure with local HTTPS certificate
          </label>
          {error && (
            <p className="error small" role="alert">
              {error}
            </p>
          )}
          <button type="submit" className="btn primary">
            Create site
          </button>
        </form>
      </div>
    </>
  );
}
