import { api } from "../lib/api";
import { useLoad } from "../lib/useLoad";
import { useServices } from "../lib/useServices";
import { ServiceCard } from "../components/ServiceCard";
import { useModules } from "../lib/modules";
import type { Screen } from "../lib/screens";
import { useT } from "../lib/i18n";

export function Dashboard({ onNavigate }: { onNavigate: (s: Screen) => void }) {
  const t = useT();
  const services = useServices();
  const sites = useLoad(api.listSites, []);
  const runtimes = useLoad(api.listRuntimes, []);
  const { isOn } = useModules();
  const shownServices = services.services.filter((s) => isOn(s.id));

  return (
    <>
      <header className="page-header">
        <div>
          <h1>{t("Dashboard")}</h1>
          <p className="muted">
            {sites.data.length === 1 ? t("1 site") : t("{count} sites", { count: sites.data.length })} ·{" "}
            {t("{count} services", { count: shownServices.length })}
          </p>
        </div>
        <button type="button" className="btn primary" onClick={() => onNavigate("Sites")}>
          {t("New site")}
        </button>
      </header>

      <section aria-labelledby="svc-h" className="stack">
        <h2 id="svc-h">{t("Services")}</h2>
        <div className="grid">
          {shownServices.length === 0 && <p className="muted">{t("All services are switched off in Settings.")}</p>}
          {shownServices.map((s) => (
            <ServiceCard key={s.id} service={s} hook={services} compact />
          ))}
        </div>
      </section>

      <div className="grid wide">
        <section aria-labelledby="sites-h" className="card stack">
          <div className="row between">
            <h2 id="sites-h">{t("Sites")}</h2>
            <button type="button" className="link" onClick={() => onNavigate("Sites")}>
              {t("View all")}
            </button>
          </div>
          {sites.data.length === 0 && <p className="muted">{t("No sites yet.")}</p>}
          {sites.data.slice(0, 5).map((site) => (
            <div key={site.domain} className="list-row">
              <span className="mono">{site.url}</span>
              <span className="muted small">{site.runtime}</span>
            </div>
          ))}
        </section>

        <section aria-labelledby="rt-h" className="card stack">
          <div className="row between">
            <h2 id="rt-h">{t("Runtimes")}</h2>
            <button type="button" className="link" onClick={() => onNavigate("Runtimes")}>
              {t("Manage")}
            </button>
          </div>
          {runtimes.data
            .filter((r) => r.available && isOn(r.kind))
            .map((r) => (
              <div key={r.kind} className="list-row">
                <span>{r.name}</span>
                <span className="mono muted small">
                  {r.versions
                    .filter((v) => v.installed)
                    .map((v) => (v.default ? t("{version} (default)", { version: v.version }) : v.version))
                    .join(", ") || t("None installed")}
                </span>
              </div>
            ))}
        </section>

        {isOn("mcp") && (
          <section aria-labelledby="mcp-h" className="card stack">
            <h2 id="mcp-h">{t("MCP server")}</h2>
            <p className="muted">
              {t("Let AI coding tools list your sites, start services and switch runtime versions.")}
            </p>
            <div className="mono small code">stdio · stackyard mcp</div>
          </section>
        )}
      </div>
    </>
  );
}
