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

const TABS = ["Overview", "Environments", "Plugins", "Themes", "Users", "Debug", "Search and replace", "Database", "WooCommerce"] as const;
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

/** The WordPress tab of a site's tools: everyday jobs without opening wp-admin. */
export function WordPressTools({ site }: { site: Site }) {
  const t = useT();
  const [overview, setOverview] = useState<WpOverview | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [tab, setTab] = useState<Tab>("Overview");

  const reload = useCallback(() => {
    setError(null);
    wp.overview(site.domain)
      .then(setOverview)
      .catch((e) => setError(errorText(e)));
  }, [site.domain]);
  useEffect(reload, [reload]);

  if (error) {
    return (
      <p className="error small" role="alert">
        {error}
      </p>
    );
  }
  if (!overview) return <p className="muted small">{t("Reading the site…")}</p>;
  const tabs = TABS.filter((x) => x !== "WooCommerce" || overview.woocommerce);
  return (
    <div className="stack">
      <div className="row wrap" role="group" aria-label={t("WordPress tools for {site}", { site: site.domain })}>
        {tabs.map((x) => (
          <button key={x} type="button" className="tab" aria-pressed={tab === x} onClick={() => setTab(x)}>
            {tabLabel(t, x)}
          </button>
        ))}
      </div>
      {tab === "Overview" && <OverviewTab site={site} overview={overview} />}
      {tab === "Environments" && <EnvironmentsTab site={site} />}
      {tab === "Plugins" && <ExtensionsTab site={site} kind="plugin" />}
      {tab === "Themes" && <ExtensionsTab site={site} kind="theme" />}
      {tab === "Users" && <UsersTab site={site} />}
      {tab === "Debug" && <DebugTab site={site} overview={overview} onChanged={setOverview} />}
      {tab === "Search and replace" && <ReplaceTab site={site} />}
      {tab === "Database" && <DatabaseTab site={site} overview={overview} />}
      {tab === "WooCommerce" && <WooTab site={site} />}
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

function OverviewTab({ site, overview }: { site: Site; overview: WpOverview }) {
  const t = useT();
  const { isOn } = useModules();
  const [disk, setDisk] = useState<WpDiskUse | null>(null);
  const [update, setUpdate] = useState<WpCoreUpdate | null>(null);
  const [env, setEnv] = useState<WpEnvironment | null>(null);
  const action = useAction();

  useEffect(() => {
    wp.diskUse(site.domain).then(setDisk).catch(() => setDisk(null));
    wp.environment(site.domain).then(setEnv).catch(() => setEnv(null));
  }, [site.domain]);
  const live = env?.live;

  return (
    <div className="stack">
      {overview.warnings.map((w) => (
        <p key={w} className="warn small">
          {w}
        </p>
      ))}
      <Facts
        rows={[
          [t("WordPress"), overview.wordpress ?? t("unknown")],
          [t("PHP"), overview.php ?? t("unknown")],
          [t("Database"), `${overview.database ?? "?"} · ${t("tables start with {prefix}", { prefix: overview.prefix })}${overview.database_server ? ` · ${overview.database_server}` : ""}`],
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
        ]}
      />
      <div className="row wrap">
        <button type="button" className="btn" onClick={() => api.openLocalPage(site.url)}>
          {t("Open site")}
        </button>
        <button type="button" className="btn" onClick={() => api.openLocalPage(`${site.url}/wp-admin/`)}>
          {t("Open wp-admin")}
        </button>
        {isOn("mailpit") && (
          <button type="button" className="btn ghost" onClick={() => api.openLocalPage("http://127.0.0.1:8025")}>
            {t("Mail caught by Mailpit")}
          </button>
        )}
        <button
          type="button"
          className="btn ghost"
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
      </div>
      {update?.available && (
        <div className="row centered wrap">
          <span className="small">{t("WordPress {version} is available.", { version: update.available })}</span>
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
            {t("Update WordPress")}
          </button>
        </div>
      )}
      {action.status}
      {!isOn("mailpit") && (
        <p className="muted small">{t("Turn on Mailpit in Settings › Modules to catch the mail this site sends.")}</p>
      )}
    </div>
  );
}

function ExtensionsTab({ site, kind }: { site: Site; kind: WpKind }) {
  const t = useT();
  const [list, setList] = useState<WpExtension[] | null>(null);
  const [slug, setSlug] = useState("");
  const action = useAction();
  const word = kind === "plugin" ? t("plugins") : t("themes");

  useEffect(() => {
    action.run(t("Reading the {what}… (the first time, WP-CLI is downloaded)", { what: word }), () => wp.extensions(site.domain, kind), (l) => {
      setList(l);
      return null;
    });
  }, [site.domain, kind]);

  const change = (name: string, act: "activate" | "deactivate" | "update" | "delete", label: string) =>
    action.run(label, () => wp.changeExtension(site.domain, kind, name, act), (l) => {
      setList(l);
      return null;
    });

  async function install(e: FormEvent) {
    e.preventDefault();
    const s = slug.trim();
    if (!s) return;
    await action.run(t("Installing {name}…", { name: s }), () => wp.installExtension(site.domain, kind, s, false), (l) => {
      setList(l);
      setSlug("");
      return t("{name} is installed.", { name: s });
    });
  }

  const updates = list?.filter((e) => e.update === "available").length ?? 0;
  return (
    <div className="stack">
      {list && (
        <div className="row between centered wrap">
          <span className="muted small">
            {updates > 0 ? t("{count} with updates", { count: updates }) : t("All up to date.")}
          </span>
          {updates > 0 && (
            <button
              type="button"
              className="btn small"
              disabled={action.busy !== null}
              onClick={() => change("*", "update", t("Updating all {what}…", { what: word }))}
            >
              {t("Update all")}
            </button>
          )}
        </div>
      )}
      {list?.map((e) => {
        const active = e.status === "active" || e.status === "parent";
        const fixed = e.status === "must-use" || e.status === "dropin";
        return (
          <div key={e.name} className="list-row">
            <span className="small">
              <strong>{e.title || e.name}</strong> <span className="muted">{e.version}</span>
              {active && <span className="ok small"> · {e.status === "parent" ? t("parent theme") : t("on")}</span>}
              {fixed && <span className="muted small"> · {e.status}</span>}
              {e.update === "available" && e.update_version && (
                <span className="warn small"> · {t("{version} available", { version: e.update_version })}</span>
              )}
            </span>
            {!fixed && (
              <span className="row">
                {e.update === "available" && (
                  <button
                    type="button"
                    className="btn small"
                    disabled={action.busy !== null}
                    onClick={() => change(e.name, "update", t("Updating {name}…", { name: e.title || e.name }))}
                  >
                    {t("Update")}
                  </button>
                )}
                {!active && (
                  <button
                    type="button"
                    className="btn small"
                    disabled={action.busy !== null}
                    onClick={() => change(e.name, "activate", t("Switching on {name}…", { name: e.title || e.name }))}
                  >
                    {kind === "theme" ? t("Use") : t("Switch on")}
                  </button>
                )}
                {active && kind === "plugin" && (
                  <button
                    type="button"
                    className="btn small ghost"
                    disabled={action.busy !== null}
                    onClick={() => change(e.name, "deactivate", t("Switching off {name}…", { name: e.title || e.name }))}
                  >
                    {t("Switch off")}
                  </button>
                )}
                {!active && (
                  <button
                    type="button"
                    className="btn small ghost"
                    aria-label={t("Delete {name}", { name: e.title || e.name })}
                    disabled={action.busy !== null}
                    onClick={() => change(e.name, "delete", t("Deleting {name}…", { name: e.title || e.name }))}
                  >
                    {t("Delete")}
                  </button>
                )}
              </span>
            )}
          </div>
        );
      })}
      <form className="row centered wrap" onSubmit={install}>
        <label className="small" htmlFor={`wp-install-${kind}`}>
          {kind === "plugin" ? t("Install a plugin from wordpress.org") : t("Install a theme from wordpress.org")}
        </label>
        <input
          id={`wp-install-${kind}`}
          className="mono"
          placeholder={kind === "plugin" ? "query-monitor" : "twentytwentyfive"}
          value={slug}
          onChange={(e) => setSlug(e.target.value)}
        />
        <button type="submit" className="btn small" disabled={action.busy !== null || !slug.trim()}>
          {t("Install")}
        </button>
      </form>
      {action.status}
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

function DatabaseTab({ site, overview }: { site: Site; overview: WpOverview }) {
  const t = useT();
  const { isOn } = useModules();
  const action = useAction();
  const [confirmImport, setConfirmImport] = useState<string | null>(null);

  return (
    <div className="stack">
      <p className="muted small">
        {t("The database {name}, tables starting with {prefix}.", { name: overview.database ?? "?", prefix: overview.prefix })}
      </p>
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
