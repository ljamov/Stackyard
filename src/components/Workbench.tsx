import { useEffect, useRef, type ReactNode } from "react";
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
import { CommandPalette } from "./CommandPalette";
import { Logo } from "./Logo";
import { api } from "../lib/api";
import { useLoad } from "../lib/useLoad";
import { useServices } from "../lib/useServices";
import { useModules } from "../lib/modules";
import { SCREENS, isTabScreen, useScreenOn, type TabScreen } from "../lib/screens";
import { closeTab, openTab, PANEL_HEIGHT, PANEL_WIDTH, setLayout, showScreen, SIDE_BAR_WIDTH, useLayout } from "../lib/layout";
import { useViewOptions } from "../lib/viewOptions";
import { CommandsProvider, useWorkbenchCommands } from "../lib/commands";
import { dispatchKeys, withKey } from "../lib/keybindings";
import { usePalette } from "../lib/palette";
import { useProblems } from "../lib/problems";
import { useUserFiles } from "../lib/useUserFiles";
import { ProblemsList } from "./ProblemsList";
import { useT } from "../lib/i18n";
import { rovingGroup, rovingKeys } from "../lib/focus";

const tabId = (s: TabScreen) => `tab-${s.replace(/\W+/g, "-")}`;

function screen(s: TabScreen): ReactNode {
  switch (s) {
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
 * a row of tabs once more than one is open), the panel with Problems and Logs,
 * and the status bar. Its layout is kept between runs (lib/layout.ts).
 */
export function Workbench() {
  const t = useT();
  const layout = useLayout();
  const view = useViewOptions();
  const panelRight = view.panelPosition === "right";
  const on = useScreenOn();
  const { modules, isOn } = useModules();
  const services = useServices();
  const sites = useLoad(api.listSites, []);
  const shownServices = services.services.filter((s) => isOn(s.id));
  const screens = SCREENS.filter(isTabScreen).filter(on);
  const logsOn = on("Logs");
  // Until the module switches load, every screen counts as off; keep the tabs.
  const loaded = modules.length > 0;
  const tabs = loaded ? layout.tabs.filter(on) : layout.tabs;
  const active = layout.active && tabs.includes(layout.active) ? layout.active : (tabs[0] ?? null);
  const panel = layout.panel;
  // Logs switched off: the panel shows Problems.
  const panelTab = layout.panelTab === "logs" && logsOn ? "logs" : "problems";
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
    tabs,
    active,
  });

  // Each key press runs the command bound to it (lib/keybindings.ts).
  const latest = useRef(commands);
  latest.current = commands;
  useEffect(() => {
    const chord = { pending: null as string | null };
    function key(e: KeyboardEvent) {
      const list = latest.current;
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

  return (
    <CommandsProvider value={commands}>
      <div className="workbench" data-side={view.sideBarPosition}>
        <Sidebar
          width={layout.sideBarWidth}
          collapsed={!layout.sideBar}
          screens={screens}
          active={active}
          sites={sites.data}
        />
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
        <div className="editor-column" data-panel={view.panelPosition}>
          <div className="editor" data-part="editor">
            {tabs.length > 1 && (
              <div
                ref={rovingGroup}
                className="tabs"
                role="tablist"
                aria-label={t("Open screens")}
                onKeyDown={(e) => {
                  rovingKeys(e, "horizontal");
                  const s = (e.target as HTMLElement).dataset.tab as TabScreen | undefined;
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
                  <div key={s} className="tab-item" data-active={s === active || undefined}>
                    <button
                      type="button"
                      role="tab"
                      id={tabId(s)}
                      className="tab-label"
                      data-roving
                      data-tab={s}
                      aria-selected={s === active}
                      aria-controls={s === active ? "editor-content" : undefined}
                      onClick={() => openTab(s)}
                      onAuxClick={(e) => e.button === 1 && closeTab(s)}
                    >
                      {t(s)}
                    </button>
                    <button
                      type="button"
                      className="icon-btn tab-close"
                      tabIndex={-1}
                      aria-label={t("Close {name}", { name: t(s) })}
                      title={withKey(t("Close {name}", { name: t(s) }), "tab.close")}
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
                role={tabs.length > 1 ? "tabpanel" : undefined}
                aria-labelledby={tabs.length > 1 ? tabId(active) : undefined}
                aria-label={tabs.length > 1 ? undefined : t(active)}
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
          {panel && (
            <>
              {panelRight ? (
                <Sash
                  orientation="vertical"
                  value={layout.panelWidth}
                  {...PANEL_WIDTH}
                  invert
                  onChange={(panelWidth) => setLayout({ panelWidth })}
                  label={t("Resize the panel")}
                />
              ) : (
                <Sash
                  orientation="horizontal"
                  value={layout.panelHeight}
                  {...PANEL_HEIGHT}
                  invert
                  onChange={(panelHeight) => setLayout({ panelHeight })}
                  label={t("Resize the panel")}
                />
              )}
              <section
                className="panel"
                style={panelRight ? { width: layout.panelWidth } : { height: layout.panelHeight }}
                aria-label={t("Panel")}
                data-part="panel"
              >
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
                  </div>
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
                <div id="panel-body" className="panel-body" role="tabpanel" aria-labelledby={`panel-tab-${panelTab}`}>
                  {panelTab === "logs" ? <Logs inPanel /> : <ProblemsList problems={problems} />}
                </div>
              </section>
            </>
          )}
        </div>
      </div>
      {view.statusBar && (
        <StatusBar
        shownServices={shownServices}
        siteCount={sites.data.length}
        logsOn={logsOn}
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
