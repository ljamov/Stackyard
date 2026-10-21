// Everything the app can do from the keyboard: each action is a command with an
// id, built once here. The palette lists them, keybindings.ts maps keys to
// their ids, and the Keyboard shortcuts screen lets people change those keys.

import { createContext, useContext, useMemo } from "react";
import { api, type Site } from "./api";
import { ACCENTS, endPreview, previewTheme, setAppearance, THEMES, useAddedThemes, useAppearance } from "./appearance";
import { importThemeFromFile } from "./themeImport";
import { currentViewOptions, DEFAULT_VIEW, DENSITIES, ICON_THEMES, setViewOptions, useViewOptions, zoomBy } from "./viewOptions";
import { closeTab, cycleTab, openTab, setLayout, showScreen, togglePanelTab } from "./layout";
import { openPalette } from "./palette";
import { focusPart } from "./focus";
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
  /** Shows what the command would do while the palette selects it (the theme picker). */
  preview?: () => void;
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
}): { commands: Command[]; places: Command[]; themes: Command[] } {
  const t = useT();
  const added = useAddedThemes();
  const { theme: shownTheme } = useAppearance();
  const options = useViewOptions();
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
      { id: "view.explorer", category: view, title: t("Show or hide the side bar"), run: () => setLayout((l) => ({ sideBar: !l.sideBar })) },
      {
        id: "view.statusBar",
        category: view,
        title: t("Show or hide the status bar"),
        run: () => setViewOptions({ statusBar: !currentViewOptions().statusBar }),
      },
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
      ...screens.map((s) => ({ id: `open.${s}`, category: open, title: t(s), run: () => showScreen(s) })),
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
      { id: "workbench.colorTheme", category: t("Theme"), title: t("Pick a color theme"), run: () => openPalette("themes") },
      ...THEMES.map((th) => ({ id: `theme.${th.id}`, category: t("Theme"), title: t(th.label), run: () => setAppearance({ theme: th.id }) })),
      ...added.map((th) => ({ id: `theme.${th.id}`, category: t("Theme"), title: th.name, run: () => setAppearance({ theme: th.id }) })),
      {
        id: "theme.import",
        category: t("Theme"),
        title: t("Import a VS Code theme…"),
        run: () => {
          openTab("Settings");
          void importThemeFromFile(t("Import a VS Code theme"));
        },
      },
      { id: "workbench.focusNextPart", category: view, title: t("Focus the next part of the window"), run: () => focusPart(1) },
      { id: "workbench.focusPreviousPart", category: view, title: t("Focus the previous part of the window"), run: () => focusPart(-1) },
      { id: "view.zoomIn", category: view, title: t("Zoom in"), run: () => zoomBy(1) },
      { id: "view.zoomOut", category: view, title: t("Zoom out"), run: () => zoomBy(-1) },
      { id: "view.zoomReset", category: view, title: t("Reset zoom"), run: () => setViewOptions({ zoom: DEFAULT_VIEW.zoom }) },
      {
        id: "view.sideBarPosition",
        category: view,
        title: options.sideBarPosition === "left" ? t("Move the side bar to the right") : t("Move the side bar to the left"),
        run: () => setViewOptions({ sideBarPosition: options.sideBarPosition === "left" ? "right" : "left" }),
      },
      {
        id: "view.panelPosition",
        category: view,
        title: options.panelPosition === "bottom" ? t("Move the panel to the right") : t("Move the panel to the bottom"),
        run: () => setViewOptions({ panelPosition: options.panelPosition === "bottom" ? "right" : "bottom" }),
      },
      ...DENSITIES.map((d) => ({ id: `density.${d.id}`, category: t("Density"), title: t(d.label), run: () => setViewOptions({ density: d.id }) })),
      ...ICON_THEMES.map((i) => ({ id: `icons.${i.id}`, category: t("Icons"), title: t(i.label), run: () => setViewOptions({ iconTheme: i.id }) })),
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
      ...screens.map((s) => ({ id: `place.screen.${s}`, category: open, title: t(s), run: () => showScreen(s) })),
    ];
    // The theme picker: the shown theme first, so Escape and Enter on it change nothing.
    const allThemes = [
      ...THEMES.map((th) => ({ id: th.id, title: t(th.label), detail: undefined as string | undefined })),
      ...added.map((th) => ({ id: th.id, title: th.name, detail: t("Added") })),
    ];
    const themes: Command[] = [
      ...allThemes.filter((th) => th.id === shownTheme),
      ...allThemes.filter((th) => th.id !== shownTheme),
    ].map((th) => ({
      id: `pick.theme.${th.id}`,
      title: th.title,
      detail: th.detail,
      preview: () => previewTheme(th.id),
      run: () => {
        endPreview();
        setAppearance({ theme: th.id });
      },
    }));
    return { commands, places, themes };
  }, [t, screens, services, shownServices, sites, logsOn, tabs, active, added, shownTheme, options]);
}

const CommandsContext = createContext<Command[]>([]);
export const CommandsProvider = CommandsContext.Provider;

/** Every command, for the Keyboard shortcuts screen. */
export function useCommandList(): Command[] {
  return useContext(CommandsContext);
}
