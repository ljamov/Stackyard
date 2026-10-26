import { useEffect, useState } from "react";
import { api, errorText, siteAddress, type Site } from "../lib/api";
import { useLoad } from "../lib/useLoad";
import { useModules } from "../lib/modules";
import { useT } from "../lib/i18n";
import { siteBrand } from "../lib/brands";
import { closeTab, setLayout, siteTab, type TabId } from "../lib/layout";
import { BrandIcon } from "../components/BrandIcon";
import { Icon } from "../components/Icon";
import { SiteActions } from "../components/SiteActions";
import { SiteSizeFacts } from "../components/SiteSize";
import { SiteThumb } from "../components/SiteThumb";
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

  async function run(open: () => Promise<unknown>) {
    setError(null);
    try {
      await open();
    } catch (e) {
      setError(errorText(e));
    }
  }

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
      <header className="site-hero">
        <SiteThumb site={site} large />
        <div className="site-hero-main">
          <div className="page-title">
            <BrandIcon id={siteBrand(site) ?? ""} fallback="sites" />
            <div>
              <h1>{site.domain}</h1>
              <p className="muted">
                {site.framework ? `${site.framework} · ` : ""}
                {site.runtime}
                {site.parked && ` · ${t("parked")}`}
              </p>
            </div>
          </div>
          <div className="site-hero-buttons">
            <button type="button" className="btn primary" onClick={() => void api.openLocalPage(address)}>
              <Icon name="external" />
              {t("Open in browser")}
            </button>
            <SiteActions site={site} onRun={(open) => void run(open)} onRemove={remove} />
          </div>
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
          <SiteSizeFacts site={site} />
        </dl>
      </section>

      {hasTools && (
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
      )}
    </>
  );
}
