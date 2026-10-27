// Life of a screen in its tab. Every open tab stays mounted while another one
// is shown, so work started in it (a sync check, a pull, a backup) goes on and
// its progress and result are still there on coming back. This tells a screen
// whether it is shown, marks a tab busy while something runs in it, and keeps
// which inner tab was picked last.

import { createContext, useContext, useEffect, useMemo, useState, type ReactNode } from "react";
import type { TabId } from "./layout";

/** The tab a screen is in and whether it is the shown one; no tab counts as shown. */
export const TabContext = createContext<{ tab: TabId | null; shown: boolean }>({ tab: null, shown: true });

/** True while this screen's tab is the shown one, so timers can rest behind other tabs. */
export function useShown() {
  return useContext(TabContext).shown;
}

const busy = new Map<TabId, number>();

/** Something is running in this tab: the side bar opens screens beside it instead of in its place. */
export function isTabBusy(tab: TabId) {
  return (busy.get(tab) ?? 0) > 0;
}

/** Marks this screen's tab busy while `running` is true. */
export function useBusy(running: boolean) {
  const { tab } = useContext(TabContext);
  useEffect(() => {
    if (!running || !tab) return;
    busy.set(tab, (busy.get(tab) ?? 0) + 1);
    return () => {
      const left = (busy.get(tab) ?? 1) - 1;
      if (left > 0) busy.set(tab, left);
      else busy.delete(tab);
    };
  }, [running, tab]);
}

const KEY = "stackyard.innerTabs";
let picked: Record<string, string> = {};
try {
  const saved = JSON.parse(localStorage.getItem(KEY) ?? "{}");
  if (saved && typeof saved === "object") picked = saved;
} catch {
  // Nothing remembered yet.
}

/** The inner tab last picked under `key` (e.g. a site's sections), kept between runs. */
export function rememberedTab(key: string): string | undefined {
  return picked[key];
}

export function rememberTab(key: string, tab: string) {
  if (picked[key] === tab) return;
  picked = { ...picked, [key]: tab };
  try {
    localStorage.setItem(KEY, JSON.stringify(picked));
  } catch {
    // Remembered until the app closes.
  }
}

/** useState for an inner tab that comes back the way it was left. */
export function useRememberedTab(key: string, first: string) {
  const [tab, setTab] = useState(() => rememberedTab(key) ?? first);
  const pick = (x: string) => {
    rememberTab(key, x);
    setTab(x);
  };
  return [tab, pick] as const;
}

/**
 * An inner tab's content that stays once it has been shown, hidden while
 * another inner tab is picked, so what runs in it goes on.
 */
export function Keep({ shown, children }: { shown: boolean; children: ReactNode }) {
  const outer = useContext(TabContext);
  const [seen, setSeen] = useState(shown);
  if (shown && !seen) setSeen(true);
  const value = useMemo(() => ({ tab: outer.tab, shown: outer.shown && shown }), [outer.tab, outer.shown, shown]);
  if (!seen && !shown) return null;
  return (
    <div className="keep" hidden={!shown}>
      <TabContext.Provider value={value}>{children}</TabContext.Provider>
    </div>
  );
}
