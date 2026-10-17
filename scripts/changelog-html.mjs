// Puts CHANGELOG.md into the website's changelog page.
//
//   node scripts/changelog-html.mjs site/changelog.html _site/changelog.html
//
// Reads the page (between <!-- CHANGELOG --> and <!-- /CHANGELOG -->) and
// writes it with every `## <version>` section as HTML, newest first. Only
// the Markdown the changelog uses is understood: lists (with wrapped lines),
// ### headings, paragraphs, **bold**, `code` and [links](url).

import { readFileSync, writeFileSync } from "node:fs";

const REPO = "https://github.com/ljamov/Stackyard";
const [template, out] = process.argv.slice(2);
if (!template || !out) throw new Error("usage: changelog-html.mjs <page> <out>");

const escape = (s) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");

function inline(text) {
  const parts = text.split(/(`[^`]*`)/);
  return parts
    .map((part) => {
      if (part.startsWith("`") && part.endsWith("`") && part.length > 1) return `<code>${escape(part.slice(1, -1))}</code>`;
      return escape(part)
        .replace(/\*\*([^*]+)\*\*/g, "<strong>$1</strong>")
        .replace(/\[([^\]]+)\]\((https?:\/\/[^)\s]+)\)/g, '<a href="$2">$1</a>');
    })
    .join("");
}

function body(lines) {
  const html = [];
  let list = null;
  let para = null;
  const flush = () => {
    if (list) html.push(`<ul>\n${list.map((i) => `  <li>${inline(i)}</li>`).join("\n")}\n</ul>`);
    if (para) html.push(`<p>${inline(para.join(" "))}</p>`);
    list = para = null;
  };
  for (const raw of lines) {
    const line = raw.trimEnd();
    if (!line.trim()) {
      if (para) flush();
      continue;
    }
    if (/^\s*[-*] /.test(line)) {
      if (para) flush();
      (list ??= []).push(line.replace(/^\s*[-*] /, ""));
    } else if (/^###+ /.test(line)) {
      flush();
      html.push(`<h3>${inline(line.replace(/^#+\s*/, ""))}</h3>`);
    } else if (list && /^\s/.test(line)) {
      list[list.length - 1] += ` ${line.trim()}`;
    } else {
      if (list) flush();
      (para ??= []).push(line.trim());
    }
  }
  flush();
  return html.join("\n");
}

const sections = [];
let current = null;
for (const line of readFileSync("CHANGELOG.md", "utf8").split("\n")) {
  const head = line.match(/^## (.+)$/);
  if (head) sections.push((current = { version: head[1].trim(), lines: [] }));
  else if (current) current.lines.push(line);
}

const id = (v) => (v.toLowerCase() === "unreleased" ? "next" : `v${v.replace(/[^\w.-]/g, "")}`);
const releases = sections.filter((s) => s.lines.some((l) => l.trim()));
const nav = `<ul class="versions" aria-label="Versions">\n${releases
  .map((s) => `  <li><a href="#${id(s.version)}">${s.version.toLowerCase() === "unreleased" ? "Next" : escape(s.version)}</a></li>`)
  .join("\n")}\n</ul>`;
const html = releases
  .map((s) => {
    const next = s.version.toLowerCase() === "unreleased";
    const title = next ? 'Coming next<span class="tag">Not released yet</span>' : `Stackyard ${escape(s.version)}`;
    const link = next ? "" : `<a class="link" href="${REPO}/releases/tag/v${escape(s.version)}">Download ${escape(s.version)}</a>`;
    return `<section class="release" id="${id(s.version)}">\n<div class="release-head"><h2>${title}</h2>${link}</div>\n${body(s.lines)}\n</section>`;
  })
  .join("\n");

const page = readFileSync(template, "utf8");
const start = page.indexOf("<!-- CHANGELOG -->");
const end = page.indexOf("<!-- /CHANGELOG -->");
if (start < 0 || end < start) throw new Error(`${template} has no <!-- CHANGELOG --> block`);
writeFileSync(out, page.slice(0, start) + nav + "\n" + html + "\n" + page.slice(end + "<!-- /CHANGELOG -->".length));
console.log(`${out}: ${releases.length} versions`);
