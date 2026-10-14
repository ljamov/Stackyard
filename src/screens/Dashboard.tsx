import { api } from "../lib/api";
import { useLoad } from "../lib/useLoad";
import { useModules } from "../lib/modules";
import type { Screen } from "../components/Sidebar";

export function Dashboard({ onNavigate }: { onNavigate: (s: Screen) => void }) {
  const services = useLoad(api.listServices, []);
  const sites = useLoad(api.listSites, []);
  const runtimes = useLoad(api.listRuntimes, []);
  const { isOn } = useModules();
  const shownServices = services.data.filter((s) => isOn(s.id));

  return (
    <>
      <header className="page-header">
        <div>
          <h1>Dashboard</h1>
          <p className="muted">
            {sites.data.length} {sites.data.length === 1 ? "site" : "sites"} · {shownServices.length} services
          </p>
        </div>
        <button type="button" className="btn primary" onClick={() => onNavigate("Sites")}>
          New site
        </button>
      </header>

      <section aria-labelledby="svc-h" className="stack">
        <h2 id="svc-h">Services</h2>
        <p className="notice">Installing and starting services arrives in the next milestones.</p>
        <div className="grid">
          {shownServices.length === 0 && <p className="muted">All services are switched off in Settings.</p>}
          {shownServices.map((s) => (
            <div key={s.id} className="card service">
              <div className="row between">
                <div>
                  <div className="strong">{s.name}</div>
                  <div className="mono muted small">
                    {s.version} · port {s.ports.join(", ")}
                  </div>
                </div>
                <span className="status">
                  <span className="dot" aria-hidden="true" />
                  Not installed
                </span>
              </div>
              <div className="muted small">{s.role}</div>
            </div>
          ))}
        </div>
      </section>

      <div className="grid wide">
        <section aria-labelledby="sites-h" className="card stack">
          <div className="row between">
            <h2 id="sites-h">Sites</h2>
            <button type="button" className="link" onClick={() => onNavigate("Sites")}>
              View all
            </button>
          </div>
          {sites.data.length === 0 && <p className="muted">No sites yet.</p>}
          {sites.data.slice(0, 5).map((site) => (
            <div key={site.domain} className="list-row">
              <span className="mono">{site.url}</span>
              <span className="muted small">{site.runtime}</span>
            </div>
          ))}
        </section>

        <section aria-labelledby="rt-h" className="card stack">
          <div className="row between">
            <h2 id="rt-h">Runtimes</h2>
            <button type="button" className="link" onClick={() => onNavigate("Runtimes")}>
              Manage
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
                    .map((v) => (v.default ? `${v.version} (default)` : v.version))
                    .join(", ") || "None installed"}
                </span>
              </div>
            ))}
        </section>

        {isOn("mcp") && (
          <section aria-labelledby="mcp-h" className="card stack">
            <h2 id="mcp-h">MCP server</h2>
            <p className="muted">
              Let AI coding tools list your sites, start services and switch runtime versions. Coming in step 4.
            </p>
            <div className="mono small code">stdio · stackyard mcp</div>
          </section>
        )}
      </div>
    </>
  );
}
