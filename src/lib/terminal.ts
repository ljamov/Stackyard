// The terminal: as many shells as wanted, each with its own tab in the
// terminal's strip, kept running while the panel is closed or shows another
// tab. The terminal sits in the panel, or in a tab of the main area, and one
// shell can be popped out into a window of its own. xterm.js is loaded the
// first time the terminal shows, so the window starts no slower for it.
//
// The shells live in the app, not the window: a window opened again picks
// up the ones still running, with what they printed.

import { useEffect, useState } from "react";
import type { Terminal } from "@xterm/xterm";
import type { FitAddon } from "@xterm/addon-fit";
import type { SearchAddon } from "@xterm/addon-search";
import { api, errorText, type BrokenTool, type ShellChoice } from "./api";
import { setLayout } from "./layout";
import { keyOf, keysFor } from "./keybindings";
import { onPaint, paintedColors, terminalTheme } from "./theme";

/** Commands that still work while typing in the terminal; every other key goes to the shell. */
export const TERMINAL_COMMANDS = [
  "workbench.commands",
  "view.terminal",
  "view.panel",
  "workbench.focusNextPart",
  "workbench.focusPreviousPart",
  "view.zoomIn",
  "view.zoomOut",
  "view.zoomReset",
];

/** Lines kept above the screen, to scroll back to. */
export const SCROLLBACK = 10_000;
const FONT_KEY = "stackyard.terminal.fontSize";
export const FONT_SIZE = { min: 9, max: 28, default: 13 };

/** One shell, as the window shows it. */
export type TerminalSession = {
  /** Stays the same when the shell restarts. */
  key: number;
  /** The app's number for the running shell; null once it ended. */
  id: number | null;
  /** The folder the shell started in; null for the home folder. */
  folder: string | null;
  /** What the folder is called: a site's address, or null for the home folder. */
  label: string | null;
  /** Which shell, from api.consoleShells(); null for the default. */
  shell: string | null;
  running: boolean;
  error: string | null;
  /** AI tools installed with npm that no longer start; installing them again fixes them. */
  broken: BrokenTool[];
  /** Shown in a window of its own. */
  poppedOut: boolean;
  /** Scrolled up from the newest line. */
  scrolledUp: boolean;
};

export type TerminalState = {
  sessions: TerminalSession[];
  active: number | null;
  shells: ShellChoice[];
  fontSize: number;
};

/** A shell's terminal on screen, with the add-ons it needs. */
export type Screen = {
  term: Terminal;
  fit: FitAddon;
  search: SearchAddon;
  host: HTMLDivElement;
};

type Live = TerminalSession & { screen: Screen };

let sessions: Live[] = [];
let active: number | null = null;
let shells: ShellChoice[] = [];
let fontSize = loadFontSize();
let nextKey = 1;
let state: TerminalState = { sessions: [], active: null, shells: [], fontSize };
const listeners = new Set<(s: TerminalState) => void>();

function loadFontSize(): number {
  try {
    const n = Number(localStorage.getItem(FONT_KEY));
    return n >= FONT_SIZE.min && n <= FONT_SIZE.max ? n : FONT_SIZE.default;
  } catch {
    return FONT_SIZE.default;
  }
}

function publish() {
  state = {
    sessions: sessions.map(({ screen: _, ...s }) => s),
    active,
    shells,
    fontSize,
  };
  listeners.forEach((l) => l(state));
}

function update(key: number, change: Partial<TerminalSession>) {
  sessions = sessions.map((s) => (s.key === key ? { ...s, ...change } : s));
  publish();
}

const byKey = (key: number | null) => sessions.find((s) => s.key === key);
const byId = (id: number) => sessions.find((s) => s.id === id);

type Modules = {
  Terminal: typeof import("@xterm/xterm").Terminal;
  FitAddon: typeof import("@xterm/addon-fit").FitAddon;
  SearchAddon: typeof import("@xterm/addon-search").SearchAddon;
  WebLinksAddon: typeof import("@xterm/addon-web-links").WebLinksAddon;
  Unicode11Addon: typeof import("@xterm/addon-unicode11").Unicode11Addon;
};

let modules: Promise<Modules> | null = null;

/** xterm.js and its add-ons, loaded once. */
export function loadXterm(): Promise<Modules> {
  modules ??= Promise.all([
    import("@xterm/xterm"),
    import("@xterm/addon-fit"),
    import("@xterm/addon-search"),
    import("@xterm/addon-web-links"),
    import("@xterm/addon-unicode11"),
    import("@xterm/xterm/css/xterm.css"),
    // The terminal measures its letters once; the font must be there by then.
    document.fonts?.load(`${FONT_SIZE.default}px "JetBrains Mono"`).catch(() => []),
  ]).then(([xterm, fit, search, links, unicode]) => ({
    Terminal: xterm.Terminal,
    FitAddon: fit.FitAddon,
    SearchAddon: search.SearchAddon,
    WebLinksAddon: links.WebLinksAddon,
    Unicode11Addon: unicode.Unicode11Addon,
  }));
  return modules;
}

const isWindows = typeof navigator !== "undefined" && /Windows/i.test(navigator.userAgent);

/** Opens a web address a program printed; anything else is left alone. */
function openLink(uri: string) {
  if (/^https?:\/\//i.test(uri)) void api.openLink(uri).catch(() => {});
}

/** Copies the selection, if any; true when something was copied. */
export function copySelection(term: Terminal): boolean {
  const text = term.getSelection();
  if (!text) return false;
  void navigator.clipboard?.writeText(text).catch(() => {});
  term.clearSelection();
  return true;
}

/** Types the clipboard's text into the shell. */
export async function pasteClipboard(term: Terminal) {
  try {
    const text = await navigator.clipboard.readText();
    if (text) term.paste(text);
  } catch {
    // No clipboard access here; Ctrl+V still pastes.
  }
  term.focus();
}

export type ScreenEvents = {
  onData: (data: string) => void;
  onResize: (cols: number, rows: number) => void;
  /** Ctrl+F, or the search button. */
  onFind?: () => void;
  onScrolledUp?: (up: boolean) => void;
};

/**
 * A terminal on screen, with web addresses clickable, Ctrl+C copying a
 * selection (and stopping the program otherwise), Ctrl+V pasting, Ctrl+F
 * searching and Ctrl+wheel changing the letter size. Not yet in the page:
 * put `host` where it should show.
 */
export async function makeScreen(events: ScreenEvents): Promise<Screen> {
  const m = await loadXterm();
  const body = getComputedStyle(document.documentElement);
  const term = new m.Terminal({
    fontFamily: `${body.getPropertyValue("--mono").trim() || '"JetBrains Mono"'}, Consolas, monospace`,
    fontSize,
    lineHeight: 1.15,
    cursorBlink: true,
    cursorStyle: "bar",
    cursorInactiveStyle: "outline",
    scrollback: SCROLLBACK,
    // A screen a program clears goes up into the lines above, not away.
    scrollOnEraseInDisplay: true,
    smoothScrollDuration: 80,
    fastScrollSensitivity: 5,
    drawBoldTextInBrightColors: true,
    minimumContrastRatio: 4.5,
    rightClickSelectsWord: false,
    allowProposedApi: true,
    // ConPTY repaints the screen itself; tell xterm.js so lines aren't lost
    // when the terminal grows or wraps.
    windowsPty: isWindows ? { backend: "conpty" } : undefined,
    theme: terminalTheme(paintedColors()),
    // Links programs mark as such (Claude Code's, for one) open in the browser.
    linkHandler: { activate: (_e, uri) => openLink(uri), allowNonHttpProtocols: false },
  });
  const fit = new m.FitAddon();
  const search = new m.SearchAddon();
  term.loadAddon(fit);
  term.loadAddon(search);
  term.loadAddon(new m.WebLinksAddon((_e, uri) => openLink(uri)));
  const unicode = new m.Unicode11Addon();
  term.loadAddon(unicode);
  term.unicode.activeVersion = "11";

  const host = document.createElement("div");
  host.className = "terminal-host";
  term.open(host);
  term.attachCustomKeyEventHandler((e) => {
    if (e.type !== "keydown") return true;
    const key = keyOf(e);
    // The few window shortcuts that work in the terminal are not sent to the shell.
    if (key && TERMINAL_COMMANDS.some((c) => keysFor(c).includes(key))) return false;
    const ctrl = e.ctrlKey || e.metaKey;
    const k = e.key.toLowerCase();
    if (ctrl && k === "c" && (e.shiftKey || term.hasSelection())) {
      copySelection(term);
      e.preventDefault();
      return false;
    }
    // Left to the browser, which pastes into the terminal.
    if (ctrl && k === "v") return false;
    if (ctrl && !e.shiftKey && k === "f" && events.onFind) {
      e.preventDefault();
      events.onFind();
      return false;
    }
    if (e.shiftKey && (e.key === "PageUp" || e.key === "PageDown")) {
      term.scrollPages(e.key === "PageUp" ? -1 : 1);
      return false;
    }
    if (ctrl && e.shiftKey && (e.key === "Home" || e.key === "End")) {
      if (e.key === "Home") term.scrollToTop();
      else term.scrollToBottom();
      return false;
    }
    return true;
  });
  // Ctrl+wheel: letter size, for every terminal.
  host.addEventListener(
    "wheel",
    (e) => {
      if (!e.ctrlKey) return;
      e.preventDefault();
      e.stopPropagation();
      setFontSize(fontSize + (e.deltaY < 0 ? 1 : -1));
    },
    { capture: true, passive: false },
  );
  // Right-click copies the selection, or pastes when nothing is selected.
  host.addEventListener("contextmenu", (e) => {
    e.preventDefault();
    if (!copySelection(term)) void pasteClipboard(term);
  });
  term.onData(events.onData);
  term.onResize(({ cols, rows }) => events.onResize(cols, rows));
  term.onScroll(() => {
    const b = term.buffer.active;
    events.onScrolledUp?.(b.viewportY < b.baseY);
  });
  term.onWriteParsed(() => {
    const b = term.buffer.active;
    events.onScrolledUp?.(b.viewportY < b.baseY);
  });
  onPaint((c) => (term.options.theme = terminalTheme(c)));
  fontListeners.add((size) => {
    term.options.fontSize = size;
    try {
      fit.fit();
    } catch {
      // Not on screen; fits when it is.
    }
  });
  return { term, fit, search, host };
}

const fontListeners = new Set<(size: number) => void>();

/** Sets the letter size of every terminal. */
export function setFontSize(size: number) {
  const next = Math.min(FONT_SIZE.max, Math.max(FONT_SIZE.min, Math.round(size)));
  if (next === fontSize) return;
  fontSize = next;
  try {
    localStorage.setItem(FONT_KEY, String(next));
  } catch {
    // Kept until the app closes.
  }
  fontListeners.forEach((l) => l(next));
  publish();
}

/** Keeps `screen` sized to `container` while it is in it; returns the undo. */
export function fitInto(screen: Screen, container: HTMLElement, focus = true): () => void {
  container.appendChild(screen.host);
  const fitNow = () => {
    if (!container.isConnected || container.clientWidth === 0) return;
    try {
      screen.fit.fit();
    } catch {
      // Hidden for a moment; fits on the next change.
    }
  };
  fitNow();
  const observer = new ResizeObserver(fitNow);
  observer.observe(container);
  if (focus) screen.term.focus();
  return () => {
    observer.disconnect();
    if (screen.host.parentElement === container) screen.host.remove();
  };
}

// --- The main window's shells ---

let started: Promise<void> | null = null;

/** Listens to the shells and picks up the ones still running from before. */
function start(): Promise<void> {
  started ??= (async () => {
    void api.onConsoleOutput((id, data, seq) => {
      const s = byId(id);
      if (s && seq > (skipUpTo.get(id) ?? 0)) s.screen.term.write(data);
    });
    void api.onConsoleExit((id) => {
      const s = byId(id);
      if (!s) return;
      update(s.key, { id: null, running: false, poppedOut: false });
      s.screen.term.write("\r\n\x1b[2m[The shell ended. Press Enter to start a new one.]\x1b[0m\r\n");
    });
    void api.onConsoleDocked((id) => {
      const s = byId(id);
      if (!s) return;
      update(s.key, { poppedOut: false });
      // The window it was in had its own size.
      const { term } = s.screen;
      void api.consoleResize(id, term.cols, term.rows).catch(() => {});
    });
    shells = await api.consoleShells().catch(() => []);
    const open = await api.consoleList().catch(() => []);
    for (const info of open) {
      const live = await addSession(info.folder, info.label, info.shell);
      live.id = info.id;
      live.running = true;
      const history = await api.consoleHistory(info.id).catch(() => ({ text: "", seq: 0 }));
      skipUpTo.set(info.id, history.seq);
      live.screen.term.write(history.text);
    }
    publish();
  })();
  return started;
}

/** Output a shell printed before its history was read, which the history holds already. */
const skipUpTo = new Map<number, number>();

async function addSession(folder: string | null, label: string | null, shell: string | null): Promise<Live> {
  const key = nextKey++;
  const screen = await makeScreen({
    onData: (data) => {
      const s = byKey(key);
      if (!s) return;
      if (s.id !== null) void api.consoleWrite(s.id, data).catch(() => {});
      // After the shell ended, Enter starts a new one in the same folder.
      else if (data === "\r") void run(key);
    },
    onResize: (cols, rows) => {
      const s = byKey(key);
      if (s?.id != null && !s.poppedOut) void api.consoleResize(s.id, cols, rows).catch(() => {});
    },
    onFind: () => findListeners.forEach((l) => l(key)),
    onScrolledUp: (up) => {
      const s = byKey(key);
      if (s && s.scrolledUp !== up) update(key, { scrolledUp: up });
    },
  });
  const live: Live = {
    key,
    id: null,
    folder,
    label,
    shell,
    running: false,
    error: null,
    broken: [],
    poppedOut: false,
    scrolledUp: false,
    screen,
  };
  sessions = [...sessions, live];
  active = key;
  publish();
  return live;
}

const findListeners = new Set<(key: number) => void>();

/** Calls `listener` with the shell whose search Ctrl+F opened. */
export function onFind(listener: (key: number) => void): () => void {
  findListeners.add(listener);
  return () => void findListeners.delete(listener);
}

/** Starts (or starts again) the shell of session `key`. */
async function run(key: number) {
  const s = byKey(key);
  if (!s) return;
  const { term, fit } = s.screen;
  if (s.id !== null) {
    void api.consoleClose(s.id);
    term.reset();
  }
  update(key, { id: null, running: false, error: null, poppedOut: false });
  try {
    fit.fit();
  } catch {
    // Not on screen yet; the size is sent once it is.
  }
  try {
    const opened = await api.consoleOpen(s.folder, s.label, s.shell, term.cols, term.rows);
    const now = byKey(key);
    if (!now) {
      void api.consoleClose(opened.id);
      return;
    }
    now.id = opened.id;
    update(key, { id: opened.id, running: true, broken: opened.broken });
  } catch (e) {
    update(key, { error: errorText(e) });
  }
}

/** Shows the terminal where it lives: its tab in the main area, else the panel. */
export function showTerminal() {
  setLayout((l) => (l.tabs.includes("terminal") ? { active: "terminal" } : { panel: true, panelTab: "terminal" }));
}

/**
 * Shows the terminal, with a shell in `folder` (a site's): the one already
 * running there, or a new one.
 */
export function openTerminal(folder: string | null = null, label: string | null = null) {
  showTerminal();
  void (async () => {
    await start();
    const there = sessions.find((s) => s.folder === folder && s.running);
    if (there) {
      active = there.key;
      publish();
      return;
    }
    await newTerminal(folder, label);
  })();
}

/** A new shell, in `folder` (the active one's when left out) and `shell` (the default when null). */
export async function newTerminal(folder?: string | null, label?: string | null, shell: string | null = null) {
  await start();
  const from = byKey(active);
  const live = await addSession(
    folder === undefined ? (from?.folder ?? null) : folder,
    label === undefined ? (from?.label ?? null) : (label ?? null),
    shell,
  );
  await run(live.key);
}

let ensuring: Promise<void> | null = null;

/** Makes sure there is a shell to show once the terminal is on screen. */
export function ensureTerminal(): Promise<void> {
  ensuring ??= (async () => {
    await start();
    if (sessions.length === 0) await newTerminal(null, null);
  })().finally(() => (ensuring = null));
  return ensuring;
}

export function selectTerminal(key: number) {
  if (!byKey(key)) return;
  active = key;
  publish();
}

/** Starts the shell again in its folder. */
export function restartTerminal(key = active) {
  if (key !== null) void run(key);
}

/** Ends a shell and removes its tab. */
export function closeTerminal(key: number) {
  const s = byKey(key);
  if (!s) return;
  if (s.id !== null) void api.consoleClose(s.id);
  s.screen.term.dispose();
  s.screen.host.remove();
  const at = sessions.indexOf(s);
  sessions = sessions.filter((x) => x !== s);
  if (active === key) active = (sessions[Math.min(at, sessions.length - 1)] ?? sessions[0])?.key ?? null;
  publish();
}

/** The program a shell is running, if any, to ask before ending it. */
export async function busyWith(key: number): Promise<string | null> {
  const s = byKey(key);
  if (s?.id == null) return null;
  return api.consoleBusy(s.id).catch(() => null);
}

/** Types `text` into a shell, as if typed, and shows it. */
export function typeInto(key: number, text: string) {
  const s = byKey(key);
  if (s?.id == null) return;
  void api.consoleWrite(s.id, text).catch(() => {});
  s.screen.term.focus();
}

export function dismissBroken(key: number) {
  update(key, { broken: [] });
}

/** Shows a shell in a window of its own. */
export async function popOut(key: number) {
  const s = byKey(key);
  if (s?.id == null) return;
  update(key, { poppedOut: true });
  try {
    await api.consoleWindow(s.id, `${s.label ?? "Terminal"} · Stackyard`);
  } catch (e) {
    update(key, { poppedOut: false, error: errorText(e) });
  }
}

/** The shell `key`'s terminal, for the component that shows it. */
export function screenOf(key: number): Screen | null {
  return byKey(key)?.screen ?? null;
}

/** Moves the terminal from the panel into a tab of the main area, or back. */
export function moveTerminal(toTab: boolean) {
  setLayout((l) => {
    if (toTab) {
      const tabs = l.tabs.includes("terminal") ? l.tabs : [...l.tabs, "terminal" as const];
      return { tabs, active: "terminal", panel: l.panelTab === "terminal" ? false : l.panel };
    }
    const tabs = l.tabs.filter((t) => t !== "terminal");
    const active = l.active === "terminal" ? (tabs[tabs.length - 1] ?? null) : l.active;
    return { tabs, active, panel: true, panelTab: "terminal" };
  });
}

export function useTerminal(): TerminalState {
  const [value, setValue] = useState(state);
  useEffect(() => {
    listeners.add(setValue);
    setValue(state);
    return () => void listeners.delete(setValue);
  }, []);
  return value;
}
