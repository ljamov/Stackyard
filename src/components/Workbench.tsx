import { useEffect, useRef, type ReactNode } from "react";
import { ActivityBar } from "./ActivityBar";
import { Explorer } from "./Explorer";
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
import { closeTab, openTab, PANEL_HEIGHT, setLayout, SIDE_BAR_WIDTH, togglePanelTab, useLayout } from "../lib/layout";
import { CommandsProvider, useWorkbenchCommands } from "../lib/commands";
import { dispatchKeys, withKey } from "../lib/keybindings";
import { usePalette } from "../lib/palette";
import { useProblems } from "../lib/problems";
import { useUserFiles } from "../lib/useUserFiles";
import { ProblemsList } from "./ProblemsList";
import { useT } from "../lib/i18n";

function screen(s: TabScreen): ReactNode {
  switch (s) {
    case "Dashboard":
      return <Dashboard onNavigate={(to) => isTabScreen(to) && openTab(to)} />;
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
 * The app's window below the title bar: activity bar, Explorer side bar, the
 * open screens as tabs, the Logs panel and the status bar. Its layout is kept
 * between runs (lib/layout.ts).
 */
export function Workbench() {
  const t = useT();
  const layout = useLayout();
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

  const toggleSideBar = () => setLayout((l) => ({ sideBar: !l.sideBar }));
  const togglePanel = () => setLayout((l) => ({ panel: !l.panel }));

  const palette = usePalette();
  const { commands, places } = useWorkbenchCommands({
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
      <div className="workbench">
        <ActivityBar
          screens={screens.filter((s) => s !== "Keyboard shortcuts")}
          active={active}
          sideBar={layout.sideBar}
          panel={panel && panelTab === "logs"}
          logsOn={logsOn}
          onOpen={openTab}
          onToggleSideBar={toggleSideBar}
          onTogglePanel={() => togglePanelTab("logs")}
        />
        {layout.sideBar && (
          <>
            <Explorer
              width={layout.sideBarWidth}
              services={services}
              shownServices={shownServices}
              sites={sites.data}
              onOpen={openTab}
            />
            <Sash
              orientation="vertical"
              value={layout.sideBarWidth}
              {...SIDE_BAR_WIDTH}
              onChange={(sideBarWidth) => setLayout({ sideBarWidth })}
              label={t("Resize the side bar")}
            />
          </>
        )}
        <div className="editor-column">
          <div className="editor">
            {tabs.length > 0 && (
              <div className="tabs" role="tablist" aria-label={t("Open screens")}>
                {tabs.map((s) => (
                  <div key={s} className="tab-item" data-active={s === active || undefined}>
                    <button
                      type="button"
                      role="tab"
                      className="tab-label"
                      aria-selected={s === active}
                      onClick={() => openTab(s)}
                      onAuxClick={(e) => e.button === 1 && closeTab(s)}
                    >
                      {t(s)}
                    </button>
                    <button
                      type="button"
                      className="icon-btn tab-close"
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
              <main className="content" role="tabpanel" aria-label={t(active)}>
                <UpdateNotice />
                {screen(active)}
              </main>
            ) : (
              <main className="editor-empty">
                <Logo size={64} />
                <p>{t("Open a screen from the bar on the left.")}</p>
              </main>
            )}
          </div>
          {panel && (
            <>
              <Sash
                orientation="horizontal"
                value={layout.panelHeight}
                {...PANEL_HEIGHT}
                invert
                onChange={(panelHeight) => setLayout({ panelHeight })}
                label={t("Resize the panel")}
              />
              <section className="panel" style={{ height: layout.panelHeight }} aria-label={t("Panel")}>
                <div className="panel-header">
                  <div className="panel-tabs" role="tablist" aria-label={t("Panel")}>
                    <button
                      type="button"
                      role="tab"
                      className="panel-tab"
                      aria-selected={panelTab === "problems"}
                      onClick={() => setLayout({ panelTab: "problems" })}
                    >
                      {t("Problems")}
                      {problems.length > 0 && <span className="count">{problems.length}</span>}
                    </button>
                    {logsOn && (
                      <button
                        type="button"
                        role="tab"
                        className="panel-tab"
                        aria-selected={panelTab === "logs"}
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
                <div className="panel-body" role="tabpanel">
                  {panelTab === "logs" ? <Logs inPanel /> : <ProblemsList problems={problems} />}
                </div>
              </section>
            </>
          )}
        </div>
      </div>
      <StatusBar
        shownServices={shownServices}
        siteCount={sites.data.length}
        logsOn={logsOn}
        panel={panel}
        panelTab={panelTab}
        problemCount={problems.length}
        onOpen={openTab}
      />
      {palette && <CommandPalette mode={palette} commands={commands} places={places} />}
    </CommandsProvider>
  );
}
