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

// Each shot: the side bar entry to open, what to do there first, and what to undo after.
const SHOTS = {
  dashboard: { screen: "Dashboard" },
  sites: {
    screen: "Sites",
    // The heading, the New site form (opened) and the site list under it.
    wait: 1200,
    prep: async (page) => {
      const fold = page.getByRole("button", { name: "New site and parked folders" });
      if ((await fold.getAttribute("aria-expanded")) !== "true") await fold.click();
      await sleep(300);
    },
  },
  // A site in its own tab, opened with Manage.
  servers: {
    screen: "Sites",
    prep: async (page) => {
      await manage(page, "blog.stack");
      await page.getByRole("tab", { name: "Servers & Git", exact: true }).click();
      await sleep(600);
    },
    after: closeTab,
  },
  // A WordPress site's stagings.
  wordpress: {
    screen: "Sites",
    prep: async (page) => {
      await manage(page, "legacy.stack");
      await page.getByRole("tab", { name: "WordPress", exact: true }).click();
      await sleep(600);
      await page.getByRole("tab", { name: "Staging", exact: true }).click();
      await sleep(800);
    },
    after: closeTab,
  },
  services: { screen: "Services" },
  runtimes: { screen: "Runtimes" },
  // The Logs panel, opened from the status bar under Home.
  logs: {
    screen: "Dashboard",
    wait: 1500,
    prep: async (page) => {
      await page.locator(".statusbar").getByRole("button", { name: "Logs" }).click();
    },
    // Closes the panel again for the shots after it.
    after: async (page) => {
      await page.locator(".statusbar").getByRole("button", { name: "Logs" }).click();
    },
  },
  // The Terminal tab in the panel, made taller, in a site's folder.
  terminal: {
    screen: "Dashboard",
    wait: 800,
    prep: async (page) => {
      await page.locator(".statusbar").getByRole("button", { name: "Terminal" }).click();
      await sleep(1200);
      await dragPanel(page, -170);
      await page.locator("#terminal-folder").selectOption({ label: "blog.stack" });
      await sleep(800);
      await page.locator(".terminal-box").click();
      for (const line of ["stackyard status", "php -v", "node -v"]) {
        await page.keyboard.type(line, { delay: 15 });
        await page.keyboard.press("Enter");
        await sleep(300);
      }
    },
    after: async (page) => {
      await dragPanel(page, 170);
      await page.locator(".statusbar").getByRole("button", { name: "Terminal" }).click();
    },
  },
  databases: {
    screen: "Databases",
    prep: async (page) => {
      await page.getByRole("button", { name: /^shop/ }).first().click().catch(() => {});
    },
  },
  images: { screen: "Images" },
  videos: { screen: "Videos" },
  "ai-tools": { screen: "AI tools", wait: 800 },
  settings: { screen: "Settings" },
  // Last, as it switches the preview to the light theme.
  light: {
    screen: "Settings",
    prep: async (page) => {
      await page.getByRole("button", { name: "Light", exact: true }).click();
      await open(page, "Dashboard");
    },
  },
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
    await page.evaluate(() => document.querySelector(".content:not([hidden])")?.scrollTo(0, 0));
    await sleep(400);
    if (shot.prep) await shot.prep(page);
    await sleep(shot.wait ?? 400);
    await page.mouse.move(0, 799);
    await page.screenshot({ path: `${OUT}/${name}.png` });
    console.log(`${OUT}/${name}.png`);
    if (shot.after) await shot.after(page);
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
  await page.getByRole("button", { name: "Start Caddy", exact: true }).waitFor();
  for (const name of ["Mailpit", "PostgreSQL", "Redis"]) {
    const install = page.getByRole("button", { name: `Install ${name}`, exact: true });
    // The sample data may have some installed already.
    if (await install.isVisible()) await install.click();
  }
  await sleep(2500);
  for (const name of ["Caddy", "MySQL", "Mailpit", "Redis"]) {
    const start = page.getByRole("button", { name: `Start ${name}`, exact: true });
    await start.waitFor();
    await start.click();
    await sleep(200);
  }
  await sleep(800);
}

// Drags the edge above the bottom panel up (negative) or down.
async function dragPanel(page, by) {
  const box = await page.locator(".sash.horizontal").boundingBox();
  const x = box.x + box.width / 2;
  const y = box.y + box.height / 2;
  await page.mouse.move(x, y);
  await page.mouse.down();
  await page.mouse.move(x, y + by, { steps: 8 });
  await page.mouse.up();
  await sleep(300);
}

async function open(page, screen) {
  await page.locator(`.sidebar [data-screen="${screen}"]`).click();
  await sleep(300);
}

// Closes the tab a shot opened, so the next shot starts from the screens alone.
async function closeTab(page) {
  await page.locator(".tab-item[data-active] .tab-close").click({ force: true });
  await sleep(200);
}

// Opens a site in its own tab with the Manage button on its row.
async function manage(page, domain) {
  await page.getByRole("group", { name: `Actions for ${domain}` }).getByRole("button", { name: "Manage" }).click();
  await sleep(300);
}
