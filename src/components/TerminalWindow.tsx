import { useEffect, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { api } from "../lib/api";
import { copySelection, FONT_SIZE, fitInto, makeScreen, setFontSize, type Screen } from "../lib/terminal";
import { useT } from "../lib/i18n";
import { Icon } from "./Icon";
import { FindBar } from "./TerminalPane";

/**
 * A terminal popped out of the main window into a window of its own. It
 * shows what the shell printed so far and goes on from there; closing the
 * window puts the terminal back in the main window, still running.
 */
export function TerminalWindow({ id }: { id: number }) {
  const t = useT();
  const box = useRef<HTMLDivElement>(null);
  const [screen, setScreen] = useState<Screen | null>(null);
  const [ended, setEnded] = useState(false);
  const [finding, setFinding] = useState(false);
  const [scrolledUp, setScrolledUp] = useState(false);
  const [size, setSize] = useState<number>(FONT_SIZE.default);
  const [title, setTitle] = useState(t("Terminal"));
  useEffect(() => {
    try {
      void getCurrentWindow()
        .title()
        .then(setTitle)
        .catch(() => {});
    } catch {
      // Not in the app (the browser preview).
    }
  }, []);

  useEffect(() => {
    let gone = false;
    let undo: (() => void) | null = null;
    const offs: Promise<() => void>[] = [];
    void (async () => {
      const made = await makeScreen({
        onData: (data) => void api.consoleWrite(id, data).catch(() => {}),
        onResize: (cols, rows) => void api.consoleResize(id, cols, rows).catch(() => {}),
        onFind: () => setFinding(true),
        onScrolledUp: setScrolledUp,
      });
      if (gone) return made.term.dispose();
      // What arrives while the history is read is held, and what the history has is skipped.
      let upTo = Number.POSITIVE_INFINITY;
      const held: { data: string; seq: number }[] = [];
      offs.push(
        api.onConsoleOutput((from, data, seq) => {
          if (from !== id) return;
          if (upTo === Number.POSITIVE_INFINITY) held.push({ data, seq });
          else if (seq > upTo) made.term.write(data);
        }),
        api.onConsoleExit((from) => {
          if (from !== id) return;
          setEnded(true);
          made.term.write("\r\n\x1b[2m[The shell ended. Close this window.]\x1b[0m\r\n");
        }),
      );
      const history = await api.consoleHistory(id).catch(() => ({ text: "", seq: 0 }));
      made.term.write(history.text);
      upTo = history.seq;
      for (const h of held) if (h.seq > upTo) made.term.write(h.data);
      setScreen(made);
      setSize(made.term.options.fontSize ?? FONT_SIZE.default);
      if (box.current) undo = fitInto(made, box.current);
      // Its size now, which the main window's terminal had until now.
      void api.consoleResize(id, made.term.cols, made.term.rows).catch(() => {});
    })();
    return () => {
      gone = true;
      undo?.();
      offs.forEach((o) => void o.then((f) => f()));
    };
  }, [id]);

  const zoom = (by: number) => {
    setFontSize(size + by);
    setSize((s) => Math.min(FONT_SIZE.max, Math.max(FONT_SIZE.min, s + by)));
  };

  return (
    <div className="terminal-window terminal-pane">
      <div className="terminal-bar">
        <div className="terminal-window-title">
          <span className="terminal-dot" data-running={!ended || undefined} aria-hidden="true" />
          {title}
        </div>
        <div className="terminal-tools">
          {screen && (
            <>
              <button type="button" className="icon-btn" title={t("Find")} aria-label={t("Find")} aria-pressed={finding} onClick={() => setFinding((f) => !f)}>
                <Icon name="search" />
              </button>
              <button type="button" className="icon-btn" title={t("Copy the selection")} aria-label={t("Copy the selection")} onClick={() => copySelection(screen.term)}>
                <Icon name="copy" />
              </button>
              <button type="button" className="icon-btn" title={t("Clear")} aria-label={t("Clear")} onClick={() => screen.term.clear()}>
                <Icon name="clear" />
              </button>
            </>
          )}
          <div className="terminal-zoom" role="group" aria-label={t("Text size")}>
            <button type="button" className="icon-btn" aria-label={t("Smaller text")} title={t("Smaller text (Ctrl+wheel)")} onClick={() => zoom(-1)}>
              A<sup>−</sup>
            </button>
            <button type="button" className="icon-btn" aria-label={t("Bigger text")} title={t("Bigger text (Ctrl+wheel)")} onClick={() => zoom(1)}>
              A<sup>+</sup>
            </button>
          </div>
          <button
            type="button"
            className="icon-btn"
            title={t("Back to the main window")}
            aria-label={t("Back to the main window")}
            onClick={() => void getCurrentWindow().close()}
          >
            <Icon name="panelWide" />
          </button>
        </div>
      </div>
      {finding && screen && <FindBar screen={screen} onClose={() => setFinding(false)} />}
      <div className="terminal-stage">
        <div ref={box} className="terminal-box" aria-label={t("Terminal")} />
        {screen && scrolledUp && (
          <button
            type="button"
            className="terminal-to-bottom"
            onClick={() => {
              screen.term.scrollToBottom();
              screen.term.focus();
            }}
          >
            <Icon name="arrowDown" />
            {t("Newest")}
          </button>
        )}
      </div>
    </div>
  );
}
