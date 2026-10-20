import { Icon } from "./Icon";
import type { ServicesHook } from "../lib/useServices";
import type { Site } from "../lib/api";
import type { TabScreen } from "../lib/screens";
import { useT } from "../lib/i18n";

/** The side bar's Explorer: services with a start and stop button, and the sites. */
export function Explorer({
  width,
  services,
  shownServices,
  sites,
  onOpen,
}: {
  width: number;
  services: ServicesHook;
  shownServices: ServicesHook["services"];
  sites: Site[];
  onOpen: (s: TabScreen) => void;
}) {
  const t = useT();
  return (
    <aside className="sidebar-view" style={{ width }} aria-label={t("Explorer")}>
      <div className="view-title">{t("Explorer")}</div>

      <section className="view-section" aria-labelledby="sb-services">
        <h2 id="sb-services">{t("Services")}</h2>
        {shownServices.length === 0 && <p className="view-empty">{t("All services are switched off in Settings.")}</p>}
        {shownServices.map((s) => {
          const running = s.state === "running" || s.state === "restarting";
          const tone = s.state === "running" ? "ok" : s.state === "crashed" ? "error" : s.state === "restarting" ? "warn" : "";
          const busy = services.busy[s.id] || s.installing;
          const action = running ? t("Stop {name}", { name: s.name }) : t("Start {name}", { name: s.name });
          return (
            <div key={s.id} className="view-row">
              <span className={`dot ${tone}`} aria-hidden="true" />
              <button type="button" className="view-link" onClick={() => onOpen("Services")}>
                {s.name}
              </button>
              {s.installed && (
                <button
                  type="button"
                  className="icon-btn"
                  disabled={busy}
                  aria-label={action}
                  title={action}
                  onClick={() => (running ? services.stop(s.id) : services.start(s.id))}
                >
                  <Icon name={running ? "stop" : "play"} />
                </button>
              )}
            </div>
          );
        })}
      </section>

      <section className="view-section" aria-labelledby="sb-sites">
        <h2 id="sb-sites">{t("Sites")}</h2>
        {sites.length === 0 && <p className="view-empty">{t("No sites yet.")}</p>}
        {sites.map((site) => (
          <div key={site.domain} className="view-row">
            <button type="button" className="view-link mono" title={site.runtime} onClick={() => onOpen("Sites")}>
              {site.domain}
            </button>
          </div>
        ))}
      </section>
    </aside>
  );
}
