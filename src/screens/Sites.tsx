import { useEffect, useRef, useState, type FormEvent } from "react";
import { api, categories, errorText, inWordPress, siteAddress, type Category, type PackProgress, type ProjectKind, type RuntimeFamily, type Site, type SiteFile, type SiteSize, type Starter, type SystemStatus } from "../lib/api";
import { useLoad } from "../lib/useLoad";
import { useModules } from "../lib/modules";
import { closeTab, openTab, setLayout, showNewSite, siteTab, useLayout } from "../lib/layout";
import { Icon } from "../components/Icon";
import { BrandIcon } from "../components/BrandIcon";
import { SCREEN_ICON } from "../components/Sidebar";
import { CATEGORY_SCREEN } from "../lib/screens";
import { SiteVersions } from "../components/SiteVersions";
import { SiteThumb } from "../components/SiteThumb";
import { SiteDatabaseLine } from "../components/SiteDatabase";
import { CloneSite } from "../components/wordpress/CloneSite";
import { ClonesList } from "../components/wordpress/Clones";
import { isSiteBackup, newClone, showClone, useClones } from "../lib/clones";
import { useT } from "../lib/i18n";
import { Progress, packState, type ProgressState } from "../components/Progress";
import { SiteActions } from "../components/SiteActions";
import { SiteList, type RowParts } from "../components/SiteList";
import { useBusy, useShown } from "../lib/tabLife";

const SITE_LABELS: Record<RuntimeFamily["kind"], string> = { php: "PHP", node: "Node", python: "Python", go: "Go", java: "Java" };

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

/** The runtimes a kind of project can run on: a PHP package on PHP, a Node.js app on Node. */
function optionsFor(kind: ProjectKind, options: { value: string; label: string }[]) {
  if (kind === "php-package") return options.filter((o) => o.value.startsWith("PHP "));
  if (kind === "node-app") return options.filter((o) => o.value.startsWith("Node "));
  return options;
}

/**
 * What a new project, app or site can start from: an empty or existing
 * folder (with a runtime to pick), a starter the app makes, or a WordPress
 * theme or plugin, tried in a WordPress made for it.
 */
type Template = { id: string; name: string; description: string; brand: string } & (
  | { via: "folder"; kind: ProjectKind }
  | { via: "starter"; starter: Starter }
  | { via: "wordpress"; kind: ProjectKind }
);

/** Starters' logos, where the starter's own id is not a logo's. */
const STARTER_BRAND: Record<string, string> = { react: "createreactapp", vue: "vite", svelte: "sveltekit", "node-server": "node", "python-scripts": "python", static: "" };

function templatesFor(category: Category, starters: Starter[]): Template[] {
  const folder: Template = {
    id: "folder",
    name: "Empty or existing folder",
    description: "A new folder, or one with your files in it already; pick what it runs on",
    brand: "",
    via: "folder",
    kind: category === "app" ? "node-app" : "website",
  };
  const fromStarters = starters
    .filter((s) => s.category === category)
    .map((s): Template => ({ id: s.id, name: s.name, description: s.description, brand: STARTER_BRAND[s.id] ?? s.id, via: "starter", starter: s }));
  if (category === "project") {
    return [
      { ...folder, kind: "website" },
      { id: "wordpress-theme", name: "WordPress theme", description: "Stackyard makes a WordPress to try the theme in, with the theme switched on", brand: "wordpress", via: "wordpress", kind: "wordpress-theme" },
      { id: "wordpress-plugin", name: "WordPress plugin", description: "Stackyard makes a WordPress to try the plugin in, with the plugin switched on", brand: "wordpress", via: "wordpress", kind: "wordpress-plugin" },
      { id: "php-package", name: "PHP package", description: "A PHP library, served with PHP so you can try it in a page", brand: "php", via: "folder", kind: "php-package" },
      ...fromStarters,
    ];
  }
  if (category === "app") return [{ ...folder, kind: "website" }, ...fromStarters];
  return [...fromStarters, { ...folder, kind: "website" }];
}

/** A starter's step ("Downloading WordPress… 40%") as words and a bar. */
function starterState(message: string): ProgressState {
  const m = /^(.*?)\s*(\d{1,3})%$/.exec(message);
  return m ? { text: m[1], percent: Math.min(100, Number(m[2])) } : { text: message, percent: null };
}

/** "My Blog 2" → "my-blog-2": what a typed name becomes in the site's address. */
function siteName(typed: string) {
  return typed
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
}

export function Sites({ category }: { category: Category }) {
  const t = useT();
  const about = categories.find((c) => c.id === category) ?? categories[2];
  // Clones and dropped backups are WordPress sites: only Sites handles them,
  // so they don't show three times while the other lists are open too.
  const forSites = category === "site";
  // The New site and Parked folders forms fold away; open or closed is kept.
  const { newSiteOpen } = useLayout();
  const sites = useLoad(api.listSites, []);
  // Sizes as last measured in each site's tab; nothing is measured here.
  const sizes = useLoad(api.siteSizes, {} as Record<string, SiteSize>);
  const runtimes = useLoad(api.listRuntimes, []);
  const parked = useLoad(api.listParked, []);
  const system = useLoad(api.systemStatus, null);
  const sitesFolder = useLoad(api.sitesFolder, "");
  const { isOn } = useModules();
  const options = runtimeOptions(runtimes.data, isOn);
  const [importing, setImporting] = useState<{ path: string; file: SiteFile } | null>(null);
  const clones = useClones();
  const tldInfo = useLoad(api.getTld, { tld: ".stack", warning: null });
  const [tldDraft, setTldDraft] = useState(".stack");
  // What was typed, as typed; `name` is the address it makes. Rewriting the
  // field on every key moved the cursor to the end and swallowed spaces.
  const [typedName, setTypedName] = useState("");
  const name = siteName(typedName);
  const [path, setPath] = useState("");
  const [runtime, setRuntime] = useState("Auto");
  const [https, setHttps] = useState(true);
  const [parkDraft, setParkDraft] = useState("");
  const [settingUp, setSettingUp] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const starters = useLoad(api.listStarters, []);
  const templates = templatesFor(category, starters.data);
  const [templateId, setTemplateId] = useState("");
  const template = templates.find((x) => x.id === templateId) ?? templates[0];
  const kind: ProjectKind = template && template.via !== "starter" ? template.kind : "website";
  const [making, setMaking] = useState<string | null>(null);
  useBusy(making !== null);
  const [made, setMade] = useState<Site | null>(null);
  // The address being made, for the progress box.
  const [makingDomain, setMakingDomain] = useState("");
  const chosenStarter = template?.via === "starter" ? template.starter : undefined;

  useEffect(() => setTldDraft(tldInfo.data.tld), [tldInfo.data.tld]);

  // A site backup dropped on the window while this screen is shown (an
  // export from the Stackyard Connect plugin, a hosting panel backup)
  // opens Clone with those files.
  const shown = useShown();
  const shownNow = useRef(shown);
  shownNow.current = shown;
  useEffect(() => {
    let off: (() => void) | undefined;
    let cancelled = false;
    api
      .onDrop((dropped) => {
        const backups = dropped.filter(isSiteBackup);
        if (forSites && shownNow.current && backups.length > 0) newClone(backups);
      })
      .then((unsubscribe) => (cancelled ? unsubscribe() : (off = unsubscribe)));
    return () => {
      cancelled = true;
      off?.();
    };
  }, [forSites]);
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
  const listed = sites.data.filter((x) => x.project.category === category);
  const kindOptions = optionsFor(kind, options);
  const chosenRuntime = kindOptions.some((o) => o.value === runtime) ? runtime : (kindOptions[0]?.value ?? "Auto");

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
    setMakingDomain(name + tldDraft);
    let ok: boolean;
    if (inWordPress(kind)) {
      setMaking(t("Getting ready to make the WordPress for {name}…", { name: name + tldDraft }));
      let list: Site[] = [];
      ok = await attempt(async () => {
        list = await api.addProject({ name, kind, path: path.trim(), https });
      });
      setMaking(null);
      if (ok) setMade(list.find((s) => s.name === name) ?? null);
    } else if (chosenStarter) {
      setMaking(t("Getting ready to make the {name} site…", { name: chosenStarter.name }));
      setMakingDomain(name + tldDraft);
      let list: Site[] = [];
      ok = await attempt(async () => {
        list = await api.addStarterSite({ name, starter: chosenStarter.id, path: path.trim(), https, category });
      });
      setMaking(null);
      if (ok) setMade(list.find((s) => s.name === name) ?? null);
    } else {
      // An empty folder means a new folder in the sites folder, made by the app.
      setMaking(t("Making the folder and adding the site…"));
      ok = await attempt(() => api.addSite({ name, path: path.trim(), runtime: chosenRuntime, https, kind, category }));
      setMaking(null);
      if (ok) setMade((await api.listSites()).find((s) => s.name === name) ?? null);
    }
    if (ok) {
      setTypedName("");
      setPath("");
    }
    sites.reload();
    system.reload();
  }

  function samePath(a: string, b: string) {
    const spell = (p: string) => p.replace(/[\\/]+$/, "").replace(/\\/g, "/").toLowerCase();
    return spell(a) === spell(b);
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
    if (await attempt(() => api.removeSite(domain))) closeTab(siteTab(domain));
    sites.reload();
    system.reload();
  }

  async function useForNewSites(folder: string) {
    await attempt(() => api.setSitesFolder(folder));
    sitesFolder.reload();
  }

  async function park(e: FormEvent) {
    e.preventDefault();
    if (await attempt(() => api.parkFolder(parkDraft))) setParkDraft("");
    // Parking can change where new sites go.
    sitesFolder.reload();
    parked.reload();
    sites.reload();
    system.reload();
  }

  async function unpark(folder: string) {
    await attempt(() => api.unparkFolder(folder));
    sitesFolder.reload();
    parked.reload();
    sites.reload();
    system.reload();
  }

  async function pickImport() {
    const [path] = await api.pickFiles("Pick a Stackyard site file or backup", undefined, false);
    if (!path) return;
    await attempt(async () => setImporting({ path, file: await api.inspectSiteFile(path) }));
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
          <h1>{t(about.plural)}</h1>
          <p className="muted">{t(about.hint)}</p>
        </div>
        <div className="row">
          {forSites && isOn("wordpress") && isOn("remote") && (
            <button type="button" className="btn" onClick={() => newClone()}>
              {t("Clone from a live server…")}
            </button>
          )}
          {isOn("sitetransfer") && (
            <button type="button" className="btn" onClick={pickImport}>
              Import site…
            </button>
          )}
        </div>
      </header>
      {forSites && clones.some((c) => c.state !== "new" && !c.open) && (
        <section className="card stack" aria-labelledby="clones-h">
          <h2 id="clones-h">{t("Clones running in the background")}</h2>
          <ClonesList minimized onShow={showClone} />
        </section>
      )}
      {clones
        .filter((c) => forSites && c.open)
        .map((c) => (
          <CloneSite
            key={c.id}
            id={c.id}
            onCloned={(domain) => {
              openTab(siteTab(domain));
              sites.reload();
            }}
          />
        ))}
      {importing && (
        <ImportSite
          path={importing.path}
          file={importing.file}
          onClose={() => setImporting(null)}
          onImported={(domain) => {
            setImporting(null);
            openTab(siteTab(domain));
            sites.reload();
            system.reload();
          }}
        />
      )}

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
        <p className="error small" role="alert" ref={(el) => el?.scrollIntoView({ block: "nearest", behavior: "smooth" })}>
          {error}
        </p>
      )}
      {made && (
        <p className="notice small row between centered" role="status">
          <span>
            <span className="mono">{made.domain}</span> is ready.
            {made.framework === "WordPress" && " Open it to finish WordPress's install (site title and your login)."}
          </span>
          <span className="row">
            <button type="button" className="btn small" onClick={() => openTab(siteTab(made.domain))}>
              {t("Manage")}
            </button>
            <button type="button" className="btn small primary" onClick={() => api.openLocalPage(made.url)}>
              {t("Open")}
            </button>
          </span>
        </p>
      )}

      <button
        type="button"
        className="disclosure"
        aria-expanded={newSiteOpen}
        aria-controls={`sites-top-${category}`}
        onClick={() => setLayout({ newSiteOpen: !newSiteOpen })}
      >
        <Icon name="chevron" />
        {t("New and parked folders")}
      </button>

      {newSiteOpen && (
        <div id={`sites-top-${category}`} className="sites-top">
          <form className="card stack form new-site" aria-labelledby={`new-h-${category}`} onSubmit={create}>
            <h2 id={`new-h-${category}`}>{t("What are you making?")}</h2>
            <div className="category-picker" role="group" aria-label={t("What are you making?")}>
              {categories.map((c) => (
                <button
                  key={c.id}
                  type="button"
                  className="category-choice"
                  aria-pressed={c.id === category}
                  disabled={making !== null}
                  onClick={() => c.id !== category && showNewSite(c.id)}
                >
                  <Icon name={SCREEN_ICON[CATEGORY_SCREEN[c.id]]} />
                  <span>
                    <strong>{t(c.name)}</strong>
                    <span className="muted small">{t(c.hint)}</span>
                  </span>
                </button>
              ))}
            </div>
            <div className="field">
              <span className="label" id={`f-start-${category}`}>
                {t("Start from")}
              </span>
              <div className="template-list" role="radiogroup" aria-labelledby={`f-start-${category}`}>
                {templates.map((x) => (
                  <button
                    key={x.id}
                    type="button"
                    role="radio"
                    className="template-choice"
                    aria-checked={x.id === template?.id}
                    disabled={making !== null}
                    title={t(x.description)}
                    onClick={() => setTemplateId(x.id)}
                  >
                    <BrandIcon id={x.brand} fallback={x.via === "folder" ? "folder" : "code"} />
                    <span className="template-name">{t(x.name)}</span>
                  </button>
                ))}
              </div>
              {template && <span className="muted small">{t(template.description)}</span>}
            </div>
            <div className="form-grid">
              <div className="field">
                <label htmlFor={`f-name-${category}`}>Name</label>
                <div className="input-affix">
                  <input
                    id={`f-name-${category}`}
                    className="mono"
                    value={typedName}
                    placeholder="hello"
                    required
                    autoComplete="off"
                    spellCheck={false}
                    aria-describedby={`f-name-hint-${category}`}
                    title={t("Letters, numbers and dashes. Spaces and other characters become dashes.")}
                    onChange={(e) => setTypedName(e.target.value)}
                  />
                  <span className="mono muted">{tldDraft}</span>
                </div>
                <span id={`f-name-hint-${category}`} className="muted small">
                  {typedName && name !== typedName ? (
                    name ? (
                      <>
                        The address will be <span className="mono">{name + tldDraft}</span>
                      </>
                    ) : (
                      <span className="warn">Use letters or numbers</span>
                    )
                  ) : (
                    "Letters, numbers and dashes"
                  )}
                </span>
              </div>
              <div className="field">
                <label htmlFor={`f-folder-${category}`}>
                  {kind === "wordpress-theme" ? t("Theme folder") : kind === "wordpress-plugin" ? t("Plugin folder") : t("Project folder")}
                </label>
                <div className="row folder-input">
                  <input
                    id={`f-folder-${category}`}
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
              {inWordPress(kind) || chosenStarter ? null : (
                <div className="field">
                  <label htmlFor={`f-rt-${category}`}>Runtime</label>
                  <select id={`f-rt-${category}`} value={chosenRuntime} onChange={(e) => setRuntime(e.target.value)}>
                    {kindOptions.map((o) => (
                      <option key={o.value} value={o.value}>
                        {o.label}
                      </option>
                    ))}
                  </select>
                </div>
              )}
              <div className="field">
                <label htmlFor={`f-tld-${category}`}>Domain ending</label>
                <input
                  id={`f-tld-${category}`}
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
              {inWordPress(kind) &&
                " " +
                  t("An empty folder gets a starting {what} to build on; a folder with files is used as it is.", {
                    what: kind === "wordpress-theme" ? t("theme") : t("plugin"),
                  })}
            </span>
            {making && (
              <div className="new-site-progress">
                <strong>{t("Creating {site}", { site: makingDomain })}</strong>
                <Progress label={t("Creating {site}", { site: makingDomain })} state={starterState(making)} />
                {(chosenStarter || inWordPress(kind)) && (
                  <span className="muted small">{t("Downloads and setup can take a few minutes. You can keep using Stackyard.")}</span>
                )}
              </div>
            )}
            <div className="row between centered">
              <label className="check">
                <input type="checkbox" checked={https} onChange={(e) => setHttps(e.target.checked)} />
                Secure with local HTTPS (SSL) certificate
              </label>
              <button type="submit" className="btn primary" disabled={making !== null || !name}>
                {making ? t("Creating…") : chosenStarter ? t("Create {name}", { name: chosenStarter.name }) : inWordPress(kind) ? t("Create {name}", { name: t(template?.name ?? "") }) : t("Create {what}", { what: t(about.name).toLowerCase() })}
              </button>
            </div>
          </form>

          <form className="card stack form parked" aria-labelledby={`park-h-${category}`} onSubmit={park}>
            <h2 id={`park-h-${category}`}>Parked folders</h2>
            <p className="muted small">
              {t("Every folder inside a parked folder shows up within a few seconds, named after the folder. A folder you make there yourself is a project; change its type in its Manage tab.")}
            </p>
            {parked.data.map((folder) => (
              <div key={folder} className="list-row">
                <span className="mono small">{folder}</span>
                {samePath(folder, sitesFolder.data) ? (
                  <span className="muted small">{t("New sites go here")}</span>
                ) : (
                  <button
                    type="button"
                    className="btn ghost small"
                    aria-label={t("Make new sites in {folder}", { folder })}
                    onClick={() => useForNewSites(folder)}
                  >
                    {t("Use for new sites")}
                  </button>
                )}
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
              <label htmlFor={`f-park-${category}`}>Folder</label>
              <div className="row folder-input">
                <input
                  id={`f-park-${category}`}
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
      )}

      <section aria-label={t("{what} list", { what: t(about.plural) })} className="card table-wrap">
        <table className="sites-table">
          <thead>
            <tr>
              <th>{t(about.name)}</th>
              <th>Runs with</th>
              <th>Folder</th>
              <th>
                <span className="sr-only">Actions</span>
              </th>
            </tr>
          </thead>
          <tbody>
            {listed.length === 0 && (
              <tr>
                <td colSpan={4} className="muted">
                  {t("Nothing here yet. Create one above, or add a folder to a parked folder.")}
                </td>
              </tr>
            )}
            {listed.length > 0 && (
              <SiteList
                sites={listed}
                columns={4}
                onError={setError}
                renderSite={(site, parts) => (
                  <SiteRow
                    key={site.domain}
                    site={site}
                    parts={parts}
                    size={sizes.data[site.domain]}
                    system={system.data}
                    families={runtimes.data}
                    onVersionChanged={() => {
                      sites.reload();
                      runtimes.reload();
                    }}
                    onRemove={remove}
                    onOpen={(open) => void attempt(open)}
                  />
                )}
              />
            )}
          </tbody>
        </table>
      </section>
    </>
  );
}

function bytesText(bytes: number) {
  if (bytes < 1024 ** 2) return `${Math.max(1, Math.round(bytes / 1024))} KB`;
  if (bytes < 1024 ** 3) return `${(bytes / 1024 ** 2).toFixed(1)} MB`;
  return `${(bytes / 1024 ** 3).toFixed(2)} GB`;
}

function SiteRow({
  site,
  parts,
  size,
  families,
  onVersionChanged,
  onRemove,
  onOpen,
  system,
}: {
  site: Site;
  /** Its drag handle and place in the order. */
  parts: RowParts;
  /** The site's size when its tab last measured it. */
  size?: SiteSize;
  families: RuntimeFamily[];
  onVersionChanged: () => void;
  onRemove: (domain: string) => void;
  /** Runs an open action, showing what went wrong if it fails. */
  onOpen: (open: () => Promise<unknown>) => void;
  system: SystemStatus | null;
}) {
  const address = siteAddress(site, system);
  return (
    <tr {...parts.row}>
      <td>
        <div className="site-cell">
          {parts.handle}
          <SiteThumb site={site} />
          <div>
            <button type="button" className="link mono" onClick={() => api.openLocalPage(address)}>
              {address}
            </button>
            <div className="muted small">
              {site.https ? "HTTPS" : "HTTP only"}
              {site.parked && " · parked"}
              {size && ` · ${bytesText(size.total)}`}
            </div>
          </div>
        </div>
      </td>
      <td>
        {site.framework ? `${site.framework} · ` : ""}
        {site.runtime}
        {site.problem && <div className="warn small">{site.problem}</div>}
        <SiteDatabaseLine domain={site.domain} />
        <SiteVersions site={site} families={families} onChanged={onVersionChanged} />
      </td>
      <td className="mono muted small">{site.root}</td>
      <td className="right nowrap">
        <SiteActions site={site} manage onRun={onOpen} onRemove={() => onRemove(site.domain)} />
      </td>
    </tr>
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
