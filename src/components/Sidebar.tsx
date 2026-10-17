import { Logo } from "./Logo";
import { useModules } from "../lib/modules";
import { useUpdate } from "../lib/updates";

export const SCREENS = [
  "Dashboard",
  "Sites",
  "Services",
  "Runtimes",
  "Logs",
  "Databases",
  "Scheduler",
  "Images",
  "Videos",
  "AI tools",
  "MCP server",
  "Settings",
] as const;

export type Screen = (typeof SCREENS)[number];

/** Screens that belong to a module and disappear when it is switched off. */
const SCREEN_MODULE: Partial<Record<Screen, string>> = {
  Logs: "logs",
  Databases: "databases",
  Scheduler: "scheduler",
  Images: "images",
  Videos: "videos",
  "AI tools": "ai",
  "MCP server": "mcp",
};

export function Sidebar({ current, onSelect }: { current: Screen; onSelect: (s: Screen) => void }) {
  const { isOn } = useModules();
  const { status: update } = useUpdate();
  const visible = SCREENS.filter((s) => !SCREEN_MODULE[s] || isOn(SCREEN_MODULE[s]!));
  return (
    <nav className="sidebar" aria-label="Main">
      <div className="brand">
        <Logo />
        <span>Stackyard</span>
      </div>
      {visible.map((screen) => (
        <button
          key={screen}
          type="button"
          className="nav-item"
          aria-current={screen === current ? "page" : undefined}
          onClick={() => onSelect(screen)}
        >
          {screen}
          {screen === "Settings" && update?.available && (
            <span className="badge" title={`Stackyard ${update.available.version} is available`}>
              Update
            </span>
          )}
        </button>
      ))}
    </nav>
  );
}
