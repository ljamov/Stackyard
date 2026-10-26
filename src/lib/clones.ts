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
  /** `new` is a form not cloning yet. */
  state: "new" | "running" | "done" | "failed";
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
}

/** Opens a new, empty clone form on the Sites screen; returns its id. */
export function newClone() {
  const id = `clone-${Date.now().toString(36)}-${++count}`;
  set([...clones, { id, state: "new", open: true, name: "", from: "", source: null, options: null, progress: null, result: null, error: null }]);
  return id;
}

async function run(id: string) {
  const clone = clones.find((c) => c.id === id);
  if (!clone?.source || !clone.options) return;
  listen();
  change(id, { state: "running", error: null, progress: null, result: null });
  try {
    change(id, { state: "done", result: await wp.clone(id, clone.source, clone.options) });
  } catch (e) {
    change(id, { state: "failed", error: errorText(e) });
  }
}

export function startClone(id: string, source: WpCloneSource, options: WpCloneOptions, from: string) {
  change(id, { source, options, from, name: options.name });
  return run(id);
}

/** Runs a stopped or failed clone again; it continues where it stopped. */
export function retryClone(id: string) {
  return run(id);
}

export function stopClone(id: string) {
  return wp.cloneStop(id).catch(() => {});
}

export function minimizeClone(id: string) {
  change(id, { open: false });
}

export function showClone(id: string) {
  change(id, { open: true });
}

/** Closes a form or a finished clone; a running one can only be minimized or stopped. */
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
