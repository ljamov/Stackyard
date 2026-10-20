// The workbench layout: which parts show, their sizes, and the open tabs.
// Kept in the window's own storage so the app opens the way it was left.

import { useEffect, useState } from "react";
import { isTabScreen, type TabScreen } from "./screens";

export type Layout = {
  sideBar: boolean;
  sideBarWidth: number;
  panel: boolean;
  panelHeight: number;
  tabs: TabScreen[];
  active: TabScreen | null;
};

export const SIDE_BAR_WIDTH = { min: 170, max: 480 };
export const PANEL_HEIGHT = { min: 120, max: 720 };

const KEY = "stackyard.layout";
const DEFAULT: Layout = {
  sideBar: true,
  sideBarWidth: 240,
  panel: false,
  panelHeight: 260,
  tabs: ["Dashboard"],
  active: "Dashboard",
};

const clamp = (n: unknown, { min, max }: { min: number; max: number }, fallback: number) =>
  typeof n === "number" && Number.isFinite(n) ? Math.min(max, Math.max(min, Math.round(n))) : fallback;

function loadLayout(): Layout {
  try {
    const saved = JSON.parse(localStorage.getItem(KEY) ?? "{}") as Partial<Record<keyof Layout, unknown>>;
    const tabs = Array.isArray(saved.tabs) ? [...new Set(saved.tabs.filter(isTabScreen))] : DEFAULT.tabs;
    return {
      sideBar: typeof saved.sideBar === "boolean" ? saved.sideBar : DEFAULT.sideBar,
      sideBarWidth: clamp(saved.sideBarWidth, SIDE_BAR_WIDTH, DEFAULT.sideBarWidth),
      panel: typeof saved.panel === "boolean" ? saved.panel : DEFAULT.panel,
      panelHeight: clamp(saved.panelHeight, PANEL_HEIGHT, DEFAULT.panelHeight),
      tabs,
      active: isTabScreen(saved.active) && tabs.includes(saved.active) ? saved.active : (tabs[0] ?? null),
    };
  } catch {
    return DEFAULT;
  }
}

let current = loadLayout();
const listeners = new Set<(l: Layout) => void>();

export function setLayout(change: Partial<Layout> | ((l: Layout) => Partial<Layout>)) {
  current = { ...current, ...(typeof change === "function" ? change(current) : change) };
  try {
    localStorage.setItem(KEY, JSON.stringify(current));
  } catch {
    // Not saved; it still applies until the app closes.
  }
  listeners.forEach((l) => l(current));
}

export function useLayout(): Layout {
  const [value, setValue] = useState(current);
  useEffect(() => {
    listeners.add(setValue);
    return () => void listeners.delete(setValue);
  }, []);
  return value;
}

/** Shows a screen, opening its tab after the current one if it isn't open. */
export function openTab(screen: TabScreen) {
  setLayout(({ tabs, active }) => {
    if (tabs.includes(screen)) return { active: screen };
    const at = active ? tabs.indexOf(active) + 1 : tabs.length;
    return { tabs: [...tabs.slice(0, at), screen, ...tabs.slice(at)], active: screen };
  });
}

/** Closes a tab; the one beside it is shown if it was the shown one. */
export function closeTab(screen: TabScreen) {
  setLayout(({ tabs, active }) => {
    const at = tabs.indexOf(screen);
    const rest = tabs.filter((s) => s !== screen);
    return { tabs: rest, active: active === screen ? (rest[Math.min(at, rest.length - 1)] ?? null) : active };
  });
}

/** Shows the next (1) or previous (-1) tab, wrapping around. */
export function cycleTab(step: 1 | -1) {
  setLayout(({ tabs, active }) => {
    if (tabs.length === 0) return {};
    const at = active ? tabs.indexOf(active) : -1;
    return { active: tabs[(at + step + tabs.length) % tabs.length] };
  });
}
