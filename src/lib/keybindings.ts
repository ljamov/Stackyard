// Which keys run which command. The defaults are below; a person's own changes
// are kept in the window's storage and replace a command's default keys.
//
// A key is written like "ctrl+shift+p": "ctrl" means Ctrl on Windows and Linux
// and ⌘ on a Mac. Two keys separated by a space are pressed one after the
// other, e.g. "ctrl+k ctrl+s".

import { useEffect, useState } from "react";

export const DEFAULT_KEYS: Record<string, string[]> = {
  "workbench.commands": ["ctrl+shift+p", "f1"],
  "workbench.quickOpen": ["ctrl+p"],
  "view.explorer": ["ctrl+b"],
  "view.panel": ["ctrl+j"],
  "tab.close": ["ctrl+w"],
  "tab.next": ["ctrl+tab", "ctrl+pagedown"],
  "tab.previous": ["ctrl+shift+tab", "ctrl+pageup"],
  "open.Settings": ["ctrl+,"],
  "open.Keyboard shortcuts": ["ctrl+k ctrl+s"],
};

const KEY = "stackyard.keybindings";
type Changes = Record<string, string[]>;

function loadChanges(): Changes {
  try {
    const saved = JSON.parse(localStorage.getItem(KEY) ?? "{}") as unknown;
    if (!saved || typeof saved !== "object" || Array.isArray(saved)) return {};
    return Object.fromEntries(
      Object.entries(saved).filter(
        (e): e is [string, string[]] => Array.isArray(e[1]) && e[1].every((k) => typeof k === "string"),
      ),
    );
  } catch {
    return {};
  }
}

let changes = loadChanges();
const listeners = new Set<(c: Changes) => void>();

/** A command's keys: the person's own when they changed them, else the defaults. */
export function keysFor(id: string, c: Changes = changes): string[] {
  return c[id] ?? DEFAULT_KEYS[id] ?? [];
}

export function isChanged(id: string, c: Changes = changes) {
  return id in c;
}

/** Sets a command's keys; `null` goes back to its defaults, `[]` leaves it without keys. */
export function setKeys(id: string, keys: string[] | null) {
  const { [id]: _, ...rest } = changes;
  changes = keys === null ? rest : { ...rest, [id]: keys };
  try {
    localStorage.setItem(KEY, JSON.stringify(changes));
  } catch {
    // Not saved; it still applies until the app closes.
  }
  listeners.forEach((l) => l(changes));
}

/** The person's changes; re-renders when they change. Pass it to keysFor(). */
export function useKeyChanges(): Changes {
  const [value, setValue] = useState(changes);
  useEffect(() => {
    listeners.add(setValue);
    return () => void listeners.delete(setValue);
  }, []);
  return value;
}

const CODES: Record<string, string> = {
  Comma: ",", Period: ".", Slash: "/", Backslash: "\\", Semicolon: ";", Quote: "'",
  BracketLeft: "[", BracketRight: "]", Minus: "-", Equal: "=", Backquote: "`",
};
const NAMES: Record<string, string> = {
  " ": "space", ArrowUp: "up", ArrowDown: "down", ArrowLeft: "left", ArrowRight: "right", Escape: "escape",
};

/** The key a keyboard event stands for, as written above; null for a modifier on its own. */
export function keyOf(e: KeyboardEvent): string | null {
  if (["Control", "Shift", "Alt", "Meta"].includes(e.key)) return null;
  let key: string;
  if (/^Key[A-Z]$/.test(e.code)) key = e.code.slice(3).toLowerCase();
  else if (/^Digit\d$/.test(e.code)) key = e.code.slice(5);
  else if (CODES[e.code]) key = CODES[e.code];
  else key = NAMES[e.key] ?? e.key.toLowerCase();
  const mods = [e.ctrlKey || e.metaKey ? "ctrl" : "", e.altKey ? "alt" : "", e.shiftKey ? "shift" : ""];
  return [...mods.filter(Boolean), key].join("+");
}

const SHOWN: Record<string, string> = { pagedown: "PageDown", pageup: "PageUp", escape: "Esc", space: "Space" };

const MAC = typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.platform || navigator.userAgent);

/** How a key is shown: "Ctrl+Shift+P", or "⌘⇧P" on a Mac. */
export function keyLabel(key: string): string {
  return key
    .split(" ")
    .map((part) =>
      part
        .split("+")
        .map((k) => {
          const mac: Record<string, string> = { ctrl: "⌘", shift: "⇧", alt: "⌥" };
          if (MAC && mac[k]) return mac[k];
          if (k === "ctrl" || k === "shift" || k === "alt") return k[0].toUpperCase() + k.slice(1);
          return SHOWN[k] ?? (k.length === 1 ? k.toUpperCase() : k[0].toUpperCase() + k.slice(1));
        })
        .join(MAC ? "" : "+"),
    )
    .join(" ");
}

/** A tooltip with the command's first key after it, e.g. "Explorer (Ctrl+B)". */
export function withKey(text: string, id: string): string {
  const key = keysFor(id)[0];
  return key ? `${text} (${keyLabel(key)})` : text;
}

/** True while the Keyboard shortcuts screen waits for a key, so keys reach it alone. */
export const recording = { on: false };

/**
 * Runs the command bound to each key press. A key that starts a two-key
 * shortcut waits up to 1.5 seconds for the second one.
 */
export function dispatchKeys(
  e: KeyboardEvent,
  commandIds: string[],
  run: (id: string) => void,
  state: { pending: string | null; timer?: ReturnType<typeof setTimeout> },
) {
  if (recording.on) return;
  const key = keyOf(e);
  if (!key) return;
  const typed = state.pending ? `${state.pending} ${key}` : key;
  clearTimeout(state.timer);
  state.pending = null;
  const bound = commandIds.map((id) => ({ id, keys: keysFor(id) }));
  const hit = bound.find((b) => b.keys.includes(typed));
  if (hit) {
    e.preventDefault();
    run(hit.id);
    return;
  }
  if (bound.some((b) => b.keys.some((k) => k.startsWith(typed + " ")))) {
    e.preventDefault();
    state.pending = typed;
    state.timer = setTimeout(() => (state.pending = null), 1500);
  }
}
