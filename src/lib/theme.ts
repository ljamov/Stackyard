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
  "statusBar.foreground": { css: "--statusbar-fg", description: "Words in the status bar" },
  "activityBar.foreground": { css: "--activity-fg", description: "The shown screen's icon in the activity bar" },
  "activityBar.inactiveForeground": { css: "--activity-inactive-fg", description: "The other icons in the activity bar" },
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
    "statusBar.foreground": "#b4bcc6",
    "activityBar.foreground": "#ffffff",
    "activityBar.inactiveForeground": "#9aa3ae",
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
    "statusBar.foreground": "#3c434d",
    "activityBar.foreground": "#000000",
    "activityBar.inactiveForeground": "#5d6671",
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
    "statusBar.foreground": "#ffffff",
    "activityBar.foreground": "#ffffff",
    "activityBar.inactiveForeground": "#d6d6d6",
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
  applyColors(themeColors(theme, accent), theme, accent);
}

/**
 * Writes a full set of colors. `kind` is the built-in theme they are closest
 * to: it sets the page's light or dark scheme and the high-contrast extras in
 * styles.css.
 */
export function applyColors(colors: ThemeColors, kind: ThemeId, accent: string) {
  const root = document.documentElement;
  for (const id of Object.keys(TOKENS) as TokenId[]) {
    root.style.setProperty(TOKENS[id].css, colors[id]);
  }
  root.style.colorScheme = kind === "light" ? "light" : "dark";
  root.dataset.theme = kind;
  root.dataset.accent = accent;
}

/**
 * A theme added from a file (stackyard-core themes.rs). Its colors use VS
 * Code's keys, so a VS Code theme works as it is; Stackyard's own token ids
 * work too and win over the VS Code keys.
 */
export type AddedTheme = { id: string; name: string; kind: ThemeId; colors: Record<string, string> };

/** Where each token looks in a VS Code theme; the first key the theme has wins. */
const VSCODE_KEYS: Record<TokenId, string[]> = {
  "window.background": ["editor.background"],
  "sideBar.background": ["sideBar.background", "editorGroupHeader.tabsBackground", "panel.background"],
  "card.background": ["editorWidget.background", "sideBarSectionHeader.background", "input.background"],
  "card.raisedBackground": ["list.inactiveSelectionBackground", "list.hoverBackground", "editor.lineHighlightBackground"],
  "border": ["panel.border", "editorGroup.border", "sideBar.border", "contrastBorder"],
  "border.strong": ["input.border", "dropdown.border", "widget.border", "contrastBorder"],
  "foreground": ["editor.foreground", "foreground"],
  "foreground.secondary": ["sideBar.foreground", "foreground"],
  "foreground.muted": ["descriptionForeground", "tab.inactiveForeground"],
  "foreground.strong": ["list.activeSelectionForeground", "tab.activeForeground", "editor.foreground"],
  "accent": ["button.background", "activityBarBadge.background", "focusBorder"],
  "accent.soft": ["button.hoverBackground"],
  "accent.foreground": ["button.foreground", "activityBarBadge.foreground"],
  "textLink.foreground": ["textLink.foreground"],
  "focusBorder": ["focusBorder"],
  "status.ok": ["testing.iconPassed", "terminal.ansiGreen", "gitDecoration.addedResourceForeground"],
  "status.warning": ["editorWarning.foreground", "list.warningForeground", "terminal.ansiYellow"],
  "status.error": ["editorError.foreground", "errorForeground", "terminal.ansiRed"],
  "status.idle": ["disabledForeground", "descriptionForeground"],
  "titleBar.closeHover.background": [],
  "titleBar.closeHover.foreground": [],
  "widget.shadow": ["widget.shadow"],
  "logo.primary": [],
  "logo.secondary": [],
  "activityBar.background": ["activityBar.background"],
  "tab.inactiveBackground": ["tab.inactiveBackground", "editorGroupHeader.tabsBackground"],
  "statusBar.background": ["statusBar.background"],
  "statusBar.foreground": ["statusBar.foreground"],
  "activityBar.foreground": ["activityBar.foreground"],
  "activityBar.inactiveForeground": ["activityBar.inactiveForeground"],
};

type Rgba = [number, number, number, number];

function parse(color: string): Rgba | null {
  const m = /^#([0-9a-f]{3,8})$/i.exec(color.trim());
  if (!m || ![3, 4, 6, 8].includes(m[1].length)) return null;
  const h = m[1].length <= 4 ? [...m[1]].map((c) => c + c).join("") : m[1];
  const n = (i: number) => parseInt(h.slice(i, i + 2), 16);
  return [n(0), n(2), n(4), h.length === 8 ? n(6) / 255 : 1];
}

function hex([r, g, b]: Rgba): string {
  return "#" + [r, g, b].map((v) => Math.round(v).toString(16).padStart(2, "0")).join("");
}

/** `a` laid over `b` with `a`'s own transparency, or mixed at `amount` of `a`. */
function mix(a: Rgba, b: Rgba, amount = a[3]): Rgba {
  return [0, 1, 2].map((i) => a[i] * amount + b[i] * (1 - amount)).concat(1) as Rgba;
}

/** WCAG relative luminance, 0 (black) to 1 (white). */
export function luminance(color: string): number {
  const c = parse(color);
  if (!c) return 0;
  const [r, g, b] = c.slice(0, 3).map((v) => {
    const s = v / 255;
    return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

/** WCAG contrast ratio between two colors, 1 to 21. */
export function contrast(a: string, b: string): number {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
}

const WHITE: Rgba = [255, 255, 255, 1];
const BLACK: Rgba = [0, 0, 0, 1];

/**
 * An added theme's full set of colors. What it leaves out is worked out from
 * its own background and text, or taken from the built-in theme of its kind,
 * so a theme with only a handful of colors still looks whole.
 */
export function addedThemeColors(theme: AddedTheme, accent: Accent): ThemeColors {
  const base = themeColors(theme.kind, accent);
  const find = (id: TokenId): Rgba | null => {
    for (const key of [id, ...VSCODE_KEYS[id]]) {
      const c = theme.colors[key] && parse(theme.colors[key]);
      if (c) return c;
    }
    return null;
  };
  const bg = find("window.background") ?? parse(base["window.background"])!;
  const solidBg: Rgba = mix(bg, parse(base["window.background"])!);
  const fg = mix(find("foreground") ?? parse(base["foreground"])!, solidBg);
  // Transparent colors are laid over the background, as VS Code draws them.
  const get = (id: TokenId, fallback: () => Rgba): string => {
    const c = find(id);
    return hex(c ? mix(c, solidBg) : fallback());
  };
  const keep = (id: TokenId) => () => parse(base[id])!;
  const between = (amount: number) => () => mix(fg, solidBg, amount);

  const colors = { ...base } as ThemeColors;
  colors["window.background"] = hex(solidBg);
  colors["foreground"] = hex(fg);
  colors["sideBar.background"] = get("sideBar.background", () => solidBg);
  colors["card.background"] = get("card.background", between(0.04));
  colors["card.raisedBackground"] = get("card.raisedBackground", between(0.1));
  colors["border"] = get("border", between(0.14));
  colors["border.strong"] = get("border.strong", between(0.26));
  colors["foreground.secondary"] = get("foreground.secondary", between(0.82));
  colors["foreground.muted"] = get("foreground.muted", between(0.62));
  colors["foreground.strong"] = get("foreground.strong", () => fg);
  const accentColor = find("accent") ? mix(find("accent")!, solidBg) : parse(base["accent"])!;
  colors["accent"] = hex(accentColor);
  colors["accent.soft"] = get("accent.soft", () => mix(accentColor, fg, 0.7));
  const onAccent = find("accent.foreground");
  colors["accent.foreground"] =
    onAccent && contrast(hex(mix(onAccent, accentColor)), colors["accent"]) >= 3
      ? hex(mix(onAccent, accentColor))
      : contrast("#ffffff", colors["accent"]) >= contrast("#000000", colors["accent"])
        ? hex(WHITE)
        : hex(BLACK);
  colors["textLink.foreground"] = get("textLink.foreground", () => accentColor);
  colors["focusBorder"] = get("focusBorder", () => accentColor);
  for (const id of ["status.ok", "status.warning", "status.error", "status.idle"] as const) {
    colors[id] = get(id, keep(id));
  }
  const shadow = find("widget.shadow");
  if (shadow) colors["widget.shadow"] = `rgba(${shadow.slice(0, 3).join(", ")}, ${shadow[3].toFixed(2)})`;
  colors["activityBar.background"] = get("activityBar.background", () => parse(colors["sideBar.background"])!);
  colors["tab.inactiveBackground"] = get("tab.inactiveBackground", () => parse(colors["sideBar.background"])!);
  colors["statusBar.background"] = get("statusBar.background", () => parse(colors["sideBar.background"])!);
  // Words on the bars must stay readable on whatever color the theme gives them.
  const readable = (id: TokenId, on: string, fallback: string) => {
    const c = find(id);
    const value = c ? hex(mix(c, parse(on)!)) : fallback;
    return contrast(value, on) >= 3 ? value : contrast("#ffffff", on) > contrast("#000000", on) ? "#ffffff" : "#000000";
  };
  colors["statusBar.foreground"] = readable("statusBar.foreground", colors["statusBar.background"], colors["foreground.secondary"]);
  colors["activityBar.foreground"] = readable("activityBar.foreground", colors["activityBar.background"], colors["foreground.strong"]);
  colors["activityBar.inactiveForeground"] = readable(
    "activityBar.inactiveForeground",
    colors["activityBar.background"],
    colors["foreground.muted"],
  );
  return colors;
}
