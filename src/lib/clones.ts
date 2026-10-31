import { useSyncExternalStore } from "react";
import { errorText } from "./api";
import { wp, type WpCloned, type WpCloneOptions, type WpCloneProgress, type WpCloneSource } from "./wordpress";

/**
 * Every "Clone from a live server" in the window: forms still being filled
 * in and clones running or finished. Any number run at once; the store
 * outlives the Sites screen, so a minimized clone keeps going while other
 * screens are open.
 */
export type Clone = {
  id: string;
  /** `new` is a form not cloning yet; `paused` waits for Resume. */
  state: "new" | "running" | "paused" | "done" | "failed";
  /** Shown as a card on the Sites screen; false when minimized. */
  open: boolean;
  /** The local site's name, e.g. `shop`. */
  name: string;
  /** Where it comes from: the live address, the server, or the backup files. */
  from: string;
  source: WpCloneSource | null;
  options: WpCloneOptions | null;
  progress: WpCloneProgress | null;
  result: WpCloned | null;
  error: string | null;
  /** Backup files dropped on the window, which the form starts with. */
  dropped?: string[];
  /** Shown on the Import backup screen instead of Sites; it looks for WordPress in `dropped` at once. */
  place?: "import";
  /** When it was first started and when it finished, in ms, to say how long it took. */
  startedAt?: number;
  endedAt?: number;
};

let clones: Clone[] = [];
const listeners = new Set<() => void>();
let listening = false;
let count = 0;

function set(next: Clone[]) {
  clones = next;
  listeners.forEach((l) => l());
}

function change(id: string, fields: Partial<Clone>) {
  set(clones.map((c) => (c.id === id ? { ...c, ...fields } : c)));
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function useClones() {
  return useSyncExternalStore(subscribe, () => clones);
}

/** Progress events name their clone; one listener serves them all. */
function listen() {
  if (listening) return;
  listening = true;
  wp.onCloneProgress((p) => {
    if (clones.some((c) => c.id === p.job && c.state === "running")) change(p.job, { progress: p });
  }).catch(() => {
    listening = false;
  });
  // A clone this window took over from a closed one ends here.
  wp.onCloneEnded((e) => {
    if (!clones.some((c) => c.id === e.job && c.state === "running")) return;
    change(
      e.job,
      e.result ? { state: "done", result: e.result } : e.paused ? { state: "paused", error: null } : { state: "failed", error: e.error },
    );
  }).catch(() => {});
}

let restored = false;

/**
 * Brings back the clones from before the app last quit, minimized: one
 * that was running (the app updated or quit under it) starts again and
 * continues where it stopped; a paused one waits for Resume, a stopped or
 * failed one for Try again.
 */
export async function restoreClones() {
  if (restored) return;
  restored = true;
  let saved;
  try {
    saved = await wp.clonesSaved();
  } catch {
    return;
  }
  const known = new Set(clones.map((c) => c.id));
  const back: Clone[] = saved
    .filter((s) => !known.has(s.job))
    .map((s) => ({
      id: s.job,
      state: s.state === "running" ? "running" : s.state === "paused" ? "paused" : "failed",
      open: false,
      name: s.options.name,
      from: s.from,
      source: s.source,
      options: s.options,
      progress: null,
      result: null,
      error: s.state === "running" || s.state === "paused" ? null : s.error,
    }));
  if (!back.length) return;
  listen();
  set([...clones, ...back]);
  // Ones still running in the app just show again; the others start again.
  for (const s of saved) if (s.state === "running" && !s.active && !known.has(s.job)) void run(s.job);
}

/** Opens a new, empty clone form on the Sites screen, or with backup files on Import backup; returns its id. */
export function newClone(dropped?: string[], place?: "import") {
  const id = `clone-${Date.now().toString(36)}-${++count}`;
  set([...clones, { id, state: "new", open: true, name: "", from: "", source: null, options: null, progress: null, result: null, error: null, dropped, place }]);
  return id;
}

/** The screen a clone's card is shown on. */
export function cloneScreen(id: string) {
  return clones.find((c) => c.id === id)?.place === "import" ? "Import backup" : "Sites";
}

/** Files a site backup can be made of: a Stackyard Connect export (.tar),
 * a hosting panel backup, a zip of the files, a database export. */
export function isSiteBackup(path: string) {
  return /\.(tar|tar\.gz|tgz|zip|sql|sql\.gz)$/i.test(path);
}

async function run(id: string) {
  const clone = clones.find((c) => c.id === id);
  if (!clone?.source || !clone.options) return;
  listen();
  change(id, { state: "running", error: null, progress: null, result: null, startedAt: clone.startedAt ?? Date.now() });
  try {
    change(id, { state: "done", result: await wp.clone(id, clone.source, clone.options, clone.from), endedAt: Date.now() });
  } catch (e) {
    const error = errorText(e);
    change(id, error === "paused" ? { state: "paused", error: null } : { state: "failed", error });
  }
}

export function startClone(id: string, source: WpCloneSource, options: WpCloneOptions, from: string) {
  change(id, { source, options, from, name: options.name });
  return run(id);
}

/** Runs a paused, stopped or failed clone again; it continues where it stopped. */
export function retryClone(id: string) {
  return run(id);
}

export function stopClone(id: string) {
  return wp.cloneStop(id).catch(() => {});
}

/**
 * Pauses a running clone: its download stops, so the other clones get the
 * whole connection, and what it has is kept for Resume.
 */
export function pauseClone(id: string) {
  return wp.clonePause(id).catch(() => {});
}

export function minimizeClone(id: string) {
  change(id, { open: false });
}

export function showClone(id: string) {
  change(id, { open: true });
}

/** Closes a form or a finished clone; a running one can only be minimized, paused or stopped. */
export function closeClone(id: string) {
  if (clones.find((c) => c.id === id)?.state === "running") return;
  set(clones.filter((c) => c.id !== id));
  wp.cloneForget(id).catch(() => {});
}

/** How far along a clone is, 0-100, or null when there is no way to tell. */
export function clonePercent(p: WpCloneProgress | null) {
  return p && p.total > 0 ? Math.min(100, Math.floor((p.done * 100) / p.total)) : null;
}

/** Back to the form of a clone that stopped or failed, to change what it clones. */
export function editClone(id: string) {
  if (clones.find((c) => c.id === id)?.state !== "running") change(id, { state: "new", open: true, error: null });
}
