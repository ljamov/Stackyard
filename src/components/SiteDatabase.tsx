import { useCallback, useEffect, useState } from "react";
import { api, errorText, type Site, type SiteDb } from "../lib/api";
import { useT } from "../lib/i18n";
import "./site-views.css";
import { useBusy, useShown } from "../lib/tabLife";

/**
 * The database a site uses and whether its server runs, checked again every
 * few seconds so starting a server elsewhere shows up here.
 */
export function useSiteDatabase(domain: string, every = 4000) {
  const [db, setDb] = useState<SiteDb | null>(null);
  const [busy, setBusy] = useState(false);
  useBusy(busy);
  const [error, setError] = useState<string | null>(null);

  const reload = useCallback(() => {
    api.siteDatabase(domain).then(setDb).catch(() => {});
  }, [domain]);
  const shown = useShown();
  useEffect(() => {
    if (!shown) return;
    reload();
    const timer = setInterval(reload, every);
    return () => clearInterval(timer);
  }, [reload, every, shown]);

  async function run(work: () => Promise<SiteDb | null>) {
    setBusy(true);
    setError(null);
    try {
      setDb(await work());
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  }

  return {
    db,
    busy,
    error,
    start: () => run(() => api.startSiteDatabase(domain)),
    setAutostart: (on: boolean) => run(() => api.setSiteDatabaseAutostart(domain, on)),
  };
}

type SiteDatabaseState = ReturnType<typeof useSiteDatabase>;

/** "Start MariaDB with the web server", for a site whose server is known. */
export function AutostartCheck({ state }: { state: SiteDatabaseState }) {
  const t = useT();
  const { db } = state;
  if (!db?.service_name) return null;
  return (
    <label className="check small">
      <input type="checkbox" checked={db.start_with_web} disabled={state.busy} onChange={(e) => state.setAutostart(e.target.checked)} />
      {t("Start {server} whenever the web server starts", { server: db.service_name })}
    </label>
  );
}

/**
 * A warning when the site's database server is not running, with the way to
 * start it: the site shows "Error establishing a database connection" until
 * then. Nothing is shown while it runs.
 */
export function SiteDatabaseNotice({ site, state }: { site: Site; state: SiteDatabaseState }) {
  const t = useT();
  const { db } = state;
  if (!db || db.running) return null;
  const server = db.service_name ?? "";
  let text: string;
  let button: string | null = null;
  if (!db.service) {
    text = t("No database server has the database {database} yet. Install and start MySQL or MariaDB on the Services screen.", {
      database: db.database,
    });
  } else if (!db.installed) {
    text = t("{site} keeps its database {database} in {server}, which is not installed. Install it on the Services screen.", {
      site: site.domain,
      database: db.database,
      server,
    });
  } else if (db.blocked_by) {
    text = t("{site} keeps its database {database} in {server}, but {other} is running. They use the same port, so only one can run at a time.", {
      site: site.domain,
      database: db.database,
      server,
      other: db.blocked_by,
    });
    button = t("Stop {other} and start {server}", { other: db.blocked_by, server });
  } else {
    text = t("{site} keeps its database {database} in {server}, which is stopped, so the site can't open its pages.", {
      site: site.domain,
      database: db.database,
      server,
    });
    button = t("Start {server}", { server });
  }
  return (
    <div className="site-db-notice" role="alert">
      <p className="small">{text}</p>
      <div className="row wrap centered">
        {button && (
          <button type="button" className="btn small primary" disabled={state.busy} onClick={state.start}>
            {state.busy ? t("Starting…") : button}
          </button>
        )}
        <AutostartCheck state={state} />
      </div>
      {state.error && <p className="error small">{state.error}</p>}
    </div>
  );
}

/** A short line for the site list: which database the site uses, and a warning when its server is off. */
export function SiteDatabaseLine({ domain }: { domain: string }) {
  const t = useT();
  const state = useSiteDatabase(domain, 10000);
  const { db } = state;
  if (!db) return null;
  if (db.running) {
    return (
      <div className="muted small">
        {t("Database {database} in {server}", { database: db.database, server: db.service_name ?? "?" })}
      </div>
    );
  }
  return (
    <div className="warn small row wrap centered tight-row">
      <span>
        {db.service_name
          ? t("{server} is not running (database {database})", { server: db.service_name, database: db.database })
          : t("No database server has {database} yet", { database: db.database })}
      </span>
      {db.service && db.installed && (
        <button type="button" className="btn small" disabled={state.busy} onClick={state.start}>
          {db.blocked_by ? t("Switch to {server}", { server: db.service_name ?? "" }) : t("Start {server}", { server: db.service_name ?? "" })}
        </button>
      )}
      {state.error && <span className="error small">{state.error}</span>}
    </div>
  );
}
