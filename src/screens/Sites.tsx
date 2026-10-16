import { useEffect, useState, type FormEvent } from "react";
import { api, errorText, type RuntimeFamily, type Site } from "../lib/api";
import { useLoad } from "../lib/useLoad";
import { useModules } from "../lib/modules";

const SITE_LABELS: Partial<Record<RuntimeFamily["kind"], string>> = { php: "PHP", node: "Node" };

/** Choices for the runtime picker, e.g. "PHP 8.4"; installed versions say so. */
function runtimeOptions(families: RuntimeFamily[], isOn: (id: string) => boolean) {
  const options = families
    .filter((f) => f.available && isOn(f.kind) && SITE_LABELS[f.kind])
    .flatMap((f) =>
      f.versions.map((v) => ({
        value: `${SITE_LABELS[f.kind]} ${v.version}`,
        label: `${SITE_LABELS[f.kind]} ${v.version}${v.installed ? "" : " (not installed)"}`,
      })),
    );
  return [{ value: "Auto", label: "Detect from the folder" }, ...options, { value: "Static", label: "Static files" }];
}

export function Sites() {
  const sites = useLoad(api.listSites, []);
  const runtimes = useLoad(api.listRuntimes, []);
  const parked = useLoad(api.listParked, []);
  const system = useLoad(api.systemStatus, null);
  const sitesFolder = useLoad(api.sitesFolder, "");
  const { isOn } = useModules();
  const options = runtimeOptions(runtimes.data, isOn);
  const tldInfo = useLoad(api.getTld, { tld: ".stack", warning: null });
  const [tldDraft, setTldDraft] = useState(".stack");
  const [name, setName] = useState("");
  const [path, setPath] = useState("");
  const [runtime, setRuntime] = useState("Auto");
  const [https, setHttps] = useState(true);
  const [parkDraft, setParkDraft] = useState("");
  const [settingUp, setSettingUp] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const starters = useLoad(api.listStarters, []);
  const [starter, setStarter] = useState("");
  const [making, setMaking] = useState<string | null>(null);
  const [made, setMade] = useState<Site | null>(null);
  const chosenStarter = starters.data.find((s) => s.id === starter);

  useEffect(() => setTldDraft(tldInfo.data.tld), [tldInfo.data.tld]);
  const reloadSites = sites.reload;
  const reloadParked = parked.reload;
  useEffect(() => {
    let off: (() => void) | undefined;
    let cancelled = false;
    api
      .onSitesChanged(() => {
        reloadSites();
        reloadParked();
      })
      .then((unsubscribe) => (cancelled ? unsubscribe() : (off = unsubscribe)));
    return () => {
      cancelled = true;
      off?.();
    };
  }, [reloadSites, reloadParked]);
  const chosenRuntime = options.some((o) => o.value === runtime) ? runtime : "Auto";

  useEffect(() => {
    let off: (() => void) | undefined;
    let cancelled = false;
    api
      .onStarterProgress((p) => setMaking(p.message))
      .then((unsubscribe) => (cancelled ? unsubscribe() : (off = unsubscribe)));
    return () => {
      cancelled = true;
      off?.();
    };
  }, []);

  async function attempt(action: () => Promise<unknown>) {
    try {
      await action();
      setError(null);
      return true;
    } catch (e) {
      setError(errorText(e));
      return false;
    }
  }

  async function saveTld() {
    if (tldDraft === tldInfo.data.tld) return;
    await attempt(() => api.setTld(tldDraft));
    tldInfo.reload();
    system.reload();
  }

  async function create(e: FormEvent) {
    e.preventDefault();
    await saveTld();
    setMade(null);
    let ok: boolean;
    if (chosenStarter) {
      setMaking(`Making the ${chosenStarter.name} site…`);
      let list: Site[] = [];
      ok = await attempt(async () => {
        list = await api.addStarterSite({ name, starter: chosenStarter.id, path: path.trim(), https });
      });
      setMaking(null);
      if (ok) setMade(list.find((s) => s.name === name) ?? null);
    } else {
      // An empty folder means a new folder in the sites folder, made by the app.
      ok = await attempt(() => api.addSite({ name, path: path.trim(), runtime: chosenRuntime, https }));
    }
    if (ok) {
      setName("");
      setPath("");
    }
    sites.reload();
    system.reload();
  }

  function folderFor(site: string) {
    const base = sitesFolder.data;
    if (!base) return `~/Code/${site}`;
    const sep = base.includes("\\") ? "\\" : "/";
    return `${base.replace(/[\\/]+$/, "")}${sep}${site}`;
  }

  async function browseProject() {
    const folder = await api.pickFolder("Select the project folder", path || sitesFolder.data);
    if (folder) setPath(folder);
  }

  async function browsePark() {
    const folder = await api.pickFolder("Select a folder to park", parkDraft || sitesFolder.data);
    if (folder) setParkDraft(folder);
  }

  async function remove(domain: string) {
    await attempt(() => api.removeSite(domain));
    sites.reload();
    system.reload();
  }

  async function park(e: FormEvent) {
    e.preventDefault();
    if (await attempt(() => api.parkFolder(parkDraft))) setParkDraft("");
    parked.reload();
    sites.reload();
    system.reload();
  }

  async function unpark(folder: string) {
    await attempt(() => api.unparkFolder(folder));
    parked.reload();
    sites.reload();
    system.reload();
  }

  async function setUp() {
    setSettingUp(true);
    await attempt(() => api.runSystemSetup());
    setSettingUp(false);
    system.reload();
    sites.reload();
  }

  return (
    <>
      <header className="page-header">
        <div>
          <h1>Sites</h1>
          <p className="muted">Local domains with HTTPS, each with its own runtime version.</p>
        </div>
      </header>

      {system.data && system.data.needed.length > 0 && (
        <section aria-labelledby="setup-h" className="card stack">
          <div className="row between">
            <h2 id="setup-h">Finish setting up local domains</h2>
            <button type="button" className="btn primary" onClick={setUp} disabled={settingUp}>
              {settingUp ? "Waiting for your password…" : "Set up"}
            </button>
          </div>
          <p className="muted">
            So addresses like <span className="mono">hello{tldInfo.data.tld}</span> open in your browser. Your computer
            asks for your password once to:
          </p>
          <ul className="muted">
            {system.data.needed.map((item) => (
              <li key={item}>{item}</li>
            ))}
          </ul>
          <p className="muted small">
            Until then, every site also works at its <span className="mono">.localhost</span> address.
          </p>
        </section>
      )}
      {system.data?.waiting_for_certificate && (
        <p className="notice small">
          Start the web server once (Services › Caddy) to create the HTTPS certificate, then finish setup here.
        </p>
      )}
      {error && (
        <p className="error small" role="alert">
          {error}
        </p>
      )}
      {made && (
        <p className="notice small row between centered" role="status">
          <span>
            <span className="mono">{made.domain}</span> is ready.
            {made.framework === "WordPress" && " Open it to finish WordPress's install (site title and your login)."}
          </span>
          <button type="button" className="btn small" onClick={() => api.openLocalPage(made.url)}>
            Open
          </button>
        </p>
      )}

      <div className="sites-top">
        <form className="card stack form new-site" aria-labelledby="new-h" onSubmit={create}>
          <h2 id="new-h">New site</h2>
          <div className="form-grid">
            <div className="field">
              <label htmlFor="f-name">Name</label>
              <div className="input-affix">
                <input
                  id="f-name"
                  className="mono"
                  value={name}
                  placeholder="hello"
                  required
                  onChange={(e) => setName(e.target.value.toLowerCase().replace(/[^a-z0-9-]/g, ""))}
                />
                <span className="mono muted">{tldDraft}</span>
              </div>
            </div>
            <div className="field">
              <label htmlFor="f-folder">Project folder</label>
              <div className="row folder-input">
                <input
                  id="f-folder"
                  className="mono grow"
                  value={path}
                  placeholder={folderFor(name || "hello")}
                  onChange={(e) => setPath(e.target.value)}
                />
                <button type="button" className="btn" onClick={browseProject}>
                  Browse…
                </button>
              </div>
            </div>
            <div className="field">
              <label htmlFor="f-starter">Start from</label>
              <select id="f-starter" value={starter} onChange={(e) => setStarter(e.target.value)}>
                <option value="">Empty or existing folder</option>
                {starters.data.map((s) => (
                  <option key={s.id} value={s.id}>
                    {s.name}
                  </option>
                ))}
              </select>
            </div>
            {chosenStarter ? (
              <div className="field">
                <span className="label">What you get</span>
                <span className="muted small">{chosenStarter.description}</span>
              </div>
            ) : (
              <div className="field">
                <label htmlFor="f-rt">Runtime</label>
                <select id="f-rt" value={chosenRuntime} onChange={(e) => setRuntime(e.target.value)}>
                  {options.map((o) => (
                    <option key={o.value} value={o.value}>
                      {o.label}
                    </option>
                  ))}
                </select>
              </div>
            )}
            <div className="field">
              <label htmlFor="f-tld">Domain ending</label>
              <input
                id="f-tld"
                className="mono"
                value={tldDraft}
                onChange={(e) => setTldDraft(e.target.value)}
                onBlur={saveTld}
              />
              {tldInfo.data.warning && <span className="warn small">{tldInfo.data.warning}</span>}
            </div>
          </div>
          <span className="muted small">
            Leave the folder empty to create <span className="mono">{folderFor(name || "hello")}</span>. The domain
            ending applies to new sites.
            {chosenStarter && " A starter needs a new or empty folder."}
          </span>
          {making && (
            <p className="muted small" role="status">
              {making}
            </p>
          )}
          <div className="row between centered">
            <label className="check">
              <input type="checkbox" checked={https} onChange={(e) => setHttps(e.target.checked)} />
              Secure with local HTTPS (SSL) certificate
            </label>
            <button type="submit" className="btn primary" disabled={making !== null}>
              {making ? "Creating…" : chosenStarter ? `Create ${chosenStarter.name} site` : "Create site"}
            </button>
          </div>
        </form>

        <form className="card stack form parked" aria-labelledby="park-h" onSubmit={park}>
          <h2 id="park-h">Parked folders</h2>
          <p className="muted small">
            Every folder inside a parked folder is a site, named after the folder. New folders show up within a few
            seconds.
          </p>
          {parked.data.map((folder) => (
            <div key={folder} className="list-row">
              <span className="mono small">{folder}</span>
              <button
                type="button"
                className="btn ghost small"
                aria-label={`Unpark ${folder}`}
                onClick={() => unpark(folder)}
              >
                Unpark
              </button>
            </div>
          ))}
          <div className="field">
            <label htmlFor="f-park">Folder</label>
            <div className="row folder-input">
              <input
                id="f-park"
                className="mono grow"
                value={parkDraft}
                placeholder={sitesFolder.data || "~/Code"}
                required
                onChange={(e) => setParkDraft(e.target.value)}
              />
              <button type="button" className="btn" onClick={browsePark}>
                Browse…
              </button>
            </div>
          </div>
          <button type="submit" className="btn">
            Park folder
          </button>
        </form>
      </div>

      <section aria-label="Site list" className="card table-wrap">
        <table>
          <thead>
            <tr>
              <th>Site</th>
              <th>Runs with</th>
              <th>Folder</th>
              <th>
                <span className="sr-only">Actions</span>
              </th>
            </tr>
          </thead>
          <tbody>
            {sites.data.length === 0 && (
              <tr>
                <td colSpan={4} className="muted">
                  No sites yet. Create one above, or add a folder to a parked folder.
                </td>
              </tr>
            )}
            {sites.data.map((site) => (
              <SiteRow key={site.domain} site={site} onRemove={remove} />
            ))}
          </tbody>
        </table>
      </section>
    </>
  );
}

function SiteRow({ site, onRemove }: { site: Site; onRemove: (domain: string) => void }) {
  return (
    <tr>
      <td>
        <button type="button" className="link mono" onClick={() => api.openLocalPage(site.url)}>
          {site.url}
        </button>
        <div className="muted small">
          {site.https ? "HTTPS" : "HTTP only"}
          {site.parked && " · parked"}
          {" · "}
          <button type="button" className="link small" onClick={() => api.openLocalPage(site.local_url)}>
            {site.local_url.replace(/^https?:\/\//, "")}
          </button>
        </div>
      </td>
      <td>
        {site.framework ? `${site.framework} · ` : ""}
        {site.runtime}
        {site.problem && <div className="warn small">{site.problem}</div>}
      </td>
      <td className="mono muted small">{site.root}</td>
      <td className="right">
        {!site.parked && (
          <button
            type="button"
            className="btn ghost small"
            aria-label={`Remove ${site.domain}`}
            onClick={() => onRemove(site.domain)}
          >
            Remove
          </button>
        )}
      </td>
    </tr>
  );
}
