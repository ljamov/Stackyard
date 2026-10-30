import { useEffect, useRef } from "react";
import type { Site } from "../lib/api";
import { attachTerminal, openTerminal, restartTerminal, useTerminal } from "../lib/terminal";
import { useT } from "../lib/i18n";

/** The Terminal tab: which folder the shell runs in, and the terminal itself. */
export function TerminalPane({ sites }: { sites: Site[] }) {
  const t = useT();
  const box = useRef<HTMLDivElement>(null);
  const terminal = useTerminal();
  useEffect(() => (box.current ? attachTerminal(box.current) : undefined), []);

  return (
    <div className="terminal-pane">
      <div className="terminal-bar">
        <label htmlFor="terminal-folder" className="muted small">
          {t("Folder")}
        </label>
        <select
          id="terminal-folder"
          value={terminal.folder ?? ""}
          onChange={(e) => {
            const site = sites.find((s) => s.project.folder === e.target.value);
            openTerminal(site?.path ?? null, site?.domain ?? null);
          }}
        >
          <option value="">{t("Home folder")}</option>
          {sites.map((s) => (
            <option key={s.domain} value={s.project.folder}>
              {s.domain}
            </option>
          ))}
        </select>
        <button type="button" className="btn small" onClick={restartTerminal}>
          {t("Restart")}
        </button>
        {terminal.error && (
          <span className="error small" role="alert">
            {terminal.error}
          </span>
        )}
      </div>
      <div ref={box} className="terminal-box" aria-label={t("Terminal")} />
    </div>
  );
}
