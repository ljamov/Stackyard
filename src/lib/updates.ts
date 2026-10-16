import { useEffect, useSyncExternalStore } from "react";
import { api, errorText, type UpdateProgress, type UpdateStatus } from "./api";

/** One copy of the update state, shared by the window's notice and Settings. */
type Store = { status: UpdateStatus | null; progress: UpdateProgress | null; error: string | null };

let store: Store = { status: null, progress: null, error: null };
const listeners = new Set<() => void>();
let started = false;

function set(change: Partial<Store>) {
  store = { ...store, ...change };
  listeners.forEach((l) => l());
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/** Starts listening once; Stackyard keeps the window's script for its whole life. */
function start() {
  if (started) return;
  started = true;
  api.updateStatus().then((status) => set({ status }), () => {});
  api.onUpdateAvailable((status) => set({ status })).catch(() => {});
  api.onUpdateProgress((progress) => set({ progress })).catch(() => {});
}

export async function checkForUpdate() {
  if (!store.status) return;
  set({ error: null, status: { ...store.status, checking: true } });
  set({ status: await api.checkForUpdate() });
}

export async function installUpdate() {
  if (!store.status) return;
  set({ error: null, status: { ...store.status, installing: true } });
  try {
    // Stackyard restarts into the new version when this succeeds.
    await api.installUpdate();
  } catch (e) {
    set({ error: errorText(e), progress: null, status: await api.updateStatus() });
  }
}

export async function seenWhatsNew() {
  set({ status: await api.seenWhatsNew() });
}

/** The app's update status, kept current when a background check finds one. */
export function useUpdate() {
  useEffect(start, []);
  return useSyncExternalStore(subscribe, () => store);
}

/** "Downloaded 40%" while an update downloads. */
export function progressText(progress: UpdateProgress | null) {
  if (!progress) return "Downloading…";
  return progress.total
    ? `Downloaded ${Math.round((progress.done / progress.total) * 100)}%`
    : `Downloaded ${(progress.done / 1048576).toFixed(1)} MB`;
}
