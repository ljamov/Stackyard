// Fails when a built-in theme, with any accent color, has text or controls
// that don't stand out enough from what they sit on (WCAG 2.2 level AA:
// 4.5:1 for text, 3:1 for the edges of fields and buttons, focus outlines
// and status dots). Runs as part of `npm run build`.
//
//   node scripts/check-contrast.mjs

import { ACCENT_COLORS, contrast, themeColors } from "../src/lib/theme.ts";

const TEXT = 4.5;
const UI = 3;

// [what, its token, the tokens it is drawn on, the ratio it needs]
const PAIRS = [
  ["text", "foreground", ["window.background", "card.background", "sideBar.background", "card.raisedBackground"], TEXT],
  ["labels", "foreground.secondary", ["window.background", "card.background", "sideBar.background", "card.raisedBackground"], TEXT],
  ["hints", "foreground.muted", ["window.background", "card.background", "sideBar.background", "card.raisedBackground"], TEXT],
  ["selected item", "foreground.strong", ["card.raisedBackground"], TEXT],
  ["links", "textLink.foreground", ["window.background", "card.background", "sideBar.background"], TEXT],
  ["text on accent", "accent.foreground", ["accent"], TEXT],
  ["running", "status.ok", ["window.background", "card.background", "sideBar.background"], TEXT],
  ["warnings", "status.warning", ["window.background", "card.background", "sideBar.background", "statusBar.background"], TEXT],
  ["errors", "status.error", ["window.background", "card.background", "sideBar.background"], TEXT],
  ["stopped dot", "status.idle", ["window.background", "card.background", "sideBar.background"], UI],
  ["status bar", "statusBar.foreground", ["statusBar.background"], TEXT],
  ["activity icons", "activityBar.inactiveForeground", ["activityBar.background"], UI],
  ["shown activity icon", "activityBar.foreground", ["activityBar.background"], UI],
  ["focus outline", "focusBorder", ["window.background", "card.background", "sideBar.background"], UI],
  ["field edges", "border.strong", ["window.background", "card.background", "sideBar.background"], UI],
  ["accent controls", "accent", ["window.background", "card.background"], UI],
];

const failed = [];
for (const theme of ["dark", "light", "high-contrast"]) {
  for (const accent of Object.keys(ACCENT_COLORS)) {
    const colors = themeColors(theme, accent);
    for (const [what, fg, backgrounds, need] of PAIRS) {
      for (const bg of backgrounds) {
        const ratio = contrast(colors[fg], colors[bg]);
        if (ratio < need) {
          failed.push(`${theme} / ${accent}: ${what} (${fg} ${colors[fg]} on ${bg} ${colors[bg]}) is ${ratio.toFixed(2)}:1, needs ${need}:1`);
        }
      }
    }
  }
}

if (failed.length) {
  console.error("Colors in src/lib/theme.ts that are too close to their background:\n");
  console.error([...new Set(failed)].join("\n"));
  process.exit(1);
}
console.log("check-contrast: every built-in theme meets WCAG AA with every accent");
