import { useState } from "react";
import type { Service } from "../lib/api";
import type { ServicesHook } from "../lib/useServices";
import { useT } from "../lib/i18n";

const isRunning = (s: Service) => s.state === "running" || s.state === "restarting";

/**
 * Stop all and Start all for a list of services, for the top right of a
 * services section. Start all starts every installed service that is
 * switched on, and lists any that could not start (MySQL and MariaDB share
 * ports, so only one of them starts).
 */
export function StartAllButtons({ hook, shown }: { hook: ServicesHook; shown: Service[] }) {
  const t = useT();
  const installed = shown.filter((s) => s.installed);
  const running = installed.filter(isRunning).length;
  const [starting, setStarting] = useState(false);
  const [problems, setProblems] = useState<string[]>([]);
  if (installed.length === 0) return null;

  async function startAll() {
    setStarting(true);
    setProblems([]);
    setProblems(await hook.startAll());
    setStarting(false);
  }

  return (
    <div className="stack start-all">
      <div className="row">
        <button
          type="button"
          className="btn small"
          disabled={running === 0}
          onClick={() => {
            setProblems([]);
            installed.filter(isRunning).forEach((s) => hook.stop(s.id));
          }}
        >
          {t("Stop all")}
        </button>
        <button
          type="button"
          className="btn primary small"
          disabled={starting || running === installed.length}
          onClick={startAll}
        >
          {starting ? t("Starting…") : t("Start all")}
        </button>
      </div>
      {problems.length > 0 && (
        <p role="alert" className="error small">
          {problems.join("\n")}
        </p>
      )}
    </div>
  );
}
