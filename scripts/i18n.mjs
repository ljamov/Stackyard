// Lists the text the app translates and checks the language files.
//
//   node scripts/i18n.mjs          how complete each file in src/locales is
//   node scripts/i18n.mjs keys     every text as a JSON template to translate
//
// Texts are the English strings passed to t("…") in src, plus the names and
// descriptions the core sends (modules, services, MCP tool titles). Names
// that read the same in every language, like MySQL, can be left out.

import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("..", import.meta.url));

function files(dir, ext) {
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name);
    return statSync(path).isDirectory() ? files(path, ext) : path.endsWith(ext) ? [path] : [];
  });
}

const unquote = (s) => JSON.parse(`"${s}"`);
const keys = new Set();
const add = (re, text, group = 1) => {
  for (const m of text.matchAll(re)) keys.add(unquote(m[group]));
};
const STR = String.raw`"((?:[^"\\]|\\.)*)"`;

for (const file of [...files(join(root, "src"), ".tsx"), ...files(join(root, "src"), ".ts")]) {
  const text = readFileSync(file, "utf8");
  add(new RegExp(String.raw`\bt\(\s*` + STR, "g"), text);
}
// Lists whose entries are shown through t(): screen names, themes, groups, states.
for (const file of ["src/lib/appearance.ts", "src/screens/Settings.tsx"]) {
  add(new RegExp(String.raw`\b(?:label|title):\s*` + STR, "g"), readFileSync(join(root, file), "utf8"));
}
const sidebar = readFileSync(join(root, "src/components/Sidebar.tsx"), "utf8");
add(new RegExp(STR, "g"), sidebar.slice(sidebar.indexOf("SCREENS = ["), sidebar.indexOf("] as const")));
const card = readFileSync(join(root, "src/components/ServiceCard.tsx"), "utf8");
add(new RegExp(String.raw`\w+: ` + STR, "g"), card.slice(card.indexOf("const STATUS"), card.indexOf("};")));

const core = join(root, "crates/stackyard-core/src");
const modules = readFileSync(join(core, "modules.rs"), "utf8");
add(new RegExp(String.raw`module\(\s*"[^"]*",\s*` + STR + String.raw`,\s*\w+,\s*` + STR, "g"), modules, 1);
add(new RegExp(String.raw`module\(\s*"[^"]*",\s*` + STR + String.raw`,\s*\w+,\s*` + STR, "g"), modules, 2);
add(new RegExp(String.raw`role: ` + STR, "g"), readFileSync(join(core, "catalog.rs"), "utf8"));
// MCP tool titles; their descriptions are written for AI tools and stay in English.
add(new RegExp(String.raw`(?:read|action)\(\s*"[^"]*",\s*` + STR, "g"), readFileSync(join(core, "mcp/tools.rs"), "utf8"));

const all = [...keys].filter((k) => /[A-Za-z]/.test(k)).sort();

if (process.argv[2] === "keys") {
  console.log(JSON.stringify({ _name: "", ...Object.fromEntries(all.map((k) => [k, ""])) }, null, 2));
} else {
  const dir = join(root, "src/locales");
  for (const name of readdirSync(dir).filter((n) => n.endsWith(".json"))) {
    const strings = JSON.parse(readFileSync(join(dir, name), "utf8"));
    const missing = all.filter((k) => !strings[k]);
    const unused = Object.keys(strings).filter((k) => !k.startsWith("_") && !keys.has(k));
    console.log(`${name}: ${all.length - missing.length}/${all.length} translated`);
    for (const k of missing) console.log(`  missing: ${k}`);
    for (const k of unused) console.log(`  not used any more: ${k}`);
  }
}
