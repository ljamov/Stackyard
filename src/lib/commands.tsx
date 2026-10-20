// Everything the app can do from the keyboard: each action is a command with an
// id, built once here. The palette lists them, keybindings.ts maps keys to
// their ids, and the Keyboard shortcuts screen lets people change those keys.

import { createContext, useContext, useMemo } from "react";
import { api, type Site } from "./api";
import { ACCENTS, setAppearance, THEMES } from "./appearance";
import { closeTab, cycleTab, openTab, setLayout, togglePanelTab } from "./layout";
import { openPalette } from "./palette";
import { checkForUpdate } from "./updates";
import type { ServicesHook } from "./useServices";
import type { TabScreen } from "./screens";
import { useT } from "./i18n";

export type Command = {
  id: string;
  title: string;
  /** Shown before the title, e.g. "Services: Start Caddy". */
  category?: string;
  /** Shown after the title, dimmed. */
  detail?: string;
  run: () => void;
};

export function useWorkbenchCommands({
  screens,
  services,
  shownServices,
  sites,
  logsOn,
  tabs,
  active,
}: {
  screens: TabScreen[];
  services: ServicesHook;
  shownServices: ServicesHook["services"];
  sites: Site[];
  logsOn: boolean;
  tabs: TabScreen[];
  active: TabScreen | null;
}): { commands: Command[]; places: Command[] } {
  const t = useT();
  return useMemo(() => {
    const view = t("View");
    const tabsCat = t("Tabs");
    const open = t("Open");
    const svc = t("Services");
    const installed = shownServices.filter((s) => s.installed);
    const isRunning = (s: (typeof installed)[number]) => s.state === "running" || s.state === "restarting";

    const commands: Command[] = [
      { id: "workbench.commands", category: view, title: t("Show all commands"), run: () => openPalette("commands") },
      { id: "workbench.quickOpen", category: view, title: t("Go to a site or screen"), run: () => openPalette("quick") },
      { id: "view.explorer", category: view, title: t("Show or hide the Explorer"), run: () => setLayout((l) => ({ sideBar: !l.sideBar })) },
      { id: "view.panel", category: view, title: t("Show or hide the panel"), run: () => setLayout((l) => ({ panel: !l.panel })) },
      { id: "view.problems", category: view, title: t("Show problems"), run: () => togglePanelTab("problems") },
      ...(logsOn ? [{ id: "view.logs", category: view, title: t("Show or hide the Logs panel"), run: () => togglePanelTab("logs") }] : []),
      ...(active ? [{ id: "tab.close", category: tabsCat, title: t("Close the tab"), run: () => closeTab(active) }] : []),
      ...(tabs.length
        ? [{ id: "tab.closeAll", category: tabsCat, title: t("Close all tabs"), run: () => setLayout({ tabs: [], active: null }) }]
        : []),
      ...(tabs.length > 1
        ? [
            { id: "tab.next", category: tabsCat, title: t("Next tab"), run: () => cycleTab(1) },
            { id: "tab.previous", category: tabsCat, title: t("Previous tab"), run: () => cycleTab(-1) },
          ]
        : []),
      ...screens.map((s) => ({ id: `open.${s}`, category: open, title: t(s), run: () => openTab(s) })),
      ...installed.map((s) =>
        isRunning(s)
          ? { id: `service.stop.${s.id}`, category: svc, title: t("Stop {name}", { name: s.name }), run: () => services.stop(s.id) }
          : { id: `service.start.${s.id}`, category: svc, title: t("Start {name}", { name: s.name }), run: () => services.start(s.id) },
      ),
      ...(installed.some((s) => !isRunning(s))
        ? [
            {
              id: "services.startAll",
              category: svc,
              title: t("Start all services"),
              run: () => installed.filter((s) => !isRunning(s)).forEach((s) => services.start(s.id)),
            },
          ]
        : []),
      ...(installed.some(isRunning)
        ? [
            {
              id: "services.stopAll",
              category: svc,
              title: t("Stop all services"),
              run: () => installed.filter(isRunning).forEach((s) => services.stop(s.id)),
            },
          ]
        : []),
      ...sites.map((site) => ({
        id: `site.open.${site.domain}`,
        category: t("Sites"),
        title: t("Open {name} in your browser", { name: site.domain }),
        run: () => void api.openLocalPage(site.url),
      })),
      ...THEMES.map((th) => ({ id: `theme.${th.id}`, category: t("Theme"), title: t(th.label), run: () => setAppearance({ theme: th.id }) })),
      ...ACCENTS.map((a) => ({ id: `accent.${a.id}`, category: t("Accent color"), title: t(a.label), run: () => setAppearance({ accent: a.id }) })),
      { id: "updates.check", title: t("Check for updates"), run: () => void checkForUpdate() },
      {
        id: "files.settings",
        title: t("Show {file} in the file manager", { file: "settings.toml" }),
        run: () => void api.showInterfaceFile("settings"),
      },
      {
        id: "files.keybindings",
        title: t("Show {file} in the file manager", { file: "keybindings.toml" }),
        run: () => void api.showInterfaceFile("keybindings"),
      },
    ];

    const places: Command[] = [
      ...sites.map((site) => ({
        id: `place.site.${site.domain}`,
        category: t("Sites"),
        title: site.domain,
        detail: site.runtime,
        run: () => void api.openLocalPage(site.url),
      })),
      ...screens.map((s) => ({ id: `place.screen.${s}`, category: open, title: t(s), run: () => openTab(s) })),
    ];
    return { commands, places };
  }, [t, screens, services, shownServices, sites, logsOn, tabs, active]);
}

const CommandsContext = createContext<Command[]>([]);
export const CommandsProvider = CommandsContext.Provider;

/** Every command, for the Keyboard shortcuts screen. */
export function useCommandList(): Command[] {
  return useContext(CommandsContext);
}
