import { api } from "../lib/api";
import { useLoad } from "../lib/useLoad";
import { useServices } from "../lib/useServices";
import { ServiceCard } from "../components/ServiceCard";
import { BrandIcon } from "../components/BrandIcon";
import { Icon } from "../components/Icon";
import { Logo } from "../components/Logo";
import { useModules } from "../lib/modules";
import { siteBrand } from "../lib/brands";
import { openPalette } from "../lib/palette";
import { openTab, showNewSite, siteTab } from "../lib/layout";
import { keyLabel, keysFor, useKeyChanges } from "../lib/keybindings";
import type { Screen } from "../lib/screens";
import { useT, type Translate } from "../lib/i18n";

/** How many sites Home lists; the Sites screen has them all. */
const SITES_SHOWN = 5;

function greeting(t: Translate, hour: number): string {
  if (hour < 5 || hour >= 18) return t("Good evening.");
  return hour < 12 ? t("Good morning.") : t("Good afternoon.");
}

/** Home: a greeting and the box that runs any command, then services, sites and runtimes. */
export function Dashboard({ onNavigate }: { onNavigate: (s: Screen) => void }) {
  const t = useT();
  useKeyChanges();
  const services = useServices();
  const sites = useLoad(api.listSites, []);
  const runtimes = useLoad(api.listRuntimes, []);
  const { isOn } = useModules();
  const shownServices = services.services.filter((s) => isOn(s.id));
  const installed = shownServices.filter((s) => s.installed);
  const isRunning = (s: (typeof installed)[number]) => s.state === "running" || s.state === "restarting";
  const running = installed.filter(isRunning).length;
  const commandKey = keysFor("workbench.commands")[0];

  return (
    <>
      <header className="home-hello">
        <h1>
          <Logo size={32} />
          <span>
            {greeting(t, new Date().getHours())} {t("What are we running?")}
          </span>
        </h1>
        <button type="button" className="ask" onClick={() => openPalette("commands")}>
          <Icon name="search" />
          <span className="grow">{t("Start a service, open a site, change a setting…")}</span>
          {commandKey && <kbd>{keyLabel(commandKey)}</kbd>}
        </button>
      </header>

      <section aria-labelledby="svc-h" className="stack">
        <div className="row between centered">
          <h2 id="svc-h">
            {t("Services")}
            {installed.length > 0 && (
              <span className="muted"> · {t("{running} of {total} running", { running, total: installed.length })}</span>
            )}
          </h2>
          {installed.length > 0 && (
            <div className="row">
              <button
                type="button"
                className="btn small"
                disabled={running === 0}
                onClick={() => installed.filter(isRunning).forEach((s) => services.stop(s.id))}
              >
                {t("Stop all")}
              </button>
              <button
                type="button"
                className="btn primary small"
                disabled={running === installed.length}
                onClick={() => installed.filter((s) => !isRunning(s)).forEach((s) => services.start(s.id))}
              >
                {t("Start all")}
              </button>
            </div>
          )}
        </div>
        <div className="grid services">
          {shownServices.length === 0 && <p className="muted">{t("All services are switched off in Settings.")}</p>}
          {shownServices.map((s) => (
            <ServiceCard key={s.id} service={s} hook={services} compact />
          ))}
        </div>
      </section>

      <section aria-labelledby="sites-h" className="stack">
        <div className="row between centered">
          <h2 id="sites-h">{t("Sites")}</h2>
          <button type="button" className="btn small" onClick={() => onNavigate("Sites")}>
            {sites.data.length > SITES_SHOWN ? t("View all") : t("Manage")}
          </button>
        </div>
        <div className="items">
          {sites.data.length === 0 && (
            <div className="item">
              <span className="grow muted">{t("No sites yet.")}</span>
              <button type="button" className="btn primary small" onClick={showNewSite}>
                {t("New site")}
              </button>
            </div>
          )}
          {sites.data.slice(0, SITES_SHOWN).map((site) => (
            <div key={site.domain} className="item">
              <BrandIcon id={siteBrand(site) ?? ""} fallback="sites" />
              <div className="grow">
                <button
                  type="button"
                  className="item-name item-open"
                  title={t("Open {name} in a tab", { name: site.domain })}
                  onClick={() => openTab(siteTab(site.domain))}
                >
                  {site.domain}
                </button>
                <div className="muted small">
                  {site.framework ? `${site.framework} · ` : ""}
                  {site.runtime}
                </div>
              </div>
              <button
                type="button"
                className="btn small"
                onClick={() => void api.openLocalPage(site.url)}
                aria-label={t("Open {name} in your browser", { name: site.domain })}
              >
                {t("Open")}
              </button>
            </div>
          ))}
        </div>
      </section>

      <section aria-labelledby="rt-h" className="stack">
        <div className="row between centered">
          <h2 id="rt-h">{t("Runtimes")}</h2>
          <button type="button" className="btn small" onClick={() => onNavigate("Runtimes")}>
            {t("Manage")}
          </button>
        </div>
        <div className="items">
          {runtimes.data
            .filter((r) => r.available && isOn(r.kind))
            .map((r) => (
              <div key={r.kind} className="item">
                <BrandIcon id={r.kind} fallback="runtimes" />
                <span className="grow item-name">{r.name}</span>
                <span className="mono muted small">
                  {r.versions
                    .filter((v) => v.installed)
                    .map((v) => (v.default ? t("{version} (default)", { version: v.version }) : v.version))
                    .join(", ") || t("None installed")}
                </span>
              </div>
            ))}
        </div>
      </section>

      {isOn("mcp") && (
        <section aria-labelledby="mcp-h" className="item">
          <span className="brand-icon plain" aria-hidden="true">
            <Icon name="mcp" />
          </span>
          <div className="grow">
            <h2 id="mcp-h" className="item-name">
              {t("MCP server")}
            </h2>
            <p className="muted small">{t("Let AI coding tools list your sites, start services and switch runtime versions.")}</p>
          </div>
          <span className="mono small code">stdio · stackyard mcp</span>
        </section>
      )}
    </>
  );
}
