import { useEffect, useState } from "react";
import { isTauri } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useT } from "../lib/i18n";
import { Logo } from "./Logo";
import { Icon } from "./Icon";
import { openPalette } from "../lib/palette";
import { withKey } from "../lib/keybindings";

/**
 * Stackyard draws its own title bar: the window has no system frame so it
 * can have rounded corners. Drag the bar to move the window, double-click
 * it to maximise.
 */
export function TitleBar({ onMaximized, search = false }: { onMaximized(maximized: boolean): void; search?: boolean }) {
  const t = useT();
  const [maximized, setMaximized] = useState(false);
  const inApp = isTauri();

  useEffect(() => {
    if (!inApp) return;
    const win = getCurrentWindow();
    let off: (() => void) | undefined;
    let cancelled = false;
    const check = () =>
      win
        .isMaximized()
        .then((m) => {
          setMaximized(m);
          onMaximized(m);
        })
        .catch(() => {});
    check();
    win.onResized(check).then((unlisten) => (cancelled ? unlisten() : (off = unlisten)));
    return () => {
      cancelled = true;
      off?.();
    };
  }, [inApp, onMaximized]);

  const win = () => getCurrentWindow();
  return (
    <header className="titlebar" data-tauri-drag-region>
      <span className="titlebar-name" data-tauri-drag-region>
        <Logo size={16} />
        Stackyard
      </span>
      {search && (
        <button
          type="button"
          className="command-center"
          title={withKey(t("Search sites and commands"), "workbench.quickOpen")}
          onClick={() => openPalette("quick")}
        >
          <Icon name="search" />
          <span>{t("Search sites and commands")}</span>
        </button>
      )}
      <span className="titlebar-fill" data-tauri-drag-region />
      {inApp && (
        <div className="titlebar-buttons">
          <button type="button" className="titlebar-btn" aria-label={t("Minimise")} onClick={() => win().minimize()}>
            <svg viewBox="0 0 12 12" aria-hidden="true">
              <path d="M2 6h8" />
            </svg>
          </button>
          <button
            type="button"
            className="titlebar-btn"
            aria-label={maximized ? t("Restore") : t("Maximise")}
            onClick={() => win().toggleMaximize()}
          >
            <svg viewBox="0 0 12 12" aria-hidden="true">
              {maximized ? (
                <path d="M3.5 4.5h4v4h-4zM4.5 4.5v-1h4v4h-1" />
              ) : (
                <rect x="2.5" y="2.5" width="7" height="7" rx="1" />
              )}
            </svg>
          </button>
          <button type="button" className="titlebar-btn close" aria-label={t("Close")} onClick={() => win().close()}>
            <svg viewBox="0 0 12 12" aria-hidden="true">
              <path d="M3 3l6 6M9 3l-6 6" />
            </svg>
          </button>
        </div>
      )}
    </header>
  );
}
