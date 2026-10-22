// Every color the interface uses, as named tokens. A theme gives each token a
// value; applyTheme() writes them as CSS variables on the page, and styles.css
// and the components use only those variables. This is the one file in src
// that holds color values (scripts/check-colors.mjs keeps it that way).

/** Token id → the CSS variable it is written to and what it colors. */
export const TOKENS = {
  "window.background": { css: "--bg", description: "Behind the screens' content" },
  "sideBar.background": { css: "--sidebar", description: "Title bar, sidebar and status bar" },
  "input.background": { css: "--input", description: "Text fields, code blocks and keys" },
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
  "tab.inactiveBackground": { css: "--tab-inactive", description: "The tab strip and the tabs not shown" },
  "statusBar.background": { css: "--statusbar", description: "The status bar along the bottom" },
  "statusBar.foreground": { css: "--statusbar-fg", description: "Words in the status bar" },
} as const;

export type TokenId = keyof typeof TOKENS;
export type ThemeColors = Record<TokenId, string>;

export type ThemeId = "dark" | "light" | "high-contrast";
export type AccentId = "amber" | "blue" | "green" | "violet" | "rose" | "teal";
/** A built-in accent, or a color of one's own written as "#rrggbb". */
export type Accent = AccentId | `#${string}`;

/** An accent and its shades; `ink` is the darker one used for text on the light theme. */
type AccentColors = { accent: string; soft: string; on: string; ink: string };

export const ACCENT_COLORS: Record<AccentId, AccentColors> = {
  amber: { accent: "#f2b035", soft: "#f7cd72", on: "#1a1300", ink: "#8a5a00" },
  blue: { accent: "#5aa9ff", soft: "#8cc4ff", on: "#00172e", ink: "#1a5fb4" },
  green: { accent: "#4fd18b", soft: "#86e3b0", on: "#00200f", ink: "#187040" },
  violet: { accent: "#b48cff", soft: "#ccb2ff", on: "#1a0939", ink: "#6a3cc4" },
  rose: { accent: "#ff7a9a", soft: "#ffa3b8", on: "#2d000f", ink: "#b0244c" },
  teal: { accent: "#3fd0c9", soft: "#7fe3de", on: "#00211f", ink: "#0f7570" },
};

/** Shown on the button that picks a color of one's own: every hue around a circle. */
export const CUSTOM_SWATCH = "conic-gradient(#ff5f5f, #ffcf5f, #6bdc7a, #5fc6ff, #8a7dff, #ff6bd0, #ff5f5f)";

/** Shown as an example where a color of one's own can be typed. */
export const CUSTOM_ACCENT_EXAMPLE = "#3fa7d6";

export const isPresetAccent = (value: unknown): value is AccentId =>
  typeof value === "string" && Object.hasOwn(ACCENT_COLORS, value);

/** A color of one's own: "#rgb" or "#rrggbb", returned as lowercase "#rrggbb", else null. */
export function customAccent(value: unknown): `#${string}` | null {
  if (typeof value !== "string") return null;
  const m = /^#?([0-9a-f]{3}|[0-9a-f]{6})$/i.exec(value.trim());
  if (!m) return null;
  const h = m[1].length === 3 ? [...m[1]].map((c) => c + c).join("") : m[1];
  return `#${h.toLowerCase()}`;
}

export const isAccent = (value: unknown): value is Accent => isPresetAccent(value) || customAccent(value) === value;

type BaseColors = Omit<ThemeColors, "accent" | "accent.soft" | "accent.foreground" | "textLink.foreground" | "focusBorder">;

const LOGO = { "logo.primary": "#f2b035", "logo.secondary": "#f7cd72" };
const CLOSE = { "titleBar.closeHover.background": "#c42b1c", "titleBar.closeHover.foreground": "#ffffff" };

const BASE: Record<ThemeId, BaseColors> = {
  // A warm sidebar beside neutral, softly raised content.
  dark: {
    "window.background": "#232427",
    "sideBar.background": "#1e1d1b",
    "input.background": "#1c1d20",
    "card.background": "#2b2c30",
    "card.raisedBackground": "#35363b",
    "border": "#38393e",
    "border.strong": "#74757c",
    "foreground": "#ececee",
    "foreground.secondary": "#c9cacf",
    "foreground.muted": "#a4a6ad",
    "foreground.strong": "#ffffff",
    "status.ok": "#6ee7a0",
    "status.warning": "#f7cd72",
    "status.error": "#ff8a80",
    "status.idle": "#7a828e",
    "widget.shadow": "rgba(0, 0, 0, 0.35)",
    "tab.inactiveBackground": "#1e1d1b",
    "statusBar.background": "#1e1d1b",
    "statusBar.foreground": "#c4c0b7",
    ...CLOSE,
    ...LOGO,
  },
  light: {
    "window.background": "#f7f7f8",
    "sideBar.background": "#f1eee7",
    "input.background": "#ffffff",
    "card.background": "#ffffff",
    "card.raisedBackground": "#ececef",
    "border": "#e4e4e7",
    "border.strong": "#86878e",
    "foreground": "#1b1b1d",
    "foreground.secondary": "#3d3d42",
    "foreground.muted": "#5d5f66",
    "foreground.strong": "#000000",
    "status.ok": "#157a46",
    "status.warning": "#8a5a00",
    "status.error": "#c0282d",
    "status.idle": "#7d858f",
    "widget.shadow": "rgba(0, 0, 0, 0.12)",
    "tab.inactiveBackground": "#f1eee7",
    "statusBar.background": "#f1eee7",
    "statusBar.foreground": "#4f4a43",
    ...CLOSE,
    ...LOGO,
  },
  // Black and white with bright borders, so every edge and state can be seen
  // without relying on shades of grey.
  "high-contrast": {
    "window.background": "#000000",
    "sideBar.background": "#000000",
    "input.background": "#000000",
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
    "tab.inactiveBackground": "#000000",
    "statusBar.background": "#000000",
    "statusBar.foreground": "#ffffff",
    ...CLOSE,
    ...LOGO,
  },
};

/**
 * The shades of a color of one's own, worked out for a theme so it passes the
 * same contrast checks as the built-in accents: the color is darkened (light
 * theme) or lightened (dark themes) only as far as it needs to be.
 */
function customAccentColors(theme: ThemeId, color: string, base: BaseColors): AccentColors {
  const c = parse(color) ?? parse(ACCENT_COLORS.amber.accent)!;
  const toward = theme === "light" ? BLACK : WHITE;
  const reach = (on: string[], ratio: number) => {
    for (let k = 0; k <= 1.0001; k += 0.02) {
      const shade = hex(mix(toward, c, k));
      if (on.every((bg) => contrast(shade, bg) >= ratio)) return shade;
    }
    return hex(toward);
  };
  const grounds = [base["window.background"], base["card.background"], base["sideBar.background"]];
  const ink = reach(grounds, 4.5);
  if (theme === "light") return { accent: ink, soft: hex(mix(c, WHITE, 0.65)), on: "#ffffff", ink };
  const fill = reach([base["window.background"], base["card.background"]], 3);
  const on = contrast("#000000", fill) >= contrast("#ffffff", fill) ? "#000000" : "#ffffff";
  return { accent: fill, soft: hex(mix(c, WHITE, 0.65)), on, ink };
}

/** An accent's shades: a built-in one's, or a color of one's own worked out for the theme. */
export function accentColors(theme: ThemeId, accent: Accent): AccentColors {
  if (isPresetAccent(accent)) return ACCENT_COLORS[accent];
  return customAccentColors(theme, customAccent(accent) ?? ACCENT_COLORS.amber.accent, BASE[theme]);
}

/** A theme's full set of colors with the chosen accent. */
export function themeColors(theme: ThemeId, accent: Accent): ThemeColors {
  const a = accentColors(theme, accent);
  // Accent-colored text needs the darker shade on light backgrounds and the
  // lighter one on black. A color of one's own has its readable shade worked out already.
  const ink = !isPresetAccent(accent) || theme === "light" ? a.ink : theme === "high-contrast" ? a.soft : a.accent;
  // On white the bright accents are too faint for buttons and switches, so the
  // light theme fills them with the darker shade and white text.
  const light = theme === "light";
  return {
    ...BASE[theme],
    "accent": light ? a.ink : a.accent,
    "accent.soft": a.soft,
    "accent.foreground": light ? "#ffffff" : a.on,
    "textLink.foreground": ink,
    "focusBorder": theme === "high-contrast" ? "#f38518" : ink,
  };
}

let painted: ThemeColors | null = null;
const paintListeners = new Set<(c: ThemeColors) => void>();

/** The colors on screen now, and a way to hear when they change (a theme preview too). */
export function paintedColors(): ThemeColors | null {
  return painted;
}

export function onPaint(listener: (c: ThemeColors) => void): () => void {
  paintListeners.add(listener);
  return () => void paintListeners.delete(listener);
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
  painted = colors;
  paintListeners.forEach((l) => l(colors));
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
  "input.background": ["input.background", "editorWidget.background"],
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
  "tab.inactiveBackground": ["tab.inactiveBackground", "editorGroupHeader.tabsBackground"],
  "statusBar.background": ["statusBar.background"],
  "statusBar.foreground": ["statusBar.foreground"],
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
  colors["input.background"] = get("input.background", () => solidBg);
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
  colors["tab.inactiveBackground"] = get("tab.inactiveBackground", () => parse(colors["sideBar.background"])!);
  colors["statusBar.background"] = get("statusBar.background", () => parse(colors["sideBar.background"])!);
  // Words on the bars must stay readable on whatever color the theme gives them.
  const readable = (id: TokenId, on: string, fallback: string) => {
    const c = find(id);
    const value = c ? hex(mix(c, parse(on)!)) : fallback;
    return contrast(value, on) >= 3 ? value : contrast("#ffffff", on) > contrast("#000000", on) ? "#ffffff" : "#000000";
  };
  colors["statusBar.foreground"] = readable("statusBar.foreground", colors["statusBar.background"], colors["foreground.secondary"]);
  return colors;
}

/**
 * The colors of the services' and runtimes' own logos (components/BrandIcon.tsx).
 * They come from the Simple Icons set, which is public domain (CC0).
 */
const BRAND_COLORS: Record<string, string> = {
  caddy: "#1f88c0",
  mysql: "#4479a1",
  mariadb: "#003545",
  postgres: "#4169e1",
  redis: "#ff4438",
  meilisearch: "#ff5caa",
  mongodb: "#47a248",
  nats: "#27aae1",
  php: "#777bb4",
  node: "#5fa04e",
  python: "#3776ab",
  go: "#00add8",
  java: "#e76f00",
  wordpress: "#21759b",
  woocommerce: "#96588a",
  astro: "#bc52ee",
  laravel: "#ff2d20",
  symfony: "#000000",
  codeigniter: "#ef4223",
  cakephp: "#d33c43",
  craftcms: "#e5422b",
  yii: "#40b3d8",
  drupal: "#0678be",
  nextjs: "#000000",
  nuxt: "#00dc82",
  sveltekit: "#ff3e00",
  remix: "#000000",
  angular: "#0f0f11",
  gatsby: "#663399",
  createreactapp: "#61dafb",
  vite: "#646cff",
  express: "#000000",
  django: "#092e20",
  fastapi: "#009688",
  flask: "#3babc3",
  springboot: "#6db33f",
  quarkus: "#4695eb",
};

/**
 * A logo's color, made light or dark enough to stand out on `background`
 * (3:1, like other icons). Null when the brand has no color here.
 */
export function brandColor(id: string, background: string): string | null {
  const c = Object.hasOwn(BRAND_COLORS, id) ? parse(BRAND_COLORS[id]) : null;
  if (!c) return null;
  const toward = luminance(background) > 0.4 ? BLACK : WHITE;
  for (let k = 0; k <= 1.0001; k += 0.05) {
    const shade = hex(mix(toward, c, k));
    if (contrast(shade, background) >= 3) return shade;
  }
  return hex(toward);
}
