import { Logo } from "./Logo";
import { useModules } from "../lib/modules";

export const SCREENS = [
  "Dashboard",
  "Sites",
  "Services",
  "Runtimes",
  "Databases",
  "AI tools",
  "MCP server",
  "Settings",
] as const;

export type Screen = (typeof SCREENS)[number];

/** Screens that belong to a module and disappear when it is switched off. */
const SCREEN_MODULE: Partial<Record<Screen, string>> = {
  Databases: "databases",
  "AI tools": "mcp",
  "MCP server": "mcp",
};

export function Sidebar({ current, onSelect }: { current: Screen; onSelect: (s: Screen) => void }) {
  const { isOn } = useModules();
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
        </button>
      ))}
    </nav>
  );
}
