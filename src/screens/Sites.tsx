import { useEffect, useRef, useState, type FormEvent } from "react";
import { api, categories, errorText, inWordPress, lists, siteAddress, type Category, type ListKind, type ProjectKind, type RuntimeFamily, type Site, type SiteSize, type SystemStatus } from "../lib/api";
import { useLoad } from "../lib/useLoad";
import { useModules } from "../lib/modules";
import { closeTab, openTab, setLayout, showNewSite, showScreen, siteTab, useLayout } from "../lib/layout";
import { Icon } from "../components/Icon";
import { BrandIcon } from "../components/BrandIcon";
import { SiteThumb } from "../components/SiteThumb";
import { SiteDatabaseLine } from "../components/SiteDatabase";
import { CloneSite } from "../components/wordpress/CloneSite";
import { ClonesList } from "../components/wordpress/Clones";
import { cloneScreen, isSiteBackup, newClone, showClone, useClones } from "../lib/clones";
import { importBackup } from "./ImportBackup";
import { useT } from "../lib/i18n";
import { Progress, type ProgressState } from "../components/Progress";
import { SiteActions } from "../components/SiteActions";
import { SiteList, type RowParts } from "../components/SiteList";
import { RemoveDialog } from "../components/RemoveDialog";
import { CATEGORY_ICON, templatesFor } from "../lib/templates";
import { useBusy, useShown } from "../lib/tabLife";

const SITE_LABELS: Record<RuntimeFamily["kind"], string> = { php: "PHP", node: "Node", python: "Python", go: "Go", java: "Java" };
const VERSION_NAMES: Record<RuntimeFamily["kind"], string> = { php: "PHP", node: "Node.js", python: "Python", go: "Go", java: "Java" };

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

/** A starter's step ("Downloading WordPress… 40%") as words and a bar. */
function starterState(message: string): ProgressState {
  const m = /^(.*?)\s*(\d{1,3})%$/.exec(message);
  return m ? { text: m[1], percent: Math.min(100, Number(m[2])) } : { text: message, percent: null };
}

/** "My Blog 2" → "my-blog-2": what a typed name becomes in the site's address. */
export function siteName(typed: string) {
  return typed
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
}

/** The versions a project runs with, as the list shows them: "PHP 8.4 · Node.js 22". */
export function versionsText(site: Site) {
  return site.versions.map((v) => `${VERSION_NAMES[v.kind]} ${v.version}`).join(" · ");
}

/**
 * Projects (every app and site), Apps or Sites: the list, and the New
 * project form above it, where an app or a site is picked first and then
 * what it starts from.
 */
export function Sites({ list }: { list: ListKind }) {
  const t = useT();
  const about = lists[list];
  // Clones and dropped backups are WordPress sites: only Sites handles them,
  // so they don't show twice while Projects is open too.
  const forSites = list === "site";
  // The New and Parked folders forms fold away; open or closed is kept.
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
  // What is being made. Apps and Sites make their own kind; Projects asks.
  const [picked, setPicked] = useState<Category>("site");
  const category: Category = list === "all" ? picked : list;
  // An app needs no address; one can be asked for.
  const [appAddress, setAppAddress] = useState(false);
  const address = category === "site" || appAddress;
  const [parkDraft, setParkDraft] = useState("");
  const [settingUp, setSettingUp] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const starters = useLoad(api.listStarters, []);
  const templates = templatesFor(category, starters.data, {
    clone: isOn("wordpress") && isOn("remote"),
    import: isOn("sitetransfer") || (isOn("wordpress") && isOn("remote")),
  });
  const [templateIds, setTemplateIds] = useState<Record<Category, string>>({ app: "", site: "" });
  const template = templates.find((x) => x.id === templateIds[category]) ?? templates[0];
  const kind: ProjectKind = template.via === "folder" || template.via === "wordpress" ? template.kind : "website";
  const [making, setMaking] = useState<string | null>(null);
  useBusy(making !== null);
  const [made, setMade] = useState<Site | null>(null);
  const [removing, setRemoving] = useState<Site | null>(null);
  // The address being made, for the progress box.
  const [makingDomain, setMakingDomain] = useState("");
  const chosenStarter = template.via === "starter" ? template.starter : undefined;
  const ending = address ? tldDraft : "";
  const ids = (what: string) => `${what}-${list}`;

  useEffect(() => setTldDraft(tldInfo.data.tld), [tldInfo.data.tld]);

  // A site backup dropped on the window while this screen is shown (an
  // export from the Stackyard Connect plugin, a hosting panel backup)
  // goes to Import backup, which starts on it.
  const shown = useShown();
  const shownNow = useRef(shown);
  shownNow.current = shown;
  useEffect(() => {
    let off: (() => void) | undefined;
    let cancelled = false;
    api
      .onDrop((dropped) => {
        const backups = dropped.filter(isSiteBackup);
        if (forSites && shownNow.current && backups.length > 0) importBackup(backups);
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
  const listed = list === "all" ? sites.data : sites.data.filter((x) => x.project.category === list);
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

  function pickCategory(c: Category) {
    if (c === category) return;
    // Apps and Sites each make their own kind; Projects makes either.
    if (list === "all") setPicked(c);
    else showNewSite(c);
  }

  async function create(e: FormEvent) {
    e.preventDefault();
    if (template.via === "clone" || template.via === "import") return;
    if (address) await saveTld();
    setMade(null);
    const domain = name + tldDraft;
    setMakingDomain(address ? domain : name);
    let ok: boolean;
    if (template.via === "wordpress") {
      setMaking(t("Making {name}…", { name }));
      let list: Site[] = [];
      ok = await attempt(async () => {
        list = await api.addProject({ name, kind, path: path.trim(), https });
      });
      setMaking(null);
      if (ok) setMade(list.find((s) => s.name === name) ?? null);
    } else if (chosenStarter) {
      setMaking(t("Getting ready to make the {name} site…", { name: chosenStarter.name }));
      let list: Site[] = [];
      ok = await attempt(async () => {
        list = await api.addStarterSite({ name, starter: chosenStarter.id, path: path.trim(), https, category, address });
      });
      setMaking(null);
      if (ok) setMade(list.find((s) => s.name === name) ?? null);
    } else {
      // An empty folder means a new folder in the sites folder, made by the app.
      setMaking(t("Making the folder and adding it…"));
      ok = await attempt(() => api.addSite({ name, path: path.trim(), runtime: chosenRuntime, https, kind, category, address }));
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

  function startClone() {
    // Clones are made on the Sites screen.
    if (!forSites) showScreen("Sites");
    newClone();
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

  function removed(site: Site, problem: string | null) {
    setRemoving(null);
    closeTab(siteTab(site.domain));
    setError(problem);
    if (made?.domain === site.domain) setMade(null);
    sites.reload();
    parked.reload();
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


  async function setUp() {
    setSettingUp(true);
    await attempt(() => api.runSystemSetup());
    setSettingUp(false);
    system.reload();
    sites.reload();
  }

  const createLabel = chosenStarter
    ? t("Create {name}", { name: chosenStarter.name })
    : template.via === "wordpress"
      ? t("Create {name}", { name: t(template.name) })
      : t("Create {what}", { what: t(category === "app" ? "App" : "Site").toLowerCase() });

  return (
    <>
      <header className="page-header">
        <div>
          <h1>{t(about.plural)}</h1>
          <p className="muted">{t(about.hint)}</p>
        </div>
        <div className="row">
          {forSites && isOn("wordpress") && isOn("remote") && (
            <button type="button" className="btn" onClick={startClone}>
              <BrandIcon id="wordpress" />
              {t("Clone a live WordPress site…")}
            </button>
          )}
        </div>
      </header>
      {forSites && clones.some((c) => c.state !== "new" && !c.open) && (
        <section className="card stack" aria-labelledby="clones-h">
          <h2 id="clones-h">{t("Clones running in the background")}</h2>
          <ClonesList
            minimized
            onShow={(id) => {
              showClone(id);
              if (cloneScreen(id) !== "Sites") showScreen(cloneScreen(id));
            }}
          />
        </section>
      )}
      {clones
        .filter((c) => forSites && c.open && c.place !== "import")
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
      {removing && (
        <RemoveDialog site={removing} onClose={() => setRemoving(null)} onRemoved={(problem) => removed(removing, problem)} />
      )}

      {system.data && system.data.needed.length > 0 && list !== "app" && (
        <section aria-labelledby={ids("setup-h")} className="card stack">
          <div className="row between">
            <h2 id={ids("setup-h")}>Finish setting up local domains</h2>
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
      {system.data?.waiting_for_certificate && list !== "app" && (
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
            {t("{name} is ready.", { name: made.address ? made.domain : made.name })}
            {made.framework === "WordPress" && !inWordPress(made.project.kind) && " Open it to finish WordPress's install (site title and your login)."}
          </span>
          <span className="row">
            <button type="button" className="btn small" onClick={() => openTab(siteTab(made.domain))}>
              {t("Manage")}
            </button>
            {made.address && (
              <button type="button" className="btn small primary" onClick={() => api.openLocalPage(made.url)}>
                {t("Open")}
              </button>
            )}
          </span>
        </p>
      )}

      <button
        type="button"
        className="disclosure"
        aria-expanded={newSiteOpen}
        aria-controls={ids("sites-top")}
        onClick={() => setLayout({ newSiteOpen: !newSiteOpen })}
      >
        <Icon name="chevron" />
        {t("New project and parked folders")}
      </button>

      {newSiteOpen && (
        <div id={ids("sites-top")} className="sites-top">
          <form className="card stack form new-site" aria-labelledby={ids("new-h")} onSubmit={create}>
            <h2 id={ids("new-h")}>{t("New project")}</h2>
            <div className="category-picker" role="group" aria-label={t("Is it an app or a site?")}>
              {categories.map((c) => (
                <button
                  key={c.id}
                  type="button"
                  className="category-choice"
                  aria-pressed={c.id === category}
                  disabled={making !== null}
                  onClick={() => pickCategory(c.id)}
                >
                  <Icon name={CATEGORY_ICON[c.id]} />
                  <span>
                    <strong>{t(c.name)}</strong>
                    <span className="muted small">{t(c.hint)}</span>
                  </span>
                </button>
              ))}
            </div>
            <div className="field">
              <span className="label" id={ids("f-start")}>
                {t("Start from")}
              </span>
              <div className="template-list" role="radiogroup" aria-labelledby={ids("f-start")}>
                {templates.map((x) => (
                  <button
                    key={x.id}
                    type="button"
                    role="radio"
                    className="template-choice"
                    aria-checked={x.id === template.id}
                    disabled={making !== null}
                    title={t(x.description)}
                    onClick={() => setTemplateIds((ids) => ({ ...ids, [category]: x.id }))}
                  >
                    <BrandIcon id={x.brand} fallback={x.via === "import" ? "archive" : CATEGORY_ICON[category]} />
                    <span className="template-name">{t(x.name)}</span>
                  </button>
                ))}
              </div>
              <span className="muted small">{t(template.description)}</span>
            </div>
            {template.via === "clone" || template.via === "import" ? (
              <div className="row">
                <button type="button" className="btn primary" onClick={template.via === "clone" ? startClone : () => showScreen("Import backup")}>
                  {template.via === "clone" ? t("Clone a live WordPress site…") : t("Open Import backup")}
                </button>
              </div>
            ) : (
              <>
                <div className="form-grid">
                  <div className="field">
                    <label htmlFor={ids("f-name")}>Name</label>
                    <div className="input-affix">
                      <input
                        id={ids("f-name")}
                        className="mono"
                        value={typedName}
                        placeholder="hello"
                        required
                        autoComplete="off"
                        spellCheck={false}
                        aria-describedby={ids("f-name-hint")}
                        title={t("Letters, numbers and dashes. Spaces and other characters become dashes.")}
                        onChange={(e) => setTypedName(e.target.value)}
                      />
                      {ending && <span className="mono muted">{ending}</span>}
                    </div>
                    <span id={ids("f-name-hint")} className="muted small">
                      {typedName && name !== typedName ? (
                        name ? (
                          <>
                            {address ? "The address will be " : "It will be called "}
                            <span className="mono">{name + ending}</span>
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
                    <label htmlFor={ids("f-folder")}>
                      {kind === "wordpress-theme" ? t("Theme folder") : kind === "wordpress-plugin" ? t("Plugin folder") : t("Project folder")}
                    </label>
                    <div className="row folder-input">
                      <input
                        id={ids("f-folder")}
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
                  {template.via === "folder" && (
                    <div className="field">
                      <label htmlFor={ids("f-rt")}>Runtime</label>
                      <select id={ids("f-rt")} value={chosenRuntime} onChange={(e) => setRuntime(e.target.value)}>
                        {kindOptions.map((o) => (
                          <option key={o.value} value={o.value}>
                            {o.label}
                          </option>
                        ))}
                      </select>
                    </div>
                  )}
                  {address && template.via !== "wordpress" && (
                    <div className="field">
                      <label htmlFor={ids("f-tld")}>Domain ending</label>
                      <input
                        id={ids("f-tld")}
                        className="mono"
                        value={tldDraft}
                        onChange={(e) => setTldDraft(e.target.value)}
                        onBlur={saveTld}
                      />
                      {tldInfo.data.warning && <span className="warn small">{tldInfo.data.warning}</span>}
                    </div>
                  )}
                </div>
                <span className="muted small">
                  Leave the folder empty to create <span className="mono">{folderFor(name || "hello")}</span>.
                  {address && template.via !== "wordpress" && " The domain ending applies to new sites."}
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
                    {chosenStarter && (
                      <span className="muted small">{t("Downloads and setup can take a few minutes. You can keep using Stackyard.")}</span>
                    )}
                  </div>
                )}
                <div className="row between centered">
                  {category === "site" ? (
                    <label className="check">
                      <input type="checkbox" checked={https} onChange={(e) => setHttps(e.target.checked)} />
                      Secure with local HTTPS (SSL) certificate
                    </label>
                  ) : template.via === "wordpress" ? (
                    <span />
                  ) : (
                    <span className="stack">
                      <label className="check">
                        <input type="checkbox" checked={appAddress} onChange={(e) => setAppAddress(e.target.checked)} />
                        {t("Give it a local address ({address})", { address: `${name || "hello"}${tldDraft}` })}
                      </label>
                      {appAddress && (
                        <label className="check">
                          <input type="checkbox" checked={https} onChange={(e) => setHttps(e.target.checked)} />
                          Secure with local HTTPS (SSL) certificate
                        </label>
                      )}
                    </span>
                  )}
                  <button type="submit" className="btn primary" disabled={making !== null || !name}>
                    {making ? t("Creating…") : createLabel}
                  </button>
                </div>
              </>
            )}
          </form>

          <form className="card stack form parked" aria-labelledby={ids("park-h")} onSubmit={park}>
            <h2 id={ids("park-h")}>Parked folders</h2>
            <p className="muted small">
              {t("Every folder inside a parked folder shows up within a few seconds, named after the folder: as a site when it has a start page or a framework, else as an app. Change it in its Manage tab.")}
            </p>
            {parked.data.map((folder) => (
              <div key={folder} className="list-row">
                <span className="mono small">{folder}</span>
                {samePath(folder, sitesFolder.data) ? (
                  <span className="muted small">{t("New projects go here")}</span>
                ) : (
                  <button
                    type="button"
                    className="btn ghost small"
                    aria-label={t("Make new projects in {folder}", { folder })}
                    onClick={() => useForNewSites(folder)}
                  >
                    {t("Use for new projects")}
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
              <label htmlFor={ids("f-park")}>Folder</label>
              <div className="row folder-input">
                <input
                  id={ids("f-park")}
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
                list={list}
                columns={4}
                onError={setError}
                renderSite={(site, parts) => (
                  <SiteRow
                    key={site.domain}
                    site={site}
                    parts={parts}
                    kindShown={list === "all"}
                    size={sizes.data[site.domain]}
                    system={system.data}
                    onRemove={setRemoving}
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
  kindShown,
  size,
  onRemove,
  onOpen,
  system,
}: {
  site: Site;
  /** Its drag handle and place in the order. */
  parts: RowParts;
  /** Say whether it is an app or a site (on Projects, which lists both). */
  kindShown: boolean;
  /** The site's size when its tab last measured it. */
  size?: SiteSize;
  onRemove: (site: Site) => void;
  /** Runs an open action, showing what went wrong if it fails. */
  onOpen: (open: () => Promise<unknown>) => void;
  system: SystemStatus | null;
}) {
  const t = useT();
  const url = site.address ? siteAddress(site, system) : site.url;
  const versions = versionsText(site);
  return (
    <tr {...parts.row}>
      <td>
        <div className="site-cell">
          {parts.handle}
          <SiteThumb site={site} />
          <div>
            {site.address ? (
              <button type="button" className="link mono" onClick={() => api.openLocalPage(url)}>
                {url}
              </button>
            ) : (
              <strong className="mono">{site.name}</strong>
            )}
            <div className="muted small">
              {kindShown && `${t(site.project.category === "app" ? "App" : "Site")} · `}
              {site.address ? (site.https ? "HTTPS" : "HTTP only") : t("No .stack address")}
              {!site.address && (
                <>
                  {" · "}
                  <button type="button" className="link mono" onClick={() => api.openLocalPage(url)}>
                    {url.replace(/^https?:\/\//, "")}
                  </button>
                </>
              )}
              {site.parked && " · parked"}
              {size && ` · ${bytesText(size.total)}`}
            </div>
          </div>
        </div>
      </td>
      <td>
        {site.framework ? `${site.framework} · ` : ""}
        {versions || site.runtime}
        {site.problem && <div className="warn small">{site.problem}</div>}
        <SiteDatabaseLine domain={site.domain} />
      </td>
      <td className="mono muted small">{site.root}</td>
      <td className="right nowrap">
        <SiteActions site={site} manage onRun={onOpen} onRemove={() => onRemove(site)} />
      </td>
    </tr>
  );
}
