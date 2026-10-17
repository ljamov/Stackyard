// Light or dark look and the accent color. Kept in the window's own storage
// and applied before the first paint, so the app never flashes the wrong look.

import { useEffect, useState } from "react";

export type Theme = "dark" | "light" | "system";
export type Accent = "amber" | "blue" | "green" | "violet" | "rose" | "teal";

export const THEMES: { id: Theme; label: string }[] = [
  { id: "dark", label: "Dark" },
  { id: "light", label: "Light" },
  { id: "system", label: "Match my computer" },
];

/** The swatch shown in Settings; the colors themselves live in styles.css. */
export const ACCENTS: { id: Accent; label: string; swatch: string }[] = [
  { id: "amber", label: "Amber", swatch: "#f2b035" },
  { id: "blue", label: "Blue", swatch: "#5aa9ff" },
  { id: "green", label: "Green", swatch: "#4fd18b" },
  { id: "violet", label: "Violet", swatch: "#b48cff" },
  { id: "rose", label: "Rose", swatch: "#ff7a9a" },
  { id: "teal", label: "Teal", swatch: "#3fd0c9" },
];

export type Appearance = { theme: Theme; accent: Accent };

const KEY = "stackyard.appearance";
const DEFAULT: Appearance = { theme: "dark", accent: "amber" };

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

const systemLight = () => window.matchMedia?.("(prefers-color-scheme: light)").matches ?? false;

/** Sets `data-theme` and `data-accent` on the page, which styles.css reads. */
export function applyAppearance(a: Appearance) {
  const root = document.documentElement;
  const light = a.theme === "light" || (a.theme === "system" && systemLight());
  root.dataset.theme = light ? "light" : "dark";
  root.dataset.accent = a.accent;
}

let current = loadAppearance();
const listeners = new Set<(a: Appearance) => void>();

/** Applies the saved look and follows the computer's when set to match it. */
export function startAppearance() {
  applyAppearance(current);
  window.matchMedia?.("(prefers-color-scheme: light)").addEventListener("change", () => applyAppearance(current));
}

export function setAppearance(change: Partial<Appearance>) {
  current = { ...current, ...change };
  try {
    localStorage.setItem(KEY, JSON.stringify(current));
  } catch {
    // Not saved; it still applies until the app closes.
  }
  applyAppearance(current);
  listeners.forEach((l) => l(current));
}

export function useAppearance(): Appearance {
  const [value, setValue] = useState(current);
  useEffect(() => {
    listeners.add(setValue);
    return () => void listeners.delete(setValue);
  }, []);
  return value;
}
