// How the window is laid out and sized: the icon set, how dense lists and
// buttons are, the zoom, which side the side bar and the panel sit on, and
// whether the status bar shows.
// Saved in settings.toml (see useUserFiles.ts) and also kept in the window's
// own storage, read before the first paint.

import { useEffect, useState } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { isTauri } from "@tauri-apps/api/core";

export type IconTheme = "stackyard" | "codicons";
export type Density = "compact" | "default" | "comfortable";
export type SideBarPosition = "left" | "right";
export type PanelPosition = "bottom" | "right";

export type ViewOptions = {
  iconTheme: IconTheme;
  density: Density;
  /** Percent, from ZOOM.min to ZOOM.max in ZOOM.step steps. */
  zoom: number;
  sideBarPosition: SideBarPosition;
  panelPosition: PanelPosition;
  /** The bar along the bottom of the window. */
  statusBar: boolean;
};

export const ICON_THEMES: { id: IconTheme; label: string }[] = [
  { id: "stackyard", label: "Stackyard" },
  { id: "codicons", label: "Codicons" },
];

export const DENSITIES: { id: Density; label: string }[] = [
  { id: "compact", label: "Compact" },
  { id: "default", label: "Default" },
  { id: "comfortable", label: "Comfortable" },
];

export const SIDE_BAR_POSITIONS: { id: SideBarPosition; label: string }[] = [
  { id: "left", label: "Left" },
  { id: "right", label: "Right" },
];

export const PANEL_POSITIONS: { id: PanelPosition; label: string }[] = [
  { id: "bottom", label: "Bottom" },
  { id: "right", label: "Right" },
];

export const ZOOM = { min: 80, max: 150, step: 10 };
export const ZOOM_LEVELS = Array.from({ length: (ZOOM.max - ZOOM.min) / ZOOM.step + 1 }, (_, i) => ZOOM.min + i * ZOOM.step);

export const DEFAULT_VIEW: ViewOptions = {
  iconTheme: "stackyard",
  density: "default",
  zoom: 100,
  sideBarPosition: "left",
  panelPosition: "bottom",
  statusBar: true,
};

const KEY = "stackyard.view";

const oneOf = <T extends string>(list: { id: T }[], value: unknown, fallback: T): T =>
  list.some((x) => x.id === value) ? (value as T) : fallback;

export function clampZoom(value: unknown): number | null {
  if (typeof value !== "number" || !Number.isFinite(value)) return null;
  const stepped = Math.round(value / ZOOM.step) * ZOOM.step;
  return Math.min(ZOOM.max, Math.max(ZOOM.min, stepped));
}

function load(): ViewOptions {
  try {
    const saved = JSON.parse(localStorage.getItem(KEY) ?? "{}") as Partial<Record<keyof ViewOptions, unknown>>;
    return {
      iconTheme: oneOf(ICON_THEMES, saved.iconTheme, DEFAULT_VIEW.iconTheme),
      density: oneOf(DENSITIES, saved.density, DEFAULT_VIEW.density),
      zoom: clampZoom(saved.zoom) ?? DEFAULT_VIEW.zoom,
      sideBarPosition: oneOf(SIDE_BAR_POSITIONS, saved.sideBarPosition, DEFAULT_VIEW.sideBarPosition),
      panelPosition: oneOf(PANEL_POSITIONS, saved.panelPosition, DEFAULT_VIEW.panelPosition),
      statusBar: typeof saved.statusBar === "boolean" ? saved.statusBar : DEFAULT_VIEW.statusBar,
    };
  } catch {
    return DEFAULT_VIEW;
  }
}

let current = load();
const listeners = new Set<(v: ViewOptions) => void>();
/** Told of every change and whether it came from the settings file. */
const changeListeners = new Set<(v: ViewOptions, fromFile: boolean) => void>();

let codicons: Promise<unknown> | null = null;

function apply(v: ViewOptions) {
  const root = document.documentElement;
  root.dataset.density = v.density;
  root.dataset.icons = v.iconTheme;
  // The icon font is only loaded once someone picks it.
  if (v.iconTheme === "codicons" && !codicons) codicons = import("@vscode/codicons/dist/codicon.css");
  if (isTauri()) {
    void getCurrentWebview()
      .setZoom(v.zoom / 100)
      .catch(() => {});
  } else {
    root.style.zoom = v.zoom === 100 ? "" : String(v.zoom / 100);
  }
}

export function startViewOptions() {
  apply(current);
}

export function currentViewOptions(): ViewOptions {
  return current;
}

export function onViewOptionsChange(listener: (v: ViewOptions, fromFile: boolean) => void): () => void {
  changeListeners.add(listener);
  return () => void changeListeners.delete(listener);
}

/** `fromFile`: the change was read from settings.toml, so it isn't written back. */
export function setViewOptions(change: Partial<ViewOptions>, fromFile = false) {
  const next = { ...current, ...change };
  if (JSON.stringify(next) === JSON.stringify(current)) return;
  current = next;
  try {
    localStorage.setItem(KEY, JSON.stringify(current));
  } catch {
    // Not saved; it still applies until the app closes.
  }
  apply(current);
  listeners.forEach((l) => l(current));
  changeListeners.forEach((l) => l(current, fromFile));
}

/** One zoom step in (1) or out (-1). */
export function zoomBy(step: 1 | -1) {
  setViewOptions({ zoom: clampZoom(current.zoom + step * ZOOM.step) ?? DEFAULT_VIEW.zoom });
}

export function useViewOptions(): ViewOptions {
  const [value, setValue] = useState(current);
  useEffect(() => {
    listeners.add(setValue);
    return () => void listeners.delete(setValue);
  }, []);
  return value;
}
