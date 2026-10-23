import { useEffect, useRef, useState, type PointerEvent, type ReactNode } from "react";
import { Sidebar } from "./Sidebar";
import { Icon } from "./Icon";
import { Sash } from "./Sash";
import { StatusBar } from "./StatusBar";
import { UpdateNotice } from "./UpdateNotice";
import { Dashboard } from "../screens/Dashboard";
import { Sites } from "../screens/Sites";
import { Runtimes } from "../screens/Runtimes";
import { Services } from "../screens/Services";
import { Settings } from "../screens/Settings";
import { McpServer } from "../screens/McpServer";
import { Databases } from "../screens/Databases";
import { Logs } from "../screens/Logs";
import { Scheduler } from "../screens/Scheduler";
import { Images } from "../screens/Images";
import { Videos } from "../screens/Videos";
import { AiTools } from "../screens/AiTools";
import { KeyboardShortcuts } from "../screens/KeyboardShortcuts";
import { ServicePage } from "../screens/ServicePage";
import { SitePage } from "../screens/SitePage";
import { BrandIcon } from "./BrandIcon";
import { frameworkBrand } from "../lib/brands";
import { CommandPalette } from "./CommandPalette";
import { Logo } from "./Logo";
import { api } from "../lib/api";
import { useLoad } from "../lib/useLoad";
import { useServices } from "../lib/useServices";
import { useModules } from "../lib/modules";
import { SCREENS, isTabScreen, useScreenOn, type TabScreen } from "../lib/screens";
import { closeTab, moveTab, openTab, PANEL_HEIGHT, setLayout, showScreen, SIDE_BAR_WIDTH, useLayout, type TabId } from "../lib/layout";
import { useViewOptions } from "../lib/viewOptions";
import { CommandsProvider, useWorkbenchCommands } from "../lib/commands";
import { dispatchKeys, withKey } from "../lib/keybindings";
import { usePalette } from "../lib/palette";
import { useProblems } from "../lib/problems";
import { useUserFiles } from "../lib/useUserFiles";
import { ProblemsList } from "./ProblemsList";
import { TerminalPane } from "./TerminalPane";
import { TERMINAL_COMMANDS } from "../lib/terminal";
import { useT } from "../lib/i18n";
import { rovingGroup, rovingKeys } from "../lib/focus";

const tabId = (s: TabId) => `tab-${s.replace(/\W+/g, "-")}`;

function screen(s: TabId): ReactNode {
  if (s.startsWith("service:")) return <ServicePage key={s} id={s.slice("service:".length)} />;
  if (s.startsWith("site:")) return <SitePage key={s} domain={s.slice("site:".length)} />;
  switch (s as TabScreen) {
    case "Dashboard":
      return <Dashboard onNavigate={(to) => isTabScreen(to) && showScreen(to)} />;
    case "Sites":
      return <Sites />;
    case "Services":
      return <Services />;
    case "Runtimes":
      return <Runtimes />;
    case "Databases":
      return <Databases />;
    case "Scheduler":
      return <Scheduler />;
    case "Images":
      return <Images />;
    case "Videos":
      return <Videos />;
    case "AI tools":
      return <AiTools />;
    case "MCP server":
      return <McpServer />;
    case "Settings":
      return <Settings />;
    case "Keyboard shortcuts":
      return <KeyboardShortcuts />;
  }
}

/**
 * The app's window below the title bar: the side bar, the shown screen (with
 * a row of tabs once more than one is open), the panel with Problems, Logs and
 * Terminal across the whole width under both, and the status bar. Its layout is kept between runs (lib/layout.ts).
 */
export function Workbench() {
  const t = useT();
  const layout = useLayout();
  const view = useViewOptions();
  const on = useScreenOn();
  const { modules, isOn } = useModules();
  const services = useServices();
  const sites = useLoad(api.listSites, []);
  const shownServices = services.services.filter((s) => isOn(s.id));
  const screens = SCREENS.filter(isTabScreen).filter(on);
  const logsOn = on("Logs");
  const terminalOn = isOn("terminal");
  // Until the module switches load, every screen counts as off; keep the tabs.
  const loaded = modules.length > 0;
  const tabOn = (id: TabId) =>
    isTabScreen(id) ? on(id) : id.startsWith("service:") ? isOn(id.slice("service:".length)) : true;
  const tabs = loaded ? layout.tabs.filter(tabOn) : layout.tabs;
  // A service or site tab always shows the row of tabs, so it can be closed.
  const showTabs = tabs.length > 1 || tabs.some((x) => !isTabScreen(x));
  /** A tab's name: the screen's, the service's or the site's. */
  const tabTitle = (id: TabId): string =>
    id.startsWith("service:")
      ? (services.services.find((x) => `service:${x.id}` === id)?.name ?? id.slice("service:".length))
      : id.startsWith("site:")
        ? id.slice("site:".length)
        : t(id);
  /** The logo before a service's or site's tab name. */
  const tabIcon = (id: TabId): ReactNode => {
    if (id.startsWith("service:")) return <BrandIcon id={id.slice("service:".length)} />;
    if (!id.startsWith("site:")) return null;
    const site = sites.data.find((x) => `site:${x.domain}` === id);
    return <BrandIcon id={frameworkBrand(site?.framework ?? null) ?? ""} fallback="sites" />;
  };
  const active = layout.active && tabs.includes(layout.active) ? layout.active : (tabs[0] ?? null);
  const panel = layout.panel;
  // Logs or Terminal switched off: the panel shows Problems.
  const panelTab =
    (layout.panelTab === "logs" && logsOn) || (layout.panelTab === "terminal" && terminalOn) ? layout.panelTab : "problems";
  const problems = useProblems();
  useUserFiles();

  useEffect(() => {
    let off: (() => void) | undefined;
    let cancelled = false;
    api.onSitesChanged(sites.reload).then((u) => (cancelled ? u() : (off = u)));
    return () => {
      cancelled = true;
      off?.();
    };
  }, [sites.reload]);

  const togglePanel = () => setLayout((l) => ({ panel: !l.panel }));

  const palette = usePalette();
  const { commands, places, themes } = useWorkbenchCommands({
    screens,
    services,
    shownServices,
    sites: sites.data,
    logsOn,
    terminalOn,
    tabs,
    active,
  });

  // Each key press runs the command bound to it (lib/keybindings.ts).
  const latest = useRef(commands);
  latest.current = commands;
  useEffect(() => {
    const chord = { pending: null as string | null };
    function key(e: KeyboardEvent) {
      // Typing in the terminal: most keys belong to the shell.
      const inTerminal = e.target instanceof Element && e.target.closest(".terminal-host");
      const list = inTerminal ? latest.current.filter((c) => TERMINAL_COMMANDS.includes(c.id)) : latest.current;
      dispatchKeys(
        e,
        list.map((c) => c.id),
        (id) => list.find((c) => c.id === id)?.run(),
        chord,
      );
    }
    window.addEventListener("keydown", key);
    return () => window.removeEventListener("keydown", key);
  }, []);

  // Dragging a tab along the row moves it there; a short press is still a click.
  const drag = useRef<{ tab: TabId; x: number; moved: boolean } | null>(null);
  const dragged = useRef(false);
  const [dragging, setDragging] = useState<TabId | null>(null);
  function dragStart(e: PointerEvent<HTMLElement>, tab: TabId) {
    if (e.button !== 0) return;
    drag.current = { tab, x: e.clientX, moved: false };
    dragged.current = false;
  }
  function dragMove(e: PointerEvent<HTMLDivElement>) {
    const d = drag.current;
    if (!d) return;
    if (!d.moved) {
      if (Math.abs(e.clientX - d.x) < 6) return;
      d.moved = true;
      e.currentTarget.setPointerCapture(e.pointerId);
      setDragging(d.tab);
      openTab(d.tab);
    }
    const items = [...e.currentTarget.querySelectorAll<HTMLElement>("[data-tab-item]")];
    const from = items.findIndex((el) => el.dataset.tabItem === d.tab);
    items.forEach((el, i) => {
      const box = el.getBoundingClientRect();
      const middle = box.left + box.width / 2;
      // Past the middle of the tab beside it: the two swap places.
      if ((i > from && e.clientX > middle && e.clientX < box.right) || (i < from && e.clientX < middle && e.clientX > box.left))
        moveTab(d.tab, el.dataset.tabItem as TabId);
    });
    // Near either end of a row that scrolls, it scrolls along.
    const row = e.currentTarget.getBoundingClientRect();
    if (e.clientX < row.left + 24) e.currentTarget.scrollLeft -= 12;
    else if (e.clientX > row.right - 24) e.currentTarget.scrollLeft += 12;
  }
  function dragEnd() {
    dragged.current = drag.current?.moved ?? false;
    drag.current = null;
    setDragging(null);
    // The click that ends a drag doesn't open anything.
    setTimeout(() => (dragged.current = false));
  }

  // Problems, Logs and Terminal: under the screen beside the side bar, or across the whole window.
  const panelPart = panel && (
    <>
      <Sash
        orientation="horizontal"
        value={layout.panelHeight}
        {...PANEL_HEIGHT}
        invert
        onChange={(panelHeight) => setLayout({ panelHeight })}
        label={t("Resize the panel")}
      />
      <section className="panel" style={{ height: layout.panelHeight }} aria-label={t("Panel")} data-part="panel">
        <div className="panel-header">
          <div
            ref={rovingGroup}
            className="panel-tabs"
            role="tablist"
            aria-label={t("Panel")}
            onKeyDown={(e) => rovingKeys(e, "horizontal")}
          >
            <button
              type="button"
              role="tab"
              id="panel-tab-problems"
              className="panel-tab"
              data-roving
              aria-selected={panelTab === "problems"}
              aria-controls="panel-body"
              onClick={() => setLayout({ panelTab: "problems" })}
            >
              {t("Problems")}
              {problems.length > 0 && <span className="count">{problems.length}</span>}
            </button>
            {logsOn && (
              <button
                type="button"
                role="tab"
                id="panel-tab-logs"
                className="panel-tab"
                data-roving
                aria-selected={panelTab === "logs"}
                aria-controls="panel-body"
                onClick={() => setLayout({ panelTab: "logs" })}
              >
                {t("Logs")}
              </button>
            )}
            {terminalOn && (
              <button
                type="button"
                role="tab"
                id="panel-tab-terminal"
                className="panel-tab"
                data-roving
                aria-selected={panelTab === "terminal"}
                aria-controls="panel-body"
                onClick={() => setLayout({ panelTab: "terminal" })}
              >
                {t("Terminal")}
              </button>
            )}
          </div>
          <div className="panel-actions">
            <button
              type="button"
              className="icon-btn"
              aria-label={layout.panelWide ? t("Panel under the screen only") : t("Panel across the whole window")}
              title={layout.panelWide ? t("Panel under the screen only") : t("Panel across the whole window")}
              aria-pressed={layout.panelWide}
              onClick={() => setLayout((l) => ({ panelWide: !l.panelWide }))}
            >
              <Icon name={layout.panelWide ? "panelNarrow" : "panelWide"} />
            </button>
            <button
              type="button"
              className="icon-btn"
              aria-label={t("Hide the panel")}
              title={withKey(t("Hide the panel"), "view.panel")}
              onClick={togglePanel}
            >
              <Icon name="close" />
            </button>
          </div>
        </div>
        <div
          id="panel-body"
          className={panelTab === "terminal" ? "panel-body flush" : "panel-body"}
          role="tabpanel"
          aria-labelledby={`panel-tab-${panelTab}`}
        >
          {panelTab === "logs" ? (
            <Logs inPanel />
          ) : panelTab === "terminal" ? (
            <TerminalPane sites={sites.data} />
          ) : (
            <ProblemsList problems={problems} />
          )}
        </div>
      </section>
    </>
  );

  return (
    <CommandsProvider value={commands}>
      <div className="workbench-area">
        <div className="workbench" data-side={view.sideBarPosition}>
          <Sidebar width={layout.sideBarWidth} collapsed={!layout.sideBar} screens={screens} active={active} sites={sites.data} />
          {layout.sideBar && (
            <>
              <Sash
                orientation="vertical"
                value={layout.sideBarWidth}
                {...SIDE_BAR_WIDTH}
                invert={view.sideBarPosition === "right"}
                onChange={(sideBarWidth) => setLayout({ sideBarWidth })}
                label={t("Resize the side bar")}
              />
            </>
          )}
          <div className="editor-column">
            <div className="editor" data-part="editor">
              {showTabs && (
                <div
                  ref={rovingGroup}
                  className="tabs"
                  role="tablist"
                  aria-label={t("Open screens")}
                  data-dragging={dragging ? true : undefined}
                  onPointerMove={dragMove}
                  onPointerUp={dragEnd}
                  onPointerCancel={dragEnd}
                  onKeyDown={(e) => {
                    const s = (e.target as HTMLElement).dataset.tab as TabId | undefined;
                    // Ctrl+Shift+Left/Right moves the focused tab.
                    if (s && e.ctrlKey && e.shiftKey && (e.key === "ArrowLeft" || e.key === "ArrowRight")) {
                      e.preventDefault();
                      const next = tabs[tabs.indexOf(s) + (e.key === "ArrowLeft" ? -1 : 1)];
                      if (next) moveTab(s, next);
                      return;
                    }
                    rovingKeys(e, "horizontal");
                    if (s && e.key === "Delete") {
                      e.preventDefault();
                      closeTab(s);
                      // Focus stays in the tabs, on the one shown next.
                      requestAnimationFrame(() =>
                        document.querySelector<HTMLElement>('.tabs [data-roving][tabindex="0"], .editor-empty')?.focus(),
                      );
                    }
                  }}
                >
                  {tabs.map((s) => (
                    <div
                      key={s}
                      className="tab-item"
                      data-tab-item={s}
                      data-active={s === active || undefined}
                      data-dragged={s === dragging || undefined}
                    >
                      <button
                        type="button"
                        role="tab"
                        id={tabId(s)}
                        className="tab-label"
                        data-roving
                        data-tab={s}
                        aria-selected={s === active}
                        aria-controls={s === active ? "editor-content" : undefined}
                        onPointerDown={(e) => dragStart(e, s)}
                        onClick={() => !dragged.current && openTab(s)}
                        onAuxClick={(e) => e.button === 1 && closeTab(s)}
                      >
                        {tabIcon(s)}
                        {tabTitle(s)}
                      </button>
                      <button
                        type="button"
                        className="icon-btn tab-close"
                        tabIndex={-1}
                        aria-label={t("Close {name}", { name: tabTitle(s) })}
                        title={withKey(t("Close {name}", { name: tabTitle(s) }), "tab.close")}
                        onClick={() => closeTab(s)}
                      >
                        <Icon name="close" />
                      </button>
                    </div>
                  ))}
                </div>
              )}
              {active ? (
                <main
                  id="editor-content"
                  className="content"
                  role={showTabs ? "tabpanel" : undefined}
                  aria-labelledby={showTabs ? tabId(active) : undefined}
                  aria-label={showTabs ? undefined : tabTitle(active)}
                  tabIndex={-1}
                >
                  <UpdateNotice />
                  {screen(active)}
                </main>
              ) : (
                <main className="editor-empty" tabIndex={-1}>
                  <Logo size={64} />
                  <p>{t("Open a screen from the side bar.")}</p>
                </main>
              )}
            </div>
            {!layout.panelWide && panelPart}
          </div>
        </div>
        {layout.panelWide && panelPart}
      </div>
      {view.statusBar && (
        <StatusBar
          shownServices={shownServices}
          siteCount={sites.data.length}
          logsOn={logsOn}
          terminalOn={terminalOn}
          panel={panel}
          panelTab={panelTab}
          problemCount={problems.length}
          onOpen={showScreen}
        />
      )}
      {palette && <CommandPalette mode={palette} commands={commands} places={places} themes={themes} />}
    </CommandsProvider>
  );
}
