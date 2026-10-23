// Typed wrappers around the WordPress commands in src-tauri/src/wordpress.rs,
// with an in-memory stand-in for the browser preview (see api.ts).

import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export type WpDebug = { debug: boolean; log: boolean; display: boolean; scripts: boolean; queries: boolean };

export type WpOverview = {
  wordpress: string | null;
  php: string | null;
  database: string | null;
  prefix: string;
  database_server: string | null;
  debug: WpDebug;
  woocommerce: string | null;
  multisite: boolean;
  environment: string | null;
  wp_cli: boolean;
  warnings: string[];
};

export type WpDiskUse = { total: number; uploads: number; plugins: number; themes: number; database: number | null };
export type WpCoreUpdate = { current: string | null; available: string | null };
export type WpKind = "plugin" | "theme";
export type WpExtension = {
  name: string;
  title: string;
  status: string;
  version: string;
  update: string;
  update_version: string | null;
};
export type WpExtensionAction = "activate" | "deactivate" | "update" | "delete";
export type WpUser = { ID: number; user_login: string; user_email: string; display_name: string; roles: string };
export type WpLog = { path: string; text: string; size: number };
export type WpReplaceReport = {
  replaced: number;
  tables: Record<string, number>;
  damaged: Record<string, number>;
  encoded: Record<string, number>;
};
export type WpReplaced = { report: WpReplaceReport; backup: string | null };
export type WooStatus = {
  version: string | null;
  hpos: boolean;
  orders: number;
  products: number;
  customers: number;
  due_actions: number;
  failed_actions: number;
  currency: string | null;
};
export type WooGateway = { id: string; title: string; enabled: boolean; test_mode: boolean | null };
export type WooView = { status: WooStatus; gateways: WooGateway[] };

export type WpProtocol = "sftp" | "ftps" | "ftp";
export type WpRoute = "ssh" | "helper" | "files";
export type WpMedia = "download" | "from-live";
export type WpSavedConnection = { id: string; name: string; site: string; host: string; user: string; protocol: WpProtocol };
export type WpConnectionInput = {
  site: string;
  name: string;
  protocol: WpProtocol;
  host: string;
  port: number | null;
  user: string;
  password: string | null;
  key_file: string | null;
  remote_dir: string;
};
export type WpCloneSource =
  | { kind: "saved"; id: string }
  | { kind: "new"; connection: WpConnectionInput }
  | { kind: "files"; files: string[] };
export type WpFound = {
  route: WpRoute | null;
  roots: string[];
  root: string;
  url: string;
  wordpress: string;
  php: string;
  mysql: string;
  prefix: string;
  tables: number;
  database_bytes: number;
  notes: string[];
};
export type WpCloneOptions = {
  name: string;
  folder: string | null;
  root: string | null;
  url: string | null;
  media: WpMedia;
  https: boolean;
};
export type WpCloneStage = "connecting" | "files" | "database" | "loading" | "finishing";
export type WpCloneProgress = { name: string; stage: WpCloneStage; message: string; done: number; total: number };
export type WpCloned = { url: string; live: string; replaced: number; notes: string[]; site: { name: string; tld: string } };
export type WpEnvironment = {
  kind: "local" | "staging";
  safe: boolean;
  live: { url: string; connection: string | null; path: string; route: WpRoute; media: WpMedia; synced_at: number } | null;
};

export type WpStaging = {
  domain: string;
  url: string;
  made_from: string | null;
  created_at: number;
  shared_media: boolean;
};
export type WpEnvironments = { main: string; stagings: WpStaging[]; from_live: boolean };
export type WpStagingFrom = { kind: "site"; domain: string } | { kind: "live" };
export type WpNewStaging = { parent: string; name: string; from: WpStagingFrom; copy_media: boolean };
export type WpTrashed = { id: string; site: { name: string; tld: string; path: string }; database: string; trashed_at: number };
export type WpSnapshot = { id: string; domain: string; reason: string; created_at: number; saved: string[]; added: string[]; bytes: number };
export type WpSnapshots = { list: WpSnapshot[]; base: number | null };

export interface WpApi {
  overview(domain: string): Promise<WpOverview>;
  diskUse(domain: string): Promise<WpDiskUse>;
  coreUpdate(domain: string): Promise<WpCoreUpdate>;
  updateCore(domain: string): Promise<void>;
  extensions(domain: string, kind: WpKind): Promise<WpExtension[]>;
  changeExtension(domain: string, kind: WpKind, name: string, action: WpExtensionAction): Promise<WpExtension[]>;
  installExtension(domain: string, kind: WpKind, slug: string, activate: boolean): Promise<WpExtension[]>;
  users(domain: string): Promise<WpUser[]>;
  addAdmin(domain: string, login: string, email: string, password: string): Promise<WpUser[]>;
  setPassword(domain: string, user: number, password: string): Promise<void>;
  /** Opens a one-time login link for the user in the browser. */
  loginAs(domain: string, user: number): Promise<void>;
  setDebug(domain: string, settings: WpDebug): Promise<WpOverview>;
  log(domain: string): Promise<WpLog>;
  clearLog(domain: string): Promise<void>;
  searchReplace(domain: string, from: string, to: string, addresses: boolean, dryRun: boolean): Promise<WpReplaced>;
  exportDatabase(domain: string, path: string): Promise<void>;
  /** Loads a SQL file; returns the backup taken first. */
  importDatabase(domain: string, path: string): Promise<string>;
  optimizeDatabase(domain: string): Promise<number>;
  clearTransients(domain: string): Promise<number>;
  openPhpMyAdmin(domain: string): Promise<void>;
  woo(domain: string): Promise<WooView>;
  wooRunActions(domain: string): Promise<string>;
  wooTestMode(domain: string, gateway: string, on: boolean): Promise<WooView>;
  wooCreate(domain: string, kind: "customers" | "orders", count: number): Promise<string[]>;
  savedConnections(): Promise<WpSavedConnection[]>;
  cloneInspect(source: WpCloneSource, root: string | null, url: string | null): Promise<WpFound>;
  clone(source: WpCloneSource, options: WpCloneOptions): Promise<WpCloned>;
  cloneStop(): Promise<void>;
  cloneUnfinished(name: string): Promise<boolean>;
  onCloneProgress(handler: (p: WpCloneProgress) => void): Promise<() => void>;
  environment(domain: string): Promise<WpEnvironment | null>;
  stagings(domain: string): Promise<WpEnvironments>;
  createStaging(staging: WpNewStaging): Promise<{ name: string; tld: string }>;
  onStagingProgress(handler: (p: WpCloneProgress) => void): Promise<() => void>;
  destroyStaging(domain: string): Promise<WpTrashed>;
  trash(): Promise<WpTrashed[]>;
  restoreTrashed(id: string): Promise<void>;
  deleteTrashed(id: string): Promise<void>;
  snapshots(domain: string): Promise<WpSnapshots>;
  restoreSnapshot(domain: string, id: string): Promise<void>;
  removeSnapshot(domain: string, id: string): Promise<void>;
}

const tauriWp: WpApi = {
  overview: (domain) => invoke("wp_overview", { domain }),
  diskUse: (domain) => invoke("wp_disk_use", { domain }),
  coreUpdate: (domain) => invoke("wp_core_update", { domain }),
  updateCore: (domain) => invoke("wp_update_core", { domain }),
  extensions: (domain, kind) => invoke("wp_extensions", { domain, kind }),
  changeExtension: (domain, kind, name, action) => invoke("wp_change_extension", { domain, kind, name, action }),
  installExtension: (domain, kind, slug, activate) => invoke("wp_install_extension", { domain, kind, slug, activate }),
  users: (domain) => invoke("wp_users", { domain }),
  addAdmin: (domain, login, email, password) => invoke("wp_add_admin", { domain, login, email, password }),
  setPassword: (domain, user, password) => invoke("wp_set_password", { domain, user, password }),
  loginAs: (domain, user) => invoke("wp_login_as", { domain, user }),
  setDebug: (domain, settings) => invoke("wp_set_debug", { domain, settings }),
  log: (domain) => invoke("wp_log", { domain }),
  clearLog: (domain) => invoke("wp_clear_log", { domain }),
  searchReplace: (domain, from, to, addresses, dryRun) =>
    invoke("wp_search_replace", { domain, from, to, addresses, dryRun }),
  exportDatabase: (domain, path) => invoke("wp_export_database", { domain, path }),
  importDatabase: (domain, path) => invoke("wp_import_database", { domain, path }),
  optimizeDatabase: (domain) => invoke("wp_optimize_database", { domain }),
  clearTransients: (domain) => invoke("wp_clear_transients", { domain }),
  openPhpMyAdmin: (domain) => invoke("wp_open_phpmyadmin", { domain }),
  woo: (domain) => invoke("wp_woo", { domain }),
  wooRunActions: (domain) => invoke("wp_woo_run_actions", { domain }),
  wooTestMode: (domain, gateway, on) => invoke("wp_woo_test_mode", { domain, gateway, on }),
  wooCreate: (domain, kind, count) => invoke("wp_woo_create", { domain, kind, count }),
  savedConnections: () => invoke("wp_saved_connections"),
  cloneInspect: (source, root, url) => invoke("wp_clone_inspect", { source, root, url }),
  clone: (source, options) => invoke("wp_clone", { source, options }),
  cloneStop: () => invoke("wp_clone_stop"),
  cloneUnfinished: (name) => invoke("wp_clone_unfinished", { name }),
  onCloneProgress: (handler) => listen<WpCloneProgress>("wp-clone-progress", (e) => handler(e.payload)),
  environment: (domain) => invoke("wp_environment", { domain }),
  stagings: (domain) => invoke("wp_stagings", { domain }),
  createStaging: (staging) => invoke("wp_staging_create", { new: staging }),
  onStagingProgress: (handler) => listen<WpCloneProgress>("wp-staging-progress", (e) => handler(e.payload)),
  destroyStaging: (domain) => invoke("wp_staging_destroy", { domain }),
  trash: () => invoke("wp_trash"),
  restoreTrashed: (id) => invoke("wp_trash_restore", { id }),
  deleteTrashed: (id) => invoke("wp_trash_delete", { id }),
  snapshots: (domain) => invoke("wp_snapshots", { domain }),
  restoreSnapshot: (domain, id) => invoke("wp_snapshot_restore", { domain, id }),
  removeSnapshot: (domain, id) => invoke("wp_snapshot_remove", { domain, id }),
};

function mockWp(): WpApi {
  const wait = (ms = 300) => new Promise<void>((r) => setTimeout(r, ms));
  let debug: WpDebug = { debug: true, log: true, display: false, scripts: false, queries: false };
  let log = "[22-Oct-2026 09:14:02 UTC] PHP Notice:  Undefined index: color in wp-content/themes/shop/header.php on line 12\n";
  const plugins: WpExtension[] = [
    { name: "woocommerce", title: "WooCommerce", status: "active", version: "10.2.1", update: "available", update_version: "10.3.0" },
    { name: "query-monitor", title: "Query Monitor", status: "inactive", version: "3.19.0", update: "none", update_version: null },
    { name: "wordpress-seo", title: "Yoast SEO", status: "active", version: "26.1", update: "none", update_version: null },
  ];
  const themes: WpExtension[] = [
    { name: "storefront", title: "Storefront", status: "active", version: "4.6.1", update: "none", update_version: null },
    { name: "twentytwentyfive", title: "Twenty Twenty-Five", status: "inactive", version: "1.3", update: "available", update_version: "1.4" },
  ];
  const users: WpUser[] = [
    { ID: 1, user_login: "admin", user_email: "admin@example.test", display_name: "admin", roles: "administrator" },
    { ID: 2, user_login: "editor", user_email: "editor@example.test", display_name: "Ed Itor", roles: "editor" },
  ];
  const gateways: WooGateway[] = [
    { id: "stripe", title: "Credit card (Stripe)", enabled: true, test_mode: true },
    { id: "bacs", title: "Direct bank transfer", enabled: false, test_mode: null },
  ];
  const overview = (): WpOverview => ({
    wordpress: "6.8.3",
    php: "8.3",
    database: "legacy",
    prefix: "wp_",
    database_server: "MySQL 8.4",
    debug,
    woocommerce: "10.2.1",
    multisite: false,
    environment: "local",
    wp_cli: true,
    warnings: debug.display ? ["Errors are shown on the pages (WP_DEBUG_DISPLAY). Fine locally; never on a live site."] : [],
  });
  const list = (kind: WpKind) => (kind === "plugin" ? plugins : themes);
  return {
    overview: async () => overview(),
    diskUse: async () => ({ total: 1.9e9, uploads: 1.6e9, plugins: 1.8e8, themes: 2.1e7, database: 4.2e7 }),
    coreUpdate: async () => ({ current: "6.8.3", available: null }),
    updateCore: async () => wait(),
    extensions: async (_d, kind) => list(kind).map((e) => ({ ...e })),
    changeExtension: async (_d, kind, name, action) => {
      await wait();
      const all = list(kind);
      for (const e of all) {
        if (name !== "*" && e.name !== name) continue;
        if (action === "activate") {
          if (kind === "theme") for (const t of all) t.status = "inactive";
          e.status = "active";
        }
        if (action === "deactivate") e.status = "inactive";
        if (action === "update" && e.update_version) {
          e.version = e.update_version;
          e.update = "none";
          e.update_version = null;
        }
      }
      if (action === "delete") all.splice(all.findIndex((e) => e.name === name), 1);
      return all.map((e) => ({ ...e }));
    },
    installExtension: async (_d, kind, slug, activate) => {
      await wait(800);
      list(kind).push({ name: slug, title: slug, status: activate ? "active" : "inactive", version: "1.0", update: "none", update_version: null });
      return list(kind).map((e) => ({ ...e }));
    },
    users: async () => users.map((u) => ({ ...u })),
    addAdmin: async (_d, login, email) => {
      users.push({ ID: users.length + 1, user_login: login, user_email: email, display_name: login, roles: "administrator" });
      return users.map((u) => ({ ...u }));
    },
    setPassword: async () => wait(),
    loginAs: async () => void window.open("about:blank", "_blank", "noopener"),
    setDebug: async (_d, settings) => {
      debug = settings;
      return overview();
    },
    log: async () => ({ path: "~/Code/legacy/wp-content/debug.log", text: log, size: log.length }),
    clearLog: async () => {
      log = "";
    },
    searchReplace: async (_d, from, _to, _a, dryRun) => {
      await wait(600);
      const n = from ? 42 : 0;
      const tables: Record<string, number> = n ? { wp_options: 6, wp_posts: 30, wp_postmeta: 6 } : {};
      return {
        report: { replaced: n, tables, damaged: {}, encoded: {} },
        backup: dryRun || !n ? null : "legacy-20261022-101500.sql.gz",
      };
    },
    exportDatabase: async () => wait(),
    importDatabase: async () => "legacy-20261022-101500.sql.gz",
    optimizeDatabase: async () => 24,
    clearTransients: async () => 118,
    openPhpMyAdmin: async () => {},
    woo: async () => ({
      status: { version: "10.2.1", hpos: true, orders: 1204, products: 86, customers: 640, due_actions: 3, failed_actions: 0, currency: "EUR" },
      gateways: gateways.map((g) => ({ ...g })),
    }),
    wooRunActions: async () => "Success: 3 scheduled tasks completed.",
    wooTestMode: async (domain, id, on) => {
      const g = gateways.find((x) => x.id === id);
      if (g) g.test_mode = on;
      return mock.woo(domain);
    },
    wooCreate: async (_d, kind, count) =>
      Array.from({ length: count }, (_, i) => (kind === "customers" ? `test-${i}@example.test` : String(1205 + i))),
    savedConnections: async () => [
      { id: "k3j9x2ab", name: "Production", site: "legacy.stack", host: "203.0.113.10", user: "master", protocol: "sftp" },
    ],
    cloneInspect: async (source) => {
      await wait(900);
      const files = source.kind === "files";
      return {
        route: files ? "files" : source.kind === "new" && source.connection.protocol !== "sftp" ? "helper" : "ssh",
        roots: files ? ["backup/public_html"] : ["/home/master/applications/shop/public_html"],
        root: files ? "backup/public_html" : "/home/master/applications/shop/public_html",
        url: files ? "" : "https://shop.example",
        wordpress: "6.8.3",
        php: files ? "" : "8.2.29",
        mysql: files ? "" : "10.6.22-MariaDB",
        prefix: "wp_",
        tables: files ? 0 : 58,
        database_bytes: 3.1e8,
        notes: [
          files
            ? "The database comes from shop.sql.gz."
            : "Files come over SFTP and the database is exported on the server with mysqldump.",
        ],
      };
    },
    clone: async (_source, options) => {
      const steps: [WpCloneStage, string][] = [
        ["connecting", "Finding WordPress on the server…"],
        ["files", "Downloading wp-content/uploads/2025/06/hero.jpg (812 of 9,410)"],
        ["database", "Copying the table wp_postmeta (12 of 58)"],
        ["loading", "Changing addresses and loading the database…"],
        ["finishing", "Switching payments to test mode…"],
      ];
      for (const [i, [stage, message]] of steps.entries()) {
        if (cloneStopped) {
          cloneStopped = false;
          throw "stopped; clone again with the same name to continue where it stopped";
        }
        for (const h of cloneHandlers) h({ name: options.name, stage, message, done: i + 1, total: steps.length });
        await wait(700);
      }
      return {
        url: `https://${options.name}.stack`,
        live: "https://shop.example",
        replaced: 1834,
        notes: ["Switched to test mode: Credit card (Stripe)."],
        site: { name: options.name, tld: ".stack" },
      };
    },
    cloneStop: async () => {
      cloneStopped = true;
    },
    cloneUnfinished: async () => false,
    onCloneProgress: async (handler) => {
      cloneHandlers.push(handler);
      return () => cloneHandlers.splice(cloneHandlers.indexOf(handler), 1);
    },
    environment: async (domain) =>
      domain.startsWith("legacy")
        ? {
            kind: "local",
            safe: true,
            live: {
              url: "https://shop.example",
              connection: "k3j9x2ab",
              path: "/home/master/applications/shop/public_html",
              route: "ssh",
              media: "from-live",
              synced_at: Date.now() / 1000 - 86400,
            },
          }
        : null,
    stagings: async (domain) => {
      const main = domain.split(".").slice(-2).join(".");
      return { main, from_live: main.startsWith("legacy"), stagings: stagings.filter((s) => s.domain.endsWith(`.${main}`)) };
    },
    createStaging: async (n) => {
      const steps: [WpCloneStage, string][] = [
        ["files", "Copying the files…"],
        ["database", "Copying the database…"],
        ["loading", "Loading the database…"],
        ["finishing", "Taking the base snapshot…"],
      ];
      for (const [i, [stage, message]] of steps.entries()) {
        for (const h of stagingHandlers) h({ name: n.name, stage, message, done: i + 1, total: steps.length });
        await wait(500);
      }
      const domain = `${n.name}.${n.parent}`;
      stagings.push({
        domain,
        url: `https://${domain}`,
        made_from: n.from.kind === "live" ? "live" : n.from.domain,
        created_at: Date.now() / 1000,
        shared_media: !n.copy_media,
      });
      return { name: n.name, tld: `.${n.parent}` };
    },
    onStagingProgress: async (handler) => {
      stagingHandlers.push(handler);
      return () => stagingHandlers.splice(stagingHandlers.indexOf(handler), 1);
    },
    destroyStaging: async (domain) => {
      const i = stagings.findIndex((s) => s.domain === domain);
      const [name, ...rest] = domain.split(".");
      stagings.splice(i, 1);
      const t = { id: `${domain}-1`, site: { name, tld: `.${rest.join(".")}`, path: `~/Code/x@${name}` }, database: `x_${name}`, trashed_at: Date.now() / 1000 };
      trashed.push(t);
      return t;
    },
    trash: async () => trashed.slice(),
    restoreTrashed: async (id) => {
      const t = trashed.splice(trashed.findIndex((x) => x.id === id), 1)[0];
      const domain = `${t.site.name}${t.site.tld}`;
      stagings.push({ domain, url: `https://${domain}`, made_from: null, created_at: t.trashed_at, shared_media: false });
    },
    deleteTrashed: async (id) => void trashed.splice(trashed.findIndex((x) => x.id === id), 1),
    snapshots: async (domain) => ({
      base: domain.split(".").length > 2 ? Date.now() / 1000 - 7200 : null,
      list: snapshots.filter((s) => s.domain === domain),
    }),
    restoreSnapshot: async () => wait(800),
    removeSnapshot: async (_d, id) => void snapshots.splice(snapshots.findIndex((s) => s.id === id), 1),
  };
}

const stagings: WpStaging[] = [
  { domain: "checkout.legacy.stack", url: "https://checkout.legacy.stack", made_from: "legacy.stack", created_at: Date.now() / 1000 - 7200, shared_media: true },
];
const trashed: WpTrashed[] = [];
const snapshots: WpSnapshot[] = [
  { id: "1792700000000", domain: "legacy.stack", reason: "Merge from checkout.legacy.stack", created_at: Date.now() / 1000 - 3600, saved: ["wp-content/themes/shop/style.css"], added: [], bytes: 4.1e7 },
];
const stagingHandlers: ((p: WpCloneProgress) => void)[] = [];

let cloneStopped = false;
const cloneHandlers: ((p: WpCloneProgress) => void)[] = [];

const mock = mockWp();

export const wp: WpApi = isTauri() ? tauriWp : mock;
