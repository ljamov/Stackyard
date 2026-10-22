// Light, dark or high-contrast look, or a theme added from a file, and the
// accent color. Saved in settings.toml (see useUserFiles.ts) and also kept in
// the window's own storage, which is read before the first paint so the app
// never flashes the wrong look. The colors themselves are in theme.ts.

import { useEffect, useState } from "react";
import {
  ACCENT_COLORS,
  addedThemeColors,
  applyColors,
  applyTheme,
  isAccent,
  type Accent,
  type AccentId,
  type AddedTheme,
  type ThemeColors,
  type ThemeId,
} from "./theme";

export type { Accent, AddedTheme } from "./theme";
/** A built-in theme, "system", or the id of a theme added from a file. */
export type Theme = ThemeId | "system" | (string & {});

export const THEMES: { id: Theme; label: string }[] = [
  { id: "dark", label: "Dark" },
  { id: "light", label: "Light" },
  { id: "high-contrast", label: "High contrast" },
  { id: "system", label: "Match my computer" },
];

/** The swatch shown in Settings is the accent itself. */
export const ACCENTS: { id: AccentId; label: string; swatch: string }[] = [
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

export function isBuiltInTheme(theme: string): theme is ThemeId | "system" {
  return THEMES.some((t) => t.id === theme);
}

export function loadAppearance(): Appearance {
  try {
    const saved = JSON.parse(localStorage.getItem(KEY) ?? "{}") as Partial<Appearance>;
    return {
      // An added theme's id is checked once the themes folder has been read.
      theme: typeof saved.theme === "string" && saved.theme ? saved.theme : DEFAULT.theme,
      accent: isAccent(saved.accent) ? saved.accent : DEFAULT.accent,
    };
  } catch {
    return DEFAULT;
  }
}

const media = (query: string) => window.matchMedia?.(query);
const LIGHT = "(prefers-color-scheme: light)";
// Windows contrast themes report forced colors; macOS "Increase contrast" reports more contrast.
const CONTRAST = ["(prefers-contrast: more)", "(forced-colors: active)"];

/** The built-in theme to show: the computer's when set to match it. */
export function resolveTheme(theme: Theme): ThemeId {
  if (theme === "system") {
    if (CONTRAST.some((q) => media(q)?.matches)) return "high-contrast";
    return media(LIGHT)?.matches ? "light" : "dark";
  }
  return isBuiltInTheme(theme) && theme !== "system" ? theme : "dark";
}

// Themes added from files (the themes folder), and the colors of the last one
// shown, kept so it paints right away on the next start before the folder is read.
let added: AddedTheme[] = [];
const addedListeners = new Set<(t: AddedTheme[]) => void>();
const COLORS_KEY = "stackyard.theme-colors";
type Painted = { id: string; kind: ThemeId; colors: ThemeColors };

function lastPainted(): Painted | null {
  try {
    const saved = JSON.parse(localStorage.getItem(COLORS_KEY) ?? "null") as Painted | null;
    return saved && typeof saved.id === "string" && saved.colors ? saved : null;
  } catch {
    return null;
  }
}

function paint(theme: Theme, accent: Accent) {
  if (isBuiltInTheme(theme)) return applyTheme(resolveTheme(theme), accent);
  const t = added.find((x) => x.id === theme);
  if (t) {
    const colors = addedThemeColors(t, accent);
    applyColors(colors, t.kind, accent);
    try {
      localStorage.setItem(COLORS_KEY, JSON.stringify({ id: t.id, kind: t.kind, colors } satisfies Painted));
    } catch {
      // Only the first paint of the next start is slower to match.
    }
    return;
  }
  const saved = lastPainted();
  if (saved?.id === theme) return applyColors(saved.colors, saved.kind, accent);
  applyTheme("dark", accent);
}

export function applyAppearance(a: Appearance) {
  paint(a.theme, a.accent);
}

/** The themes read from the themes folder; the shown one is painted again. */
export function setAddedThemes(themes: AddedTheme[]) {
  if (JSON.stringify(themes) === JSON.stringify(added)) return;
  added = themes;
  addedListeners.forEach((l) => l(added));
  applyAppearance(current);
}

export function addedThemes(): AddedTheme[] {
  return added;
}

export function useAddedThemes(): AddedTheme[] {
  const [value, setValue] = useState(added);
  useEffect(() => {
    addedListeners.add(setValue);
    return () => void addedListeners.delete(setValue);
  }, []);
  return value;
}

/** Shows a theme without choosing it, while the theme picker moves over it. */
export function previewTheme(theme: Theme) {
  paint(theme, current.accent);
}

/** Back to the chosen theme after a preview. */
export function endPreview() {
  applyAppearance(current);
}

/** The name to show for a theme: a built-in one's (to translate) or an added one's. */
export function themeName(theme: Theme): { label: string; added: boolean } {
  const builtIn = THEMES.find((t) => t.id === theme);
  if (builtIn) return { label: builtIn.label, added: false };
  return { label: added.find((t) => t.id === theme)?.name ?? theme, added: true };
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
