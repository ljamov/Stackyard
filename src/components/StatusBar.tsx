import type { ServicesHook } from "../lib/useServices";
import type { TabScreen } from "../lib/screens";
import { useUpdate } from "../lib/updates";
import { useT } from "../lib/i18n";

/** The bar along the bottom: live state, each item opening where it is managed. */
export function StatusBar({
  shownServices,
  siteCount,
  logsOn,
  panel,
  onOpen,
  onTogglePanel,
}: {
  shownServices: ServicesHook["services"];
  siteCount: number;
  logsOn: boolean;
  panel: boolean;
  onOpen: (s: TabScreen) => void;
  onTogglePanel: () => void;
}) {
  const t = useT();
  const { status: update } = useUpdate();
  const running = shownServices.filter((s) => s.state === "running").length;
  const crashed = shownServices.some((s) => s.state === "crashed");
  const tone = crashed ? "error" : running > 0 ? "ok" : "";

  return (
    <footer className="statusbar" aria-label={t("Status bar")}>
      <button type="button" className="status-item" onClick={() => onOpen("Services")}>
        <span className={`dot ${tone}`} aria-hidden="true" />
        {t("{running} of {total} services running", { running, total: shownServices.length })}
      </button>
      <button type="button" className="status-item" onClick={() => onOpen("Sites")}>
        {siteCount === 1 ? t("1 site") : t("{count} sites", { count: siteCount })}
      </button>
      <div className="status-spacer" />
      {update?.available && (
        <button type="button" className="status-item highlight" onClick={() => onOpen("Settings")}>
          {t("Stackyard {version} is available", { version: update.available.version })}
        </button>
      )}
      {logsOn && (
        <button type="button" className="status-item" aria-pressed={panel} onClick={onTogglePanel}>
          {t("Logs")}
        </button>
      )}
    </footer>
  );
}
