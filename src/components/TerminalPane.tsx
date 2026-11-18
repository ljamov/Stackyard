import { useEffect, useRef, useState } from "react";
import type { Site } from "../lib/api";
import {
  busyWith,
  closeTerminal,
  copySelection,
  dismissBroken,
  ensureTerminal,
  FONT_SIZE,
  fitInto,
  moveTerminal,
  newTerminal,
  onFind,
  popOut,
  restartTerminal,
  screenOf,
  selectTerminal,
  setFontSize,
  showTerminal,
  typeInto,
  useTerminal,
  type Screen,
  type TerminalSession,
} from "../lib/terminal";
import { useT } from "../lib/i18n";
import { paintedColors, terminalFindColors } from "../lib/theme";
import { Icon } from "./Icon";

type Place = "panel" | "tab" | "elsewhere";

/**
 * The terminal: a strip of shells, the shown one's screen, and its tools.
 * `place` is where it shows: the panel, a tab of the main area, or (for the
 * panel while the terminal is in a tab) elsewhere.
 */
export function TerminalPane({ sites, place }: { sites: Site[]; place: Place }) {
  const t = useT();
  if (place === "elsewhere") {
    return (
      <div className="terminal-pane terminal-elsewhere">
        <p className="muted">{t("The terminal is open in a tab of its own.")}</p>
        <div className="row">
          <button type="button" className="btn small" onClick={showTerminal}>
            {t("Show it")}
          </button>
          <button type="button" className="btn small" onClick={() => moveTerminal(false)}>
            {t("Move it back here")}
          </button>
        </div>
      </div>
    );
  }
  return <Terminals sites={sites} place={place} />;
}

type Confirm = { key: number; action: "restart" | "close"; program: string };

function Terminals({ sites, place }: { sites: Site[]; place: "panel" | "tab" }) {
  const t = useT();
  const terminal = useTerminal();
  const box = useRef<HTMLDivElement>(null);
  const [menu, setMenu] = useState(false);
  const [finding, setFinding] = useState(false);
  const [confirm, setConfirm] = useState<Confirm | null>(null);
  const shown = terminal.sessions.find((s) => s.key === terminal.active) ?? null;
  const screen = shown ? screenOf(shown.key) : null;

  useEffect(() => void ensureTerminal(), []);
  useEffect(() => onFind(() => setFinding(true)), []);
  // The shown shell's screen goes into the box; the others wait off the page.
  useEffect(() => {
    if (!box.current || !screen || shown?.poppedOut) return;
    return fitInto(screen, box.current);
  }, [screen, shown?.poppedOut]);

  const shellName = (s: TerminalSession) =>
    s.shell ? (terminal.shells.find((x) => x.id === s.shell)?.name ?? s.shell) : (terminal.shells[0]?.name ?? "");
  const name = (s: TerminalSession) => s.label ?? t("Home folder");

  /** Asks first when the shell is running a program, which ending it stops. */
  async function guarded(key: number, action: "restart" | "close") {
    const program = await busyWith(key);
    if (program) setConfirm({ key, action, program });
    else act(key, action);
  }
  function act(key: number, action: "restart" | "close") {
    setConfirm(null);
    if (action === "restart") restartTerminal(key);
    else closeTerminal(key);
  }

  return (
    <div className="terminal-pane" data-place={place}>
      <div className="terminal-bar">
        <div className="terminal-tabs" role="tablist" aria-label={t("Terminals")}>
          {terminal.sessions.map((s) => (
            <div key={s.key} className="terminal-tab" data-active={s.key === terminal.active || undefined}>
              <button
                type="button"
                role="tab"
                aria-selected={s.key === terminal.active}
                title={`${name(s)} · ${shellName(s)}${s.folder ? `\n${s.folder}` : ""}`}
                onClick={() => selectTerminal(s.key)}
                onAuxClick={(e) => e.button === 1 && void guarded(s.key, "close")}
              >
                <span className="terminal-dot" data-running={s.running || undefined} aria-hidden="true" />
                <span className="terminal-tab-name">{name(s)}</span>
                {terminal.shells.length > 1 && <span className="terminal-tab-shell">{shellName(s)}</span>}
                {s.poppedOut && <Icon name="newWindow" />}
              </button>
              <button
                type="button"
                className="icon-btn terminal-tab-close"
                aria-label={t("Close {name}", { name: name(s) })}
                title={t("Close {name}", { name: name(s) })}
                onClick={() => void guarded(s.key, "close")}
              >
                <Icon name="close" />
              </button>
            </div>
          ))}
          <div className="terminal-new">
            <button
              type="button"
              className="icon-btn"
              aria-label={t("New terminal")}
              title={t("New terminal")}
              onClick={() => void newTerminal()}
            >
              <Icon name="plus" />
            </button>
            <button
              type="button"
              className="icon-btn terminal-new-more"
              aria-label={t("New terminal in…")}
              title={t("New terminal in…")}
              aria-haspopup="menu"
              aria-expanded={menu}
              onClick={() => setMenu((m) => !m)}
            >
              <Icon name="chevron" />
            </button>
            {menu && <NewMenu sites={sites} onClose={() => setMenu(false)} />}
          </div>
        </div>
        <div className="terminal-tools">
          {screen && (
            <>
              <ToolButton icon="search" label={t("Find")} onClick={() => setFinding((f) => !f)} pressed={finding} />
              <ToolButton icon="copy" label={t("Copy the selection")} onClick={() => copySelection(screen.term)} />
              <ToolButton icon="clear" label={t("Clear")} onClick={() => screen.term.clear()} />
            </>
          )}
          <div className="terminal-zoom" role="group" aria-label={t("Text size")}>
            <button
              type="button"
              className="icon-btn"
              aria-label={t("Smaller text")}
              title={t("Smaller text (Ctrl+wheel)")}
              disabled={terminal.fontSize <= FONT_SIZE.min}
              onClick={() => setFontSize(terminal.fontSize - 1)}
            >
              A<sup>−</sup>
            </button>
            <button
              type="button"
              className="icon-btn"
              aria-label={t("Bigger text")}
              title={t("Bigger text (Ctrl+wheel)")}
              disabled={terminal.fontSize >= FONT_SIZE.max}
              onClick={() => setFontSize(terminal.fontSize + 1)}
            >
              A<sup>+</sup>
            </button>
          </div>
          {shown && (
            <>
              <ToolButton icon="restart" label={t("Restart")} onClick={() => void guarded(shown.key, "restart")} />
              <ToolButton
                icon="newWindow"
                label={t("Open in a new window")}
                disabled={!shown.running || shown.poppedOut}
                onClick={() => void popOut(shown.key)}
              />
            </>
          )}
          <ToolButton
            icon={place === "panel" ? "tab" : "panelWide"}
            label={place === "panel" ? t("Open in a tab") : t("Move to the panel")}
            onClick={() => moveTerminal(place === "panel")}
          />
        </div>
      </div>

      {confirm && (
        <div className="terminal-notice warn" role="alert">
          <span>
            {t(
              "{program} is still running in this terminal. {action} ends it, and an update it is making would be cut off.",
              {
                program: confirm.program,
                action: confirm.action === "restart" ? t("Restarting") : t("Closing"),
              },
            )}
          </span>
          <button type="button" className="btn small danger" onClick={() => act(confirm.key, confirm.action)}>
            {confirm.action === "restart" ? t("Restart anyway") : t("Close anyway")}
          </button>
          <button type="button" className="btn small" onClick={() => setConfirm(null)}>
            {t("Cancel")}
          </button>
        </div>
      )}
      {shown?.broken.map((b) => (
        <div key={b.id} className="terminal-notice" role="status">
          <span>
            {t(
              "{name} won't start: an update of it was cut off and left nothing to put back. Installing it again fixes it; your settings and sign-in stay.",
              { name: b.name },
            )}
          </span>
          <button
            type="button"
            className="btn small primary"
            onClick={() => {
              typeInto(shown.key, `${b.reinstall}\r`);
              dismissBroken(shown.key);
            }}
          >
            {t("Install {name} again", { name: b.name })}
          </button>
          <button type="button" className="icon-btn" aria-label={t("Dismiss")} onClick={() => dismissBroken(shown.key)}>
            <Icon name="close" />
          </button>
        </div>
      ))}
      {shown?.error && (
        <div className="terminal-notice error" role="alert">
          {shown.error}
        </div>
      )}
      {finding && screen && <FindBar screen={screen} onClose={() => setFinding(false)} />}

      <div className="terminal-stage">
        {shown?.poppedOut ? (
          <div className="terminal-elsewhere">
            <p className="muted">{t("This terminal is open in a window of its own. Close that window to bring it back here.")}</p>
          </div>
        ) : (
          <div ref={box} className="terminal-box" aria-label={t("Terminal")} />
        )}
        {screen && shown?.scrolledUp && !shown.poppedOut && (
          <button
            type="button"
            className="terminal-to-bottom"
            title={t("Scroll to the newest line (Ctrl+Shift+End)")}
            onClick={() => {
              screen.term.scrollToBottom();
              screen.term.focus();
            }}
          >
            <Icon name="arrowDown" />
            {t("Newest")}
          </button>
        )}
        {terminal.sessions.length === 0 && (
          <div className="terminal-elsewhere">
            <button type="button" className="btn small" onClick={() => void newTerminal(null, null)}>
              {t("New terminal")}
            </button>
          </div>
        )}
      </div>
    </div>
  );
}

function ToolButton({
  icon,
  label,
  onClick,
  pressed,
  disabled,
}: {
  icon: Parameters<typeof Icon>[0]["name"];
  label: string;
  onClick: () => void;
  pressed?: boolean;
  disabled?: boolean;
}) {
  return (
    <button
      type="button"
      className="icon-btn"
      aria-label={label}
      title={label}
      aria-pressed={pressed}
      disabled={disabled}
      onClick={onClick}
    >
      <Icon name={icon} />
    </button>
  );
}

/** Starts a terminal with another shell, or in a site's folder. */
function NewMenu({ sites, onClose }: { sites: Site[]; onClose: () => void }) {
  const t = useT();
  const { shells } = useTerminal();
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    ref.current?.querySelector<HTMLElement>("button")?.focus();
    const away = (e: PointerEvent) => !ref.current?.parentElement?.contains(e.target as Node) && onClose();
    const esc = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    document.addEventListener("pointerdown", away);
    document.addEventListener("keydown", esc);
    return () => {
      document.removeEventListener("pointerdown", away);
      document.removeEventListener("keydown", esc);
    };
  }, [onClose]);
  const pick = (run: () => void) => () => {
    onClose();
    run();
  };
  return (
    <div ref={ref} className="terminal-menu" role="menu">
      {shells.length > 0 && <div className="terminal-menu-head">{t("Shell")}</div>}
      {shells.map((s) => (
        <button key={s.id} type="button" role="menuitem" onClick={pick(() => void newTerminal(undefined, undefined, s.id))}>
          <Icon name="terminal" />
          {s.name}
        </button>
      ))}
      <div className="terminal-menu-head">{t("Folder")}</div>
      <button type="button" role="menuitem" onClick={pick(() => void newTerminal(null, null))}>
        <Icon name="folder" />
        {t("Home folder")}
      </button>
      {sites.map((s) => (
        <button
          key={s.domain}
          type="button"
          role="menuitem"
          onClick={pick(() => void newTerminal(s.project.folder, s.domain))}
        >
          <Icon name="folder" />
          {s.domain}
        </button>
      ))}
    </div>
  );
}

/** Finds text in the shown terminal, the lines above the screen included. */
export function FindBar({ screen, onClose }: { screen: Screen; onClose: () => void }) {
  const t = useT();
  const input = useRef<HTMLInputElement>(null);
  const [text, setText] = useState(() => screen.term.getSelection().split("\n")[0] ?? "");
  const [caseSensitive, setCase] = useState(false);
  const [regex, setRegex] = useState(false);
  const [results, setResults] = useState<{ index: number; count: number } | null>(null);

  useEffect(() => {
    input.current?.focus();
    input.current?.select();
    const sub = screen.search.onDidChangeResults(({ resultIndex, resultCount }) =>
      setResults({ index: resultIndex, count: resultCount }),
    );
    return () => {
      sub.dispose();
      screen.search.clearDecorations();
    };
  }, [screen]);

  const options = () => ({
    caseSensitive,
    regex,
    incremental: true,
    decorations: terminalFindColors(paintedColors()),
  });
  const find = (back = false, value = text) => {
    if (!value) {
      screen.search.clearDecorations();
      setResults(null);
      return;
    }
    try {
      if (back) screen.search.findPrevious(value, options());
      else screen.search.findNext(value, options());
    } catch {
      // A half-typed pattern; it finds once it is whole.
    }
  };
  useEffect(() => find(false), [caseSensitive, regex]); // eslint-disable-line react-hooks/exhaustive-deps

  return (
    <div className="terminal-find" role="search">
      <input
        ref={input}
        type="text"
        value={text}
        placeholder={t("Find")}
        aria-label={t("Find in the terminal")}
        onChange={(e) => {
          setText(e.target.value);
          find(false, e.target.value);
        }}
        onKeyDown={(e) => {
          if (e.key === "Enter") {
            e.preventDefault();
            find(e.shiftKey);
          } else if (e.key === "Escape") {
            e.preventDefault();
            onClose();
            screen.term.focus();
          }
        }}
      />
      <span className="muted small terminal-find-count" aria-live="polite">
        {text && results ? (results.count === 0 ? t("No results") : `${results.index + 1} / ${results.count}`) : ""}
      </span>
      <button
        type="button"
        className="icon-btn terminal-find-opt"
        aria-pressed={caseSensitive}
        title={t("Match case")}
        onClick={() => setCase((c) => !c)}
      >
        Aa
      </button>
      <button
        type="button"
        className="icon-btn terminal-find-opt"
        aria-pressed={regex}
        title={t("Regular expression")}
        onClick={() => setRegex((r) => !r)}
      >
        .*
      </button>
      <ToolButton icon="arrowUp" label={t("Previous (Shift+Enter)")} onClick={() => find(true)} />
      <ToolButton icon="arrowDown" label={t("Next (Enter)")} onClick={() => find(false)} />
      <ToolButton
        icon="close"
        label={t("Close")}
        onClick={() => {
          onClose();
          screen.term.focus();
        }}
      />
    </div>
  );
}
