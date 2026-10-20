// Fails when a color value is written anywhere in src except src/lib/theme.ts,
// so every color stays a theme token. Runs as part of `npm run build`.
//
//   node scripts/check-colors.mjs

import { readdirSync, readFileSync } from "node:fs";
import { join, relative } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = fileURLToPath(new URL("../src/", import.meta.url));
const ALLOWED = new Set(["lib/theme.ts"]);
const COLOR = /#[0-9a-fA-F]{3,8}\b|\b(?:rgba?|hsla?)\(/;

function files(dir) {
  return readdirSync(dir, { withFileTypes: true }).flatMap((e) => {
    const path = join(dir, e.name);
    if (e.isDirectory()) return e.name === "locales" ? [] : files(path);
    return /\.(tsx?|css)$/.test(e.name) ? [path] : [];
  });
}

const found = [];
for (const path of files(ROOT)) {
  const name = relative(ROOT, path).replaceAll("\\", "/");
  if (ALLOWED.has(name)) continue;
  readFileSync(path, "utf8")
    .split("\n")
    .forEach((line, i) => {
      if (COLOR.test(line)) found.push(`src/${name}:${i + 1}: ${line.trim()}`);
    });
}

if (found.length) {
  console.error("Colors belong in src/lib/theme.ts as tokens. Use a CSS variable instead:\n");
  console.error(found.join("\n"));
  process.exit(1);
}
console.log("check-colors: no color values outside src/lib/theme.ts");
