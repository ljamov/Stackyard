import { useCallback, useEffect, useState, type FormEvent, type ReactNode } from "react";
import { api, errorText, type Site } from "../../lib/api";
import { useModules } from "../../lib/modules";
import { useT } from "../../lib/i18n";
import {
  wp,
  type WooView,
  type WpCoreUpdate,
  type WpDebug,
  type WpDiskUse,
  type WpEnvironment,
  type WpExtension,
  type WpKind,
  type WpLog,
  type WpOverview,
  type WpReplaced,
  type WpUser,
} from "../../lib/wordpress";
import { EnvironmentsTab } from "./Environments";
import { Icon } from "../Icon";
import { SiteThumb } from "../SiteThumb";
import { AutostartCheck, SiteDatabaseNotice, useSiteDatabase } from "../SiteDatabase";
import "../site-views.css";

export function size(bytes: number) {
  if (bytes < 1024 ** 2) return `${Math.max(1, Math.round(bytes / 1024))} KB`;
  if (bytes < 1024 ** 3) return `${(bytes / 1024 ** 2).toFixed(1)} MB`;
  return `${(bytes / 1024 ** 3).toFixed(2)} GB`;
}

type T = ReturnType<typeof useT>;

/** Runs one action at a time, with its result or error shown under the buttons. */
function useAction() {
  const [busy, setBusy] = useState<string | null>(null);
  const [done, setDone] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const run = useCallback(async <R,>(label: string, work: () => Promise<R>, message?: (r: R) => string | null) => {
    setBusy(label);
    setDone(null);
    setError(null);
    try {
      const result = await work();
      if (message) setDone(message(result));
      return result;
    } catch (e) {
      setError(errorText(e));
      return undefined;
    } finally {
      setBusy(null);
    }
  }, []);
  const status = (
    <>
      {busy && (
        <p className="muted small" role="status">
          {busy}
        </p>
      )}
      {done && (
        <p className="notice small" role="status">
          {done}
        </p>
      )}
      {error && (
        <p className="error small" role="alert">
          {error}
        </p>
      )}
    </>
  );
  return { busy, run, status, setError };
}

const TABS = ["Overview", "Plugins", "Themes", "Users", "Environments", "Database", "Debug", "Search and replace", "WooCommerce"] as const;
type Tab = (typeof TABS)[number];

function tabLabel(t: T, tab: Tab) {
  return {
    Overview: t("Overview"),
    Environments: t("Environments"),
    Plugins: t("Plugins"),
    Themes: t("Themes"),
    Users: t("Users"),
    Debug: t("Debug"),
    "Search and replace": t("Search and replace"),
    Database: t("Database"),
    WooCommerce: t("WooCommerce"),
  }[tab];
}

/** The tab each site was on, so coming back to a site opens where it was left. */
const lastTab = new Map<string, Tab>();

/**
 * The WordPress view of a site, laid out like Local or WordPress Studio: the
 * site's picture and its main buttons on top, then tabs for everyday jobs
 * without opening wp-admin.
 */
export function WordPressTools({ site }: { site: Site }) {
  const t = useT();
  const [overview, setOverview] = useState<WpOverview | null>(null);
  const [env, setEnv] = useState<WpEnvironment | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [tab, setTabState] = useState<Tab>(() => lastTab.get(site.domain) ?? "Overview");
  const database = useSiteDatabase(site.domain);
  const setTab = (x: Tab) => {
    lastTab.set(site.domain, x);
    setTabState(x);
  };

  const reload = useCallback(() => {
    setError(null);
    wp.overview(site.domain)
      .then(setOverview)
      .catch((e) => setError(errorText(e)));
    wp.environment(site.domain)
      .then(setEnv)
      .catch(() => setEnv(null));
  }, [site.domain]);
  useEffect(reload, [reload]);

  const tabs = TABS.filter((x) => x !== "WooCommerce" || overview?.woocommerce);
  const shown = tabs.includes(tab) ? tab : "Overview";
  return (
    <div className="wp-view">
      <header className="wp-hero">
        <SiteThumb site={site} large />
        <div className="wp-hero-main">
          <div className="wp-hero-title">
            <h2>{site.name}</h2>
            {env?.kind === "staging" && <span className="wp-pill">{t("Staging")}</span>}
            {env?.live && <span className="wp-pill">{t("Copy of a live site")}</span>}
          </div>
          <button type="button" className="link mono small" onClick={() => api.openLocalPage(site.url)}>
            {site.url.replace(/^https?:\/\//, "")}
          </button>
          <div className="wp-facts-line small">
            {overview?.woocommerce ? (
              <span>
                {t("WooCommerce")} {overview.woocommerce}
              </span>
            ) : null}
            <span>
              {t("WordPress")} {overview?.wordpress ?? "…"}
            </span>
            <span>
              {t("PHP")} {overview?.php ?? "…"}
            </span>
            {database.db && (
              <span className={database.db.running ? "wp-db ok" : "wp-db warn"}>
                <span className={database.db.running ? "dot ok" : "dot warn"} aria-hidden="true" />
                {database.db.service_name ?? t("Database")} {database.db.running ? t("running") : t("stopped")}
              </span>
            )}
          </div>
          <div className="row wrap wp-hero-buttons">
            <button type="button" className="btn primary" onClick={() => api.openLocalPage(site.url)}>
              {t("Open site")}
            </button>
            <button type="button" className="btn" onClick={() => api.openLocalPage(`${site.url}/wp-admin/`)}>
              {t("WP Admin")}
            </button>
          </div>
        </div>
      </header>
      <SiteDatabaseNotice site={site} state={database} />
      {error && (
        <p className="error small" role="alert">
          {error}
        </p>
      )}
      <div className="wp-tabs" role="tablist" aria-label={t("WordPress tools for {site}", { site: site.domain })}>
        {tabs.map((x) => (
          <button key={x} type="button" role="tab" className="wp-tab" aria-selected={shown === x} onClick={() => setTab(x)}>
            {tabLabel(t, x)}
          </button>
        ))}
      </div>
      <div className="wp-panel" role="tabpanel">
        {!overview ? (
          !error && <p className="muted small">{t("Reading the site…")}</p>
        ) : (
          <>
            {shown === "Overview" && <OverviewTab site={site} overview={overview} env={env} database={database} onTab={setTab} />}
            {shown === "Environments" && <EnvironmentsTab site={site} />}
            {shown === "Plugins" && <ExtensionsTab site={site} kind="plugin" wpCli={overview.wp_cli} blockTheme={overview.block_theme} />}
            {shown === "Themes" && <ExtensionsTab site={site} kind="theme" wpCli={overview.wp_cli} blockTheme={overview.block_theme} />}
            {shown === "Users" && <UsersTab site={site} />}
            {shown === "Debug" && <DebugTab site={site} overview={overview} onChanged={setOverview} />}
            {shown === "Search and replace" && <ReplaceTab site={site} />}
            {shown === "Database" && <DatabaseTab site={site} overview={overview} onChanged={setOverview} />}
            {shown === "WooCommerce" && <WooTab site={site} />}
          </>
        )}
      </div>
    </div>
  );
}

function Facts({ rows }: { rows: [string, ReactNode][] }) {
  return (
    <dl className="facts">
      {rows.map(([k, v]) => (
        <div key={k} className="list-row">
          <dt className="muted small">{k}</dt>
          <dd className="small">{v}</dd>
        </div>
      ))}
    </dl>
  );
}

/** Shortcuts into wp-admin, like WordPress Studio's "Customize" box. */
function shortcuts(t: T, blockTheme: boolean): [string, string][] {
  return [
    ...(blockTheme
      ? ([
          [t("Site editor"), "site-editor.php"],
          [t("Styles"), "site-editor.php?path=%2Fwp_global_styles"],
          [t("Patterns"), "site-editor.php?path=%2Fpatterns"],
        ] as [string, string][])
      : ([
          [t("Customizer"), "customize.php"],
          [t("Menus"), "nav-menus.php"],
          [t("Widgets"), "widgets.php"],
        ] as [string, string][])),
    [t("Pages"), "edit.php?post_type=page"],
    [t("Posts"), "edit.php"],
    [t("Media"), "upload.php"],
    [t("Settings"), "options-general.php"],
    [t("Permalinks"), "options-permalink.php"],
  ];
}

function OverviewTab({
  site,
  overview,
  env,
  database,
  onTab,
}: {
  site: Site;
  overview: WpOverview;
  env: WpEnvironment | null;
  database: ReturnType<typeof useSiteDatabase>;
  onTab: (tab: Tab) => void;
}) {
  const t = useT();
  const { isOn } = useModules();
  const [disk, setDisk] = useState<WpDiskUse | null>(null);
  const [update, setUpdate] = useState<WpCoreUpdate | null>(null);
  const action = useAction();

  useEffect(() => {
    wp.diskUse(site.domain).then(setDisk).catch(() => setDisk(null));
  }, [site.domain]);
  const live = env?.live;
  const db = database.db;

  const rows: [string, ReactNode][] = [
    [t("Site domain"), <span className="mono">{site.domain}</span>],
    [t("SSL"), site.https ? t("Local HTTPS certificate") : t("HTTP only")],
    [t("Web server"), "Caddy"],
    [t("PHP version"), overview.php ?? t("unknown")],
    [
      t("Database"),
      db ? (
        <>
          <span className="mono">{db.database}</span>
          {` · ${db.service_name ?? t("no server has it yet")}`}
          {db.service_name && ` · ${db.running ? t("running") : t("stopped")}`}
        </>
      ) : (
        overview.database ?? "?"
      ),
    ],
    [t("Table prefix"), <span className="mono">{overview.prefix}</span>],
    [t("WordPress version"), overview.wordpress ?? t("unknown")],
    ...(overview.woocommerce ? ([[t("WooCommerce"), overview.woocommerce]] as [string, ReactNode][]) : []),
    [t("Environment"), overview.environment ?? t("not set")],
    ...(live
      ? ([
          [
            t("Live site"),
            <>
              <span className="mono">{live.url}</span>
              {` · ${t("copied {date}", { date: new Date(live.synced_at * 1000).toLocaleString() })}`}
              {live.media === "from-live" ? ` · ${t("media loads from the live site")}` : ""}
            </>,
          ],
        ] as [string, ReactNode][])
      : []),
    ...(overview.multisite ? ([[t("Multisite"), t("yes")]] as [string, ReactNode][]) : []),
    [
      t("Disk use"),
      disk
        ? `${size(disk.total)} ${t("in all")} · ${t("media")} ${size(disk.uploads)} · ${t("plugins")} ${size(disk.plugins)} · ${t("themes")} ${size(disk.themes)}${disk.database != null ? ` · ${t("database")} ${size(disk.database)}` : ""}`
        : t("Counting…"),
    ],
  ];

  return (
    <div className="wp-overview">
      <section className="wp-card" aria-labelledby="wp-details-h">
        <h3 id="wp-details-h">{t("Site details")}</h3>
        {overview.warnings.map((w) => (
          <p key={w} className="warn small">
            {w}
          </p>
        ))}
        <Facts rows={rows} />
        {db?.service_name && db.running && <AutostartCheck state={database} />}
        <div className="row wrap">
          <button
            type="button"
            className="btn small"
            disabled={action.busy !== null}
            onClick={() =>
              action.run(
                overview.wp_cli ? t("Checking for a new WordPress…") : t("Downloading WP-CLI, then checking for a new WordPress…"),
                () => wp.coreUpdate(site.domain),
                (u) => {
                  setUpdate(u);
                  return u.available ? null : t("WordPress is up to date.");
                },
              )
            }
          >
            {t("Check for a WordPress update")}
          </button>
          {update?.available && (
            <button
              type="button"
              className="btn small primary"
              disabled={action.busy !== null}
              onClick={() =>
                action.run(t("Updating WordPress…"), () => wp.updateCore(site.domain), () => {
                  setUpdate(null);
                  return t("WordPress is updated.");
                })
              }
            >
              {t("Update to WordPress {version}", { version: update.available })}
            </button>
          )}
        </div>
        {action.status}
      </section>
      <div className="wp-side">
        <section className="wp-card" aria-labelledby="wp-customize-h">
          <h3 id="wp-customize-h">{t("Customize")}</h3>
          <div className="wp-shortcuts">
            {shortcuts(t, overview.block_theme).map(([label, path]) => (
              <button key={path} type="button" className="wp-shortcut" onClick={() => api.openLocalPage(`${site.url}/wp-admin/${path}`)}>
                {label}
                <Icon name="external" />
              </button>
            ))}
          </div>
        </section>
        <section className="wp-card" aria-labelledby="wp-tools-h">
          <h3 id="wp-tools-h">{t("Tools")}</h3>
          <div className="wp-shortcuts">
            <button type="button" className="wp-shortcut" onClick={() => onTab("Plugins")}>
              {t("Plugins")}
            </button>
            <button type="button" className="wp-shortcut" onClick={() => onTab("Themes")}>
              {t("Themes")}
            </button>
            <button type="button" className="wp-shortcut" onClick={() => onTab("Users")}>
              {t("Log in as a user")}
            </button>
            <button type="button" className="wp-shortcut" onClick={() => onTab("Debug")}>
              {t("Debug log")}
            </button>
            {isOn("mailpit") && (
              <button type="button" className="wp-shortcut" onClick={() => api.openLocalPage("http://127.0.0.1:8025")}>
                {t("Mail caught by Mailpit")}
                <Icon name="external" />
              </button>
            )}
          </div>
          {!isOn("mailpit") && (
            <p className="muted small">{t("Turn on Mailpit in Settings › Modules to catch the mail this site sends.")}</p>
          )}
        </section>
      </div>
    </div>
  );
}

/** Plugin and theme lists already read this session, so switching tabs shows them at once. */
const extensionLists = new Map<string, WpExtension[]>();
/** Lists whose updates were checked with wordpress.org this session. */
const updatesChecked = new Set<string>();
/** Theme pictures already read, by site and theme. */
const themePictures = new Map<string, string | null>();

type Filter = "all" | "active" | "inactive" | "updates";

function ExtensionsTab({ site, kind, wpCli, blockTheme }: { site: Site; kind: WpKind; wpCli: boolean; blockTheme: boolean }) {
  const t = useT();
  const key = `${site.domain}|${kind}`;
  const [list, setList] = useState<WpExtension[] | null>(() => extensionLists.get(key) ?? null);
  const [checking, setChecking] = useState(false);
  const [filter, setFilter] = useState<Filter>("all");
  const [search, setSearch] = useState("");
  const [slug, setSlug] = useState("");
  const action = useAction();
  const word = kind === "plugin" ? t("plugins") : t("themes");

  const keep = useCallback(
    (l: WpExtension[]) => {
      extensionLists.set(key, l);
      setList(l);
    },
    [key],
  );

  const checkUpdates = useCallback(async () => {
    setChecking(true);
    try {
      keep(await wp.extensions(site.domain, kind, true));
      updatesChecked.add(key);
    } catch (e) {
      action.setError(errorText(e));
    } finally {
      setChecking(false);
    }
  }, [site.domain, kind, key, keep]);

  useEffect(() => {
    const cached = extensionLists.get(key);
    setList(cached ?? null);
    (async () => {
      if (cached) {
        // Shown at once; read again quietly in case it changed in wp-admin.
        await wp.extensions(site.domain, kind, false).then(keep).catch(() => {});
      } else {
        const first = await action.run(
          wpCli ? t("Reading the {what}…", { what: word }) : t("Downloading WP-CLI (only this once), then reading the {what}…", { what: word }),
          () => wp.extensions(site.domain, kind, false),
        );
        if (!first) return;
        keep(first);
      }
      if (!updatesChecked.has(key)) await checkUpdates();
    })();
  }, [key]);

  const change = (name: string, act: "activate" | "deactivate" | "update" | "delete", label: string) =>
    action.run(label, () => wp.changeExtension(site.domain, kind, name, act), (l) => {
      keep(l);
      return null;
    });

  async function install(activate: boolean) {
    const s = slug.trim();
    if (!s) return;
    await action.run(t("Installing {name}…", { name: s }), () => wp.installExtension(site.domain, kind, s, activate), (l) => {
      keep(l);
      setSlug("");
      return activate ? t("{name} is installed and active.", { name: s }) : t("{name} is installed.", { name: s });
    });
  }

  const isActive = (e: WpExtension) => e.status === "active" || e.status === "parent" || e.status === "active-network";
  const counts = {
    all: list?.length ?? 0,
    active: list?.filter(isActive).length ?? 0,
    inactive: list?.filter((e) => e.status === "inactive").length ?? 0,
    updates: list?.filter((e) => e.update === "available").length ?? 0,
  };
  const needle = search.trim().toLowerCase();
  const shown = (list ?? []).filter((e) => {
    if (filter === "active" && !isActive(e)) return false;
    if (filter === "inactive" && e.status !== "inactive") return false;
    if (filter === "updates" && e.update !== "available") return false;
    return !needle || e.title.toLowerCase().includes(needle) || e.name.includes(needle);
  });
  const filters: [Filter, string][] = [
    ["all", t("All")],
    ["active", t("Active")],
    ["inactive", t("Inactive")],
    ["updates", t("Update available")],
  ];
  const disabled = action.busy !== null;

  return (
    <div className="stack">
      <form
        className="wp-add"
        onSubmit={(e: FormEvent) => {
          e.preventDefault();
          install(false);
        }}
      >
        <label className="small" htmlFor={`wp-install-${kind}`}>
          {kind === "plugin" ? t("Add a plugin from wordpress.org") : t("Add a theme from wordpress.org")}
        </label>
        <input
          id={`wp-install-${kind}`}
          className="mono"
          placeholder={kind === "plugin" ? "query-monitor" : "twentytwentyfive"}
          value={slug}
          spellCheck={false}
          onChange={(e) => setSlug(e.target.value)}
        />
        <button type="submit" className="btn small" disabled={disabled || !slug.trim()}>
          {t("Install")}
        </button>
        <button type="button" className="btn small primary" disabled={disabled || !slug.trim()} onClick={() => install(true)}>
          {t("Install and activate")}
        </button>
      </form>

      {list && (
        <div className="wp-list-bar">
          <div className="wp-filters" role="group" aria-label={t("Show")}>
            {filters
              .filter(([f]) => f !== "updates" || counts.updates > 0)
              .map(([f, label]) => (
                <button key={f} type="button" className="wp-filter" aria-pressed={filter === f} onClick={() => setFilter(f)}>
                  {label} <span className="wp-count">{counts[f]}</span>
                </button>
              ))}
          </div>
          <div className="row wp-list-tools">
            {counts.updates > 1 && (
              <button
                type="button"
                className="btn small"
                disabled={disabled}
                onClick={() => change("*", "update", t("Updating all {what}…", { what: word }))}
              >
                {t("Update all")}
              </button>
            )}
            <button type="button" className="btn small ghost" disabled={checking} onClick={checkUpdates}>
              {checking ? t("Checking for updates…") : t("Check for updates")}
            </button>
            <input
              type="search"
              className="wp-search"
              placeholder={kind === "plugin" ? t("Search plugins") : t("Search themes")}
              aria-label={kind === "plugin" ? t("Search plugins") : t("Search themes")}
              value={search}
              onChange={(e) => setSearch(e.target.value)}
            />
          </div>
        </div>
      )}
      {action.status}
      {list && shown.length === 0 && <p className="muted small">{t("Nothing to show.")}</p>}
      {kind === "plugin" ? (
        <div className="wp-plugins">
          {shown.map((e) => (
            <PluginRow key={e.name} e={e} active={isActive(e)} disabled={disabled} change={change} />
          ))}
        </div>
      ) : (
        <div className="wp-themes">
          {shown.map((e) => (
            <ThemeCard key={e.name} site={site} e={e} active={isActive(e)} blockTheme={blockTheme} disabled={disabled} change={change} />
          ))}
        </div>
      )}
    </div>
  );
}

type Change = (name: string, act: "activate" | "deactivate" | "update" | "delete", label: string) => unknown;

/** A plugin as the Plugins screen in wp-admin shows it: Activate, Deactivate and Delete under its name. */
function PluginRow({ e, active, disabled, change }: { e: WpExtension; active: boolean; disabled: boolean; change: Change }) {
  const t = useT();
  const name = e.title || e.name;
  const fixed = e.status === "must-use" || e.status === "dropin";
  return (
    <div className="wp-plugin" data-active={active || undefined}>
      <div className="wp-plugin-main">
        <strong>{name}</strong>
        <span className="muted small mono">{e.name}</span>
      </div>
      <div className="wp-plugin-meta small">
        <span className="muted">{t("Version {version}", { version: e.version })}</span>
        <span className={active ? "wp-state on" : "wp-state"}>
          {fixed ? (e.status === "must-use" ? t("Must-use") : t("Drop-in")) : active ? t("Active") : t("Inactive")}
        </span>
      </div>
      {!fixed && (
        <div className="wp-plugin-actions">
          {active ? (
            <button type="button" className="btn small" disabled={disabled} onClick={() => change(e.name, "deactivate", t("Deactivating {name}…", { name }))}>
              {t("Deactivate")}
            </button>
          ) : (
            <>
              <button
                type="button"
                className="btn small primary"
                disabled={disabled}
                onClick={() => change(e.name, "activate", t("Activating {name}…", { name }))}
              >
                {t("Activate")}
              </button>
              <button
                type="button"
                className="btn small ghost danger"
                aria-label={t("Delete {name}", { name })}
                disabled={disabled}
                onClick={() => change(e.name, "delete", t("Deleting {name}…", { name }))}
              >
                {t("Delete")}
              </button>
            </>
          )}
        </div>
      )}
      {e.update === "available" && e.update_version && (
        <div className="wp-update small">
          <span>{t("There is a new version of {name} available: {version}.", { name, version: e.update_version })}</span>
          <button type="button" className="btn small" disabled={disabled} onClick={() => change(e.name, "update", t("Updating {name}…", { name }))}>
            {t("Update now")}
          </button>
        </div>
      )}
    </div>
  );
}

/** A theme as Appearance › Themes shows it: its picture, and Activate on the ones not in use. */
function ThemeCard({
  site,
  e,
  active,
  blockTheme,
  disabled,
  change,
}: {
  site: Site;
  e: WpExtension;
  active: boolean;
  blockTheme: boolean;
  disabled: boolean;
  change: Change;
}) {
  const t = useT();
  const name = e.title || e.name;
  const key = `${site.domain}|${e.name}`;
  const [picture, setPicture] = useState<string | null>(() => themePictures.get(key) ?? null);
  useEffect(() => {
    if (themePictures.has(key)) return;
    wp.themeScreenshot(site.domain, e.name)
      .catch(() => null)
      .then((url) => {
        themePictures.set(key, url);
        setPicture(url);
      });
  }, [key]);
  return (
    <div className="wp-theme" data-active={active || undefined}>
      <div className="wp-theme-picture">
        {picture ? <img src={picture} alt="" draggable={false} /> : <span className="muted small">{name}</span>}
        {e.update === "available" && e.update_version && (
          <button
            type="button"
            className="wp-theme-update small"
            disabled={disabled}
            onClick={() => change(e.name, "update", t("Updating {name}…", { name }))}
          >
            {t("New version {version}. Update now", { version: e.update_version })}
          </button>
        )}
      </div>
      <div className="wp-theme-bar">
        <span className="wp-theme-name" title={name}>
          {active && e.status !== "parent" && <span className="wp-theme-active">{t("Active:")}</span>} {name}
          {e.status === "parent" && <span className="muted small"> · {t("parent theme")}</span>}
        </span>
        {active ? (
          e.status !== "parent" && (
            <button
              type="button"
              className="btn small"
              onClick={() => api.openLocalPage(`${site.url}/wp-admin/${blockTheme ? "site-editor.php" : "customize.php"}`)}
            >
              {t("Customize")}
            </button>
          )
        ) : (
          <span className="row">
            <button
              type="button"
              className="btn small primary"
              disabled={disabled}
              onClick={() => change(e.name, "activate", t("Activating {name}…", { name }))}
            >
              {t("Activate")}
            </button>
            <button
              type="button"
              className="btn small ghost danger"
              aria-label={t("Delete {name}", { name })}
              disabled={disabled}
              onClick={() => change(e.name, "delete", t("Deleting {name}…", { name }))}
            >
              {t("Delete")}
            </button>
          </span>
        )}
      </div>
    </div>
  );
}

function newPassword() {
  const chars = "abcdefghijkmnpqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ23456789";
  const bytes = new Uint8Array(16);
  crypto.getRandomValues(bytes);
  return Array.from(bytes, (b) => chars[b % chars.length]).join("");
}

function UsersTab({ site }: { site: Site }) {
  const t = useT();
  const [users, setUsers] = useState<WpUser[] | null>(null);
  const [adding, setAdding] = useState(false);
  const [login, setLogin] = useState("");
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState(newPassword);
  const action = useAction();

  useEffect(() => {
    action.run(t("Reading the users…"), () => wp.users(site.domain), (u) => {
      setUsers(u);
      return null;
    });
  }, [site.domain]);

  async function add(e: FormEvent) {
    e.preventDefault();
    const pw = password;
    await action.run(t("Adding {name}…", { name: login }), () => wp.addAdmin(site.domain, login, email, pw), (u) => {
      setUsers(u);
      setAdding(false);
      const message = t("{name} can log in with the password {password}", { name: login, password: pw });
      setLogin("");
      setEmail("");
      setPassword(newPassword());
      return message;
    });
  }

  return (
    <div className="stack">
      <p className="muted small">{t("Log in as anyone with a one-time link that works for five minutes, on this computer only.")}</p>
      {users?.map((u) => (
        <div key={u.ID} className="list-row">
          <span className="small">
            <strong>{u.user_login}</strong> <span className="muted">{u.user_email} · {u.roles}</span>
          </span>
          <span className="row">
            <button
              type="button"
              className="btn small"
              disabled={action.busy !== null}
              onClick={() => action.run(t("Opening a login link…"), () => wp.loginAs(site.domain, u.ID))}
            >
              {t("Log in as")}
            </button>
            <button
              type="button"
              className="btn small ghost"
              disabled={action.busy !== null}
              onClick={() => {
                const pw = newPassword();
                action.run(t("Setting a new password…"), () => wp.setPassword(site.domain, u.ID, pw), () =>
                  t("{name} can log in with the password {password}", { name: u.user_login, password: pw }),
                );
              }}
            >
              {t("New password")}
            </button>
          </span>
        </div>
      ))}
      {adding ? (
        <form className="card stack form" onSubmit={add}>
          <div className="form-grid">
            <div className="field">
              <label htmlFor="wp-user-login">{t("User name")}</label>
              <input id="wp-user-login" value={login} required onChange={(e) => setLogin(e.target.value)} />
            </div>
            <div className="field">
              <label htmlFor="wp-user-email">{t("E-mail")}</label>
              <input id="wp-user-email" type="email" value={email} required onChange={(e) => setEmail(e.target.value)} />
            </div>
            <div className="field">
              <label htmlFor="wp-user-password">{t("Password")}</label>
              <input id="wp-user-password" className="mono" value={password} minLength={8} required onChange={(e) => setPassword(e.target.value)} />
            </div>
          </div>
          <div className="row">
            <button type="submit" className="btn primary" disabled={action.busy !== null}>
              {t("Add administrator")}
            </button>
            <button type="button" className="btn ghost" onClick={() => setAdding(false)}>
              {t("Cancel")}
            </button>
          </div>
        </form>
      ) : (
        <div className="row">
          <button type="button" className="btn" onClick={() => setAdding(true)}>
            {t("Add an administrator…")}
          </button>
        </div>
      )}
      {action.status}
    </div>
  );
}

function DebugTab({ site, overview, onChanged }: { site: Site; overview: WpOverview; onChanged: (o: WpOverview) => void }) {
  const t = useT();
  const [log, setLog] = useState<WpLog | null>(null);
  const [follow, setFollow] = useState(true);
  const action = useAction();

  const read = useCallback(() => {
    wp.log(site.domain).then(setLog).catch(() => {});
  }, [site.domain]);
  useEffect(() => {
    read();
    if (!follow) return;
    const timer = setInterval(read, 2000);
    return () => clearInterval(timer);
  }, [read, follow]);

  const set = (change: Partial<WpDebug>) =>
    action.run(t("Saving wp-config.php…"), () => wp.setDebug(site.domain, { ...overview.debug, ...change }), (o) => {
      onChanged(o);
      return null;
    });

  const flags: [keyof WpDebug, string, string][] = [
    ["debug", "WP_DEBUG", t("Report PHP errors and notices")],
    ["log", "WP_DEBUG_LOG", t("Write them to debug.log")],
    ["display", "WP_DEBUG_DISPLAY", t("Show them on the pages")],
    ["scripts", "SCRIPT_DEBUG", t("Load full, unminified scripts and styles")],
    ["queries", "SAVEQUERIES", t("Keep every database query for the page (slower)")],
  ];
  return (
    <div className="stack">
      {flags.map(([key, name, label]) => (
        <label key={key} className="check">
          <input
            type="checkbox"
            checked={overview.debug[key]}
            disabled={action.busy !== null || (key !== "debug" && key !== "scripts" && key !== "queries" && !overview.debug.debug)}
            onChange={(e) => set({ [key]: e.target.checked })}
          />
          <span className="mono small">{name}</span> <span className="small">{label}</span>
        </label>
      ))}
      {action.status}
      <div className="row between centered wrap">
        <span className="muted small mono">{log?.path}</span>
        <span className="row">
          <label className="check small">
            <input type="checkbox" checked={follow} onChange={(e) => setFollow(e.target.checked)} />
            {t("Follow")}
          </label>
          <button
            type="button"
            className="btn small ghost"
            disabled={!log?.size}
            onClick={() => action.run(t("Clearing debug.log…"), () => wp.clearLog(site.domain)).then(read)}
          >
            {t("Clear")}
          </button>
        </span>
      </div>
      <pre className="log wp-log mono small" aria-label="debug.log" tabIndex={0}>
        {log?.text || t("debug.log is empty.")}
      </pre>
    </div>
  );
}

function ReplaceTab({ site }: { site: Site }) {
  const t = useT();
  const [from, setFrom] = useState("");
  const [to, setTo] = useState("");
  const [addresses, setAddresses] = useState(true);
  const [preview, setPreview] = useState<WpReplaced | null>(null);
  const action = useAction();

  useEffect(() => setPreview(null), [from, to, addresses]);

  return (
    <div className="stack">
      <p className="muted small">
        {t(
          "Changes text everywhere in the database, including inside serialized data (widgets, theme settings, page builders), whose lengths are counted again. A backup is taken first.",
        )}
      </p>
      <div className="form-grid">
        <div className="field">
          <label htmlFor="wp-sr-from">{t("Find")}</label>
          <input id="wp-sr-from" className="mono" value={from} placeholder="https://example.com" onChange={(e) => setFrom(e.target.value)} />
        </div>
        <div className="field">
          <label htmlFor="wp-sr-to">{t("Replace with")}</label>
          <input id="wp-sr-to" className="mono" value={to} placeholder={site.url} onChange={(e) => setTo(e.target.value)} />
        </div>
      </div>
      <label className="check small">
        <input type="checkbox" checked={addresses} onChange={(e) => setAddresses(e.target.checked)} />
        {t("These are site addresses: also change the http://, www. and escaped forms")}
      </label>
      <div className="row wrap">
        <button
          type="button"
          className="btn"
          disabled={action.busy !== null || !from}
          onClick={() =>
            action.run(t("Counting…"), () => wp.searchReplace(site.domain, from, to, addresses, true), (r) => {
              setPreview(r);
              return null;
            })
          }
        >
          {t("Preview")}
        </button>
        <button
          type="button"
          className="btn primary"
          disabled={action.busy !== null || !preview || preview.report.replaced === 0}
          onClick={() =>
            action.run(t("Replacing…"), () => wp.searchReplace(site.domain, from, to, addresses, false), (r) => {
              setPreview(null);
              return t("Replaced {count} places. The database as it was is backed up as {file} (Databases screen).", {
                count: r.report.replaced,
                file: r.backup ?? "",
              });
            })
          }
        >
          {preview ? t("Replace {count} places", { count: preview.report.replaced }) : t("Replace")}
        </button>
      </div>
      {preview && <ReplaceReport replaced={preview} t={t} />}
      {action.status}
    </div>
  );
}

export function ReplaceReport({ replaced, t }: { replaced: WpReplaced; t: T }) {
  const { report } = replaced;
  return (
    <div className="stack tight">
      <p className="small">
        {report.replaced === 0 ? t("Nothing found.") : t("{count} places would change:", { count: report.replaced })}
      </p>
      {Object.entries(report.tables).map(([table, n]) => (
        <div key={table} className="list-row">
          <span className="mono small">{table}</span>
          <span className="num small">{n}</span>
        </div>
      ))}
      {Object.keys(report.damaged).length > 0 && (
        <p className="warn small">
          {t("Serialized values whose lengths were already wrong (changed as plain text): {tables}", {
            tables: Object.entries(report.damaged)
              .map(([k, n]) => `${k} (${n})`)
              .join(", "),
          })}
        </p>
      )}
      {Object.keys(report.encoded).length > 0 && (
        <p className="warn small">
          {t("Encoded (base64) values that hold the text, left as they are: {tables}", {
            tables: Object.entries(report.encoded)
              .map(([k, n]) => `${k} (${n})`)
              .join(", "),
          })}
        </p>
      )}
    </div>
  );
}

function DatabaseTab({
  site,
  overview,
  onChanged,
}: {
  site: Site;
  overview: WpOverview;
  onChanged: (o: WpOverview) => void;
}) {
  const t = useT();
  const { isOn } = useModules();
  const action = useAction();
  const [confirmImport, setConfirmImport] = useState<string | null>(null);
  const [prefix, setPrefix] = useState<string | null>(null);
  const prefixValid = prefix !== null && /^[A-Za-z0-9_]{1,20}$/.test(prefix) && prefix !== overview.prefix;

  return (
    <div className="stack">
      <div className="row centered wrap">
        <span className="muted small">
          {t("The database {name}, tables starting with {prefix}.", { name: overview.database ?? "?", prefix: overview.prefix })}
        </span>
        {prefix === null && (
          <button
            type="button"
            className="btn small ghost"
            disabled={action.busy !== null || !overview.database}
            onClick={() => setPrefix(overview.prefix)}
          >
            {t("Change prefix…")}
          </button>
        )}
      </div>
      {prefix !== null && (
        <form
          className="stack tight"
          onSubmit={(ev) => {
            ev.preventDefault();
            if (!prefixValid) return;
            const next = prefix;
            setPrefix(null);
            action.run(t("Renaming the tables to {prefix}…", { prefix: next }), () => wp.changePrefix(site.domain, next), (r) => {
              onChanged(r.overview);
              return t("The tables now start with {prefix}. The database as it was is backed up as {file} (Databases screen).", {
                prefix: next,
                file: r.backup,
              });
            });
          }}
        >
          <div className="row centered wrap">
            <label className="row centered small">
              {t("New prefix")}
              <input
                className="mono"
                value={prefix}
                autoFocus
                spellCheck={false}
                maxLength={20}
                aria-invalid={!prefixValid || undefined}
                onChange={(ev) => setPrefix(ev.target.value)}
              />
            </label>
            <button type="submit" className="btn small primary" disabled={!prefixValid}>
              {t("Change prefix")}
            </button>
            <button type="button" className="btn small ghost" onClick={() => setPrefix(null)}>
              {t("Cancel")}
            </button>
          </div>
          <span className="muted small">
            {t("Letters, digits and underscores, e.g. shop_. Every table is renamed, with the user roles and user settings that carry the prefix, and wp-config.php is updated. A backup is taken first.")}
          </span>
        </form>
      )}
      <div className="row wrap">
        <button
          type="button"
          className="btn"
          disabled={action.busy !== null}
          onClick={async () => {
            const dest = await api.pickSaveFile(t("Export the database"), `${overview.database ?? site.name}.sql.gz`);
            if (dest) action.run(t("Exporting…"), () => wp.exportDatabase(site.domain, dest), () => t("Saved to {path}.", { path: dest }));
          }}
        >
          {t("Export…")}
        </button>
        <button
          type="button"
          className="btn"
          disabled={action.busy !== null}
          onClick={async () => {
            const [file] = await api.pickFiles(t("Pick a .sql or .sql.gz file"), undefined, false);
            if (file) setConfirmImport(file);
          }}
        >
          {t("Import…")}
        </button>
        <button
          type="button"
          className="btn ghost"
          disabled={action.busy !== null}
          onClick={() => action.run(t("Optimizing…"), () => wp.optimizeDatabase(site.domain), (n) => t("Optimized {count} tables.", { count: n }))}
        >
          {t("Optimize tables")}
        </button>
        <button
          type="button"
          className="btn ghost"
          disabled={action.busy !== null}
          onClick={() =>
            action.run(t("Clearing transients…"), () => wp.clearTransients(site.domain), (n) =>
              t("Removed {count} cached values (transients).", { count: n }),
            )
          }
        >
          {t("Clear transients")}
        </button>
        {isOn("phpmyadmin") && (
          <button type="button" className="btn ghost" onClick={() => action.run(t("Opening phpMyAdmin…"), () => wp.openPhpMyAdmin(site.domain))}>
            {t("Open in phpMyAdmin")}
          </button>
        )}
      </div>
      {confirmImport && (
        <div className="row centered wrap">
          <span className="warn small">
            {t("Load {file} into {name}? Its tables replace the ones of the same name. A backup is taken first.", {
              file: confirmImport,
              name: overview.database ?? "?",
            })}
          </span>
          <button
            type="button"
            className="btn small primary"
            onClick={() => {
              const file = confirmImport;
              setConfirmImport(null);
              action.run(t("Importing…"), () => wp.importDatabase(site.domain, file), (backup) =>
                backup ? t("Imported. The database as it was is backed up as {file} (Databases screen).", { file: backup }) : t("Imported."),
              );
            }}
          >
            {t("Import")}
          </button>
          <button type="button" className="btn small ghost" onClick={() => setConfirmImport(null)}>
            {t("Cancel")}
          </button>
        </div>
      )}
      {action.status}
    </div>
  );
}

function WooTab({ site }: { site: Site }) {
  const t = useT();
  const [view, setView] = useState<WooView | null>(null);
  const action = useAction();

  const reload = useCallback(() => {
    wp.woo(site.domain)
      .then(setView)
      .catch((e) => action.setError(errorText(e)));
  }, [site.domain]);
  useEffect(reload, [reload]);

  if (!view) return action.status;
  const s = view.status;
  return (
    <div className="stack">
      <Facts
        rows={[
          [t("WooCommerce"), s.version ?? "?"],
          [t("Orders are kept"), s.hpos ? t("in WooCommerce's own tables (HPOS)") : t("as posts (older storage)")],
          [t("Orders"), s.orders.toLocaleString()],
          [t("Products"), s.products.toLocaleString()],
          [t("Customers"), s.customers.toLocaleString()],
          [t("Scheduled actions"), t("{due} due, {failed} failed", { due: s.due_actions, failed: s.failed_actions })],
        ]}
      />
      <div className="row wrap">
        <button
          type="button"
          className="btn"
          disabled={action.busy !== null}
          onClick={() => action.run(t("Running due actions…"), () => wp.wooRunActions(site.domain), (m) => m || t("Done.")).then(reload)}
        >
          {t("Run due actions")}
        </button>
        <button
          type="button"
          className="btn ghost"
          disabled={action.busy !== null}
          onClick={() =>
            action.run(t("Clearing transients…"), () => wp.clearTransients(site.domain), (n) =>
              t("Removed {count} cached values (transients).", { count: n }),
            )
          }
        >
          {t("Clear transients")}
        </button>
        <button
          type="button"
          className="btn ghost"
          disabled={action.busy !== null}
          onClick={() =>
            action
              .run(t("Making test orders…"), () => wp.wooCreate(site.domain, "orders", 3), (ids) =>
                t("Made test orders {list}.", { list: ids.map((i) => `#${i}`).join(", ") }),
              )
              .then(reload)
          }
        >
          {t("Make 3 test orders")}
        </button>
        <button
          type="button"
          className="btn ghost"
          disabled={action.busy !== null}
          onClick={() =>
            action
              .run(t("Making test customers…"), () => wp.wooCreate(site.domain, "customers", 3), (list) =>
                t("Made test customers {list}.", { list: list.join(", ") }),
              )
              .then(reload)
          }
        >
          {t("Make 3 test customers")}
        </button>
      </div>
      {action.status}
      <h3 className="small">{t("Payment methods")}</h3>
      {view.gateways.length === 0 && <p className="muted small">{t("No payment method has been set up yet.")}</p>}
      {view.gateways.map((g) => (
        <div key={g.id} className="list-row">
          <span className="small">
            <strong>{g.title}</strong> <span className="muted">{g.enabled ? t("on") : t("off")}</span>
            {g.test_mode === true && <span className="ok small"> · {t("test mode")}</span>}
            {g.test_mode === false && <span className="warn small"> · {t("live mode: real payments")}</span>}
          </span>
          {g.test_mode === null ? (
            <span className="muted small">{t("switch it in its own settings")}</span>
          ) : (
            <button
              type="button"
              className="btn small"
              disabled={action.busy !== null}
              onClick={() => action.run(t("Saving…"), () => wp.wooTestMode(site.domain, g.id, !g.test_mode), (v) => {
                setView(v);
                return null;
              })}
            >
              {g.test_mode ? t("Switch to live mode") : t("Switch to test mode")}
            </button>
          )}
        </div>
      ))}
    </div>
  );
}
