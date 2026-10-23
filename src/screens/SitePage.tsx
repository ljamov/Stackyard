import { useEffect, useState } from "react";
import { api, errorText, siteAddress, type Site } from "../lib/api";
import { useLoad } from "../lib/useLoad";
import { useModules } from "../lib/modules";
import { useT } from "../lib/i18n";
import { frameworkBrand } from "../lib/brands";
import { openTerminal } from "../lib/terminal";
import { closeTab, setLayout, siteTab, type TabId } from "../lib/layout";
import { BrandIcon } from "../components/BrandIcon";
import { SiteTools } from "../components/SiteTools";
import { SiteVersions } from "../components/SiteVersions";

/** One site in a tab of its own: its addresses, versions and every site tool, across the whole window. */
export function SitePage({ domain }: { domain: string }) {
  const t = useT();
  const sites = useLoad(api.listSites, null as Site[] | null);
  const runtimes = useLoad(api.listRuntimes, []);
  const system = useLoad(api.systemStatus, null);
  const { isOn } = useModules();
  const [error, setError] = useState<string | null>(null);
  const hasTools = ["sitesettings", "sitebackups", "sitetransfer", "share", "remote", "git", "wordpress"].some(isOn);

  const reloadSites = sites.reload;
  useEffect(() => {
    let off: (() => void) | undefined;
    let cancelled = false;
    api.onSitesChanged(reloadSites).then((u) => (cancelled ? u() : (off = u)));
    return () => {
      cancelled = true;
      off?.();
    };
  }, [reloadSites]);

  if (!sites.data) return null;
  const site = sites.data.find((s) => s.domain === domain);
  if (!site) return <p className="muted">{t("This site isn't in Stackyard any more.")}</p>;
  const address = siteAddress(site, system.data);

  async function remove() {
    setError(null);
    try {
      await api.removeSite(domain);
      closeTab(siteTab(domain));
    } catch (e) {
      setError(errorText(e));
    }
  }

  return (
    <>
      <header className="page-header">
        <div className="page-title">
          <BrandIcon id={frameworkBrand(site.framework) ?? ""} fallback="sites" />
          <div>
            <h1>{site.domain}</h1>
            <p className="muted">
              {site.framework ? `${site.framework} · ` : ""}
              {site.runtime}
              {site.parked && ` · ${t("parked")}`}
            </p>
          </div>
        </div>
        <div className="row-actions">
          {!site.parked && (
            <button type="button" className="btn ghost" onClick={remove}>
              {t("Remove")}
            </button>
          )}
          {isOn("terminal") && (
            <button type="button" className="btn" onClick={() => openTerminal(site.path, site.domain)}>
              {t("Terminal")}
            </button>
          )}
          <button type="button" className="btn primary" onClick={() => void api.openLocalPage(site.url)}>
            {t("Open in browser")}
          </button>
        </div>
      </header>

      {error && (
        <p role="alert" className="error">
          {error}
        </p>
      )}
      {site.problem && <p className="warn">{site.problem}</p>}

      <section className="card facts-card" aria-label={t("Details")}>
        <dl className="facts-grid">
          <dt>{t("Address")}</dt>
          <dd>
            <button type="button" className="link mono" onClick={() => api.openLocalPage(address)}>
              {address}
            </button>
          </dd>
          <dt>{t("Folder")}</dt>
          <dd className="mono connect">{site.root}</dd>
          <dt>{t("Runs with")}</dt>
          <dd>
            <SiteVersions
              site={site}
              families={runtimes.data}
              onChanged={() => {
                sites.reload();
                runtimes.reload();
              }}
            />
          </dd>
        </dl>
      </section>

      {hasTools && (
        <section className="card site-page-tools" aria-label={t("Site tools")}>
          <SiteTools
            site={site}
            onChanged={(renamed) => {
              // A new domain ending renames the site; its tab follows it.
              if (renamed && renamed !== domain) {
                const from = siteTab(domain);
                const to = siteTab(renamed);
                const swap = (x: TabId) => (x === from ? to : x);
                setLayout((l) => ({ tabs: l.tabs.map(swap), active: l.active && swap(l.active) }));
              }
              sites.reload();
            }}
          />
        </section>
      )}
    </>
  );
}
