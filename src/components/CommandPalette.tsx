import { useEffect, useId, useMemo, useRef, useState, type KeyboardEvent } from "react";
import { endPreview } from "../lib/appearance";
import type { Command } from "../lib/commands";
import { keyLabel, keysFor, useKeyChanges } from "../lib/keybindings";
import { closePalette, matchScore, recentCommands, rememberCommand, type PaletteMode } from "../lib/palette";
import { useT } from "../lib/i18n";

/**
 * The search box over the screen. With ">" first it lists commands, otherwise
 * sites and screens; opened as the theme picker it lists color themes and
 * shows each one while it is selected. Arrow keys move, Enter runs, Escape
 * closes (and puts the theme back).
 */
export function CommandPalette({
  mode,
  commands,
  places,
  themes,
}: {
  mode: Exclude<PaletteMode, null>;
  commands: Command[];
  places: Command[];
  themes: Command[];
}) {
  const t = useT();
  const changes = useKeyChanges();
  const [text, setText] = useState(mode === "commands" ? ">" : "");
  const [selected, setSelected] = useState(0);
  const input = useRef<HTMLInputElement>(null);
  const list = useRef<HTMLUListElement>(null);
  const listId = useId();
  const isThemes = mode === "themes";
  const isCommands = !isThemes && text.startsWith(">");
  const query = isCommands ? text.slice(1).trim() : text.trim();

  const shown = useMemo(() => {
    const pool = isThemes ? themes : isCommands ? commands : places;
    if (!query) {
      if (!isCommands) return pool;
      const recent = recentCommands();
      const first = recent.map((id) => pool.find((c) => c.id === id)).filter((c): c is Command => !!c);
      return [...first, ...pool.filter((c) => !recent.includes(c.id))];
    }
    return pool
      .map((c) => ({ c, score: matchScore(query, `${c.category ?? ""} ${c.title} ${c.detail ?? ""}`) }))
      .filter((m): m is { c: Command; score: number } => m.score !== null)
      .sort((a, b) => b.score - a.score)
      .map((m) => m.c);
  }, [isThemes, isCommands, query, commands, places, themes]);

  // Closing puts focus back where it was, unless the command moved it.
  const opener = useRef(document.activeElement as HTMLElement | null);
  useEffect(() => {
    const before = opener.current;
    input.current?.focus();
    return () =>
      void requestAnimationFrame(() => {
        const now = document.activeElement;
        if ((!now || now === document.body) && before?.isConnected) before.focus();
      });
  }, []);
  // The theme picker shows the selected theme; closing without picking puts the chosen one back.
  useEffect(() => {
    if (isThemes) shown[selected]?.preview?.();
  }, [isThemes, shown, selected]);
  useEffect(() => () => void (isThemes && endPreview()), [isThemes]);
  useEffect(() => setSelected(0), [text]);
  useEffect(() => {
    list.current?.querySelector('[aria-selected="true"]')?.scrollIntoView({ block: "nearest" });
  }, [selected]);

  function run(c: Command | undefined) {
    if (!c) return;
    closePalette();
    if (isCommands) rememberCommand(c.id);
    c.run();
  }

  function key(e: KeyboardEvent) {
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      const step = e.key === "ArrowDown" ? 1 : -1;
      setSelected((s) => (shown.length ? (s + step + shown.length) % shown.length : 0));
    } else if (e.key === "Enter") {
      e.preventDefault();
      run(shown[selected]);
    } else if (e.key === "Escape") {
      e.preventDefault();
      closePalette();
    }
  }

  const option = (i: number) => `${listId}-${i}`;
  return (
    <div className="palette-backdrop" onMouseDown={(e) => e.target === e.currentTarget && closePalette()}>
      <div className="palette" role="dialog" aria-modal="true" aria-label={t("Command palette")}>
        <input
          ref={input}
          className="palette-input"
          role="combobox"
          aria-expanded="true"
          aria-controls={listId}
          aria-activedescendant={shown.length ? option(selected) : undefined}
          aria-label={isThemes ? t("Pick a color theme") : t("Search sites and screens, or type > for commands")}
          placeholder={isThemes ? t("Pick a color theme") : t("Search sites and screens, or type > for commands")}
          value={text}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={key}
          onBlur={(e) => !e.currentTarget.closest(".palette")?.contains(e.relatedTarget) && closePalette()}
          spellCheck={false}
        />
        <ul ref={list} id={listId} className="palette-list" role="listbox" aria-label={t("Results")}>
          {shown.length === 0 && <li className="palette-empty">{t("Nothing found.")}</li>}
          {shown.map((c, i) => {
            const keys = isCommands ? keysFor(c.id, changes) : [];
            return (
              <li
                key={c.id}
                id={option(i)}
                role="option"
                aria-selected={i === selected}
                className="palette-item"
                onMouseMove={() => setSelected(i)}
                onMouseDown={(e) => {
                  e.preventDefault();
                  run(c);
                }}
              >
                <span className="palette-title">
                  {c.category && <span className="muted">{c.category}: </span>}
                  {c.title}
                  {c.detail && <span className="muted small"> {c.detail}</span>}
                </span>
                {keys[0] && <kbd className="palette-key">{keyLabel(keys[0])}</kbd>}
              </li>
            );
          })}
        </ul>
      </div>
    </div>
  );
}
