// Whether the command palette is open, and in which mode. Anything can open it:
// the title bar's search box, a shortcut, another command.

import { useEffect, useState } from "react";

/** "commands" opens with ">" typed, listing commands; "quick" lists sites and screens. */
export type PaletteMode = "commands" | "quick" | null;

let mode: PaletteMode = null;
const listeners = new Set<(m: PaletteMode) => void>();

export function openPalette(m: Exclude<PaletteMode, null>) {
  mode = m;
  listeners.forEach((l) => l(mode));
}

export function closePalette() {
  mode = null;
  listeners.forEach((l) => l(mode));
}

export function usePalette(): PaletteMode {
  const [value, setValue] = useState(mode);
  useEffect(() => {
    listeners.add(setValue);
    return () => void listeners.delete(setValue);
  }, []);
  return value;
}

/** Commands run most recently first, so the palette offers them on top. */
const RECENT = "stackyard.recent-commands";

export function recentCommands(): string[] {
  try {
    const saved = JSON.parse(localStorage.getItem(RECENT) ?? "[]") as unknown;
    return Array.isArray(saved) ? saved.filter((s): s is string => typeof s === "string") : [];
  } catch {
    return [];
  }
}

export function rememberCommand(id: string) {
  try {
    localStorage.setItem(RECENT, JSON.stringify([id, ...recentCommands().filter((r) => r !== id)].slice(0, 5)));
  } catch {
    // Not remembered; nothing else depends on it.
  }
}

/**
 * How well `query` matches `text`: its letters in order, anywhere in the text.
 * Higher is better; null when it doesn't match. Letters in a row and at the
 * start of a word count for more, so "ss" finds "Start services" first.
 */
export function matchScore(query: string, text: string): number | null {
  const q = query.toLowerCase().replace(/\s+/g, "");
  if (!q) return 0;
  const s = text.toLowerCase();
  let score = 0;
  let at = -1;
  for (const ch of q) {
    const next = s.indexOf(ch, at + 1);
    if (next < 0) return null;
    if (next === at + 1) score += 3;
    if (next === 0 || /[\s:./-]/.test(s[next - 1])) score += 5;
    score -= Math.min(next - at - 1, 10) * 0.1;
    at = next;
  }
  return score - s.length * 0.01;
}
