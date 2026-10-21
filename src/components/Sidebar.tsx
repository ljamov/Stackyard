import type { MouseEvent } from "react";
import { Icon, type IconName } from "./Icon";
import { Logo } from "./Logo";
import { BrandIcon } from "./BrandIcon";
import { api, type Site } from "../lib/api";
import { frameworkBrand } from "../lib/brands";
import { openPalette } from "../lib/palette";
import { openTab, showScreen } from "../lib/layout";
import { useUpdate } from "../lib/updates";
import { useT } from "../lib/i18n";
import { withKey } from "../lib/keybindings";
import { rovingGroup, rovingKeys } from "../lib/focus";
import type { Screen, TabScreen } from "../lib/screens";

export const SCREEN_ICON: Record<Screen, IconName> = {
  Dashboard: "dashboard",
  Sites: "sites",
  Services: "services",
  Runtimes: "runtimes",
  Logs: "logs",
  Databases: "databases",
  Scheduler: "scheduler",
  Images: "images",
  Videos: "videos",
  "AI tools": "ai",
  "MCP server": "mcp",
  Settings: "settings",
  "Keyboard shortcuts": "keyboard",
};

/** How many sites the side bar lists; the Sites screen has them all. */
const SITES_SHOWN = 5;

/**
 * The side bar: the logo and search, New site, every screen by name, a few
 * sites to open in the browser, and Settings at the bottom. A click shows a
 * screen in place of the shown one; Ctrl+click or a middle click opens it as
 * a tab beside it. The screens are one Tab stop; the arrow keys move between them.
 */
export function Sidebar({
  width,
  screens,
  active,
  sites,
}: {
  width: number;
  screens: TabScreen[];
  active: TabScreen | null;
  sites: Site[];
}) {
  const t = useT();
  const { status: update } = useUpdate();

  const open = (s: TabScreen, e: MouseEvent) => (e.ctrlKey || e.metaKey ? openTab(s) : showScreen(s));
  const item = (s: TabScreen, extra?: string) => (
    <button
      key={s}
      type="button"
      className="side-item"
      data-roving
      data-screen={s}
      aria-label={extra ? `${t(s)}: ${extra}` : undefined}
      title={withKey(t(s), `open.${s}`)}
      aria-current={s === active ? "page" : undefined}
      onClick={(e) => open(s, e)}
      onAuxClick={(e) => e.button === 1 && openTab(s)}
    >
      <Icon name={SCREEN_ICON[s]} />
      <span>{t(s)}</span>
      {extra && <span className="side-dot" aria-hidden="true" title={extra} />}
    </button>
  );

  return (
    <aside className="sidebar" style={{ width }} aria-label={t("Side bar")} data-part="sidebar">
      <div className="side-brand">
        <Logo size={22} />
        <span className="side-name">Stackyard</span>
        <button
          type="button"
          className="icon-btn"
          aria-label={t("Search sites and commands")}
          title={withKey(t("Search sites and commands"), "workbench.quickOpen")}
          onClick={() => openPalette("quick")}
        >
          <Icon name="search" />
        </button>
      </div>

      {screens.includes("Sites") && (
        <button type="button" className="side-new" onClick={() => showScreen("Sites")}>
          <span className="side-plus" aria-hidden="true">
            <Icon name="plus" />
          </span>
          {t("New site")}
        </button>
      )}

      <nav ref={rovingGroup} className="side-nav" aria-label={t("Screens")} onKeyDown={(e) => rovingKeys(e, "vertical")}>
        {screens.filter((s) => s !== "Settings" && s !== "Keyboard shortcuts").map((s) => item(s))}
      </nav>

      {sites.length > 0 && (
        <section className="side-sites" aria-labelledby="side-sites-h">
          <h2 id="side-sites-h">{t("Your sites")}</h2>
          {sites.slice(0, SITES_SHOWN).map((site) => (
            <button
              key={site.domain}
              type="button"
              className="side-site"
              title={t("Open {name} in your browser", { name: site.domain })}
              onClick={() => void api.openLocalPage(site.url)}
            >
              <BrandIcon id={frameworkBrand(site.framework) ?? ""} fallback="sites" />
              <span>{site.domain}</span>
            </button>
          ))}
        </section>
      )}

      {screens.includes("Settings") && (
        <div className="side-bottom">
          {item(
            "Settings",
            update?.available ? t("Stackyard {version} is available", { version: update.available.version }) : undefined,
          )}
        </div>
      )}
    </aside>
  );
}
