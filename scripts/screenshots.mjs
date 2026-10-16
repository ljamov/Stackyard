// Takes the screenshots in docs/images (used by the README and the website)
// from the browser preview of the UI, which runs on the in-memory sample data
// in src/lib/api.ts.
//
//   npm i --no-save playwright
//   node scripts/screenshots.mjs            # every screen
//   node scripts/screenshots.mjs logs sites # only these
//
// Shots are 1920x1200 (1280x800 at 1.5x) PNGs, reduced to 256 colours by
// scripts/shrink-png.py afterwards when Python with Pillow is available.

import { spawn, spawnSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { setTimeout as sleep } from "node:timers/promises";
import { chromium } from "playwright";

const PORT = 1499;
const URL = `http://127.0.0.1:${PORT}/`;
const OUT = "docs/images";

// Each shot: the sidebar entry to open, then what to do there first.
const SHOTS = {
  dashboard: { screen: "Dashboard" },
  sites: {
    screen: "Sites",
    // The site list, with the end of the New site form above it.
    prep: async (page) => {
      await page.locator(".content table").evaluate((t) => t.scrollIntoView({ block: "end" }));
    },
  },
  servers: {
    screen: "Sites",
    prep: async (page) => {
      await page.getByRole("button", { name: "Servers and Git for blog.stack" }).click();
      await sleep(600);
      await page.getByRole("button", { name: "Servers and Git for blog.stack" }).evaluate((b) => b.scrollIntoView({ block: "start" }));
    },
  },
  services: { screen: "Services" },
  runtimes: { screen: "Runtimes" },
  logs: { screen: "Logs", wait: 1500 },
  databases: {
    screen: "Databases",
    prep: async (page) => {
      await page.getByRole("button", { name: /^shop/ }).first().click().catch(() => {});
    },
  },
  images: { screen: "Images" },
  settings: { screen: "Settings" },
};

const wanted = process.argv.slice(2);
const names = wanted.length ? wanted : Object.keys(SHOTS);
for (const n of names) if (!SHOTS[n]) throw new Error(`unknown shot ${n}; known: ${Object.keys(SHOTS).join(", ")}`);

const { version } = JSON.parse(readFileSync("package.json", "utf8"));
const vite = spawn(process.execPath, ["node_modules/vite/bin/vite.js", "--port", String(PORT), "--strictPort", "--host", "127.0.0.1"], {
  stdio: "ignore",
  env: { ...process.env, VITE_APP_VERSION: version },
});
try {
  for (let i = 0; ; i++) {
    try {
      if ((await fetch(URL)).ok) break;
    } catch {}
    if (i > 100) throw new Error("the dev server did not start");
    await sleep(200);
  }

  const browser = await chromium.launch(
    existsSync("/opt/pw-browsers/chromium") ? { executablePath: "/opt/pw-browsers/chromium" } : {},
  );
  const page = await browser.newPage({ viewport: { width: 1280, height: 800 }, deviceScaleFactor: 1.5, colorScheme: "dark" });
  await page.goto(URL);
  await page.locator(".sidebar").waitFor();
  await prepare(page);

  for (const name of names) {
    const shot = SHOTS[name];
    await open(page, shot.screen);
    await page.evaluate(() => document.querySelector(".content")?.scrollTo(0, 0));
    await sleep(400);
    if (shot.prep) await shot.prep(page);
    await sleep(shot.wait ?? 400);
    await page.mouse.move(0, 799);
    await page.screenshot({ path: `${OUT}/${name}.png` });
    console.log(`${OUT}/${name}.png`);
  }
  await browser.close();
} finally {
  vite.kill();
}

const py = spawnSync("python3", ["scripts/shrink-png.py", ...names.map((n) => `${OUT}/${n}.png`)], { stdio: "inherit" });
if (py.status !== 0) console.log("PNGs left full size (needs python3 with Pillow)");

// Sets the sample data up the way a working install looks: local domains set
// up, a few more modules on, the web server, databases and mail running.
async function prepare(page) {
  await page.addStyleTag({ content: ".frame { border-radius: 0; border: 0; }" });
  await open(page, "Sites");
  const setUp = page.getByRole("button", { name: "Set up", exact: true });
  if (await setUp.isVisible().catch(() => false)) {
    await setUp.click();
    await sleep(800);
  }

  await open(page, "Settings");
  for (const name of ["Mailpit", "PostgreSQL", "Redis", "Task scheduler", "MCP server"]) {
    const box = page.getByRole("switch", { name: `${name} module`, exact: true });
    await box.waitFor({ state: "attached" });
    await box.setChecked(true, { force: true });
    await sleep(200);
  }

  await open(page, "Runtimes");
  for (const version of ["8.5", "8.3"]) {
    await page.getByRole("button", { name: `Install PHP ${version}`, exact: true }).click();
  }
  await sleep(2500);

  await open(page, "Services");
  for (const name of ["Mailpit", "PostgreSQL", "Redis"]) {
    const install = page.getByRole("button", { name: `Install ${name}`, exact: true });
    await install.waitFor();
    await install.click();
  }
  await page.getByRole("button", { name: "Install Redis", exact: true }).waitFor({ state: "detached" });
  await sleep(1500);
  for (const name of ["Caddy", "MySQL", "Mailpit", "Redis"]) {
    const start = page.getByRole("button", { name: `Start ${name}`, exact: true });
    await start.waitFor();
    await start.click();
    await sleep(200);
  }
  await sleep(800);
}

async function open(page, screen) {
  await page.locator(".sidebar .nav-item", { hasText: new RegExp(`^${screen}`) }).click();
  await sleep(300);
}
