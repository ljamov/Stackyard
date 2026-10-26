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
  /** How the live site is set, for a copy of one. Push and pull never carry wp-config.php. */
  live_debug: WpDebug | null;
  woocommerce: string | null;
  multisite: boolean;
  environment: string | null;
  wp_cli: boolean;
  /** The theme in use (its folder name), when the database could be read. */
  theme: string | null;
  /** Whether that theme is a block theme (edited in the site editor, not the Customizer). */
  block_theme: boolean;
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
/** A clone kept from before the app last quit; `running` ones start again. */
export type WpSavedClone = {
  job: string;
  from: string;
  source: WpCloneSource;
  options: WpCloneOptions;
  state: "running" | "stopped" | "failed";
  error: string | null;
  /** Still running in the app: the window was closed and opened again. */
  active: boolean;
};
export type WpCloneEnded = { job: string; result: WpCloned | null; error: string | null };
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
  /** Back up the site that has this name and put the copy in its place. */
  replace?: boolean;
};
export type WpCloneStage = "connecting" | "files" | "database" | "loading" | "finishing";
/** `job` is the clone this is about (empty for a staging). */
export type WpCloneProgress = { job: string; name: string; stage: WpCloneStage; message: string; done: number; total: number };
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

export type WpChange = "added" | "changed" | "removed";
export type WpMergeLevel = "code" | "settings" | "everything";
export type WpFileChange = { path: string; group: string; change: WpChange; target: WpChange | null; conflict: boolean };
export type WpSwitch = { id: string; title: string; on: boolean; conflict: boolean };
export type WpSettingChange = { name: string; change: WpChange; conflict: boolean; staging: string | null; target: string | null };
export type WpChangeList = {
  source: string;
  target: string;
  base_at: number;
  files: WpFileChange[];
  switches: WpSwitch[];
  settings: WpSettingChange[];
  store: { hpos: boolean; orders: number } | null;
  target_new_posts: number;
  staging_new_users: number;
};
export type WpMergeChoice = { level: WpMergeLevel; files: string[]; switches: string[]; settings: string[] };
export type WpMerged = { snapshot: string; files: number; switches: number; settings: number; notes: string[] };
export type WpPostSummary = { id: number; title: string; post_type: string; status: string; slug: string; modified: string };
export type WpExisting = "update" | "skip" | "new";
export type WpMoveChoice = { ids: number[]; existing: Record<number, WpExisting>; comments: boolean };
export type WpPlannedPost = {
  source_id: number;
  title: string;
  post_type: string;
  action: "add" | "update" | "skip" | "reuse";
  target_id: number;
  picked: boolean;
  exists: boolean;
};
export type WpMoveWarning = { post: number; title: string; text: string };
export type WpMovePlan = {
  source: string;
  target: string;
  posts: WpPlannedPost[];
  counts: { posts: number; meta: number; new_terms: number; terms: number; comments: number; menu_items: number; files: number };
  files: string[];
  warnings: WpMoveWarning[];
};
export type WpMoved = { snapshot: string; posts: number; files: number; warnings: WpMoveWarning[] };
export type WpWay = "push" | "pull";
export type WpSyncChanges = {
  way: WpWay;
  domain: string;
  live_url: string;
  base_at: number;
  files: WpFileChange[];
  switches: WpSwitch[];
  settings: WpSettingChange[];
  live_content_changed: boolean;
  local_content_changed: boolean;
  live_wordpress: string;
  live_php: string;
  local_wordpress: string;
  notes: string[];
};
export type WpPushed = { backup: string; files: number; switches: number; settings: number; site_answers: boolean; notes: string[] };
export type WpPulled = { snapshot: string; files: number; switches: number; settings: number; notes: string[] };
export type WpLiveBackup = { id: string; domain: string; created_at: number; database: string[]; saved: string[]; added: string[]; bytes: number };
export type WpSyncProgress = { domain: string; stage: WpCloneStage; message: string; done: number; total: number };
export type WpPostsMoved = { posts: number; files: number; warnings: WpMoveWarning[]; undo: string };

export interface WpApi {
  overview(domain: string): Promise<WpOverview>;
  diskUse(domain: string): Promise<WpDiskUse>;
  coreUpdate(domain: string): Promise<WpCoreUpdate>;
  updateCore(domain: string): Promise<void>;
  /** `checkUpdates` asks wordpress.org for new versions (a few seconds); without it the list shows what WordPress last found. */
  extensions(domain: string, kind: WpKind, checkUpdates: boolean): Promise<WpExtension[]>;
  /** A theme's own picture as a data: URL. */
  themeScreenshot(domain: string, theme: string): Promise<string | null>;
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
  /** Renames the tables to a new prefix; returns the backup taken first. */
  changePrefix(domain: string, prefix: string): Promise<{ overview: WpOverview; backup: string }>;
  openPhpMyAdmin(domain: string): Promise<void>;
  woo(domain: string): Promise<WooView>;
  wooRunActions(domain: string): Promise<string>;
  wooTestMode(domain: string, gateway: string, on: boolean): Promise<WooView>;
  wooCreate(domain: string, kind: "customers" | "orders", count: number): Promise<string[]>;
  savedConnections(): Promise<WpSavedConnection[]>;
  /** `job` names one clone; any number can run at once. */
  cloneInspect(job: string, source: WpCloneSource, root: string | null, url: string | null): Promise<WpFound>;
  clone(job: string, source: WpCloneSource, options: WpCloneOptions, from: string): Promise<WpCloned>;
  clonesSaved(): Promise<WpSavedClone[]>;
  onCloneEnded(handler: (e: WpCloneEnded) => void): Promise<() => void>;
  cloneStop(job: string): Promise<void>;
  /** Logs out of the server typed in for a clone that is closed. */
  cloneForget(job: string): Promise<void>;
  cloneUnfinished(name: string): Promise<boolean>;
  onCloneProgress(handler: (p: WpCloneProgress) => void): Promise<() => void>;
  environment(domain: string): Promise<WpEnvironment | null>;
  stagings(domain: string): Promise<WpEnvironments>;
  createStaging(staging: WpNewStaging): Promise<{ name: string; tld: string }>;
  onStagingProgress(handler: (p: WpCloneProgress) => void): Promise<() => void>;
  stagingStop(): Promise<void>;
  destroyStaging(domain: string): Promise<WpTrashed>;
  trash(): Promise<WpTrashed[]>;
  restoreTrashed(id: string): Promise<void>;
  deleteTrashed(id: string): Promise<void>;
  snapshots(domain: string): Promise<WpSnapshots>;
  restoreSnapshot(domain: string, id: string): Promise<void>;
  removeSnapshot(domain: string, id: string): Promise<void>;
  mergeChanges(source: string, target: string): Promise<WpChangeList>;
  merge(source: string, target: string, choice: WpMergeChoice): Promise<WpMerged>;
  postsList(domain: string, search: string): Promise<WpPostSummary[]>;
  postsPlan(source: string, target: string, choice: WpMoveChoice): Promise<WpMovePlan>;
  postsMove(source: string, target: string, choice: WpMoveChoice): Promise<WpMoved>;
  syncChanges(domain: string, way: WpWay): Promise<WpSyncChanges>;
  /** `confirm` is the main site's name, typed for an Everything push. */
  push(domain: string, choice: WpMergeChoice, confirm: string): Promise<WpPushed>;
  pull(domain: string, choice: WpMergeChoice): Promise<WpPulled>;
  syncStop(): Promise<void>;
  onSyncProgress(handler: (p: WpSyncProgress) => void): Promise<() => void>;
  liveBackups(domain: string): Promise<WpLiveBackup[]>;
  restoreLiveBackup(domain: string, id: string): Promise<void>;
  removeLiveBackup(domain: string, id: string): Promise<void>;
  /** Reads the live database to pick single posts from; returns when. */
  readLive(domain: string): Promise<number>;
  liveReadAt(domain: string): Promise<number | null>;
  livePosts(domain: string, search: string): Promise<WpPostSummary[]>;
  livePostsPlan(domain: string, way: WpWay, choice: WpMoveChoice): Promise<WpMovePlan>;
  livePostsMove(domain: string, way: WpWay, choice: WpMoveChoice): Promise<WpPostsMoved>;
}

const tauriWp: WpApi = {
  overview: (domain) => invoke("wp_overview", { domain }),
  diskUse: (domain) => invoke("wp_disk_use", { domain }),
  coreUpdate: (domain) => invoke("wp_core_update", { domain }),
  updateCore: (domain) => invoke("wp_update_core", { domain }),
  extensions: (domain, kind, checkUpdates) => invoke("wp_extensions", { domain, kind, checkUpdates }),
  themeScreenshot: (domain, theme) => invoke("wp_theme_screenshot", { domain, theme }),
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
  changePrefix: (domain, prefix) => invoke("wp_change_prefix", { domain, prefix }),
  openPhpMyAdmin: (domain) => invoke("wp_open_phpmyadmin", { domain }),
  woo: (domain) => invoke("wp_woo", { domain }),
  wooRunActions: (domain) => invoke("wp_woo_run_actions", { domain }),
  wooTestMode: (domain, gateway, on) => invoke("wp_woo_test_mode", { domain, gateway, on }),
  wooCreate: (domain, kind, count) => invoke("wp_woo_create", { domain, kind, count }),
  savedConnections: () => invoke("wp_saved_connections"),
  cloneInspect: (job, source, root, url) => invoke("wp_clone_inspect", { job, source, root, url }),
  clone: (job, source, options, from) => invoke("wp_clone", { job, source, options, from }),
  clonesSaved: () => invoke("wp_clones_saved"),
  onCloneEnded: (handler) => listen<WpCloneEnded>("wp-clone-ended", (e) => handler(e.payload)),
  cloneStop: (job) => invoke("wp_clone_stop", { job }),
  cloneForget: (job) => invoke("wp_clone_forget", { job }),
  cloneUnfinished: (name) => invoke("wp_clone_unfinished", { name }),
  onCloneProgress: (handler) => listen<WpCloneProgress>("wp-clone-progress", (e) => handler(e.payload)),
  environment: (domain) => invoke("wp_environment", { domain }),
  stagings: (domain) => invoke("wp_stagings", { domain }),
  createStaging: (staging) => invoke("wp_staging_create", { new: staging }),
  onStagingProgress: (handler) => listen<WpCloneProgress>("wp-staging-progress", (e) => handler(e.payload)),
  stagingStop: () => invoke("wp_staging_stop"),
  destroyStaging: (domain) => invoke("wp_staging_destroy", { domain }),
  trash: () => invoke("wp_trash"),
  restoreTrashed: (id) => invoke("wp_trash_restore", { id }),
  deleteTrashed: (id) => invoke("wp_trash_delete", { id }),
  snapshots: (domain) => invoke("wp_snapshots", { domain }),
  restoreSnapshot: (domain, id) => invoke("wp_snapshot_restore", { domain, id }),
  removeSnapshot: (domain, id) => invoke("wp_snapshot_remove", { domain, id }),
  mergeChanges: (source, target) => invoke("wp_merge_changes", { source, target }),
  merge: (source, target, choice) => invoke("wp_merge", { source, target, choice }),
  postsList: (domain, search) => invoke("wp_posts_list", { domain, search }),
  postsPlan: (source, target, choice) => invoke("wp_posts_plan", { source, target, choice }),
  postsMove: (source, target, choice) => invoke("wp_posts_move", { source, target, choice }),
  syncChanges: (domain, way) => invoke("wp_sync_changes", { domain, way }),
  push: (domain, choice, confirm) => invoke("wp_push", { domain, choice, confirm }),
  pull: (domain, choice) => invoke("wp_pull", { domain, choice }),
  syncStop: () => invoke("wp_sync_stop"),
  onSyncProgress: (handler) => listen<WpSyncProgress>("wp-sync-progress", (e) => handler(e.payload)),
  liveBackups: (domain) => invoke("wp_live_backups", { domain }),
  restoreLiveBackup: (domain, id) => invoke("wp_live_backup_restore", { domain, id }),
  removeLiveBackup: (domain, id) => invoke("wp_live_backup_remove", { domain, id }),
  readLive: (domain) => invoke("wp_live_read", { domain }),
  liveReadAt: (domain) => invoke("wp_live_read_at", { domain }),
  livePosts: (domain, search) => invoke("wp_live_posts", { domain, search }),
  livePostsPlan: (domain, way, choice) => invoke("wp_live_posts_plan", { domain, way, choice }),
  livePostsMove: (domain, way, choice) => invoke("wp_live_posts_move", { domain, way, choice }),
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
  let prefix = "wp_";
  const overview = (): WpOverview => ({
    wordpress: "6.8.3",
    php: "8.3",
    database: "legacy",
    prefix,
    database_server: "MySQL 8.4",
    debug,
    live_debug: { debug: false, log: false, display: false, scripts: false, queries: false },
    woocommerce: "10.2.1",
    multisite: false,
    environment: "local",
    wp_cli: true,
    theme: "storefront",
    block_theme: false,
    warnings: debug.display ? ["Errors are shown on the pages (WP_DEBUG_DISPLAY). Fine locally; never on a live site."] : [],
  });
  const list = (kind: WpKind) => (kind === "plugin" ? plugins : themes);
  return {
    overview: async () => overview(),
    diskUse: async () => ({ total: 1.9e9, uploads: 1.6e9, plugins: 1.8e8, themes: 2.1e7, database: 4.2e7 }),
    coreUpdate: async () => ({ current: "6.8.3", available: null }),
    updateCore: async () => wait(),
    extensions: async (_d, kind, checkUpdates) => {
      if (checkUpdates) await wait(900);
      return list(kind).map((e) => ({ ...e }));
    },
    themeScreenshot: async (_d, theme) => (theme === "storefront" ? `${import.meta.env.BASE_URL}preview/site.jpg` : null),
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
    changePrefix: async (_d, next) => {
      await wait(800);
      if (!/^[A-Za-z0-9_]+$/.test(next)) throw new Error("The table prefix can only have letters, digits and underscores");
      prefix = next;
      return { overview: overview(), backup: "legacy-20261023-124500.sql.gz" };
    },
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
    cloneInspect: async (_job, source) => {
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
    clone: async (job, _source, options) => {
      const steps: [WpCloneStage, string][] = [
        ["connecting", "Finding WordPress on the server…"],
        ["files", "Downloading wp-content/uploads/2025/06/hero.jpg (812 of 9,410)"],
        ["database", "Copying the table wp_postmeta (12 of 58)"],
        ["loading", "Changing addresses and loading the database…"],
        ["finishing", "Switching payments to test mode…"],
      ];
      for (const [i, [stage, message]] of steps.entries()) {
        if (cloneStopped.delete(job)) {
          throw "stopped; clone again with the same name to continue where it stopped";
        }
        // The files stage counts bytes, like the app's.
        const [done, total] = stage === "files" ? [2.1e8, 4.6e8] : [i + 1, steps.length];
        for (const h of cloneHandlers) h({ job, name: options.name, stage, message, done, total });
        await wait(stage === "files" ? 6000 : 1500);
      }
      return {
        url: `https://${options.name}.stack`,
        live: "https://shop.example",
        replaced: 1834,
        notes: ["Switched to test mode: Credit card (Stripe)."],
        site: { name: options.name, tld: ".stack" },
      };
    },
    cloneStop: async (job) => void cloneStopped.add(job),
    cloneForget: async () => {},
    clonesSaved: async () => [],
    onCloneEnded: async () => () => {},
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
        for (const h of stagingHandlers) h({ job: "", name: n.name, stage, message, done: i + 1, total: steps.length });
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
    stagingStop: async () => {},
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
    mergeChanges: async (source, target) => {
      await wait(600);
      return {
        source,
        target,
        base_at: Date.now() / 1000 - 7200,
        files: [
          { path: "wp-content/themes/shop/style.css", group: "theme:shop", change: "changed", target: null, conflict: false },
          { path: "wp-content/themes/shop/checkout.php", group: "theme:shop", change: "added", target: null, conflict: false },
          { path: "wp-content/themes/shop/header.php", group: "theme:shop", change: "changed", target: "changed", conflict: true },
          { path: "wp-content/plugins/fast-checkout/fast-checkout.php", group: "plugin:fast-checkout", change: "added", target: null, conflict: false },
          { path: "wp-content/plugins/fast-checkout/readme.txt", group: "plugin:fast-checkout", change: "added", target: null, conflict: false },
          { path: "wp-content/uploads/2026/10/banner.jpg", group: "media", change: "added", target: null, conflict: false },
        ],
        switches: [{ id: "plugin:fast-checkout", title: "Fast Checkout", on: true, conflict: false }],
        settings: [
          { name: "blogdescription", change: "changed", conflict: false, staging: "Shirts that last", target: "Just another WordPress site" },
          { name: "woocommerce_enable_guest_checkout", change: "changed", conflict: false, staging: "yes", target: "no" },
          { name: "widget_block", change: "changed", conflict: true, staging: "a:3:{i:2;a:1:{s:7:\"content\";…", target: "a:2:{i:2;a:1:{s:7:\"content\";…" },
        ],
        store: { hpos: true, orders: 12 },
        target_new_posts: 2,
        staging_new_users: 1,
      };
    },
    merge: async (_s, target, choice) => {
      await wait(1200);
      snapshots.unshift({ id: `${Date.now()}`, domain: target, reason: `Merge from ${_s}`, created_at: Date.now() / 1000, saved: choice.files, added: [], bytes: 2.4e7 });
      return { snapshot: snapshots[0].id, files: choice.files.length, switches: choice.switches.length, settings: choice.settings.length, notes: [] };
    },
    postsList: async (_d, search) =>
      mockPosts.filter((p) => !search || p.title.toLowerCase().includes(search.toLowerCase()) || p.slug.includes(search.toLowerCase())),
    postsPlan: async (source, target, choice) => {
      await wait(400);
      const picked = mockPosts.filter((p) => choice.ids.includes(p.id));
      const posts: WpPlannedPost[] = picked.map((p, i) => {
        const exists = p.slug === "about";
        const existing = choice.existing[p.id] ?? "update";
        return {
          source_id: p.id,
          title: p.title,
          post_type: p.post_type,
          action: exists ? existing === "new" ? "add" : existing : "add",
          target_id: exists && existing !== "new" ? 2 : 140 + i,
          picked: true,
          exists,
        };
      });
      if (picked.length) {
        posts.push({ source_id: 77, title: "banner", post_type: "attachment", action: "add", target_id: 150, picked: false, exists: false });
        posts.push({ source_id: 31, title: "logo", post_type: "attachment", action: "reuse", target_id: 12, picked: false, exists: false });
      }
      const warnings: WpMoveWarning[] = picked.some((p) => p.id === 96)
        ? [{ post: 96, title: "Summer sale", text: "_wcfm_product_ref: a number in a plugin's setting (WCFM) that can't be checked safely; left as it is" }]
        : [];
      return {
        source,
        target,
        posts,
        counts: { posts: posts.length, meta: 24, new_terms: 1, terms: 3, comments: choice.comments ? 2 : 0, menu_items: 0, files: picked.length ? 4 : 0 },
        files: picked.length ? ["2026/10/banner.jpg", "2026/10/banner-300x200.jpg", "2026/10/banner-768x512.jpg", "2026/10/banner-1024x683.jpg"] : [],
        warnings,
      };
    },
    postsMove: async (source, target, choice) => {
      const plan = await mock.postsPlan(source, target, choice);
      await wait(800);
      snapshots.unshift({ id: `${Date.now()}`, domain: target, reason: `Posts moved from ${source}`, created_at: Date.now() / 1000, saved: [], added: plan.files, bytes: 1.2e6 });
      return { snapshot: snapshots[0].id, posts: plan.posts.filter((p) => p.action !== "skip" && p.action !== "reuse").length, files: plan.files.length, warnings: plan.warnings };
    },
    syncChanges: async (domain, way) => {
      const steps: [WpCloneStage, string][] = [
        ["connecting", "Connecting to the live server…"],
        ["files", "Listing the live files…"],
        ["database", "Reading the live settings…"],
      ];
      for (const [i, [stage, message]] of steps.entries()) {
        for (const h of syncHandlers) h({ domain, stage, message, done: i + 1, total: steps.length });
        await wait(400);
      }
      const push = way === "push";
      return {
        way,
        domain,
        live_url: "https://shop.example",
        base_at: Date.now() / 1000 - 86400,
        files: push
          ? [
              { path: "wp-content/themes/shop/style.css", group: "theme:shop", change: "changed", target: null, conflict: false },
              { path: "wp-content/themes/shop/checkout.php", group: "theme:shop", change: "added", target: null, conflict: false },
              { path: "wp-content/plugins/fast-checkout/fast-checkout.php", group: "plugin:fast-checkout", change: "added", target: null, conflict: false },
              { path: "wp-content/plugins/fast-checkout/readme.txt", group: "plugin:fast-checkout", change: "added", target: null, conflict: false },
            ]
          : [
              { path: "wp-content/plugins/woocommerce/woocommerce.php", group: "plugin:woocommerce", change: "changed", target: null, conflict: false },
              { path: "wp-content/themes/shop/header.php", group: "theme:shop", change: "changed", target: "changed", conflict: true },
            ],
        switches: push ? [{ id: "plugin:fast-checkout", title: "Fast Checkout", on: true, conflict: false }] : [],
        settings: push
          ? [{ name: "woocommerce_enable_guest_checkout", change: "changed", conflict: false, staging: "yes", target: "no" }]
          : [{ name: "blogdescription", change: "changed", conflict: false, staging: "Shirts that last, since 2009", target: "Shirts that last" }],
        live_content_changed: true,
        local_content_changed: false,
        live_wordpress: "6.8.3",
        live_php: "8.2.29",
        local_wordpress: "6.8.3",
        notes: [],
      };
    },
    push: async (domain, choice) => {
      await wait(1200);
      liveBackups.unshift({ id: `${Date.now()}`, domain, created_at: Date.now() / 1000, database: ["database/wp_options.sql.gz"], saved: choice.files, added: [], bytes: 3.2e8 });
      return { backup: liveBackups[0].id, files: choice.files.length, switches: choice.switches.length, settings: choice.settings.length, site_answers: true, notes: [] };
    },
    pull: async (domain, choice) => {
      await wait(1000);
      snapshots.unshift({ id: `${Date.now()}`, domain, reason: "Pull from live", created_at: Date.now() / 1000, saved: choice.files, added: [], bytes: 1.1e7 });
      return { snapshot: snapshots[0].id, files: choice.files.length, switches: choice.switches.length, settings: choice.settings.length, notes: [] };
    },
    syncStop: async () => {},
    onSyncProgress: async (handler) => {
      syncHandlers.push(handler);
      return () => syncHandlers.splice(syncHandlers.indexOf(handler), 1);
    },
    liveBackups: async (domain) => liveBackups.filter((b) => b.domain === domain),
    restoreLiveBackup: async () => wait(1200),
    removeLiveBackup: async (_d, id) => void liveBackups.splice(liveBackups.findIndex((b) => b.id === id), 1),
    readLive: async () => {
      await wait(900);
      liveRead = Date.now() / 1000;
      return liveRead;
    },
    liveReadAt: async () => liveRead,
    livePosts: async (_d, search) => mock.postsList(_d, search),
    livePostsPlan: async (domain, way, choice) => mock.postsPlan(way === "push" ? domain : "live", way === "push" ? "live" : domain, choice),
    livePostsMove: async (domain, way, choice) => {
      const moved = await mock.postsMove(way === "push" ? domain : "live", way === "push" ? "live" : domain, choice);
      return { posts: moved.posts, files: moved.files, warnings: moved.warnings, undo: moved.snapshot };
    },
  };
}

const syncHandlers: ((p: WpSyncProgress) => void)[] = [];
const liveBackups: WpLiveBackup[] = [
  { id: "1792600000", domain: "legacy.stack", created_at: Date.now() / 1000 - 172800, database: ["database/wp_posts.sql.gz"], saved: ["wp-content/themes/shop/style.css"], added: [], bytes: 2.9e8 },
];
let liveRead: number | null = null;

const mockPosts: WpPostSummary[] = [
  { id: 98, title: "New checkout, step by step", post_type: "post", status: "draft", slug: "new-checkout", modified: "2026-10-22 18:40:11" },
  { id: 96, title: "Summer sale", post_type: "page", status: "publish", slug: "summer-sale", modified: "2026-10-22 17:02:45" },
  { id: 2, title: "About", post_type: "page", status: "publish", slug: "about", modified: "2026-10-22 16:10:03" },
  { id: 91, title: "Linen shirt", post_type: "product", status: "publish", slug: "linen-shirt", modified: "2026-10-21 12:00:00" },
  { id: 54, title: "Hello world!", post_type: "post", status: "publish", slug: "hello-world", modified: "2026-10-01 09:00:00" },
];

const stagings: WpStaging[] = [
  { domain: "checkout.legacy.stack", url: "https://checkout.legacy.stack", made_from: "legacy.stack", created_at: Date.now() / 1000 - 7200, shared_media: true },
];
const trashed: WpTrashed[] = [];
const snapshots: WpSnapshot[] = [
  { id: "1792700000000", domain: "legacy.stack", reason: "Merge from checkout.legacy.stack", created_at: Date.now() / 1000 - 3600, saved: ["wp-content/themes/shop/style.css"], added: [], bytes: 4.1e7 },
];
const stagingHandlers: ((p: WpCloneProgress) => void)[] = [];

const cloneStopped = new Set<string>();
const cloneHandlers: ((p: WpCloneProgress) => void)[] = [];

const mock = mockWp();

export const wp: WpApi = isTauri() ? tauriWp : mock;
