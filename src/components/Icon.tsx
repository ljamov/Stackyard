// Icons for the workbench. The Stackyard set is line icons drawn here on a
// 24-unit grid in the text color; the Codicons set (Settings › Appearance ›
// Icons) is the icon font VS Code uses. Sizes come from the CSS around them.

import { useViewOptions } from "../lib/viewOptions";

const PATHS = {
  explorer: "M4 5.5h6l2 2h8v11H4zM4 9.5h16",
  dashboard: "M4 4h7v7H4zM13 4h7v4h-7zM13 10h7v10h-7zM4 13h7v7H4z",
  sites: "M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zM3 12h18M12 3c2.5 2.5 3.5 5.5 3.5 9s-1 6.5-3.5 9c-2.5-2.5-3.5-5.5-3.5-9s1-6.5 3.5-9z",
  services: "M4 4h16v6H4zM4 14h16v6H4zM7.5 7h.01M7.5 17h.01M11 7h6M11 17h6",
  runtimes: "M8.5 7 3.5 12l5 5M15.5 7l5 5-5 5M13.5 5l-3 14",
  databases:
    "M12 3c4.4 0 8 1.3 8 3s-3.6 3-8 3-8-1.3-8-3 3.6-3 8-3zM4 6v12c0 1.7 3.6 3 8 3s8-1.3 8-3V6M4 12c0 1.7 3.6 3 8 3s8-1.3 8-3",
  scheduler: "M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zM12 7v5l3.5 2",
  images: "M3.5 4.5h17v15h-17zM3.5 16l5-5 4 4 3-3 5 5M15.5 9.5h.01",
  videos: "M3.5 5.5h12v13h-12zM15.5 10l5-3v10l-5-3",
  ai: "M12 3l1.8 5.2L19 10l-5.2 1.8L12 17l-1.8-5.2L5 10l5.2-1.8zM18.5 15.5l.8 2.2 2.2.8-2.2.8-.8 2.2-.8-2.2-2.2-.8 2.2-.8z",
  mcp: "M9 3v5M15 3v5M6 8h12v3a6 6 0 0 1-12 0zM12 17v4",
  logs: "M4 4.5h16v15H4zM7.5 9l3 3-3 3M12.5 15h4",
  settings:
    "M19 10.1L21.5 10.5L21.5 13.5L19 13.9L18.2 15.6L19.8 17.6L17.6 19.8L15.6 18.2L13.9 19L13.5 21.5L10.5 21.5L10.1 19L8.4 18.2L6.4 19.8L4.2 17.6L5.8 15.6L5 13.9L2.5 13.5L2.5 10.5L5 10.1L5.8 8.4L4.2 6.4L6.4 4.2L8.4 5.8L10.1 5L10.5 2.5L13.5 2.5L13.9 5L15.6 5.8L17.6 4.2L19.8 6.4L18.2 8.4zM12 9.2a2.8 2.8 0 1 0 0 5.6 2.8 2.8 0 0 0 0-5.6z",
  sidebar: "M4 5h16v14H4zM9.5 5v14",
  plus: "M12 5v14M5 12h14",
  sun: "M12 8a4 4 0 1 0 0 8 4 4 0 0 0 0-8zM12 2.5v2M12 19.5v2M2.5 12h2M19.5 12h2M5.3 5.3l1.4 1.4M17.3 17.3l1.4 1.4M5.3 18.7l1.4-1.4M17.3 6.7l1.4-1.4",
  moon: "M19.5 14.5A8 8 0 0 1 9.5 4.5a8 8 0 1 0 10 10z",
  terminal: "M3.5 5h17v14h-17zM7 9.5l3 2.5-3 2.5M12.5 15h4.5",
  external: "M14 4.5h5.5V10M19.5 4.5 11 13M17.5 14v5.5h-13v-13H10",
  keyboard: "M3 6.5h18v11H3zM6.5 10h.01M10 10h.01M13.5 10h.01M17 10h.01M8 14h8",
  search: "M10.5 4a6.5 6.5 0 1 0 0 13 6.5 6.5 0 0 0 0-13zM15.5 15.5 20 20",
  play: "M8 5.5v13l10.5-6.5z",
  stop: "M7 7h10v10H7z",
  close: "M7 7l10 10M17 7 7 17",
  // The panel across the whole window, or only under the screen beside the side bar.
  panelWide: "M4 5h16v14H4zM4 14h16",
  panelNarrow: "M4 5h16v14H4zM9.5 5v14M9.5 14H20",
  chevron: "M9 6l6 6-6 6",
  // Site actions and the sections of a site's tab.
  folder: "M3.5 6.5a1 1 0 0 1 1-1h5l2 2h8a1 1 0 0 1 1 1v9a1 1 0 0 1-1 1h-15a1 1 0 0 1-1-1z",
  code: "M8.5 7 3.5 12l5 5M15.5 7l5 5-5 5",
  trash: "M4.5 6.5h15M9.5 6.5V4.5h5v2M6.5 6.5l1 13h9l1-13M10 10v6M14 10v6",
  camera: "M3.5 7.5h4l1.5-2h6l1.5 2h4v11h-17zM12 10a3 3 0 1 0 0 6 3 3 0 0 0 0-6z",
  manage: "M4 7h9M17 7h3M4 17h3M11 17h9M15 5v4M9 15v4",
  archive: "M3.5 4.5h17v4h-17zM5 8.5v11h14v-11M10 12h4",
  link: "M10 14a4 4 0 0 0 5.7 0l3-3a4 4 0 0 0-5.7-5.7l-1 1M14 10a4 4 0 0 0-5.7 0l-3 3a4 4 0 0 0 5.7 5.7l1-1",
  git: "M7 4v11M7 15a2.5 2.5 0 1 0 0 5 2.5 2.5 0 0 0 0-5zM17 4a2.5 2.5 0 1 0 0 5 2.5 2.5 0 0 0 0-5zM17 9c0 4-4 4-9 6.5",
} as const;

export type IconName = keyof typeof PATHS;

/** The same icons in the Codicons font. */
const CODICONS: Record<IconName, string> = {
  explorer: "files",
  dashboard: "dashboard",
  sites: "globe",
  services: "server",
  runtimes: "code",
  databases: "database",
  scheduler: "watch",
  images: "file-media",
  videos: "device-camera-video",
  ai: "sparkle",
  mcp: "plug",
  logs: "output",
  settings: "settings-gear",
  keyboard: "keyboard",
  search: "search",
  play: "play",
  stop: "debug-stop",
  close: "close",
  sidebar: "layout-sidebar-left",
  plus: "add",
  sun: "color-mode",
  moon: "color-mode",
  terminal: "terminal",
  external: "link-external",
  panelWide: "layout-panel-justify",
  panelNarrow: "layout-panel",
  chevron: "chevron-right",
  folder: "folder-opened",
  code: "code",
  trash: "trash",
  camera: "device-camera",
  manage: "settings",
  archive: "archive",
  link: "link",
  git: "source-control",
};

export function Icon({ name }: { name: IconName }) {
  const { iconTheme } = useViewOptions();
  if (iconTheme === "codicons") return <span className={`codicon codicon-${CODICONS[name]}`} aria-hidden="true" />;
  return (
    <svg viewBox="0 0 24 24" aria-hidden="true">
      <path d={PATHS[name]} />
    </svg>
  );
}
