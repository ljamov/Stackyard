import { useModules } from "./modules";

export const SCREENS = [
  "Dashboard",
  "Projects",
  "Apps",
  "Sites",
  "Services",
  "Runtimes",
  "Logs",
  "Databases",
  "Scheduler",
  "Images",
  "Videos",
  "AI tools",
  "MCP server",
  "Settings",
  "Keyboard shortcuts",
] as const;

export type Screen = (typeof SCREENS)[number];

/** The screen that lists apps, and the one that lists sites. */
export const CATEGORY_SCREEN = { app: "Apps", site: "Sites" } as const;

/** Screens that open as tabs; Logs lives in the bottom panel instead. */
export type TabScreen = Exclude<Screen, "Logs">;

export const isTabScreen = (s: unknown): s is TabScreen => SCREENS.includes(s as Screen) && s !== "Logs";

/** Screens that belong to a module and disappear when it is switched off. */
const SCREEN_MODULE: Partial<Record<Screen, string>> = {
  Logs: "logs",
  Databases: "databases",
  Scheduler: "scheduler",
  Images: "images",
  Videos: "videos",
  "AI tools": "ai",
  "MCP server": "mcp",
};

/** Whether a screen's module is on (screens without a module always are). */
export function useScreenOn(): (s: Screen) => boolean {
  const { isOn } = useModules();
  return (s) => !SCREEN_MODULE[s] || isOn(SCREEN_MODULE[s]!);
}
