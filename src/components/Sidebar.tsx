import { Logo } from "./Logo";

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

export function Sidebar({ current, onSelect }: { current: Screen; onSelect: (s: Screen) => void }) {
  return (
    <nav className="sidebar" aria-label="Main">
      <div className="brand">
        <Logo />
        <span>Stackyard</span>
      </div>
      {SCREENS.map((screen) => (
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
