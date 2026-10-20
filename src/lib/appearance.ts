// Light, dark or high-contrast look and the accent color. Saved in settings.toml
// (see useUserFiles.ts) and also kept in the window's own storage, which is read
// before the first paint so the app never flashes the wrong look. The colors
// themselves are in theme.ts.

import { useEffect, useState } from "react";
import { ACCENT_COLORS, applyTheme, type Accent, type ThemeId } from "./theme";

export type { Accent } from "./theme";
export type Theme = ThemeId | "system";

export const THEMES: { id: Theme; label: string }[] = [
  { id: "dark", label: "Dark" },
  { id: "light", label: "Light" },
  { id: "high-contrast", label: "High contrast" },
  { id: "system", label: "Match my computer" },
];

/** The swatch shown in Settings is the accent itself. */
export const ACCENTS: { id: Accent; label: string; swatch: string }[] = [
  { id: "amber", label: "Amber", swatch: ACCENT_COLORS.amber.accent },
  { id: "blue", label: "Blue", swatch: ACCENT_COLORS.blue.accent },
  { id: "green", label: "Green", swatch: ACCENT_COLORS.green.accent },
  { id: "violet", label: "Violet", swatch: ACCENT_COLORS.violet.accent },
  { id: "rose", label: "Rose", swatch: ACCENT_COLORS.rose.accent },
  { id: "teal", label: "Teal", swatch: ACCENT_COLORS.teal.accent },
];

export type Appearance = { theme: Theme; accent: Accent };

const KEY = "stackyard.appearance";
export const DEFAULT_APPEARANCE: Appearance = { theme: "dark", accent: "amber" };
const DEFAULT = DEFAULT_APPEARANCE;

export function loadAppearance(): Appearance {
  try {
    const saved = JSON.parse(localStorage.getItem(KEY) ?? "{}") as Partial<Appearance>;
    return {
      theme: THEMES.some((t) => t.id === saved.theme) ? saved.theme! : DEFAULT.theme,
      accent: ACCENTS.some((a) => a.id === saved.accent) ? saved.accent! : DEFAULT.accent,
    };
  } catch {
    return DEFAULT;
  }
}

const media = (query: string) => window.matchMedia?.(query);
const LIGHT = "(prefers-color-scheme: light)";
// Windows contrast themes report forced colors; macOS "Increase contrast" reports more contrast.
const CONTRAST = ["(prefers-contrast: more)", "(forced-colors: active)"];

/** The theme to show: the computer's when set to match it. */
export function resolveTheme(theme: Theme): ThemeId {
  if (theme !== "system") return theme;
  if (CONTRAST.some((q) => media(q)?.matches)) return "high-contrast";
  return media(LIGHT)?.matches ? "light" : "dark";
}

export function applyAppearance(a: Appearance) {
  applyTheme(resolveTheme(a.theme), a.accent);
}

let current = loadAppearance();
const listeners = new Set<(a: Appearance) => void>();
/** Told of every change and whether it came from the settings file. */
const changeListeners = new Set<(a: Appearance, fromFile: boolean) => void>();

export function onAppearanceChange(listener: (a: Appearance, fromFile: boolean) => void): () => void {
  changeListeners.add(listener);
  return () => void changeListeners.delete(listener);
}

export function currentAppearance(): Appearance {
  return current;
}

/** Applies the saved look and follows the computer's when set to match it. */
export function startAppearance() {
  applyAppearance(current);
  for (const q of [LIGHT, ...CONTRAST]) {
    media(q)?.addEventListener("change", () => applyAppearance(current));
  }
}

/** `fromFile`: the change was read from settings.toml, so it isn't written back. */
export function setAppearance(change: Partial<Appearance>, fromFile = false) {
  const before = current;
  current = { ...current, ...change };
  if (current.theme === before.theme && current.accent === before.accent) return;
  try {
    localStorage.setItem(KEY, JSON.stringify(current));
  } catch {
    // Not saved; it still applies until the app closes.
  }
  applyAppearance(current);
  listeners.forEach((l) => l(current));
  changeListeners.forEach((l) => l(current, fromFile));
}

export function useAppearance(): Appearance {
  const [value, setValue] = useState(current);
  useEffect(() => {
    listeners.add(setValue);
    return () => void listeners.delete(setValue);
  }, []);
  return value;
}
