import { useState, type MouseEvent } from "react";
import { Icon, type IconName } from "./Icon";
import { Logo } from "./Logo";
import { BrandIcon } from "./BrandIcon";
import { categories, type Site } from "../lib/api";
import { siteBrand } from "../lib/brands";
import { openPalette } from "../lib/palette";
import { openTab, showNewSite, showScreen, siteTab, type TabId } from "../lib/layout";
import { useUpdate } from "../lib/updates";
import { useT } from "../lib/i18n";
import { withKey } from "../lib/keybindings";
import { rovingGroup, rovingKeys } from "../lib/focus";
import { type Screen, type TabScreen } from "../lib/screens";
import { CATEGORY_ICON } from "../lib/templates";

export const SCREEN_ICON: Record<Screen, IconName> = {
  Dashboard: "dashboard",
  Projects: "projects",
  Apps: "apps",
  Sites: "sites",
  "Import backup": "import",
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

/** How many apps and sites the side bar lists of each; their screens have them all. */
const SHOWN = 4;

/**
 * The side bar: the logo and search, New site, every screen by name, a few
 * sites to open in tabs of their own, and Settings at the bottom. A click shows a
 * screen in place of the shown one; Ctrl+click or a middle click opens it as
 * a tab beside it. The screens are one Tab stop; the arrow keys move between them.
 * Collapsed (Ctrl+B) it is a rail of the same icons, each named in its tooltip.
 */
export function Sidebar({
  width,
  collapsed,
  screens,
  active,
  sites,
}: {
  width: number;
  collapsed: boolean;
  screens: TabScreen[];
  active: TabId | null;
  sites: Site[];
}) {
  const t = useT();
  const { status: update } = useUpdate();
  // New project asks first what is being made: an app or a site.
  const [choosing, setChoosing] = useState(false);

  const open = (s: TabScreen, e: MouseEvent) => (e.ctrlKey || e.metaKey ? openTab(s) : showScreen(s));
  // Apps and Sites are the two kinds of Projects, listed under it.
  const sub = (s: TabScreen) => (s === "Apps" || s === "Sites") && screens.includes("Projects");
  const item = (s: TabScreen, extra?: string) => (
    <button
      key={s}
      type="button"
      className={sub(s) ? "side-item side-sub" : "side-item"}
      data-roving
      data-screen={s}
      aria-label={extra ? `${t(s)}: ${extra}` : collapsed ? t(s) : undefined}
      title={withKey(t(s), `open.${s}`)}
      aria-current={s === active ? "page" : undefined}
      onClick={(e) => open(s, e)}
      onAuxClick={(e) => e.button === 1 && openTab(s)}
    >
      <Icon name={SCREEN_ICON[s]} />
      <span className="side-label">{t(s)}</span>
      {extra && <span className="side-dot" aria-hidden="true" title={extra} />}
    </button>
  );

  return (
    <aside
      className="sidebar"
      data-collapsed={collapsed || undefined}
      style={collapsed ? undefined : { width }}
      aria-label={t("Side bar")}
      data-part="sidebar"
    >
      <div className="side-brand">
        <Logo size={22} />
        <span className="side-name side-label">Stackyard</span>
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
        <button
          type="button"
          className="side-new"
          aria-label={collapsed ? t("New project") : undefined}
          title={t("New project: an app or a site")}
          aria-expanded={choosing}
          aria-controls="side-new-choices"
          onClick={() => setChoosing(!choosing)}
        >
          <span className="side-plus" aria-hidden="true">
            <Icon name="plus" />
          </span>
          <span className="side-label">{t("New project")}</span>
        </button>
      )}
      {screens.includes("Sites") && choosing && (
        <div
          id="side-new-choices"
          className="side-new-choices"
          role="group"
          aria-label={t("Is it an app or a site?")}
          onKeyDown={(e) => e.key === "Escape" && setChoosing(false)}
        >
          {categories.map((c) => (
            <button
              key={c.id}
              type="button"
              className="side-item"
              title={t(c.hint)}
              aria-label={collapsed ? t("New {what}", { what: t(c.name) }) : undefined}
              onClick={() => {
                setChoosing(false);
                showNewSite(c.id);
              }}
            >
              <Icon name={CATEGORY_ICON[c.id]} />
              <span className="side-label">{t(c.name)}</span>
            </button>
          ))}
        </div>
      )}

      <nav ref={rovingGroup} className="side-nav" aria-label={t("Screens")} onKeyDown={(e) => rovingKeys(e, "vertical")}>
        {screens.filter((s) => s !== "Settings" && s !== "Keyboard shortcuts").map((s) => item(s))}
      </nav>

      {sites.length > 0 && (
        <section className="side-sites" aria-labelledby="side-sites-h">
          <h2 id="side-sites-h" className={collapsed ? "sr-only" : undefined}>
            {t("Your projects")}
          </h2>
          {categories.map((c) => {
            const mine = sites.filter((s) => s.project.category === c.id);
            if (mine.length === 0) return null;
            return (
              <div key={c.id} className="side-group" role="group" aria-labelledby={`side-${c.id}-h`}>
                <h3 id={`side-${c.id}-h`}>{t(c.plural)}</h3>
                {mine.slice(0, SHOWN).map((site) => (
                  <button
                    key={site.domain}
                    type="button"
                    className="side-site"
                    title={t("Open {name} in a tab", { name: site.address ? site.domain : site.name })}
                    aria-label={collapsed ? t("Open {name} in a tab", { name: site.address ? site.domain : site.name }) : undefined}
                    aria-current={active === siteTab(site.domain) ? "page" : undefined}
                    onClick={() => openTab(siteTab(site.domain))}
                  >
                    <BrandIcon id={siteBrand(site) ?? ""} fallback={CATEGORY_ICON[c.id]} />
                    <span className="side-label">{site.address ? site.domain : site.name}</span>
                  </button>
                ))}
                {mine.length > SHOWN && !collapsed && (
                  <button type="button" className="side-site muted" onClick={() => showScreen(c.id === "app" ? "Apps" : "Sites")}>
                    <span className="side-label">{t("All {count} {what}", { count: mine.length, what: t(c.plural).toLowerCase() })}</span>
                  </button>
                )}
              </div>
            );
          })}
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
