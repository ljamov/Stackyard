// The Terminal tab in the panel: one shell at a time, kept running while the
// panel is closed or shows another tab. xterm.js is loaded the first time the
// tab opens, so the window starts no slower for it.

import { useEffect, useState } from "react";
import type { Terminal } from "@xterm/xterm";
import type { FitAddon } from "@xterm/addon-fit";
import { api, errorText } from "./api";
import { setLayout } from "./layout";
import { keyOf, keysFor } from "./keybindings";
import { onPaint, paintedColors, type ThemeColors } from "./theme";

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

export type TerminalState = {
  /** The folder the shell started in; null for the home folder. */
  folder: string | null;
  /** What the folder is called in the tab: a site's address, or null for the home folder. */
  label: string | null;
  running: boolean;
  error: string | null;
};

type Loaded = { term: Terminal; fit: FitAddon; host: HTMLDivElement };

let state: TerminalState = { folder: null, label: null, running: false, error: null };
let id: number | null = null;
let loaded: Promise<Loaded> | null = null;
let wanted: { folder: string | null; label: string | null } | null = null;
const listeners = new Set<(s: TerminalState) => void>();

function set(change: Partial<TerminalState>) {
  state = { ...state, ...change };
  listeners.forEach((l) => l(state));
}

function theme(c: ThemeColors | null) {
  if (!c) return {};
  return {
    background: c["window.background"],
    foreground: c["foreground"],
    cursor: c["accent"],
    cursorAccent: c["window.background"],
    selectionBackground: c["card.raisedBackground"],
  };
}

function load(): Promise<Loaded> {
  loaded ??= Promise.all([import("@xterm/xterm"), import("@xterm/addon-fit"), import("@xterm/xterm/css/xterm.css")]).then(
    ([{ Terminal }, { FitAddon }]) => {
      const body = getComputedStyle(document.documentElement);
      const term = new Terminal({
        fontFamily: body.getPropertyValue("--mono") || "monospace",
        fontSize: 13,
        cursorBlink: true,
        allowProposedApi: false,
        theme: theme(paintedColors()),
      });
      const fit = new FitAddon();
      term.loadAddon(fit);
      const host = document.createElement("div");
      host.className = "terminal-host";
      term.open(host);
      // The few window shortcuts that work in the terminal are not sent to the shell.
      term.attachCustomKeyEventHandler((e) => {
        const key = e.type === "keydown" ? keyOf(e) : null;
        return !key || !TERMINAL_COMMANDS.some((c) => keysFor(c).includes(key));
      });
      term.onData((data) => id !== null && void api.consoleWrite(id, data).catch(() => {}));
      term.onResize(({ cols, rows }) => id !== null && void api.consoleResize(id, cols, rows).catch(() => {}));
      onPaint((c) => (term.options.theme = theme(c)));
      void api.onConsoleOutput((from, data) => from === id && term.write(data));
      void api.onConsoleExit((from) => {
        if (from !== id) return;
        id = null;
        set({ running: false });
        term.write("\r\n\x1b[2m[The shell ended. Press Enter to start a new one.]\x1b[0m\r\n");
      });
      // After the shell ended, Enter starts a new one in the same folder.
      term.onKey(({ domEvent }) => {
        if (id === null && domEvent.key === "Enter") void start(state.folder, state.label);
      });
      return { term, fit, host };
    },
  );
  return loaded;
}

async function start(folder: string | null, label: string | null) {
  const { term, fit } = await load();
  if (id !== null) {
    const old = id;
    id = null;
    void api.consoleClose(old);
    term.reset();
  }
  set({ folder, label, running: false, error: null });
  try {
    fit.fit();
  } catch {
    // Not on screen yet; the size is sent once it is.
  }
  try {
    id = await api.consoleOpen(folder, term.cols, term.rows);
    set({ running: true });
  } catch (e) {
    set({ error: errorText(e) });
  }
}

/** Shows the Terminal tab, with a shell in `folder` (a site's) if one isn't running there already. */
export function openTerminal(folder: string | null = null, label: string | null = null) {
  setLayout({ panel: true, panelTab: "terminal" });
  if (id !== null && state.folder === folder) return;
  if (loaded || id !== null) void start(folder, label);
  else wanted = { folder, label };
}

/** Starts the shell again in its folder. */
export function restartTerminal() {
  void start(state.folder, state.label);
}

/** Puts the terminal into `container` while the tab is shown, sized to fit it. */
export function attachTerminal(container: HTMLElement): () => void {
  let gone = false;
  let observer: ResizeObserver | null = null;
  void load().then(({ term, fit, host }) => {
    if (gone) return;
    container.appendChild(host);
    const fitNow = () => {
      try {
        fit.fit();
      } catch {
        // Hidden for a moment; fits on the next change.
      }
    };
    fitNow();
    observer = new ResizeObserver(fitNow);
    observer.observe(container);
    if (id === null && !state.error) {
      const w = wanted ?? { folder: state.folder, label: state.label };
      wanted = null;
      void start(w.folder, w.label);
    }
    term.focus();
  });
  return () => {
    gone = true;
    observer?.disconnect();
  };
}

export function useTerminal(): TerminalState {
  const [value, setValue] = useState(state);
  useEffect(() => {
    listeners.add(setValue);
    return () => void listeners.delete(setValue);
  }, []);
  return value;
}
