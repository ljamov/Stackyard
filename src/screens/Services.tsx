import { useEffect, useState } from "react";
import { api, errorText } from "../lib/api";
import { useModules } from "../lib/modules";
import { useServices } from "../lib/useServices";
import { ServiceCard } from "../components/ServiceCard";

export function Services() {
  const hook = useServices();
  const { isOn } = useModules();
  const shown = hook.services.filter((s) => isOn(s.id));
  const [logFor, setLogFor] = useState<string | null>(null);
  const logService = shown.find((s) => s.id === logFor);

  return (
    <>
      <header className="page-header">
        <div>
          <h1>Services</h1>
          <p className="muted">
            Everything listens on 127.0.0.1 only. Databases start with user <code className="mono">root</code> and no
            password.
          </p>
        </div>
      </header>

      <div className="grid">
        {shown.length === 0 && <p className="muted">All services are switched off in Settings.</p>}
        {shown.map((s) => (
          <ServiceCard key={s.id} service={s} hook={hook} onShowLog={setLogFor} />
        ))}
      </div>

      {logService && <LogPanel id={logService.id} name={logService.name} onClose={() => setLogFor(null)} />}
    </>
  );
}

function LogPanel({ id, name, onClose }: { id: string; name: string; onClose: () => void }) {
  const [lines, setLines] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    const load = () =>
      api
        .serviceLog(id)
        .then((l) => live && (setLines(l), setError(null)))
        .catch((e) => live && setError(errorText(e)));
    load();
    const timer = setInterval(load, 2000);
    return () => {
      live = false;
      clearInterval(timer);
    };
  }, [id]);

  return (
    <section aria-label={`${name} log`} className="card stack">
      <div className="row between">
        <h2>{name} log</h2>
        <button type="button" className="btn ghost small" onClick={onClose}>
          Close
        </button>
      </div>
      {error && <p className="error small">{error}</p>}
      <pre className="code mono small log">{lines.length ? lines.join("\n") : "Nothing logged yet."}</pre>
    </section>
  );
}
