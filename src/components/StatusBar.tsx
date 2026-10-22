import { useEffect, useRef, useState } from "react";
import type { ServicesHook } from "../lib/useServices";
import type { TabScreen } from "../lib/screens";
import { togglePanelTab, type PanelTab } from "../lib/layout";
import { useUpdate } from "../lib/updates";
import { useT } from "../lib/i18n";
import { SERVICE_STATE } from "./ServiceCard";
import { Icon } from "./Icon";
import { setAppearance } from "../lib/appearance";

/** The bar along the bottom: live state, each item opening where it is managed. */
export function StatusBar({
  shownServices,
  siteCount,
  logsOn,
  panel,
  panelTab,
  problemCount,
  onOpen,
}: {
  shownServices: ServicesHook["services"];
  siteCount: number;
  logsOn: boolean;
  panel: boolean;
  panelTab: PanelTab;
  problemCount: number;
  onOpen: (s: TabScreen) => void;
}) {
  const t = useT();
  const { status: update } = useUpdate();
  const running = shownServices.filter((s) => s.state === "running").length;
  const crashed = shownServices.some((s) => s.state === "crashed");
  const tone = crashed ? "error" : running > 0 ? "ok" : "";

  // Screen readers hear when a service starts, stops or crashes.
  const seen = useRef<Record<string, string> | null>(null);
  const [said, setSaid] = useState("");
  useEffect(() => {
    const now = Object.fromEntries(shownServices.map((s) => [s.id, s.state]));
    const before = seen.current;
    seen.current = now;
    if (!before) return;
    const changed = shownServices.filter((s) => before[s.id] && before[s.id] !== s.state);
    if (changed.length) setSaid(changed.map((s) => `${s.name}: ${t(SERVICE_STATE[s.state])}`).join(", "));
  }, [shownServices, t]);

  return (
    <footer className="statusbar" aria-label={t("Status bar")} data-part="statusbar">
      <span className="sr-only" role="status">
        {said}
      </span>
      <button type="button" className="status-item" onClick={() => onOpen("Services")}>
        <span className={`dot ${tone}`} aria-hidden="true" />
        {t("{running} of {total} services running", { running, total: shownServices.length })}
      </button>
      <button type="button" className="status-item" onClick={() => onOpen("Sites")}>
        {siteCount === 1 ? t("1 site") : t("{count} sites", { count: siteCount })}
      </button>
      {problemCount > 0 && (
        <button
          type="button"
          className="status-item warn"
          aria-pressed={panel && panelTab === "problems"}
          onClick={() => togglePanelTab("problems")}
        >
          <span aria-hidden="true">⚠</span> {problemCount === 1 ? t("1 problem") : t("{count} problems", { count: problemCount })}
        </button>
      )}
      <div className="status-spacer" />
      {update?.available && (
        <button type="button" className="status-item highlight" onClick={() => onOpen("Settings")}>
          {t("Stackyard {version} is available", { version: update.available.version })}
        </button>
      )}
      {logsOn && (
        <button
          type="button"
          className="status-item"
          aria-pressed={panel && panelTab === "logs"}
          onClick={() => togglePanelTab("logs")}
        >
          <Icon name="logs" />
          {t("Logs")}
        </button>
      )}
      <ThemeSwitch />
    </footer>
  );
}

/** Switches between the light and dark themes; from any other theme it goes to the opposite kind. */
function ThemeSwitch() {
  const t = useT();
  const [light, setLight] = useState(() => document.documentElement.dataset.theme === "light");
  useEffect(() => {
    const root = document.documentElement;
    const watch = new MutationObserver(() => setLight(root.dataset.theme === "light"));
    watch.observe(root, { attributes: true, attributeFilter: ["data-theme"] });
    return () => watch.disconnect();
  }, []);
  const label = light ? t("Switch to the dark theme") : t("Switch to the light theme");
  return (
    <button
      type="button"
      className="status-item"
      aria-label={label}
      title={label}
      onClick={() => setAppearance({ theme: light ? "dark" : "light" })}
    >
      <Icon name={light ? "moon" : "sun"} />
    </button>
  );
}
