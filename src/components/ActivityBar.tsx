import { Icon, type IconName } from "./Icon";
import { useUpdate } from "../lib/updates";
import { useT } from "../lib/i18n";
import { withKey } from "../lib/keybindings";
import type { Screen, TabScreen } from "../lib/screens";

export const SCREEN_ICON: Record<Screen, IconName> = {
  Dashboard: "dashboard",
  Sites: "sites",
  Services: "services",
  Runtimes: "runtimes",
  Logs: "logs",
  Databases: "databases",
  Scheduler: "scheduler",
  Images: "images",
  Videos: "videos",
  "AI tools": "ai",
  "MCP server": "mcp",
  Settings: "settings",
  "Keyboard shortcuts": "keyboard",
};

/**
 * The column of icons on the far left: Explorer shows or hides the side bar,
 * each screen opens as a tab, and Logs shows or hides the bottom panel.
 */
export function ActivityBar({
  screens,
  active,
  sideBar,
  panel,
  logsOn,
  onOpen,
  onToggleSideBar,
  onTogglePanel,
}: {
  screens: TabScreen[];
  active: TabScreen | null;
  sideBar: boolean;
  panel: boolean;
  logsOn: boolean;
  onOpen: (s: TabScreen) => void;
  onToggleSideBar: () => void;
  onTogglePanel: () => void;
}) {
  const t = useT();
  const { status: update } = useUpdate();
  const item = (s: TabScreen) => (
    <button
      key={s}
      type="button"
      className="activity-item"
      data-screen={s}
      aria-label={t(s)}
      title={withKey(t(s), `open.${s}`)}
      aria-current={s === active ? "page" : undefined}
      onClick={() => onOpen(s)}
    >
      <Icon name={SCREEN_ICON[s]} />
      {s === "Settings" && update?.available && (
        <span
          className="activity-dot"
          title={t("Stackyard {version} is available", { version: update.available.version })}
        />
      )}
    </button>
  );

  return (
    <nav className="activitybar" aria-label={t("Main")}>
      <button
        type="button"
        className="activity-item"
        aria-label={t("Explorer")}
        title={withKey(t("Explorer"), "view.explorer")}
        aria-pressed={sideBar}
        onClick={onToggleSideBar}
      >
        <Icon name="explorer" />
      </button>
      {screens.filter((s) => s !== "Settings").map(item)}
      <div className="activity-spacer" />
      {logsOn && (
        <button
          type="button"
          className="activity-item"
          data-screen="Logs"
          aria-label={t("Logs")}
          title={withKey(t("Logs"), "view.panel")}
          aria-pressed={panel}
          onClick={onTogglePanel}
        >
          <Icon name="logs" />
        </button>
      )}
      {screens.includes("Settings") && item("Settings")}
    </nav>
  );
}
