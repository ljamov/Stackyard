import { useEffect, useState } from "react";
import { api, errorText, type Module, type Service } from "../lib/api";
import { useModules } from "../lib/modules";
import { useServices } from "../lib/useServices";
import { ServiceCard } from "../components/ServiceCard";
import { BrandIcon } from "../components/BrandIcon";
import { useT } from "../lib/i18n";
import { useBusy } from "../lib/tabLife";

export function Services() {
  const t = useT();
  const hook = useServices();
  const { modules, isOn } = useModules();
  const shown = hook.services.filter((s) => isOn(s.id));
  // Services switched off in Settings, so people can see what else there is.
  const off = modules.filter((m) => m.group === "service" && m.available && !m.enabled);
  const [logFor, setLogFor] = useState<string | null>(null);
  const logService = shown.find((s) => s.id === logFor);

  return (
    <>
      <header className="page-header">
        <div>
          <h1>{t("Services")}</h1>
          <p className="muted">
            {t("Everything listens on 127.0.0.1 only. Databases start with user root and no password.")}
          </p>
        </div>
      </header>

      <div className="grid">
        {shown.length === 0 && <p className="muted">{t("All services are switched off in Settings.")}</p>}
        {shown.map((s) => (
          <ServiceCard key={s.id} service={s} hook={hook} onShowLog={setLogFor} />
        ))}
      </div>

      {off.length > 0 && (
        <section className="stack" aria-labelledby="services-off-h">
          <div>
            <h2 id="services-off-h">{t("Turned off")}</h2>
            <p className="muted small">
              {t("These are in Stackyard too. A turned-off service downloads nothing and runs nothing until you turn it on.")}
            </p>
          </div>
          <div className="grid">
            {off.map((m) => (
              <DisabledCard key={m.id} module={m} service={hook.services.find((s) => s.id === m.id)} />
            ))}
          </div>
        </section>
      )}

      {logService && <ServiceLog id={logService.id} name={logService.name} onClose={() => setLogFor(null)} />}
    </>
  );
}

/** A service whose module is off: what it is, and a button to turn it on. */
function DisabledCard({ module: m, service }: { module: Module; service?: Service }) {
  const t = useT();
  const { setModule } = useModules();
  const [busy, setBusy] = useState(false);
  useBusy(busy);
  const [error, setError] = useState<string | null>(null);

  async function turnOn() {
    setBusy(true);
    setError(null);
    try {
      await setModule(m.id, true);
    } catch (e) {
      setError(errorText(e));
      setBusy(false);
    }
  }

  return (
    <div className="card service disabled">
      <div className="service-top">
        <BrandIcon id={m.id} />
        <div className="service-name">
          <span className="strong">{m.name}</span>
          {service && <span className="muted small">{service.version}</span>}
        </div>
        <button
          type="button"
          className="btn small"
          disabled={busy}
          onClick={turnOn}
          aria-label={t("Turn on {name}", { name: m.name })}
        >
          {busy ? t("Turning on…") : t("Turn on")}
        </button>
      </div>
      <div className="service-meta small">
        <span className="status">
          <span className="dot" aria-hidden="true" />
          {t("Disabled")}
          {service && ` · ${t(service.role)}`}
        </span>
      </div>
      <p className="muted small">{t(m.description)}</p>
      {error && (
        <p role="alert" className="error small">
          {error}
        </p>
      )}
    </div>
  );
}

/** A service's own log, read again every two seconds; `onClose` adds a Close button. */
export function ServiceLog({ id, name, onClose }: { id: string; name: string; onClose?: () => void }) {
  const t = useT();
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
    <section aria-label={t("{name} log", { name })} className="card stack">
      <div className="row between">
        <h2>{t("{name} log", { name })}</h2>
        {onClose && (
          <button type="button" className="btn ghost small" onClick={onClose}>
            {t("Close")}
          </button>
        )}
      </div>
      {error && <p className="error small">{error}</p>}
      <pre className="code mono small log">{lines.length ? lines.join("\n") : t("Nothing logged yet.")}</pre>
    </section>
  );
}
