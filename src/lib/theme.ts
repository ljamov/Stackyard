// Every color the interface uses, as named tokens. A theme gives each token a
// value; applyTheme() writes them as CSS variables on the page, and styles.css
// and the components use only those variables. This is the one file in src
// that holds color values (scripts/check-colors.mjs keeps it that way).

/** Token id → the CSS variable it is written to and what it colors. */
export const TOKENS = {
  "window.background": { css: "--bg", description: "Behind the screens' content" },
  "sideBar.background": { css: "--sidebar", description: "Title bar, sidebar, text fields and code blocks" },
  "card.background": { css: "--surface", description: "Cards and buttons" },
  "card.raisedBackground": { css: "--surface-2", description: "Selected items, notices and table headers" },
  "border": { css: "--border", description: "Card and row borders" },
  "border.strong": { css: "--border-strong", description: "Field, button and window borders" },
  "foreground": { css: "--text", description: "Main text" },
  "foreground.secondary": { css: "--text-2", description: "Labels and menu items" },
  "foreground.muted": { css: "--muted", description: "Hints and less important text" },
  "foreground.strong": { css: "--text-strong", description: "The selected menu item" },
  "accent": { css: "--accent", description: "Main buttons, switches and highlights" },
  "accent.soft": { css: "--accent-soft", description: "A lighter accent" },
  "accent.foreground": { css: "--on-accent", description: "Text on the accent color" },
  "textLink.foreground": { css: "--accent-ink", description: "Links and accent-colored text" },
  "focusBorder": { css: "--focus", description: "The outline around the focused control" },
  "status.ok": { css: "--ok", description: "Running, done" },
  "status.warning": { css: "--warn", description: "Starting, needs attention" },
  "status.error": { css: "--error", description: "Crashed, failed" },
  "status.idle": { css: "--idle", description: "Stopped" },
  "titleBar.closeHover.background": { css: "--close-hover", description: "The window's close button under the pointer" },
  "titleBar.closeHover.foreground": { css: "--on-close-hover", description: "Its × under the pointer" },
  "widget.shadow": { css: "--shadow", description: "Shadow under pop-up help" },
  "logo.primary": { css: "--logo-1", description: "The logo's outer bars" },
  "logo.secondary": { css: "--logo-2", description: "The logo's middle bar" },
  "activityBar.background": { css: "--activity-bg", description: "The bar of screen icons on the far left" },
  "tab.inactiveBackground": { css: "--tab-inactive", description: "The tab strip and the tabs not shown" },
  "statusBar.background": { css: "--statusbar", description: "The status bar along the bottom" },
} as const;

export type TokenId = keyof typeof TOKENS;
export type ThemeColors = Record<TokenId, string>;

export type ThemeId = "dark" | "light" | "high-contrast";
export type Accent = "amber" | "blue" | "green" | "violet" | "rose" | "teal";

/** An accent and its shades; `ink` is the darker one used for text on the light theme. */
type AccentColors = { accent: string; soft: string; on: string; ink: string };

export const ACCENT_COLORS: Record<Accent, AccentColors> = {
  amber: { accent: "#f2b035", soft: "#f7cd72", on: "#1a1300", ink: "#8a5a00" },
  blue: { accent: "#5aa9ff", soft: "#8cc4ff", on: "#00172e", ink: "#1a5fb4" },
  green: { accent: "#4fd18b", soft: "#86e3b0", on: "#00200f", ink: "#187040" },
  violet: { accent: "#b48cff", soft: "#ccb2ff", on: "#1a0939", ink: "#6a3cc4" },
  rose: { accent: "#ff7a9a", soft: "#ffa3b8", on: "#2d000f", ink: "#b0244c" },
  teal: { accent: "#3fd0c9", soft: "#7fe3de", on: "#00211f", ink: "#0f7570" },
};

type BaseColors = Omit<ThemeColors, "accent" | "accent.soft" | "accent.foreground" | "textLink.foreground" | "focusBorder">;

const LOGO = { "logo.primary": "#f2b035", "logo.secondary": "#f7cd72" };
const CLOSE = { "titleBar.closeHover.background": "#c42b1c", "titleBar.closeHover.foreground": "#ffffff" };

const BASE: Record<ThemeId, BaseColors> = {
  dark: {
    "window.background": "#111418",
    "sideBar.background": "#0b0d10",
    "card.background": "#181c22",
    "card.raisedBackground": "#1f242c",
    "border": "#262c35",
    "border.strong": "#333a45",
    "foreground": "#e8eaed",
    "foreground.secondary": "#b4bcc6",
    "foreground.muted": "#9aa3ae",
    "foreground.strong": "#ffffff",
    "status.ok": "#6ee7a0",
    "status.warning": "#f7cd72",
    "status.error": "#ff8a80",
    "status.idle": "#5b6470",
    "widget.shadow": "rgba(0, 0, 0, 0.35)",
    "activityBar.background": "#08090c",
    "tab.inactiveBackground": "#0b0d10",
    "statusBar.background": "#0b0d10",
    ...CLOSE,
    ...LOGO,
  },
  light: {
    "window.background": "#f5f6f8",
    "sideBar.background": "#eaecf0",
    "card.background": "#ffffff",
    "card.raisedBackground": "#e2e6eb",
    "border": "#dde1e6",
    "border.strong": "#c3c9d2",
    "foreground": "#171a1f",
    "foreground.secondary": "#3c434d",
    "foreground.muted": "#5d6671",
    "foreground.strong": "#000000",
    "status.ok": "#157a46",
    "status.warning": "#8a5a00",
    "status.error": "#c0282d",
    "status.idle": "#7d858f",
    "widget.shadow": "rgba(0, 0, 0, 0.12)",
    "activityBar.background": "#e2e6eb",
    "tab.inactiveBackground": "#eaecf0",
    "statusBar.background": "#e2e6eb",
    ...CLOSE,
    ...LOGO,
  },
  // Black and white with bright borders, so every edge and state can be seen
  // without relying on shades of grey.
  "high-contrast": {
    "window.background": "#000000",
    "sideBar.background": "#000000",
    "card.background": "#000000",
    "card.raisedBackground": "#1a1a1a",
    "border": "#6fc3df",
    "border.strong": "#6fc3df",
    "foreground": "#ffffff",
    "foreground.secondary": "#ffffff",
    "foreground.muted": "#d6d6d6",
    "foreground.strong": "#ffffff",
    "status.ok": "#3fe08a",
    "status.warning": "#ffd75e",
    "status.error": "#ff7b72",
    "status.idle": "#bdbdbd",
    "widget.shadow": "rgba(0, 0, 0, 0)",
    "activityBar.background": "#000000",
    "tab.inactiveBackground": "#000000",
    "statusBar.background": "#000000",
    ...CLOSE,
    ...LOGO,
  },
};

/** A theme's full set of colors with the chosen accent. */
export function themeColors(theme: ThemeId, accent: Accent): ThemeColors {
  const a = ACCENT_COLORS[accent];
  // Accent-colored text needs the darker shade on light backgrounds and the
  // lighter one on black.
  const ink = theme === "light" ? a.ink : theme === "high-contrast" ? a.soft : a.accent;
  return {
    ...BASE[theme],
    "accent": a.accent,
    "accent.soft": a.soft,
    "accent.foreground": a.on,
    "textLink.foreground": ink,
    "focusBorder": theme === "high-contrast" ? "#f38518" : ink,
  };
}

/** Writes a theme's colors as CSS variables on the page. */
export function applyTheme(theme: ThemeId, accent: Accent) {
  const root = document.documentElement;
  const colors = themeColors(theme, accent);
  for (const id of Object.keys(TOKENS) as TokenId[]) {
    root.style.setProperty(TOKENS[id].css, colors[id]);
  }
  root.style.colorScheme = theme === "light" ? "light" : "dark";
  root.dataset.theme = theme;
  root.dataset.accent = accent;
}
