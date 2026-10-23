// Typed wrappers around the Tauri commands in src-tauri/src/lib.rs.
// Outside Tauri (plain `npm run dev` in a browser) an in-memory stand-in
// is used so the screens can be developed and previewed without the core.

import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";

export type LanguageFile = { code: string; name: string; strings: Record<string, string> };
export type LanguageFiles = {
  folder: string;
  languages: LanguageFile[];
  problems: { file: string; problem: string }[];
};

/** A file people may edit by hand, as the core read it (see stackyard-core interface.rs). */
export type InterfaceFile = {
  path: string;
  values: Record<string, unknown>;
  /** What is wrong with the file and where, when it is not valid TOML. */
  error: string | null;
};

/** Color themes added to the themes folder (see stackyard-core themes.rs). */
export type ThemeFiles = {
  folder: string;
  themes: { id: string; name: string; kind: "dark" | "light" | "high-contrast"; colors: Record<string, string> }[];
  problems: { file: string; problem: string }[];
};

/** Which hand-editable file: `settings.toml` or `keybindings.toml`. */
export type InterfaceFileName = "settings" | "keybindings";

export type SiteDatabase = { server: "mysql" | "postgres"; name: string };

/** A site's own settings, beyond its runtime version. */
export type SiteSettings = {
  /** Folder served, inside the project; null lets Stackyard pick it. */
  root?: string | null;
  env?: Record<string, string>;
  /** php.ini values for this site only. */
  php?: Record<string, string>;
  command?: string | null;
  database?: SiteDatabase | null;
};

export type SiteSettingsInfo = {
  domain: string;
  tld: string;
  https: boolean;
  settings: SiteSettings;
  parked: boolean;
  detected_root: string;
  detected_database: SiteDatabase | null;
  php: boolean;
  port: number | null;
};

export type SiteBackup = { site: string; file: string; size: number; created: number; database: string | null };

/** What a backup or export wrote. */
export type Packed = { path: string; size: number; files: number; database: string | null; warning: string | null };

export type SiteFile = {
  name: string;
  domain: string;
  runtime: string | null;
  database: string | null;
  created: number;
  made_by: string;
  folder: string;
};

export type ShareLink = { site: string; url: string | null };
export type ShareStatus = { cloudflared: string | null; links: ShareLink[]; web_running: boolean };

export type PhpMyAdminStatus = {
  installed: string | null;
  enabled: boolean;
  installing: boolean;
  url: string | null;
  local_url: string | null;
  problem: string | null;
};

export type SetupOffer = {
  id: string;
  name: string;
  group: "runtime" | "service";
  description: string;
  /** Runtimes: the version setup installs. */
  version: string | null;
  recommended: boolean;
};

export type SetupChoices = { sites_folder: string; install: string[] };

export type SetupStatus = {
  needed: boolean;
  home: string;
  default_home: string;
  default_sites_folder: string;
  /** An earlier installation's folder, whose files move along to a new folder. */
  existing: string | null;
  /** Picked before a restart, waiting to be applied. */
  choices: SetupChoices | null;
  move_error: string | null;
  offers: SetupOffer[];
};

export type RuntimeKind = "php" | "node" | "python" | "go" | "java";

export type RuntimeVersion = {
  version: string;
  /** Exact installed release, e.g. "8.4.26"; null when not installed. */
  installed: string | null;
  default: boolean;
  installing: boolean;
};

export type RuntimeFamily = {
  kind: RuntimeKind;
  name: string;
  available: boolean;
  versions: RuntimeVersion[];
};

export type InstallProgress = {
  kind: RuntimeKind;
  version: string;
  step: "resolving" | "downloading" | "unpacking";
  done?: number;
  total?: number | null;
};

export type ServiceState =
  | { state: "stopped" }
  | { state: "running"; pid: number }
  | { state: "restarting"; attempt: number }
  | { state: "crashed"; code: number | null };

export type Service = {
  id: string;
  name: string;
  /** Release line, e.g. "8.4". */
  version: string;
  ports: number[];
  role: string;
  /** Page to open while it runs, if any. */
  web: string | null;
  /** How apps connect, e.g. "mongodb://127.0.0.1:27017". */
  connect: string | null;
  /** Exact installed release, or null. */
  installed: string | null;
  installing: boolean;
} & ServiceState;

export type ServiceProgress = Omit<InstallProgress, "kind" | "version"> & { id: string };

export type Site = {
  name: string;
  domain: string;
  url: string;
  /** The `.localhost` address, which works before system setup. */
  local_url: string;
  path: string;
  /** The folder the web server serves, e.g. the project's `public/`. */
  root: string;
  runtime: string;
  framework: string | null;
  https: boolean;
  /** Found in a parked folder; remove it by unparking the folder. */
  parked: boolean;
  /** Why the site cannot work yet, if it cannot. */
  problem: string | null;
  /** The runtime versions the project uses, for the version pickers. */
  versions: SiteVersion[];
};

export type SiteVersion = {
  kind: RuntimeKind;
  version: string;
  installed: boolean;
  /** Saved with the site, pinned in a version file, or the default. */
  from: "site" | "pinned" | "default" | "newest";
  /** The version file that decides, e.g. `.nvmrc`, when one does. */
  file: string | null;
  /** Whether "Default" can be picked. */
  can_unset: boolean;
};

export type SystemStatus = {
  /** What setup would change; empty when everything is set up. */
  needed: string[];
  waiting_for_certificate: boolean;
  resolver: "mac-resolver" | "systemd-resolved" | "hosts-file";
  dns_running: boolean;
};

export type TerminalStatus = {
  /** The `stackyard` command-line program ships next to the app. */
  available: boolean;
  /** The commands exist and new terminals find them. */
  on: boolean;
  folder: string;
  /** Start-up files, or the Windows Path, that carry the setting. */
  places: string[];
};

export type LogFile = {
  id: string;
  name: string;
  size: number;
  /** Last written, in milliseconds since 1970. */
  modified: number;
};

export type CleanupItem = {
  id: string;
  title: string;
  detail: string;
  size: number;
  /** Safe to remove; picked unless unpicked. */
  recommended: boolean;
  /** Deletes data no download brings back. */
  permanent: boolean;
};

export type CleanupReport = {
  usage: { name: string; size: number }[];
  total: number;
  items: CleanupItem[];
};

export type CleanupOutcome = { freed: number; cleaned: string[]; problems: string[] };

export type UpdateStatus = {
  current: string;
  /** This build knows the key updates are signed with. */
  configured: boolean;
  checking: boolean;
  installing: boolean;
  available: { version: string; notes: string | null } | null;
  /** Milliseconds since 1970. */
  checked_at: number | null;
  problem: string | null;
  /** What this version brought, from the changelog. */
  whats_new: string | null;
  /** Stackyard has just restarted into this version after an update. */
  just_updated: boolean;
};

export type UpdateProgress = { done: number; total: number | null };

export type LogChunk = {
  lines: string[];
  /** Pass back to read what was written since. */
  offset: number;
  /** The log started over; what was shown before is gone. */
  reset: boolean;
};

export type McpTool = {
  name: string;
  title: string;
  description: string;
  /** Off until the user allows it: it changes something, or reads what is in databases. */
  action: boolean;
  allowed: boolean;
};

export type McpActivity = {
  /** Milliseconds since 1970. */
  at: number;
  /** The AI tool, as it named itself. */
  client: string;
  tool: string;
  summary: string;
  ok: boolean;
  error?: string;
};

export type McpStatus = {
  /** How to add Stackyard to AI tools; null when the command-line program is missing. */
  setup: { program: string; claude_code: string; json: string } | null;
  tools: McpTool[];
  /** Newest first. */
  activity: McpActivity[];
};

export type Module = {
  id: string;
  name: string;
  group: "runtime" | "service" | "feature";
  description: string;
  on_by_default: boolean;
  available: boolean;
  enabled: boolean;
};

export type DbServer = { id: string; name: string; version: string | null };
export type DbServerStatus = { server: DbServer | null; problem: string | null };
export type Database = { name: string; tables: number; size: number };
export type DbTable = { name: string; rows: number; size: number; kind: string };
export type ResultSet = { columns: string[]; rows: (string | null)[][]; truncated: boolean };
/** "postgres" for PostgreSQL; MySQL and MariaDB are "mysql". */
export type DbKind = "mysql" | "postgres";
export type DbBackup = { server: DbKind; database: string; file: string; size: number; created: number };

export type TaskInput = {
  name: string;
  command: string;
  folder: string;
  schedule: string;
  enabled: boolean;
};

export type TaskRun = {
  /** Milliseconds since 1970. */
  started: number;
  took_ms: number;
  outcome: "succeeded" | "failed" | "stopped" | "could-not-start";
  exit_code: number | null;
  message: string | null;
  by_hand: boolean;
};

export type Task = TaskInput & {
  id: string;
  /** e.g. "Every day at 02:30"; null for schedules shown as written. */
  description: string | null;
  next_run: number | null;
  last: TaskRun | null;
  running_since: number | null;
  /** Something that stops it from running, e.g. a folder that is gone. */
  problem: string | null;
};

export type SchedulePreview = { description: string | null; runs: number[] };

export type TldInfo = { tld: string; warning: string | null };

export type NewSite = { name: string; path: string; runtime: string; https: boolean };

export type RemoteProtocol = "sftp" | "ftps" | "ftp";

export type RemoteConnection = {
  id: string;
  site: string;
  name: string;
  protocol: RemoteProtocol;
  host: string;
  port: number;
  user: string;
  key_file?: string | null;
  /** The folder on the server that matches the site's folder. */
  remote_dir: string;
  /** Part of the site's folder to transfer; empty means all of it. */
  local_dir: string;
  skip: string[];
  /** The trusted server key (SFTP) or certificate (FTPS). */
  server_key?: string | null;
  has_password: boolean;
  busy: boolean;
};

export type RemoteConnections = { connections: RemoteConnection[]; secret_store: string };

export type RemoteInput = {
  site: string;
  name: string;
  protocol: RemoteProtocol;
  host: string;
  port: number | null;
  user: string;
  key_file: string | null;
  remote_dir: string;
  local_dir: string;
  skip: string[];
  /** null keeps the saved password; "" removes it. */
  password: string | null;
};

export type RemoteTest = {
  start_dir: string | null;
  error: string | null;
  /** A certificate the system does not trust, which can be trusted by hand. */
  certificate: string | null;
  key_changed: boolean;
};

export type RemoteEntry = { name: string; dir: boolean; size: number; modified: number | null };
export type RemoteListing = { dir: string; root: string; entries: RemoteEntry[] };
export type TransferDirection = "upload" | "download";
/** Which files a transfer copies; files missing on the other side always go. */
export type TransferCompare = "changed" | "newer" | "different" | "size" | "missing" | "all";

export type PlannedFile = {
  path: string;
  action: "upload" | "download" | "delete-remote" | "delete-local";
  reason: "new" | "changed" | "extra" | "picked";
  size: number;
};

export type TransferPlan = { direction: TransferDirection; files: PlannedFile[]; bytes: number; unchanged: number };
export type TransferOutcome = { done: number; bytes: number; failed: { path: string; error: string }[]; stopped: boolean };

export type RemoteProgress = { id: string } & (
  | { step: "scanning"; folder: string; found: number }
  | { step: "transfer"; index: number; total: number; path: string; done: number; size: number }
);

export type GitChange = {
  path: string;
  kind: "added" | "modified" | "deleted" | "renamed" | "new" | "conflict";
};

export type GitStatus = {
  installed: boolean;
  repo: boolean;
  /** The repository's top folder, when the site is inside a larger one. */
  root: string | null;
  branch: string | null;
  upstream: string | null;
  ahead: number;
  behind: number;
  remote: string | null;
  changes: GitChange[];
  more_changes: number;
  identity: boolean;
};
/** A project a new site can start as, instead of an empty folder. */
export type Starter = { id: string; name: string; description: string };
export type NewStarterSite = { name: string; starter: string; path: string; https: boolean };
export type StarterProgress = { name: string; message: string };

export type ImageFormat = "webp" | "avif" | "same";
export type ImageOptions = {
  formats: ImageFormat[];
  quality: number;
  max_width: number | null;
  /** With max_width: every image this many pixels high, cut from the middle. Null keeps the shape. */
  height: number | null;
  /** Null puts new files next to the originals. */
  out_dir: string | null;
  /** Overwrite originals with smaller JPG/PNG copies. */
  replace: boolean;
};
export type ImageMade = { format: ImageFormat; path: string; size: number; skipped: string | null };
export type ImageResult = { source: string; size: number; made: ImageMade[]; error: string | null };
export type ImagesFound = { count: number; size: number };
export type ImagesProgress = { done: number; total: number; result: ImageResult };

export type VideoFormat = "mp4" | "webm" | "gif";
export type VideoQuality = "small" | "balanced" | "high";
export type VideoOptions = {
  formats: VideoFormat[];
  quality: VideoQuality;
  max_width: number | null;
  /** Leave the sound out. */
  mute: boolean;
  out_dir: string | null;
};
export type VideoMade = { format: VideoFormat; path: string; size: number; skipped: string | null };
export type VideoResult = { source: string; size: number; made: VideoMade[]; error: string | null };
export type VideosProgress = {
  done: number;
  total: number;
  current: string | null;
  format: VideoFormat | null;
  percent: number | null;
  result: VideoResult | null;
};
export type FfmpegStatus = {
  /** The FFmpeg that will be used; null when there is none yet. */
  path: string | null;
  /** Stackyard downloaded it, so it can remove it. */
  downloaded: boolean;
  version: string | null;
  installing: boolean;
};
export type AiTool = {
  id: string;
  name: string;
  maker: string;
  about: string;
  package: string;
  /** What you type to start it, e.g. "claude". */
  command: string;
  min_node: number;
  docs: string;
  /** Installed version; null when not installed. */
  installed: string | null;
  /** The Node.js version it runs on. */
  node: string | null;
  /** Newest version on npm, once checked. */
  latest: string | null;
  update: boolean;
  installing: boolean;
};
export type AiProgress = { id: string; text: string };
export type FfmpegProgress = { step: "resolving" | "downloading" | "unpacking"; done?: number; total?: number | null };

export interface Api {
  listRuntimes(): Promise<RuntimeFamily[]>;
  installRuntime(kind: RuntimeKind, version: string): Promise<RuntimeFamily[]>;
  uninstallRuntime(kind: RuntimeKind, version: string): Promise<RuntimeFamily[]>;
  setDefaultRuntime(kind: RuntimeKind, version: string): Promise<RuntimeFamily[]>;
  /** Calls `handler` for install progress; resolves to an unsubscribe function. */
  onInstallProgress(handler: (p: InstallProgress) => void): Promise<() => void>;
  listServices(): Promise<Service[]>;
  installService(id: string): Promise<Service[]>;
  uninstallService(id: string): Promise<Service[]>;
  startService(id: string): Promise<Service[]>;
  stopService(id: string): Promise<Service[]>;
  serviceLog(id: string): Promise<string[]>;
  openLocalPage(url: string): Promise<void>;
  onServiceProgress(handler: (p: ServiceProgress) => void): Promise<() => void>;
  getTld(): Promise<TldInfo>;
  setTld(value: string): Promise<TldInfo>;
  /** Translation files added to the languages folder in Stackyard's home. */
  languageFiles(): Promise<LanguageFiles>;
  openLanguagesFolder(): Promise<void>;
  /** The `[interface]` section of settings.toml: the window's own settings. */
  interfaceSettings(): Promise<InterfaceFile>;
  saveInterfaceSettings(values: Record<string, unknown>): Promise<void>;
  keybindings(): Promise<InterfaceFile>;
  saveKeybindings(values: Record<string, unknown>): Promise<void>;
  /** Shows the file in the file manager (made first if missing) to open in an editor. */
  showInterfaceFile(file: InterfaceFileName): Promise<void>;
  /** Color themes in the themes folder, and files there that can't be used. */
  themeFiles(): Promise<ThemeFiles>;
  /** Copies a VS Code theme (.json) or extension (.vsix) in; returns the ids added. */
  importTheme(path: string): Promise<string[]>;
  removeTheme(id: string): Promise<void>;
  openThemesFolder(): Promise<void>;
  /** Asks for a VS Code theme file or extension to import. */
  pickThemeFile(title: string): Promise<string | null>;
  listModules(): Promise<Module[]>;
  setModule(id: string, enabled: boolean): Promise<Module[]>;
  listSites(): Promise<Site[]>;
  /** Picks a runtime version for a site; null goes back to the default. */
  setSiteVersion(domain: string, kind: RuntimeKind, version: string | null): Promise<Site[]>;
  addSite(site: NewSite): Promise<Site[]>;
  listStarters(): Promise<Starter[]>;
  /** Opens the system's file picker for images; empty when cancelled. */
  pickImages(start?: string): Promise<string[]>;
  findImages(paths: string[]): Promise<ImagesFound>;
  optimizeImages(paths: string[], options: ImageOptions): Promise<ImageResult[]>;
  onImagesProgress(handler: (p: ImagesProgress) => void): Promise<() => void>;
  /** Opens the system's file picker for videos; empty when cancelled. */
  pickVideos(start?: string): Promise<string[]>;
  aiTools(): Promise<AiTool[]>;
  /** Like aiTools, with the newest version of each from npm. */
  checkAiTools(): Promise<AiTool[]>;
  /** Installs a tool, or updates it to the newest version. */
  installAiTool(id: string): Promise<AiTool[]>;
  removeAiTool(id: string): Promise<AiTool[]>;
  /** Opens a terminal window in `folder` running the tool. */
  openAiTool(id: string, folder: string): Promise<void>;
  /** Opens the tool's install and sign-in guide in the browser. */
  openAiToolDocs(id: string): Promise<void>;
  onAiProgress(handler: (p: AiProgress) => void): Promise<() => void>;
  ffmpegStatus(): Promise<FfmpegStatus>;
  installFfmpeg(): Promise<FfmpegStatus>;
  removeFfmpeg(): Promise<FfmpegStatus>;
  onFfmpegProgress(handler: (p: FfmpegProgress) => void): Promise<() => void>;
  findVideos(paths: string[]): Promise<ImagesFound>;
  optimizeVideos(paths: string[], options: VideoOptions): Promise<VideoResult[]>;
  stopVideos(): Promise<void>;
  onVideosProgress(handler: (p: VideosProgress) => void): Promise<() => void>;
  /** Files and folders dropped on the window. */
  onDrop(handler: (paths: string[]) => void): Promise<() => void>;
  /** Downloads and sets up the project, then adds the site. */
  addStarterSite(site: NewStarterSite): Promise<Site[]>;
  onStarterProgress(handler: (p: StarterProgress) => void): Promise<() => void>;
  removeSite(name: string): Promise<Site[]>;
  listParked(): Promise<string[]>;
  /** Where new sites get their folder. */
  sitesFolder(): Promise<string>;
  /** Opens the system's folder picker; null when cancelled. */
  pickFolder(title: string, start?: string): Promise<string | null>;
  phpMyAdminStatus(): Promise<PhpMyAdminStatus>;
  /** Downloads phpMyAdmin and adds its site; progress comes as service progress with id "phpmyadmin". */
  installPhpMyAdmin(): Promise<PhpMyAdminStatus>;
  uninstallPhpMyAdmin(): Promise<PhpMyAdminStatus>;
  /** Starts the web server and database if needed, then opens phpMyAdmin in the browser. */
  openPhpMyAdmin(): Promise<void>;
  setupStatus(): Promise<SetupStatus>;
  /** Saves the picks; true when the app restarts to move to the new folder. */
  setupChoose(home: string, sitesFolder: string, install: string[]): Promise<boolean>;
  setupApply(): Promise<void>;
  setupFinish(): Promise<void>;
  parkFolder(path: string): Promise<string[]>;
  unparkFolder(path: string): Promise<string[]>;
  systemStatus(): Promise<SystemStatus>;
  /** Shows the system's password prompt, then applies setup. */
  runSystemSetup(): Promise<SystemStatus>;
  terminalStatus(): Promise<TerminalStatus>;
  /** Creates or removes the terminal commands and their PATH entry. */
  setTerminal(on: boolean): Promise<TerminalStatus>;
  /** Fires when a terminal or an AI tool changed the sites. */
  onSitesChanged(handler: () => void): Promise<() => void>;
  /** Starts a shell for the Terminal tab in `folder` (the home folder when null); returns its number. */
  consoleOpen(folder: string | null, cols: number, rows: number): Promise<number>;
  consoleWrite(id: number, data: string): Promise<void>;
  consoleResize(id: number, cols: number, rows: number): Promise<void>;
  consoleClose(id: number): Promise<void>;
  onConsoleOutput(handler: (id: number, data: string) => void): Promise<() => void>;
  onConsoleExit(handler: (id: number) => void): Promise<() => void>;
  updateStatus(): Promise<UpdateStatus>;
  checkForUpdate(): Promise<UpdateStatus>;
  /** Stops every service, installs the update and restarts Stackyard. */
  installUpdate(): Promise<void>;
  /** The person has seen what this version brought. */
  seenWhatsNew(): Promise<UpdateStatus>;
  onUpdateAvailable(handler: (s: UpdateStatus) => void): Promise<() => void>;
  onUpdateProgress(handler: (p: UpdateProgress) => void): Promise<() => void>;
  listLogs(): Promise<LogFile[]>;
  /** The last `lines` lines when `from` is null, else what was written since `from`. */
  readLog(id: string, from: number | null, lines: number): Promise<LogChunk>;
  /** Empties every log; returns how many were cleared. */
  clearLogs(): Promise<number>;
  /** Empties one log; false when it had nothing in it yet. */
  clearLog(id: string): Promise<boolean>;
  /** The servers the Databases screen can switch between. */
  dbServers(): Promise<DbKind[]>;
  dbServer(server?: DbKind): Promise<DbServerStatus>;
  dbList(server?: DbKind): Promise<Database[]>;
  dbCreate(name: string, server?: DbKind): Promise<Database[]>;
  dbDrop(name: string, server?: DbKind): Promise<Database[]>;
  dbTables(database: string, server?: DbKind): Promise<DbTable[]>;
  dbBrowse(database: string, table: string, offset: number, limit: number, server?: DbKind): Promise<ResultSet>;
  dbQuery(database: string | null, sql: string, server?: DbKind): Promise<ResultSet[]>;
  dbBackup(database: string, server?: DbKind): Promise<DbBackup[]>;
  dbBackups(): Promise<DbBackup[]>;
  dbRestore(database: string, file: string, into: string | null, server?: DbKind): Promise<Database[]>;
  dbRemoveBackup(database: string, file: string): Promise<DbBackup[]>;
  mcpStatus(): Promise<McpStatus>;
  setMcpTool(name: string, allowed: boolean): Promise<McpStatus>;
  listTasks(): Promise<Task[]>;
  addTask(task: TaskInput): Promise<Task[]>;
  updateTask(id: string, task: TaskInput): Promise<Task[]>;
  setTaskEnabled(id: string, enabled: boolean): Promise<Task[]>;
  removeTask(id: string): Promise<Task[]>;
  runTask(id: string): Promise<Task[]>;
  stopTask(id: string): Promise<Task[]>;
  /** Rejects with why when the schedule is not valid. */
  previewSchedule(schedule: string): Promise<SchedulePreview>;
  taskLog(id: string): Promise<string[]>;
  cleanupScan(): Promise<CleanupReport>;
  cleanupRun(ids: string[]): Promise<CleanupOutcome>;
  remoteConnections(site: string): Promise<RemoteConnections>;
  /** Adds a connection when `id` is null, else changes it. */
  remoteSave(id: string | null, input: RemoteInput): Promise<RemoteConnections>;
  remoteRemove(id: string): Promise<RemoteConnections>;
  /** Trusts a certificate or server key; null forgets the trusted one. */
  remoteTrust(id: string, fingerprint: string | null): Promise<RemoteConnections>;
  remoteTest(id: string): Promise<RemoteTest>;
  remoteBrowse(id: string, dir: string | null): Promise<RemoteListing>;
  /** What would be transferred; `paths` limits it to picked files and folders. */
  remotePlan(
    id: string,
    direction: TransferDirection,
    compare: TransferCompare,
    del: boolean,
    paths: string[] | null,
  ): Promise<TransferPlan>;
  remoteRun(id: string, files: PlannedFile[]): Promise<TransferOutcome>;
  remoteStop(id: string): Promise<void>;
  remoteLocalFolder(id: string): Promise<string>;
  onRemoteProgress(handler: (p: RemoteProgress) => void): Promise<() => void>;
  /** Opens the system's file picker; empty when cancelled. */
  pickFiles(title: string, start?: string, multiple?: boolean): Promise<string[]>;
  /** Opens the system's save dialog; null when cancelled. */
  pickSaveFile(title: string, name: string): Promise<string | null>;
  siteSettings(domain: string): Promise<SiteSettingsInfo>;
  saveSiteSettings(domain: string, tld: string, https: boolean, settings: SiteSettings): Promise<{ sites: Site[]; domain: string }>;
  siteBackups(domain: string): Promise<SiteBackup[]>;
  backupSite(domain: string): Promise<Packed>;
  /** Puts a backup back; the site as it is now is backed up first. */
  restoreSite(domain: string, file: string): Promise<Packed>;
  removeSiteBackup(domain: string, file: string): Promise<SiteBackup[]>;
  openSiteBackups(domain: string): Promise<void>;
  exportSite(domain: string, dest: string): Promise<Packed>;
  inspectSiteFile(path: string): Promise<SiteFile>;
  importSite(path: string, name: string | null, folder: string | null): Promise<{ sites: Site[]; domain: string; warning: string | null }>;
  shareStatus(): Promise<ShareStatus>;
  shareInstall(): Promise<ShareStatus>;
  shareUninstall(): Promise<ShareStatus>;
  /** Makes a public link (downloading cloudflared first if needed) and waits for its address. */
  shareStart(domain: string): Promise<ShareLink>;
  shareStop(domain: string): Promise<ShareStatus>;
  openShareLink(domain: string): Promise<void>;
  gitStatus(site: string): Promise<GitStatus>;
  gitLink(site: string, url: string): Promise<GitStatus>;
  gitFetch(site: string): Promise<GitStatus>;
  gitPull(site: string): Promise<GitStatus>;
  gitPush(site: string): Promise<GitStatus>;
  gitCommit(site: string, message: string, name: string | null, email: string | null): Promise<GitStatus>;
}

const tauriApi: Api = {
  listRuntimes: () => invoke("list_runtimes"),
  installRuntime: (kind, version) => invoke("install_runtime", { kind, version }),
  uninstallRuntime: (kind, version) => invoke("uninstall_runtime", { kind, version }),
  setDefaultRuntime: (kind, version) => invoke("set_default_runtime", { kind, version }),
  onInstallProgress: (handler) => listen<InstallProgress>("runtime-progress", (e) => handler(e.payload)),
  listServices: () => invoke("list_services"),
  installService: (id) => invoke("install_service", { id }),
  uninstallService: (id) => invoke("uninstall_service", { id }),
  startService: (id) => invoke("start_service", { id }),
  stopService: (id) => invoke("stop_service", { id }),
  serviceLog: (id) => invoke("service_log", { id }),
  openLocalPage: (url) => invoke("open_local_page", { url }),
  onServiceProgress: (handler) => listen<ServiceProgress>("service-progress", (e) => handler(e.payload)),
  getTld: () => invoke("get_tld"),
  setTld: (value) => invoke("set_tld", { value }),
  languageFiles: () => invoke("language_files"),
  openLanguagesFolder: () => invoke("open_languages_folder"),
  interfaceSettings: () => invoke("interface_settings"),
  saveInterfaceSettings: (values) => invoke("save_interface_settings", { values }),
  keybindings: () => invoke("keybindings"),
  saveKeybindings: (values) => invoke("save_keybindings", { values }),
  showInterfaceFile: (file) => invoke("show_interface_file", { file }),
  themeFiles: () => invoke("theme_files"),
  importTheme: (path) => invoke("import_theme", { path }),
  removeTheme: (id) => invoke("remove_theme", { id }),
  openThemesFolder: () => invoke("open_themes_folder"),
  pickThemeFile: async (title) => {
    const picked = await openDialog({
      multiple: false,
      title,
      filters: [{ name: "VS Code theme", extensions: ["json", "jsonc", "vsix"] }],
    });
    return typeof picked === "string" ? picked : null;
  },
  listModules: () => invoke("list_modules"),
  setModule: (id, enabled) => invoke("set_module", { id, enabled }),
  listSites: () => invoke("list_sites"),
  setSiteVersion: (domain, kind, version) => invoke("set_site_version", { domain, kind, version }),
  addSite: (site) => invoke("add_site", site),
  listStarters: () => invoke("list_starters"),
  pickImages: async (start) => {
    const picked = await openDialog({
      multiple: true,
      title: "Select images",
      defaultPath: start || undefined,
      filters: [{ name: "Images", extensions: ["jpg", "jpeg", "png"] }],
    });
    return Array.isArray(picked) ? picked : typeof picked === "string" ? [picked] : [];
  },
  findImages: (paths) => invoke("find_images", { paths }),
  optimizeImages: (paths, options) => invoke("optimize_images", { paths, options }),
  onImagesProgress: (handler) => listen<ImagesProgress>("images-progress", (e) => handler(e.payload)),
  pickVideos: async (start) => {
    const picked = await openDialog({
      multiple: true,
      title: "Select videos",
      defaultPath: start || undefined,
      filters: [{ name: "Videos and GIFs", extensions: ["mp4", "m4v", "mov", "webm", "mkv", "avi", "gif"] }],
    });
    return Array.isArray(picked) ? picked : typeof picked === "string" ? [picked] : [];
  },
  aiTools: () => invoke("ai_tools"),
  checkAiTools: () => invoke("ai_tools_check"),
  installAiTool: (id) => invoke("ai_tool_install", { id }),
  removeAiTool: (id) => invoke("ai_tool_remove", { id }),
  openAiTool: (id, folder) => invoke("ai_tool_open", { id, folder }),
  openAiToolDocs: (id) => invoke("ai_tool_docs", { id }),
  onAiProgress: (handler) => listen<AiProgress>("ai-progress", (e) => handler(e.payload)),
  ffmpegStatus: () => invoke("videos_status"),
  installFfmpeg: () => invoke("videos_install_ffmpeg"),
  removeFfmpeg: () => invoke("videos_remove_ffmpeg"),
  onFfmpegProgress: (handler) => listen<FfmpegProgress>("ffmpeg-progress", (e) => handler(e.payload)),
  findVideos: (paths) => invoke("find_videos", { paths }),
  optimizeVideos: (paths, options) => invoke("optimize_videos", { paths, options }),
  stopVideos: () => invoke("stop_videos"),
  onVideosProgress: (handler) => listen<VideosProgress>("videos-progress", (e) => handler(e.payload)),
  onDrop: (handler) =>
    getCurrentWebview().onDragDropEvent((e) => {
      if (e.payload.type === "drop") handler(e.payload.paths);
    }),
  addStarterSite: (site) => invoke("add_starter_site", site),
  onStarterProgress: (handler) => listen<StarterProgress>("starter-progress", (e) => handler(e.payload)),
  removeSite: (name) => invoke("remove_site", { name }),
  listParked: () => invoke("list_parked"),
  sitesFolder: () => invoke("sites_folder"),
  pickFolder: async (title, start) => {
    const picked = await openDialog({ directory: true, multiple: false, title, defaultPath: start || undefined });
    return typeof picked === "string" ? picked : null;
  },
  phpMyAdminStatus: () => invoke("phpmyadmin_status"),
  installPhpMyAdmin: () => invoke("phpmyadmin_install"),
  uninstallPhpMyAdmin: () => invoke("phpmyadmin_uninstall"),
  openPhpMyAdmin: () => invoke("phpmyadmin_open"),
  setupStatus: () => invoke("setup_status"),
  setupChoose: (home, sitesFolder, install) => invoke("setup_choose", { home, sitesFolder, install }),
  setupApply: () => invoke("setup_apply"),
  setupFinish: () => invoke("setup_finish"),
  parkFolder: (path) => invoke("park_folder", { path }),
  unparkFolder: (path) => invoke("unpark_folder", { path }),
  systemStatus: () => invoke("system_status"),
  runSystemSetup: () => invoke("run_system_setup"),
  terminalStatus: () => invoke("terminal_status"),
  setTerminal: (on) => invoke("set_terminal", { on }),
  onSitesChanged: (handler) => listen("sites-changed", () => handler()),
  consoleOpen: (folder, cols, rows) => invoke("console_open", { folder, cols, rows }),
  consoleWrite: (id, data) => invoke("console_write", { id, data }),
  consoleResize: (id, cols, rows) => invoke("console_resize", { id, cols, rows }),
  consoleClose: (id) => invoke("console_close", { id }),
  onConsoleOutput: (handler) =>
    listen<{ id: number; data: string }>("console-output", (e) => handler(e.payload.id, e.payload.data)),
  onConsoleExit: (handler) => listen<{ id: number }>("console-exit", (e) => handler(e.payload.id)),
  updateStatus: () => invoke("update_status"),
  checkForUpdate: () => invoke("check_for_update"),
  installUpdate: () => invoke("install_update"),
  seenWhatsNew: () => invoke("seen_whats_new"),
  onUpdateAvailable: (handler) => listen<UpdateStatus>("update-available", (e) => handler(e.payload)),
  onUpdateProgress: (handler) => listen<UpdateProgress>("update-progress", (e) => handler(e.payload)),
  listLogs: () => invoke("list_logs"),
  readLog: (id, from, lines) => invoke("read_log", { id, from, lines }),
  clearLogs: () => invoke("clear_logs"),
  clearLog: (id) => invoke("clear_log", { id }),
  dbServers: () => invoke("db_servers"),
  dbServer: (server) => invoke("db_server", { server }),
  dbList: (server) => invoke("db_list", { server }),
  dbCreate: (name, server) => invoke("db_create", { name, server }),
  dbDrop: (name, server) => invoke("db_drop", { name, server }),
  dbTables: (database, server) => invoke("db_tables", { database, server }),
  dbBrowse: (database, table, offset, limit, server) => invoke("db_browse", { database, table, offset, limit, server }),
  dbQuery: (database, sql, server) => invoke("db_query", { database, sql, server }),
  dbBackup: (database, server) => invoke("db_backup", { database, server }),
  dbBackups: () => invoke("db_backups"),
  dbRestore: (database, file, into, server) => invoke("db_restore", { database, file, into, server }),
  dbRemoveBackup: (database, file) => invoke("db_remove_backup", { database, file }),
  cleanupScan: () => invoke("cleanup_scan"),
  cleanupRun: (ids) => invoke("cleanup_run", { ids }),
  mcpStatus: () => invoke("mcp_status"),
  setMcpTool: (name, allowed) => invoke("set_mcp_tool", { name, allowed }),
  listTasks: () => invoke("list_tasks"),
  addTask: (task) => invoke("add_task", { task }),
  updateTask: (id, task) => invoke("update_task", { id, task }),
  setTaskEnabled: (id, enabled) => invoke("set_task_enabled", { id, enabled }),
  removeTask: (id) => invoke("remove_task", { id }),
  runTask: (id) => invoke("run_task", { id }),
  stopTask: (id) => invoke("stop_task", { id }),
  previewSchedule: (schedule) => invoke("preview_schedule", { schedule }),
  taskLog: (id) => invoke("task_log", { id }),
  remoteConnections: (site) => invoke("remote_connections", { site }),
  remoteSave: (id, input) => invoke("remote_save", { id, input }),
  remoteRemove: (id) => invoke("remote_remove", { id }),
  remoteTrust: (id, fingerprint) => invoke("remote_trust", { id, fingerprint }),
  remoteTest: (id) => invoke("remote_test", { id }),
  remoteBrowse: (id, dir) => invoke("remote_browse", { id, dir }),
  remotePlan: (id, direction, compare, del, paths) =>
    invoke("remote_plan", { id, direction, compare, delete: del, paths }),
  remoteRun: (id, files) => invoke("remote_run", { id, files }),
  remoteStop: (id) => invoke("remote_stop", { id }),
  remoteLocalFolder: (id) => invoke("remote_local_folder", { id }),
  onRemoteProgress: (handler) => listen<RemoteProgress>("remote-progress", (e) => handler(e.payload)),
  pickFiles: async (title, start, multiple = true) => {
    const picked = await openDialog({ directory: false, multiple, title, defaultPath: start || undefined });
    if (picked === null) return [];
    return Array.isArray(picked) ? picked : [picked];
  },
  pickSaveFile: async (title, name) =>
    saveDialog({ title, defaultPath: name, filters: [{ name: "Stackyard site", extensions: ["zip"] }] }),
  siteSettings: (domain) => invoke("site_settings", { domain }),
  saveSiteSettings: (domain, tld, https, settings) => invoke("save_site_settings", { domain, tld, https, settings }),
  siteBackups: (domain) => invoke("site_backups", { domain }),
  backupSite: (domain) => invoke("backup_site", { domain }),
  restoreSite: (domain, file) => invoke("restore_site", { domain, file }),
  removeSiteBackup: (domain, file) => invoke("remove_site_backup", { domain, file }),
  openSiteBackups: (domain) => invoke("open_site_backups", { domain }),
  exportSite: (domain, dest) => invoke("export_site", { domain, dest }),
  inspectSiteFile: (path) => invoke("inspect_site_file", { path }),
  importSite: (path, name, folder) => invoke("import_site", { path, name, folder }),
  shareStatus: () => invoke("share_status"),
  shareInstall: () => invoke("share_install"),
  shareUninstall: () => invoke("share_uninstall"),
  shareStart: (domain) => invoke("share_start", { domain }),
  shareStop: (domain) => invoke("share_stop", { domain }),
  openShareLink: (domain) => invoke("open_share_link", { domain }),
  gitStatus: (site) => invoke("git_status", { site }),
  gitLink: (site, url) => invoke("git_link", { site, url }),
  gitFetch: (site) => invoke("git_fetch", { site }),
  gitPull: (site) => invoke("git_pull", { site }),
  gitPush: (site) => invoke("git_push", { site }),
  gitCommit: (site, message, name, email) => invoke("git_commit", { site, message, name, email }),
};

let consoleCount = 0;
const consoleLine: Record<number, string> = {};
const consolePrompt: Record<number, string> = {};
const consoleOut = new Set<(id: number, data: string) => void>();

function browserApi(): Api {
  const family = (kind: RuntimeKind, name: string, versions: string[], available: boolean): RuntimeFamily => ({
    kind,
    name,
    available,
    versions: versions.map((version) => ({ version, installed: null, default: false, installing: false })),
  });
  let runtimes: RuntimeFamily[] = [
    family("php", "PHP", ["8.5", "8.4", "8.3", "8.2", "8.1", "8.0", "7.4"], true),
    family("node", "Node.js", ["26", "24", "22", "20"], true),
    family("python", "Python", ["3.15", "3.14", "3.13", "3.12", "3.11"], true),
    family("go", "Go", ["1.27", "1.26", "1.25"], true),
    family("java", "Java", ["25", "21", "17", "11"], true),
  ];
  // Sample state so previews show both installed and missing versions.
  runtimes[0].versions[1] = { version: "8.4", installed: "8.4.26", default: true, installing: false };
  runtimes[1].versions[2] = { version: "22", installed: "22.23.3", default: true, installing: false };
  const progressHandlers = new Set<(p: InstallProgress) => void>();
  const updateVersion = (kind: RuntimeKind, version: string, change: (v: RuntimeVersion) => RuntimeVersion) => {
    runtimes = runtimes.map((f) =>
      f.kind !== kind ? f : { ...f, versions: f.versions.map((v) => (v.version === version ? change(v) : v)) },
    );
  };
  const fixDefault = (kind: RuntimeKind) => {
    const f = runtimes.find((r) => r.kind === kind)!;
    if (f.versions.some((v) => v.default && v.installed)) return;
    const newest = f.versions.find((v) => v.installed);
    runtimes = runtimes.map((r) =>
      r.kind !== kind ? r : { ...r, versions: r.versions.map((v) => ({ ...v, default: v === newest })) },
    );
  };
  const service = (
    id: string,
    name: string,
    version: string,
    ports: number[],
    role: string,
    web: string | null,
    installed: string | null,
    connect: string | null = null,
  ): Service => ({ id, name, version, ports, role, web, connect, installed, installing: false, state: "stopped" });
  let services: Service[] = [
    service("caddy", "Caddy", "2", [80, 443], "Web server", "http://localhost", "2.10.2"),
    service("mysql", "MySQL", "8.4", [3306], "Database", null, "8.4.6", "mysql://root@127.0.0.1:3306"),
    service("mariadb", "MariaDB", "11.8", [3306], "Database", null, null, "mysql://root@127.0.0.1:3306"),
    service("postgres", "PostgreSQL", "18", [5432], "Database", null, "18.0", "postgresql://postgres@127.0.0.1:5432"),
    service("redis", "Redis", "8", [6379], "Cache and queues", null, null, "redis://127.0.0.1:6379"),
    service("mailpit", "Mailpit", "1", [1025, 8025], "Mail catcher", "http://127.0.0.1:8025", null, "smtp://127.0.0.1:1025"),
    service("meilisearch", "Meilisearch", "1", [7700], "Search engine", "http://127.0.0.1:7700", null, "http://127.0.0.1:7700"),
    service("mongodb", "MongoDB", "9.0", [27017], "Document database", null, null, "mongodb://127.0.0.1:27017"),
    service(
      "seaweedfs",
      "SeaweedFS",
      "4",
      [8333, 23646, 9333, 9340, 8888],
      "S3-compatible storage",
      "http://127.0.0.1:23646",
      null,
      "http://127.0.0.1:8333 · key stackyard · secret stackyard-secret",
    ),
    service("nats", "NATS", "2", [4222, 8222], "Message queue", "http://127.0.0.1:8222", null, "nats://127.0.0.1:4222"),
  ];
  const serviceHandlers = new Set<(p: ServiceProgress) => void>();
  const starterHandlers = new Set<(p: StarterProgress) => void>();
  const imageHandlers = new Set<(p: ImagesProgress) => void>();
  const videoHandlers = new Set<(p: VideosProgress) => void>();
  const ffmpegHandlers = new Set<(p: FfmpegProgress) => void>();
  const aiHandlers = new Set<(p: AiProgress) => void>();
  const aiTool = (
    id: string,
    name: string,
    maker: string,
    about: string,
    pkg: string,
    command: string,
    min_node: number,
    docs: string,
  ): AiTool => ({ id, name, maker, about, package: pkg, command, min_node, docs, installed: null, node: null, latest: null, update: false, installing: false });
  let aiTools: AiTool[] = [
    { ...aiTool("claude", "Claude Code", "Anthropic", "Anthropic's coding agent. Sign in with a Claude account or an API key.", "@anthropic-ai/claude-code", "claude", 22, "https://code.claude.com/docs/en/setup"), installed: "2.1.296", node: "24" },
    { ...aiTool("codex", "Codex CLI", "OpenAI", "OpenAI's coding agent. Sign in with ChatGPT or an API key.", "@openai/codex", "codex", 18, "https://github.com/openai/codex"), installed: "0.163.0", node: "24" },
    aiTool("gemini", "Gemini CLI", "Google", "Google's coding agent. Sign in with a Google account or an API key.", "@google/gemini-cli", "gemini", 20, "https://github.com/google-gemini/gemini-cli"),
    aiTool("copilot", "GitHub Copilot CLI", "GitHub", "GitHub's coding agent. Needs a Copilot plan.", "@github/copilot", "copilot", 22, "https://github.com/github/copilot-cli"),
    aiTool("qwen", "Qwen Code", "Qwen (Alibaba)", "Coding agent for Qwen models. Sign in with Qwen or use an API key.", "@qwen-code/qwen-code", "qwen", 22, "https://github.com/QwenLM/qwen-code"),
    aiTool("opencode", "OpenCode", "SST", "Open-source coding agent that works with many model providers.", "@opencode/cli", "opencode", 20, "https://github.com/sst/opencode"),
  ];
  const aiLatest: Record<string, string> = { claude: "2.1.302", codex: "0.163.0", gemini: "0.64.0", copilot: "1.0.98", qwen: "0.25.2", opencode: "2.0.32" };
  let ffmpeg: FfmpegStatus = { path: null, downloaded: false, version: null, installing: false };
  let videosStopped = false;
  const logs: Record<string, string[]> = {};
  const updateService = (id: string, change: (s: Service) => Service) => {
    services = services.map((s) => (s.id === id ? change(s) : s));
    return services;
  };
  const mod = (
    id: string,
    name: string,
    group: Module["group"],
    description: string,
    on: boolean,
    available: boolean,
  ): Module => ({ id, name, group, description, on_by_default: on, available, enabled: on && available });
  let pma: PhpMyAdminStatus = { installed: null, enabled: false, installing: false, url: null, local_url: null, problem: null };
  let modules: Module[] = [
    mod("php", "PHP", "runtime", "PHP versions for sites and the shell", true, true),
    mod("node", "Node.js", "runtime", "Node.js versions for sites and the shell", true, true),
    mod("python", "Python", "runtime", "Python versions for Django, Flask and FastAPI sites and the shell", false, true),
    mod("go", "Go", "runtime", "Go toolchains for Go sites and the shell", false, true),
    mod("java", "Java", "runtime", "Java JDKs (Eclipse Temurin) for Spring Boot, Quarkus and Micronaut sites and the shell", false, true),
    mod("caddy", "Caddy", "service", "Web server for local sites, with automatic HTTPS", true, true),
    mod("mysql", "MySQL", "service", "MySQL database server", true, true),
    mod("mariadb", "MariaDB", "service", "MySQL-compatible database (Windows and Linux)", false, true),
    mod("mailpit", "Mailpit", "service", "Catches outgoing mail for testing", false, true),
    mod("meilisearch", "Meilisearch", "service", "Fast search engine for apps, with a search preview page", false, true),
    mod("postgres", "PostgreSQL", "service", "PostgreSQL database server", false, true),
    mod("redis", "Redis", "service", "Cache and queues (macOS and Linux)", false, true),
    mod("mongodb", "MongoDB", "service", "Document database (MongoDB Community Server, downloaded from mongodb.com)", false, true),
    mod("seaweedfs", "SeaweedFS", "service", "S3-compatible storage for uploads and buckets, like MinIO or AWS S3", false, true),
    mod("nats", "NATS", "service", "Message queue with JetStream, for jobs and events between apps", false, true),
    mod("shell", "Stackyard Shell", "feature", "Terminal and PATH shims with each project's versions", true, true),
    mod("updates", "Updates", "feature", "Checks for new Stackyard versions every half hour and tells you; you choose when to install", true, true),
    mod("logs", "Log viewer", "feature", "Live logs from every service, with search", true, true),
    mod("ai", "AI coding tools", "feature", "Install Claude Code, Codex, Gemini CLI and more on Stackyard's Node.js, and open them in a site's folder", true, true),
    mod("mcp", "MCP server", "feature", "Lets AI coding tools see and manage sites and services", false, true),
    mod("databases", "Databases screen", "feature", "Browse tables, run queries, back up and restore MySQL, MariaDB and PostgreSQL", true, true),
    mod("remote", "Remote servers", "feature", "Upload and download site files over SFTP, FTPS or FTP (Cloudways, cPanel, any VPS)", true, true),
    mod("git", "Git", "feature", "Link sites to GitHub or any Git host, then pull, commit and push (uses Git on your computer)", true, true),
    mod("sitesettings", "Site settings", "feature", "Per-site folder served, domain ending, HTTPS, environment variables, PHP settings and a start command", true, true),
    mod("sitebackups", "Site backups", "feature", "Back up a site's files and database in one zip, and restore it", true, true),
    mod("sitetransfer", "Site import and export", "feature", "Pack a site, with its database and settings, into one file to open on another computer", true, true),
    mod("wordpress", "WordPress tools", "feature", "Plugins, themes, users, debug log, search and replace and WooCommerce tools for WordPress sites (WP-CLI is downloaded when you first use them)", true, true),
    mod("share", "Public links", "feature", "Share a local site at a temporary public address through Cloudflare's free quick tunnels (cloudflared is downloaded when you first use it)", false, true),
    mod("terminal", "Terminal", "feature", "A terminal in the window's panel that opens in a site's folder with its PHP and Node.js versions", true, true),
    mod("scheduler", "Task scheduler", "feature", "Run commands on a schedule in a project folder, like cron", false, true),
    mod("images", "Image optimizer", "feature", "Make WebP and AVIF copies of JPG and PNG images, and smaller JPGs and PNGs", true, true),
    mod("videos", "Video optimizer", "feature", "Make web-ready MP4, WebM and GIF copies of videos with FFmpeg (downloaded when you first use it)", true, true),
  ];
  let dbs: Database[] = [
    { name: "blog", tables: 3, size: 360448 },
    { name: "shop", tables: 3, size: 1769472 },
  ];
  let pgDbs: Database[] = [{ name: "analytics", tables: 2, size: 8_421_376 }];
  let backups: DbBackup[] = [
    { server: "mysql", database: "shop", file: "shop-20261014-020000.sql.gz", size: 412377, created: Date.now() - 22 * 3600_000 },
  ];
  // A rough stand-in for the core's schedule reading, enough for previews.
  const samplePreview = (schedule: string): SchedulePreview => {
    const s = schedule.trim().replace(/\s+/g, " ");
    const known: Record<string, [string, number]> = {
      "* * * * *": ["Every minute", 1],
      "*/5 * * * *": ["Every 5 minutes", 5],
      "*/15 * * * *": ["Every 15 minutes", 15],
      "0 * * * *": ["Every hour, on the hour", 60],
      "0 0 * * *": ["Every day at 00:00", 1440],
      "@daily": ["Every day at 00:00", 1440],
      "@hourly": ["Every hour, on the hour", 60],
      "30 2 * * *": ["Every day at 02:30", 1440],
    };
    if (s.split(" ").length !== 5 && !s.startsWith("@"))
      throw "a schedule has five parts: minute hour day month weekday, e.g. `*/5 * * * *`";
    const [description, every] = known[s] ?? [null, 60];
    const start = Math.ceil(Date.now() / 60_000) * 60_000;
    const first = every === 1 ? 0 : every;
    return { description, runs: [0, 1, 2].map((i) => start + (first + i * every) * 60_000) };
  };
  const task = (id: string, input: TaskInput, preview: SchedulePreview, last: TaskRun | null): Task => ({
    ...input,
    id,
    description: preview.description,
    next_run: input.enabled ? preview.runs[0] : null,
    last,
    running_since: null,
    problem: null,
  });
  let tasks: Task[] = [
    task(
      "k3v9xq2m",
      { name: "Laravel scheduler (blog)", command: "php artisan schedule:run", folder: "~/Code/blog", schedule: "* * * * *", enabled: true },
      samplePreview("* * * * *"),
      { started: Date.now() - 40_000, took_ms: 420, outcome: "succeeded", exit_code: 0, message: null, by_hand: false },
    ),
    task(
      "p7c2wd8n",
      { name: "Nightly backup", command: "./scripts/backup.sh", folder: "~/Code/shop", schedule: "30 2 * * *", enabled: true },
      samplePreview("30 2 * * *"),
      { started: Date.now() - 20 * 3600_000, took_ms: 8300, outcome: "failed", exit_code: 1, message: null, by_hand: false },
    ),
  ];
  let tld = ".stack";
  const previewFiles: Record<InterfaceFileName, InterfaceFile> = {
    settings: { path: "C:\\stackyard\\settings.toml", values: {}, error: null },
    keybindings: { path: "C:\\stackyard\\keybindings.toml", values: {}, error: null },
  };
  (window as unknown as { stackyardFiles: typeof previewFiles }).stackyardFiles = previewFiles;
  // Added themes, empty in the preview; push one from the console to try it.
  const previewThemes: ThemeFiles = { folder: "C:\\stackyard\\themes", themes: [], problems: [] };
  (window as unknown as { stackyardThemes: ThemeFiles }).stackyardThemes = previewThemes;
  const sample = (name: string, runtime: string, framework: string | null, parked: boolean, problem: string | null = null): Site => ({
    name,
    domain: `${name}.stack`,
    url: `https://${name}.stack`,
    local_url: `https://${name}.localhost`,
    path: `~/Code/${name}`,
    root: `~/Code/${name}${framework === "Laravel" ? "/public" : ""}`,
    runtime,
    framework,
    https: true,
    parked,
    problem,
    versions: [],
  });
  const isInstalled = (kind: RuntimeKind, version: string) =>
    runtimes.some((f) => f.kind === kind && f.versions.some((v) => v.version === version && v.installed));
  /** What the app reports for a site: its own runtime, then Node.js. */
  const withVersions = (site: Site): Site => {
    const [label, version] = site.runtime.split(" ");
    const labels: Record<string, RuntimeKind> = { PHP: "php", Node: "node", Python: "python", Go: "go", Java: "java" };
    const main: RuntimeKind | null = labels[label] ?? null;
    const own = main && {
      kind: main,
      version,
      installed: isInstalled(main, version),
      from: site.parked ? ("default" as const) : ("site" as const),
      file: null,
      can_unset: site.parked,
    };
    const node = site.versions.find((v) => v.kind === "node" && v.from === "pinned");
    const versions: SiteVersion[] = own ? [own] : [];
    if (main !== "node")
      versions.push(
        node ?? { kind: "node", version: "22", installed: isInstalled("node", "22"), from: "default", file: null, can_unset: true },
      );
    return { ...site, versions };
  };
  let sites: Site[] = [
    sample("blog", "PHP 8.4", "Laravel", true),
    sample("shop", "Node 22 · dev server on port 5173", "Vite", true),
    sample("legacy", "PHP 7.4", "WordPress", false, "PHP 7.4 is not installed"),
  ];
  let parked = ["~/Code"];
  let system: SystemStatus = {
    needed: ["Send .stack names to Stackyard", "Trust Stackyard's local HTTPS certificate"],
    waiting_for_certificate: false,
    resolver: "mac-resolver",
    dns_running: true,
  };
  let terminal: TerminalStatus = {
    available: true,
    on: false,
    folder: "~/Library/Application Support/Stackyard/bin",
    places: [],
  };
  const tool = (name: string, title: string, description: string, action: boolean): McpTool => ({
    name,
    title,
    description,
    action,
    allowed: !action,
  });
  let mcp: McpStatus = {
    setup: {
      program: "~/Library/Application Support/Stackyard/bin/stackyard",
      claude_code: 'claude mcp add --scope user stackyard -- "~/Library/Application Support/Stackyard/bin/stackyard" mcp',
      json: JSON.stringify(
        { mcpServers: { stackyard: { command: "~/Library/Application Support/Stackyard/bin/stackyard", args: ["mcp"] } } },
        null,
        2,
      ),
    },
    tools: [
      tool("list_sites", "List sites", "Lists the local sites Stackyard serves.", false),
      tool("list_services", "List services", "Lists the services and whether they run.", false),
      tool("list_runtimes", "List runtimes", "Lists installed PHP and Node.js versions.", false),
      tool("list_databases", "List databases", "Lists MySQL or MariaDB databases and their tables.", false),
      tool("read_logs", "Read a log", "Returns the last lines of a service's log.", false),
      tool("start_service", "Start a service", "Starts an installed service in the background.", true),
      tool("stop_service", "Stop a service", "Stops a running service.", true),
      tool("restart_service", "Restart a service", "Stops a service and starts it again.", true),
      tool("create_site", "Create a site", "Adds a local site for a project folder.", true),
      tool("set_site_runtime", "Switch a site's version", "Switches the PHP or Node.js version a site runs on.", true),
      tool("query_database", "Query a database", "Runs one read-only SELECT on MySQL or MariaDB.", true),
      tool("change_database", "Change a database", "Runs SQL that changes data or tables; never drops a database.", true),
      tool("list_wordpress_environments", "List WordPress environments", "Lists WordPress sites, their stagings and live sites.", false),
      tool("create_wordpress_staging", "Create a WordPress staging", "Makes a full copy of a WordPress site to try changes on.", true),
      tool("run_wp_cli", "Run WP-CLI", "Runs a WP-CLI command in a local WordPress site.", true),
      tool("merge_wordpress_staging", "Merge a WordPress staging", "Lists or merges a staging's changes into another copy.", true),
      tool("destroy_wordpress_staging", "Destroy a WordPress staging", "Moves a staging to the trash for 7 days.", true),
    ],
    activity: [
      { at: Date.now() - 60_000, client: "claude-code", tool: "list_sites", summary: "", ok: true },
      {
        at: Date.now() - 30_000,
        client: "claude-code",
        tool: "start_service",
        summary: "caddy",
        ok: false,
        error: "Start a service is turned off.",
      },
    ],
  };
  const logTicks: Record<string, number> = {};
  const warn = (t: string) =>
    t === ".local" ? ".local is used by Bonjour/mDNS for devices on your network" : null;
  // What the preview's terminal prints for a command, worded like the real one.
  let nextPid = 4205;
  const consoleReply = (command: string): string[] => {
    const installed = (kind: RuntimeKind) =>
      runtimes.find((f) => f.kind === kind)?.versions.find((v) => v.default && v.installed)?.installed;
    if (command === "stackyard status") {
      const shown = services.filter((s) => s.installed);
      const width = Math.max(...shown.map((s) => s.name.length));
      const web = shown.some((s) => s.id === "caddy" && s.state === "running");
      return [
        "Stackyard is running in the Stackyard app (pid 4180).",
        ...shown.map((s) => `  ${s.name.padEnd(width)} ${s.state === "running" ? `running (pid ${s.pid})` : s.state}`),
        ...(web ? sites.map((s) => `  ${s.https ? "https" : "http"}://${s.domain}`) : []),
      ];
    }
    if (command === "php -v" && installed("php")) return [`PHP ${installed("php")} (cli) (NTS)`];
    if (command === "node -v" && installed("node")) return [`v${installed("node")}`];
    return [command, "(the browser preview has no real shell)"];
  };
  const mock: Api = {
    listRuntimes: async () => runtimes,
    installRuntime: async (kind, version) => {
      updateVersion(kind, version, (v) => ({ ...v, installing: true }));
      const total = 30_000_000;
      for (let done = 0; done <= total; done += 3_000_000) {
        progressHandlers.forEach((h) => h({ kind, version, step: "downloading", done, total }));
        await new Promise((r) => setTimeout(r, 120));
      }
      progressHandlers.forEach((h) => h({ kind, version, step: "unpacking" }));
      await new Promise((r) => setTimeout(r, 300));
      updateVersion(kind, version, (v) => ({ ...v, installing: false, installed: `${version}.0` }));
      fixDefault(kind);
      return runtimes;
    },
    uninstallRuntime: async (kind, version) => {
      updateVersion(kind, version, (v) => ({ ...v, installed: null, default: false }));
      fixDefault(kind);
      return runtimes;
    },
    setDefaultRuntime: async (kind, version) => {
      runtimes = runtimes.map((f) =>
        f.kind !== kind ? f : { ...f, versions: f.versions.map((v) => ({ ...v, default: v.version === version })) },
      );
      return runtimes;
    },
    onInstallProgress: async (handler) => {
      progressHandlers.add(handler);
      return () => progressHandlers.delete(handler);
    },
    listServices: async () => services,
    installService: async (id) => {
      updateService(id, (s) => ({ ...s, installing: true }));
      for (let done = 0; done <= 40_000_000; done += 5_000_000) {
        serviceHandlers.forEach((h) => h({ id, step: "downloading", done, total: 40_000_000 }));
        await new Promise((r) => setTimeout(r, 100));
      }
      return updateService(id, (s) => ({ ...s, installing: false, installed: `${s.version}.0` }));
    },
    uninstallService: async (id) => updateService(id, (s) => ({ ...s, installed: null, state: "stopped" })),
    startService: async (id) => {
      const busy = services.find((s) => s.id !== id && s.state === "running" && s.ports.some((p) => services.find((x) => x.id === id)!.ports.includes(p)));
      if (busy) throw `port ${busy.ports[0]} is already in use, so ${services.find((s) => s.id === id)!.name} cannot start`;
      logs[id] = [...(logs[id] ?? []), `started ${id} (sample log line)`];
      return updateService(id, (s) => ({ ...s, state: "running", pid: (nextPid += 37) }) as Service);
    },
    stopService: async (id) => {
      logs[id] = [...(logs[id] ?? []), `stopped ${id}`];
      return updateService(id, (s) => ({ ...s, state: "stopped" }) as Service);
    },
    serviceLog: async (id) => logs[id] ?? [],
    openLocalPage: async (url) => void window.open(url, "_blank", "noopener"),
    onServiceProgress: async (handler) => {
      serviceHandlers.add(handler);
      return () => serviceHandlers.delete(handler);
    },
    getTld: async () => ({ tld, warning: warn(tld) }),
    setTld: async (value) => {
      const clean = value.trim().replace(/^\.+/, "").toLowerCase();
      if (!/^[a-z0-9-]+(\.[a-z0-9-]+)*$/.test(clean)) throw `invalid domain ending "${value}"`;
      tld = `.${clean}`;
      return { tld, warning: warn(tld) };
    },
    languageFiles: async () => ({ folder: "C:\\stackyard\\languages", languages: [], problems: [] }),
    openLanguagesFolder: async () => {},
    // The preview keeps both files in memory; edit them from the console with
    // window.stackyardFiles to see live reload and the Problems panel.
    interfaceSettings: async () => ({ ...previewFiles.settings }),
    saveInterfaceSettings: async (values) => void (previewFiles.settings = { ...previewFiles.settings, values, error: null }),
    keybindings: async () => ({ ...previewFiles.keybindings }),
    saveKeybindings: async (values) => void (previewFiles.keybindings = { ...previewFiles.keybindings, values, error: null }),
    showInterfaceFile: async () => {},
    themeFiles: async () => ({ ...previewThemes, themes: [...previewThemes.themes] }),
    importTheme: async () => {
      throw "Importing a theme needs the Stackyard app.";
    },
    removeTheme: async (id) => void (previewThemes.themes = previewThemes.themes.filter((t) => t.id !== id)),
    openThemesFolder: async () => {},
    pickThemeFile: async () => "C:\\Users\\you\\Downloads\\theme.json",
    listModules: async () => modules,
    setModule: async (id, enabled) => {
      modules = modules.map((m) => (m.id === id && m.available ? { ...m, enabled } : m));
      return modules;
    },
    listSites: async () => (sites = sites.map(withVersions)),
    setSiteVersion: async (domain, kind, version) => {
      sites = sites.map((site) => {
        if (site.domain !== domain) return site;
        const own = site.runtime.toLowerCase().startsWith(kind);
        if (own && version) {
          const rest = site.runtime.split(" ").slice(2).join(" ");
          const name = { php: "PHP", node: "Node", python: "Python", go: "Go", java: "Java" }[kind];
          return withVersions({ ...site, runtime: [name, version, rest].filter(Boolean).join(" "), problem: null });
        }
        const pinned: SiteVersion[] = version
          ? [{ kind, version, installed: isInstalled(kind, version), from: "pinned", file: ".stackyard", can_unset: true }]
          : [];
        return withVersions({ ...site, versions: pinned });
      });
      return sites;
    },
    addSite: async ({ name, path, runtime, https }) => {
      if (!/^[a-z0-9]([a-z0-9-]*[a-z0-9])?$/.test(name)) throw `invalid site name "${name}"`;
      const domain = name + tld;
      if (sites.some((s) => s.domain === domain)) throw `a site named "${domain}" already exists`;
      const scheme = https ? "https" : "http";
      const site: Site = {
        ...sample(name, runtime === "Auto" ? "Static" : runtime, null, false),
        domain,
        url: `${scheme}://${domain}`,
        local_url: `${scheme}://${name}.localhost`,
        path,
        root: path,
        https,
      };
      sites = [site, ...sites];
      return sites;
    },
    listStarters: async () => [
      { id: "wordpress", name: "WordPress", description: "The latest WordPress, with its database made in MySQL or MariaDB" },
      { id: "laravel", name: "Laravel", description: "A new Laravel app, made with Composer" },
      { id: "astro", name: "Astro", description: "A new Astro site; Stackyard runs its dev server" },
    ],
    addStarterSite: async ({ name, starter, path, https }) => {
      for (const message of ["Downloading… 40%", "Downloading… 100%", "Setting it up…"]) {
        starterHandlers.forEach((h) => h({ name, message }));
        await new Promise((r) => setTimeout(r, 400));
      }
      const runtime = starter === "astro" ? "Node 22" : "PHP 8.4";
      const all = await mock.addSite({ name, path: path || `C:\\stackyard\\www\\${name}`, runtime, https });
      const label = { wordpress: "WordPress", laravel: "Laravel", astro: "Astro" }[starter] ?? null;
      sites = all.map((s) => (s.name === name ? { ...s, framework: label } : s));
      return sites;
    },
    pickImages: async () => {
      const typed = window.prompt("Image paths, separated by commas", "C:\\stackyard\\www\\blog\\hero.jpg");
      return typed ? typed.split(",").map((p) => p.trim()).filter(Boolean) : [];
    },
    findImages: async (paths) => ({ count: paths.length * 6, size: paths.length * 9_400_000 }),
    optimizeImages: async (paths, options) => {
      const files = paths.flatMap((p) =>
        /\.(jpe?g|png)$/i.test(p) ? [p] : ["hero.jpg", "team.png", "logo.png"].map((f) => `${p}\\img\\${f}`),
      );
      const results: ImageResult[] = [];
      for (const [i, source] of files.entries()) {
        await new Promise((r) => setTimeout(r, 350));
        const size = 1_200_000 + i * 300_000;
        const ratio: Record<ImageFormat, number> = { webp: 0.18, avif: 0.11, same: 0.62 };
        const result: ImageResult = {
          source,
          size,
          error: null,
          made: options.formats.map((format) => ({
            format,
            path: source.replace(/\.[a-z]+$/i, format === "same" ? ".min$&" : `.${format}`),
            size: Math.round(size * ratio[format] * (options.quality / 80)),
            skipped: null,
          })),
        };
        results.push(result);
        imageHandlers.forEach((h) => h({ done: i + 1, total: files.length, result }));
      }
      return results;
    },
    onImagesProgress: async (handler) => {
      imageHandlers.add(handler);
      return () => imageHandlers.delete(handler);
    },
    pickVideos: async () => {
      const typed = window.prompt("Video paths, separated by commas", "C:\\stackyard\\www\\blog\\intro.mp4");
      return typed ? typed.split(",").map((p) => p.trim()).filter(Boolean) : [];
    },
    aiTools: async () => aiTools,
    checkAiTools: async () => {
      await new Promise((r) => setTimeout(r, 300));
      return (aiTools = aiTools.map((t) => {
        const latest = aiLatest[t.id] ?? null;
        return { ...t, latest, update: !!t.installed && !!latest && t.installed !== latest };
      }));
    },
    installAiTool: async (id) => {
      const tool = aiTools.find((t) => t.id === id);
      if (!tool) throw new Error(`no AI tool "${id}"`);
      aiHandlers.forEach((h) => h({ id, text: `Installing ${tool.name} with npm…` }));
      await new Promise((r) => setTimeout(r, 1500));
      const latest = aiLatest[id] ?? "1.0.0";
      return (aiTools = aiTools.map((t) => (t.id === id ? { ...t, installed: latest, latest, update: false, node: "24" } : t)));
    },
    removeAiTool: async (id) => (aiTools = aiTools.map((t) => (t.id === id ? { ...t, installed: null, node: null, update: false } : t))),
    openAiTool: async () => {},
    openAiToolDocs: async (id) => {
      const docs = aiTools.find((t) => t.id === id)?.docs;
      if (docs) window.open(docs, "_blank");
    },
    onAiProgress: async (handler) => {
      aiHandlers.add(handler);
      return () => aiHandlers.delete(handler);
    },
    ffmpegStatus: async () => ffmpeg,
    installFfmpeg: async () => {
      for (const done of [0, 40, 90, 156]) {
        ffmpegHandlers.forEach((h) => h({ step: "downloading", done: done * 1_000_000, total: 156_000_000 }));
        await new Promise((r) => setTimeout(r, 300));
      }
      ffmpegHandlers.forEach((h) => h({ step: "unpacking" }));
      ffmpeg = { path: "C:\\stackyard\\tools\\ffmpeg\\bin\\ffmpeg.exe", downloaded: true, version: "ffmpeg version n9.0", installing: false };
      return ffmpeg;
    },
    removeFfmpeg: async () => (ffmpeg = { path: null, downloaded: false, version: null, installing: false }),
    onFfmpegProgress: async (handler) => {
      ffmpegHandlers.add(handler);
      return () => ffmpegHandlers.delete(handler);
    },
    findVideos: async (paths) => ({ count: paths.length * 2, size: paths.length * 48_000_000 }),
    optimizeVideos: async (paths, options) => {
      videosStopped = false;
      const files = paths.flatMap((p) => (/\.[a-z0-9]+$/i.test(p) ? [p] : [`${p}\\media\\intro.mp4`, `${p}\\media\\demo.mov`]));
      const results: VideoResult[] = [];
      const ratio: Record<VideoFormat, number> = { mp4: 0.32, webm: 0.24, gif: 1.6 };
      for (const [i, source] of files.entries()) {
        for (const format of options.formats) {
          for (const percent of [0, 35, 70, 100]) {
            if (videosStopped) return results;
            videoHandlers.forEach((h) => h({ done: i, total: files.length, current: source, format, percent, result: null }));
            await new Promise((r) => setTimeout(r, 200));
          }
        }
        const size = 24_000_000 + i * 6_000_000;
        const result: VideoResult = {
          source,
          size,
          error: null,
          made: options.formats.map((format) => ({
            format,
            path: source.replace(/\.[a-z0-9]+$/i, `.${format}`),
            size: Math.round(size * ratio[format]),
            skipped: null,
          })),
        };
        results.push(result);
        videoHandlers.forEach((h) => h({ done: i + 1, total: files.length, current: null, format: null, percent: null, result }));
      }
      return results;
    },
    stopVideos: async () => {
      videosStopped = true;
    },
    onVideosProgress: async (handler) => {
      videoHandlers.add(handler);
      return () => videoHandlers.delete(handler);
    },
    onDrop: async () => () => {},
    onStarterProgress: async (handler) => {
      starterHandlers.add(handler);
      return () => starterHandlers.delete(handler);
    },
    removeSite: async (name) => {
      sites = sites.filter((s) => s.domain !== name && s.name !== name);
      return sites;
    },
    listParked: async () => parked,
    sitesFolder: async () => "C:\\stackyard\\www",
    pickFolder: async (_title, start) => window.prompt("Folder", start ?? "") || null,
    setupStatus: async () => ({
      needed: new URLSearchParams(location.search).has("setup"),
      home: "C:\\stackyard",
      default_home: "C:\\stackyard",
      default_sites_folder: "C:\\stackyard\\www",
      existing: null,
      choices: null,
      move_error: null,
      offers: [
        { id: "php", name: "PHP", group: "runtime", description: "PHP versions for sites and the shell", version: "8.5", recommended: true },
        { id: "node", name: "Node.js", group: "runtime", description: "Node.js versions for sites and the shell", version: "24", recommended: true },
        { id: "python", name: "Python", group: "runtime", description: "Python versions for Django, Flask and FastAPI sites and the shell", version: "3.15", recommended: false },
        { id: "go", name: "Go", group: "runtime", description: "Go toolchains for Go sites and the shell", version: "1.27", recommended: false },
        { id: "java", name: "Java", group: "runtime", description: "Java JDKs (Eclipse Temurin) for Spring Boot, Quarkus and Micronaut sites and the shell", version: "25", recommended: false },
        { id: "caddy", name: "Caddy", group: "service", description: "Web server for local sites, with automatic HTTPS", version: null, recommended: true },
        { id: "mysql", name: "MySQL", group: "service", description: "MySQL database server", version: null, recommended: true },
        { id: "mariadb", name: "MariaDB", group: "service", description: "MySQL-compatible database (Windows and Linux)", version: null, recommended: false },
        { id: "phpmyadmin", name: "phpMyAdmin", group: "service", description: "Full database manager in your browser at phpmyadmin.stack: import, export, edit tables", version: null, recommended: true },
        { id: "mailpit", name: "Mailpit", group: "service", description: "Catches outgoing mail for testing", version: null, recommended: true },
        { id: "postgres", name: "PostgreSQL", group: "service", description: "PostgreSQL database server", version: null, recommended: false },
        { id: "meilisearch", name: "Meilisearch", group: "service", description: "Fast search engine for apps", version: null, recommended: false },
        { id: "mongodb", name: "MongoDB", group: "service", description: "Document database", version: null, recommended: false },
        { id: "seaweedfs", name: "SeaweedFS", group: "service", description: "S3-compatible storage", version: null, recommended: false },
        { id: "nats", name: "NATS", group: "service", description: "Message queue", version: null, recommended: false },
      ],
    }),
    phpMyAdminStatus: async () => pma,
    installPhpMyAdmin: async () => {
      await new Promise((r) => setTimeout(r, 800));
      pma = { ...pma, installed: "5.2.2", enabled: true, url: "https://phpmyadmin.stack", local_url: "https://phpmyadmin.localhost" };
      return pma;
    },
    uninstallPhpMyAdmin: async () => {
      pma = { ...pma, installed: null, enabled: false, url: null, local_url: null };
      return pma;
    },
    openPhpMyAdmin: async () => {},
    setupChoose: async () => false,
    setupApply: async () => {},
    setupFinish: async () => {},
    parkFolder: async (path) => {
      if (!path.trim()) throw "choose a folder";
      parked = parked.includes(path) ? parked : [...parked, path];
      return parked;
    },
    unparkFolder: async (path) => {
      parked = parked.filter((p) => p !== path);
      sites = sites.filter((s) => !s.parked || parked.length > 0);
      return parked;
    },
    systemStatus: async () => system,
    runSystemSetup: async () => {
      await new Promise((r) => setTimeout(r, 400));
      system = { ...system, needed: [] };
      return system;
    },
    terminalStatus: async () => terminal,
    setTerminal: async (on) => {
      terminal = { ...terminal, on, places: on ? ["~/.zshrc"] : [] };
      return terminal;
    },
    onSitesChanged: async () => () => {},
    // The preview has no shell: the terminal answers a few commands from the
    // sample data and repeats anything else.
    consoleOpen: async (folder) => {
      const id = ++consoleCount;
      consoleLine[id] = "";
      const prompt = `${folder ?? "~"} $ `;
      setTimeout(() => consoleOut.forEach((h) => h(id, prompt)), 50);
      consolePrompt[id] = prompt;
      return id;
    },
    consoleWrite: async (id, data) => {
      for (const ch of data) {
        if (ch === "\r") {
          const typed = consoleLine[id];
          consoleLine[id] = "";
          const reply = typed.trim() ? consoleReply(typed.trim()) : [];
          consoleOut.forEach((h) => h(id, `\r\n${reply.map((l) => `${l}\r\n`).join("")}${consolePrompt[id]}`));
        } else if (ch === "\u007f") {
          if (consoleLine[id]) {
            consoleLine[id] = consoleLine[id].slice(0, -1);
            consoleOut.forEach((h) => h(id, "\b \b"));
          }
        } else if (ch >= " ") {
          consoleLine[id] += ch;
          consoleOut.forEach((h) => h(id, ch));
        }
      }
    },
    consoleResize: async () => {},
    consoleClose: async () => {},
    onConsoleOutput: async (handler) => {
      consoleOut.add(handler);
      return () => void consoleOut.delete(handler);
    },
    onConsoleExit: async () => () => {},
    updateStatus: async () => update,
    checkForUpdate: async () => {
      await new Promise((r) => setTimeout(r, 600));
      update = {
        ...update,
        checked_at: Date.now(),
        available: {
          version: "0.2.0",
          notes: "- Databases screen: browse tables and run SQL.\n- Task scheduler.\n- Disk space in Settings.",
        },
      };
      return update;
    },
    installUpdate: async () => {
      update = { ...update, installing: true };
    },
    seenWhatsNew: async () => {
      update = { ...update, just_updated: false };
      return update;
    },
    onUpdateAvailable: async () => () => {},
    onUpdateProgress: async () => () => {},
    listLogs: async () => [
      { id: "caddy", name: "Caddy", size: 48_213, modified: Date.now() },
      { id: "mysql", name: "MySQL", size: 9_120, modified: Date.now() - 600_000 },
      { id: "php-8.4", name: "PHP 8.4", size: 2_048, modified: Date.now() - 60_000 },
    ],
    readLog: async (id, from) => {
      const at = () => new Date().toISOString();
      if (from === null) {
        logTicks[id] = 0;
        return {
          lines: [
            `${at()} [info] ${id} started`,
            `${at()} [info] listening on 127.0.0.1`,
            `${at()} [warn] certificate for blog.stack expires in 6 days`,
            `${at()} [error] upstream php-8.4 refused the connection`,
            `${at()} [info] GET https://blog.stack/ 200 12ms`,
          ],
          offset: 5,
          reset: false,
        };
      }
      logTicks[id] = (logTicks[id] ?? 0) + 1;
      const lines = logTicks[id] % 2 ? [`${at()} [info] GET https://shop.stack/api/cart 200 ${logTicks[id]}ms`] : [];
      return { lines, offset: from + lines.length, reset: false };
    },
    clearLogs: async () => 3,
    clearLog: async () => true,
    dbServers: async () => ["mysql", "postgres"],
    dbServer: async (server) =>
      server === "postgres"
        ? { server: { id: "postgres", name: "PostgreSQL 18", version: "18.0" }, problem: null }
        : { server: { id: "mysql", name: "MySQL 8.4", version: "8.4.6" }, problem: null },
    dbList: async (server) => (server === "postgres" ? pgDbs : dbs),
    dbCreate: async (name, server) => {
      if (!/^[A-Za-z0-9_-]{1,64}$/.test(name)) throw "use letters, digits, _ and - for database names (up to 64)";
      const list = server === "postgres" ? pgDbs : dbs;
      if (list.some((d) => d.name === name)) throw `Can't create database '${name}'; database exists`;
      const next = [...list, { name, tables: 0, size: 0 }].sort((a, b) => a.name.localeCompare(b.name));
      if (server === "postgres") pgDbs = next;
      else dbs = next;
      return next;
    },
    dbDrop: async (name, server) => {
      if (server === "postgres") return (pgDbs = pgDbs.filter((d) => d.name !== name));
      dbs = dbs.filter((d) => d.name !== name);
      return dbs;
    },
    dbTables: async (database) =>
      database === "analytics"
        ? [
            { name: "events", rows: 182_400, size: 7_340_032, kind: "BASE TABLE" },
            { name: "reports.daily", rows: 365, size: 65_536, kind: "BASE TABLE" },
          ]
        : database === "shop"
        ? [
            { name: "customers", rows: 1250, size: 196608, kind: "BASE TABLE" },
            { name: "orders", rows: 8730, size: 1572864, kind: "BASE TABLE" },
            { name: "order_totals", rows: 0, size: 0, kind: "VIEW" },
          ]
        : database === "blog"
          ? [
              { name: "comments", rows: 312, size: 81920, kind: "BASE TABLE" },
              { name: "posts", rows: 48, size: 262144, kind: "BASE TABLE" },
              { name: "users", rows: 6, size: 16384, kind: "BASE TABLE" },
            ]
          : [],
    dbBrowse: async (_db, table, offset, limit) => ({
      columns: ["id", "name", "email", "created_at", "note"],
      rows: Array.from({ length: Math.min(limit, 42) }, (_, i) => [
        String(offset + i + 1),
        `${table === "orders" ? "Order" : "Customer"} ${offset + i + 1}`,
        `person${offset + i + 1}@example.com`,
        "2026-10-14 22:05:09",
        i % 3 === 0 ? null : "Leaves the parcel with the neighbour when nobody is home",
      ]),
      truncated: false,
    }),
    dbQuery: async (_db, sql) => {
      if (!/select/i.test(sql)) return [];
      if (/selec\b/i.test(sql)) throw "You have an error in your SQL syntax";
      return [{ columns: ["id", "total"], rows: [["1", "9.99"], ["2", null]], truncated: false }];
    },
    dbBackup: async (database, server = "mysql") => {
      const file = `${database}-20261015-001500${server === "postgres" ? ".pg" : ""}.sql.gz`;
      backups = [{ server, database, file, size: 48213, created: Date.now() }, ...backups];
      return backups;
    },
    dbBackups: async () => backups,
    dbRestore: async (_db, _file, _into, server) => (server === "postgres" ? pgDbs : dbs),
    dbRemoveBackup: async (database, file) => {
      backups = backups.filter((b) => !(b.database === database && b.file === file));
      return backups;
    },
    cleanupScan: async () => {
      await new Promise((r) => setTimeout(r, 400));
      const items = cleanup;
      return {
        usage: [
          { name: "Runtimes", size: 412_000_000 },
          { name: "Services", size: 655_000_000 },
          { name: "Service data", size: 188_000_000 },
          { name: "Downloads", size: 31_500_000 },
          { name: "Logs", size: 6_200_000 },
        ].sort((a, b) => b.size - a.size),
        total: 1_292_700_000,
        items,
      };
    },
    cleanupRun: async (ids) => {
      const done = cleanup.filter((i) => ids.includes(i.id));
      cleanup = cleanup.filter((i) => !ids.includes(i.id) || i.id === "logs");
      return { freed: done.reduce((n, i) => n + i.size, 0), cleaned: done.map((i) => i.title), problems: [] };
    },
    mcpStatus: async () => mcp,
    setMcpTool: async (name, allowed) => {
      mcp = { ...mcp, tools: mcp.tools.map((t) => (t.name === name && t.action ? { ...t, allowed } : t)) };
      return mcp;
    },
    listTasks: async () => tasks,
    addTask: async (input) => {
      const preview = samplePreview(input.schedule);
      if (!input.name.trim()) throw "give the task a name";
      if (!input.command.trim()) throw "enter the command to run";
      if (tasks.some((t) => t.name.toLowerCase() === input.name.trim().toLowerCase()))
        throw `a task called ${input.name} already exists`;
      const id = Math.random().toString(36).slice(2, 10).padEnd(8, "x");
      tasks = [...tasks, task(id, { ...input, name: input.name.trim() }, preview, null)];
      return tasks;
    },
    updateTask: async (id, input) => {
      const preview = samplePreview(input.schedule);
      tasks = tasks.map((t) => (t.id === id ? task(id, input, preview, t.last) : t));
      return tasks;
    },
    setTaskEnabled: async (id, enabled) => {
      tasks = tasks.map((t) => (t.id === id ? { ...t, enabled, next_run: enabled ? t.next_run ?? Date.now() + 60_000 : null } : t));
      return tasks;
    },
    removeTask: async (id) => {
      tasks = tasks.filter((t) => t.id !== id);
      return tasks;
    },
    runTask: async (id) => {
      const started = Date.now();
      tasks = tasks.map((t) => (t.id === id ? { ...t, running_since: started } : t));
      setTimeout(() => {
        tasks = tasks.map((t) =>
          t.id === id
            ? {
                ...t,
                running_since: null,
                last: { started, took_ms: 1200, outcome: "succeeded", exit_code: 0, message: null, by_hand: true },
              }
            : t,
        );
      }, 1500);
      return tasks;
    },
    stopTask: async (id) => {
      tasks = tasks.map((t) => (t.id === id ? { ...t, running_since: null } : t));
      return tasks;
    },
    previewSchedule: async (schedule) => samplePreview(schedule),
    taskLog: async (id) => {
      const t = tasks.find((x) => x.id === id);
      return t?.last
        ? [`--- 2026-10-14 22:05 ${t.name} started on schedule: ${t.command} ---`, "INFO  Running scheduled tasks.", "--- finished with exit code 0 after 0.4 s ---"]
        : [];
    },
    remoteConnections: async (site) => ({
      connections: remotes.filter((c) => c.site === site),
      secret_store: "Windows Credential Manager",
    }),
    remoteSave: async (id, input) => {
      const { password, ...rest } = input;
      const port = input.port ?? (input.protocol === "sftp" ? 22 : 21);
      const saved: RemoteConnection = {
        ...rest,
        port,
        id: id ?? Math.random().toString(36).slice(2, 10),
        has_password: password === null ? (remotes.find((c) => c.id === id)?.has_password ?? false) : password !== "",
        busy: false,
        server_key: null,
      };
      remotes = id ? remotes.map((c) => (c.id === id ? saved : c)) : [...remotes, saved];
      return { connections: remotes.filter((c) => c.site === input.site), secret_store: "Windows Credential Manager" };
    },
    remoteRemove: async (id) => {
      const site = remotes.find((c) => c.id === id)?.site ?? "";
      remotes = remotes.filter((c) => c.id !== id);
      return { connections: remotes.filter((c) => c.site === site), secret_store: "Windows Credential Manager" };
    },
    remoteTrust: async (id, fingerprint) => {
      remotes = remotes.map((c) => (c.id === id ? { ...c, server_key: fingerprint } : c));
      const site = remotes.find((c) => c.id === id)?.site ?? "";
      return { connections: remotes.filter((c) => c.site === site), secret_store: "Windows Credential Manager" };
    },
    remoteTest: async (id) => {
      await new Promise((r) => setTimeout(r, 600));
      const c = remotes.find((x) => x.id === id);
      if (c?.protocol === "ftps" && !c.server_key)
        return {
          start_dir: null,
          error: "the server's certificate is not trusted (invalid peer certificate: UnknownIssuer); if this is your server, trust certificate SHA256:3F:A0:…",
          certificate: "SHA256:3F:A0:11:9C",
          key_changed: false,
        };
      return { start_dir: `/home/${c?.user ?? "user"}`, error: null, certificate: null, key_changed: false };
    },
    remoteBrowse: async (id, dir) => {
      const c = remotes.find((x) => x.id === id);
      const root = c?.remote_dir || "/home/master/applications/app/public_html";
      const at = dir ?? root;
      const entries: RemoteEntry[] =
        at === root
          ? [
              { name: "wp-content", dir: true, size: 0, modified: null },
              { name: "wp-includes", dir: true, size: 0, modified: null },
              { name: "index.php", dir: false, size: 405, modified: Date.now() / 1000 - 86_400 },
              { name: "wp-config.php", dir: false, size: 3_120, modified: Date.now() / 1000 - 3 * 86_400 },
            ]
          : [{ name: "readme.txt", dir: false, size: 1_024, modified: Date.now() / 1000 - 7200 }];
      return { dir: at, root, entries };
    },
    remotePlan: async (id, direction, compare, del, paths) => {
      for (let i = 0; i < 4; i++) {
        remoteHandlers.forEach((h) => h({ id, step: "scanning", folder: ["", "wp-content", "wp-content/themes", "wp-includes"][i], found: i * 40 }));
        await new Promise((r) => setTimeout(r, 200));
      }
      const copy = direction === "upload" ? "upload" : "download";
      const files: PlannedFile[] = paths
        ? paths.map((p) => ({ path: p, action: copy, reason: "picked", size: 2_048 }))
        : [
            ...(compare === "missing"
              ? []
              : [
                  { path: "index.php", action: copy, reason: "changed", size: 412 } as PlannedFile,
                  { path: "wp-content/themes/site/style.css", action: copy, reason: "changed", size: 18_400 } as PlannedFile,
                ]),
            { path: "wp-content/uploads/2026/10/hero.jpg", action: copy, reason: "new", size: 482_000 },
            ...(del
              ? [{ path: "old-page.html", action: direction === "upload" ? "delete-remote" : "delete-local", reason: "extra", size: 900 } as PlannedFile]
              : []),
          ];
      return { direction, files, bytes: files.reduce((n, f) => n + (f.action.startsWith("delete") ? 0 : f.size), 0), unchanged: 1_204 };
    },
    remoteRun: async (id, files) => {
      for (const [index, f] of files.entries()) {
        for (const part of [0, 0.5, 1]) {
          remoteHandlers.forEach((h) => h({ id, step: "transfer", index, total: files.length, path: f.path, done: f.size * part, size: f.size }));
          await new Promise((r) => setTimeout(r, 150));
        }
      }
      return { done: files.length, bytes: files.reduce((n, f) => n + f.size, 0), failed: [], stopped: false };
    },
    remoteStop: async () => {},
    remoteLocalFolder: async () => "C:\\stackyard\\www\\blog",
    onRemoteProgress: async (handler) => {
      remoteHandlers.add(handler);
      return () => remoteHandlers.delete(handler);
    },
    pickFiles: async () => [],
    pickSaveFile: async (_title, name) => `/home/you/Downloads/${name}`,
    siteSettings: async (domain) => {
      const site = sites.find((s) => s.domain === domain);
      if (!site) throw `no site named ${domain}`;
      return {
        domain,
        tld: "." + domain.split(".").slice(1).join("."),
        https: site.https,
        settings: siteSettings[domain] ?? {},
        parked: site.parked,
        detected_root: site.root.startsWith(site.path) ? site.root.slice(site.path.length).replace(/^[\\/]+/, "") : "",
        detected_database: site.framework === "WordPress" ? { server: "mysql", name: site.name } : null,
        php: site.runtime.startsWith("PHP"),
        port: site.runtime.startsWith("PHP") || site.runtime === "Static" ? null : 3000,
      };
    },
    saveSiteSettings: async (domain, tld, https, settings) => {
      const name = domain.split(".")[0];
      const next = `${name}${tld.startsWith(".") ? tld : "." + tld}`;
      if (next !== domain && sites.some((s) => s.domain === next)) throw `a site named "${next}" already exists`;
      delete siteSettings[domain];
      siteSettings[next] = settings;
      sites = sites.map((s) =>
        s.domain === domain
          ? {
              ...s,
              domain: next,
              https,
              url: `${https ? "https" : "http"}://${next}`,
              root: settings.root ? `${s.path}/${settings.root}` : s.root,
            }
          : s,
      );
      return { sites, domain: next };
    },
    siteBackups: async (domain) => siteBackups.filter((b) => b.site === domain),
    backupSite: async (domain) => {
      await new Promise((r) => setTimeout(r, 700));
      const backup: SiteBackup = {
        site: domain,
        file: `${domain}-${new Date().toISOString().replace(/\D/g, "").slice(0, 14)}.zip`,
        size: 18_400_000,
        created: Date.now(),
        database: domain.startsWith("blog") ? "blog" : null,
      };
      siteBackups = [backup, ...siteBackups];
      return { path: `~/stackyard/backups/sites/${domain}/${backup.file}`, size: backup.size, files: 1840, database: backup.database, warning: null };
    },
    restoreSite: async (domain) => {
      await new Promise((r) => setTimeout(r, 900));
      const backup: SiteBackup = { site: domain, file: `${domain}-before-restore.zip`, size: 18_000_000, created: Date.now(), database: null };
      siteBackups = [backup, ...siteBackups];
      return { path: backup.file, size: backup.size, files: 1840, database: null, warning: null };
    },
    removeSiteBackup: async (domain, file) => {
      siteBackups = siteBackups.filter((b) => !(b.site === domain && b.file === file));
      return siteBackups.filter((b) => b.site === domain);
    },
    openSiteBackups: async () => {},
    exportSite: async (domain, dest) => {
      await new Promise((r) => setTimeout(r, 700));
      return { path: dest, size: 18_400_000, files: 1840, database: domain.startsWith("blog") ? "blog" : null, warning: null };
    },
    inspectSiteFile: async () => ({
      name: "shop",
      domain: "shop.stack",
      runtime: "PHP 8.4",
      database: "shop",
      created: Date.now() - 3 * 86400_000,
      made_by: "0.5.0",
      folder: "/home/you/stackyard/www/shop",
    }),
    importSite: async (_path, name) => {
      const site = sample(name || "shop-copy", "PHP 8.4", "Laravel", false);
      if (sites.some((s) => s.domain === site.domain)) throw `a site named "${site.domain}" already exists`;
      sites = [...sites, site];
      return { sites, domain: site.domain, warning: null };
    },
    shareStatus: async () => share,
    shareInstall: async () => {
      await new Promise((r) => setTimeout(r, 900));
      share = { ...share, cloudflared: "2026.10.1" };
      return share;
    },
    shareUninstall: async () => {
      share = { ...share, cloudflared: null, links: [] };
      return share;
    },
    shareStart: async (domain) => {
      await new Promise((r) => setTimeout(r, 1500));
      const link = { site: domain, url: "https://seasonal-tiger-hold-example.trycloudflare.com" };
      share = { ...share, cloudflared: share.cloudflared ?? "2026.10.1", links: [...share.links.filter((l) => l.site !== domain), link] };
      return link;
    },
    shareStop: async (domain) => {
      share = { ...share, links: share.links.filter((l) => l.site !== domain) };
      return share;
    },
    openShareLink: async () => {},
    gitStatus: async (site) => git[site] ?? { ...noGit },
    gitLink: async (site, url) => {
      git[site] = { ...noGit, repo: true, branch: "main", upstream: "origin/main", remote: url, identity: false, changes: [{ path: "index.php", kind: "modified" }] };
      return git[site];
    },
    gitFetch: async (site) => {
      git[site] = { ...(git[site] ?? noGit), behind: 2 };
      return git[site];
    },
    gitPull: async (site) => {
      git[site] = { ...(git[site] ?? noGit), behind: 0 };
      return git[site];
    },
    gitPush: async (site) => {
      git[site] = { ...(git[site] ?? noGit), ahead: 0 };
      return git[site];
    },
    gitCommit: async (site, message) => {
      if (!message.trim()) throw "write a short note about what changed";
      git[site] = { ...(git[site] ?? noGit), changes: [], ahead: (git[site]?.ahead ?? 0) + 1, identity: true };
      return git[site];
    },
  };
  return mock;
}

const siteSettings: Record<string, SiteSettings> = {
  "blog.stack": { env: { WP_ENVIRONMENT_TYPE: "local" }, php: { memory_limit: "1G" } },
};
let siteBackups: SiteBackup[] = [
  { site: "blog.stack", file: "blog.stack-20261016-180000.zip", size: 21_300_000, created: Date.now() - 20 * 3600_000, database: "blog" },
];
let share: ShareStatus = { cloudflared: null, links: [], web_running: true };

const remoteHandlers = new Set<(p: RemoteProgress) => void>();
let remotes: RemoteConnection[] = [
  {
    id: "k3m9q2xa",
    site: "blog.stack",
    name: "Cloudways",
    protocol: "sftp",
    host: "203.0.113.10",
    port: 22,
    user: "master_abc",
    key_file: null,
    remote_dir: "/home/master/applications/abcdefgh/public_html",
    local_dir: "",
    skip: [".git", "node_modules", ".DS_Store", "Thumbs.db", ".env"],
    server_key: "SHA256:n+vJBBzrXQJlMmY88FCv2Pks3lIHM7ZZTJAG00xMZMs",
    has_password: true,
    busy: false,
  },
];
const noGit: GitStatus = {
  installed: true,
  repo: false,
  root: null,
  branch: null,
  upstream: null,
  ahead: 0,
  behind: 0,
  remote: null,
  changes: [],
  more_changes: 0,
  identity: true,
};
const git: Record<string, GitStatus> = {
  "blog.stack": {
    ...noGit,
    repo: true,
    branch: "main",
    upstream: "origin/main",
    ahead: 1,
    remote: "https://github.com/you/blog.git",
    changes: [
      { path: "wp-content/themes/site/style.css", kind: "modified" },
      { path: "wp-content/themes/site/footer.php", kind: "new" },
    ],
  },
};

let update: UpdateStatus = {
  current: import.meta.env.VITE_APP_VERSION ?? "0.1.0",
  configured: true,
  checking: false,
  installing: false,
  available: null,
  checked_at: null,
  problem: null,
  whats_new: "- First release.",
  just_updated: false,
};

let cleanup: CleanupItem[] = [
  {
    id: "downloads",
    title: "Leftover downloads",
    detail: "Archives from installs that stopped part-way. Installs download again when needed.",
    size: 31_500_000,
    recommended: true,
    permanent: false,
  },
  {
    id: "old-logs",
    title: "Previous logs",
    detail: "Older halves of logs that grew past their size limit.",
    size: 4_900_000,
    recommended: true,
    permanent: false,
  },
  {
    id: "logs",
    title: "Current logs",
    detail: "Empties every log, including the record of what AI tools did. Services keep running and write new lines as usual.",
    size: 1_300_000,
    recommended: false,
    permanent: false,
  },
  {
    id: "runtime-php-8.2",
    title: "PHP 8.2",
    detail: "Installed (8.2.29), but no site uses it and it isn't the default. A project pinned to it outside your sites would need it installed again.",
    size: 96_000_000,
    recommended: false,
    permanent: false,
  },
  {
    id: "data-mariadb",
    title: "MariaDB data",
    detail: "MariaDB is no longer installed, but its data was kept in case it comes back. Deleting it can't be undone.",
    size: 188_000_000,
    recommended: false,
    permanent: true,
  },
];

export const api: Api = isTauri() ? tauriApi : browserApi();

export function errorText(e: unknown): string {
  return typeof e === "string" ? e : e instanceof Error ? e.message : String(e);
}
