import { useState } from "react";
import { api } from "../lib/api";
import { useLoad } from "../lib/useLoad";
import { useModules } from "../lib/modules";

export function Runtimes() {
  const runtimes = useLoad(api.listRuntimes, []);
  const { isOn } = useModules();
  const shown = runtimes.data.filter((r) => isOn(r.kind));
  const [selected, setSelected] = useState("php");
  const family = shown.find((r) => r.kind === selected) ?? shown[0];

  return (
    <>
      <header className="page-header">
        <div>
          <h1>Runtimes</h1>
          <p className="muted">Install several versions side by side. Each site can pin its own version.</p>
        </div>
      </header>

      <div role="group" aria-label="Language" className="row wrap">
        {shown.length === 0 && <p className="muted">All runtimes are switched off in Settings.</p>}
        {shown.map((r) => (
          <button
            key={r.kind}
            type="button"
            className="tab"
            aria-pressed={r.kind === family?.kind}
            onClick={() => setSelected(r.kind)}
          >
            {r.name}
          </button>
        ))}
      </div>

      {family && (
        <section aria-label={`${family.name} versions`} className="card">
          {!family.available && <p className="notice inset">{family.name} support is coming after the prototype.</p>}
          {family.versions.map((version) => (
            <div key={version} className="version-row">
              <div className="row">
                <span className="mono strong">{version}</span>
                <span className="muted small">Not installed</span>
              </div>
              <button
                type="button"
                className="btn primary small"
                disabled
                title="Downloads arrive in the runtime manager milestone"
              >
                Install
              </button>
            </div>
          ))}
        </section>
      )}

      <p className="card dashed muted">
        Per-project override: add a <code className="mono">.stackyard</code> file to a project folder, or pick a
        version on the Sites screen.
      </p>
    </>
  );
}
