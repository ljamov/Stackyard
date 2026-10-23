// The workbench layout: which parts show, their sizes, and the open tabs.
// Kept in the window's own storage so the app opens the way it was left.

import { useEffect, useState } from "react";
import { isTabScreen, type TabScreen } from "./screens";

/**
 * What a tab shows: a screen, one service ("service:mysql") or one site
 * ("site:blog.stack"), each of which can be open in a tab of its own.
 */
export type TabId = TabScreen | `service:${string}` | `site:${string}`;

export const serviceTab = (id: string): TabId => `service:${id}`;
export const siteTab = (domain: string): TabId => `site:${domain}`;

export const isTabId = (x: unknown): x is TabId =>
  isTabScreen(x) || (typeof x === "string" && /^(service|site):./.test(x));

export type Layout = {
  /** The side bar with screen names; false narrows it to a rail of icons. */
  sideBar: boolean;
  sideBarWidth: number;
  panel: boolean;
  panelHeight: number;
  /** What the bottom panel shows. */
  panelTab: PanelTab;
  tabs: TabId[];
  active: TabId | null;
};

export type PanelTab = "problems" | "logs" | "terminal";

export const SIDE_BAR_WIDTH = { min: 170, max: 480 };
export const PANEL_HEIGHT = { min: 120, max: 720 };

const KEY = "stackyard.layout";
const DEFAULT: Layout = {
  sideBar: true,
  sideBarWidth: 240,
  panel: false,
  panelHeight: 260,
  panelTab: "logs",
  tabs: ["Dashboard"],
  active: "Dashboard",
};

const clamp = (n: unknown, { min, max }: { min: number; max: number }, fallback: number) =>
  typeof n === "number" && Number.isFinite(n) ? Math.min(max, Math.max(min, Math.round(n))) : fallback;

function loadLayout(): Layout {
  try {
    const saved = JSON.parse(localStorage.getItem(KEY) ?? "{}") as Partial<Record<keyof Layout, unknown>>;
    const tabs = Array.isArray(saved.tabs) ? [...new Set(saved.tabs.filter(isTabId))] : DEFAULT.tabs;
    return {
      sideBar: typeof saved.sideBar === "boolean" ? saved.sideBar : DEFAULT.sideBar,
      sideBarWidth: clamp(saved.sideBarWidth, SIDE_BAR_WIDTH, DEFAULT.sideBarWidth),
      panel: typeof saved.panel === "boolean" ? saved.panel : DEFAULT.panel,
      panelHeight: clamp(saved.panelHeight, PANEL_HEIGHT, DEFAULT.panelHeight),
      panelTab: saved.panelTab === "problems" || saved.panelTab === "terminal" ? saved.panelTab : DEFAULT.panelTab,
      tabs,
      active: isTabId(saved.active) && tabs.includes(saved.active) ? saved.active : (tabs[0] ?? null),
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

/** Shows the panel on a tab; showing the tab it already shows hides the panel. */
export function togglePanelTab(tab: PanelTab) {
  setLayout(({ panel, panelTab }) => (panel && panelTab === tab ? { panel: false } : { panel: true, panelTab: tab }));
}

/** Shows a screen, service or site, opening its tab after the current one if it isn't open. */
export function openTab(screen: TabId) {
  setLayout(({ tabs, active }) => {
    if (tabs.includes(screen)) return { active: screen };
    const at = active ? tabs.indexOf(active) + 1 : tabs.length;
    return { tabs: [...tabs.slice(0, at), screen, ...tabs.slice(at)], active: screen };
  });
}

/**
 * Shows a screen in place of the shown one, the way the side bar opens them:
 * a screen that already has a tab is just shown. openTab() keeps the shown one
 * open beside it instead.
 */
export function showScreen(screen: TabId) {
  setLayout(({ tabs, active }) => {
    if (tabs.includes(screen)) return { active: screen };
    if (!active || !tabs.includes(active)) return { tabs: [...tabs, screen], active: screen };
    return { tabs: tabs.map((s) => (s === active ? screen : s)), active: screen };
  });
}

/** Closes a tab; the one beside it is shown if it was the shown one. */
export function closeTab(screen: TabId) {
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
