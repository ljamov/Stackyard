// Importing a VS Code color theme: from Settings › Appearance or the palette.
// What went wrong is kept here so the Appearance card can show it, wherever
// the import was started.

import { useEffect, useState } from "react";
import { api, errorText } from "./api";
import { currentAppearance, DEFAULT_APPEARANCE, setAddedThemes, setAppearance } from "./appearance";

export type ImportState = { busy: boolean; error: string | null; added: string[] };

let state: ImportState = { busy: false, error: null, added: [] };
const listeners = new Set<(s: ImportState) => void>();

function set(next: ImportState) {
  state = next;
  listeners.forEach((l) => l(state));
}

/** Asks for a file, copies its themes into the themes folder and shows the first one. */
export async function importThemeFromFile(title: string) {
  if (state.busy) return;
  let path: string | null;
  try {
    path = await api.pickThemeFile(title);
  } catch (e) {
    return set({ busy: false, error: errorText(e), added: [] });
  }
  if (!path) return;
  set({ busy: true, error: null, added: [] });
  try {
    const added = await api.importTheme(path);
    const files = await api.themeFiles();
    setAddedThemes(files.themes);
    if (added[0]) setAppearance({ theme: added[0] });
    set({ busy: false, error: null, added });
  } catch (e) {
    set({ busy: false, error: errorText(e), added: [] });
  }
}

export async function removeAddedTheme(id: string) {
  try {
    await api.removeTheme(id);
    if (currentAppearance().theme === id) setAppearance({ theme: DEFAULT_APPEARANCE.theme });
    const files = await api.themeFiles();
    setAddedThemes(files.themes);
    set({ ...state, error: null });
  } catch (e) {
    set({ ...state, error: errorText(e) });
  }
}

export function useThemeImport(): ImportState {
  const [value, setValue] = useState(state);
  useEffect(() => {
    listeners.add(setValue);
    return () => void listeners.delete(setValue);
  }, []);
  return value;
}
