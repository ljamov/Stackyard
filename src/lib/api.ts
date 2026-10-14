// Typed wrappers around the Tauri commands in src-tauri/src/lib.rs.
// Outside Tauri (plain `npm run dev` in a browser) an in-memory stand-in
// is used so the screens can be developed and previewed without the core.

import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

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
  /** Changes something, so the user decides whether AI tools may use it. */
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
  listModules(): Promise<Module[]>;
  setModule(id: string, enabled: boolean): Promise<Module[]>;
  listSites(): Promise<Site[]>;
  addSite(site: NewSite): Promise<Site[]>;
  removeSite(name: string): Promise<Site[]>;
  listParked(): Promise<string[]>;
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
  listLogs(): Promise<LogFile[]>;
  /** The last `lines` lines when `from` is null, else what was written since `from`. */
  readLog(id: string, from: number | null, lines: number): Promise<LogChunk>;
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
  listModules: () => invoke("list_modules"),
  setModule: (id, enabled) => invoke("set_module", { id, enabled }),
  listSites: () => invoke("list_sites"),
  addSite: (site) => invoke("add_site", site),
  removeSite: (name) => invoke("remove_site", { name }),
  listParked: () => invoke("list_parked"),
  parkFolder: (path) => invoke("park_folder", { path }),
  unparkFolder: (path) => invoke("unpark_folder", { path }),
  systemStatus: () => invoke("system_status"),
  runSystemSetup: () => invoke("run_system_setup"),
  terminalStatus: () => invoke("terminal_status"),
  setTerminal: (on) => invoke("set_terminal", { on }),
  onSitesChanged: (handler) => listen("sites-changed", () => handler()),
  listLogs: () => invoke("list_logs"),
  readLog: (id, from, lines) => invoke("read_log", { id, from, lines }),
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
};

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
    family("python", "Python", ["3.13", "3.12", "3.11"], false),
    family("go", "Go", ["1.23", "1.22"], false),
    family("java", "Java", ["21", "17"], false),
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
  ): Service => ({ id, name, version, ports, role, web, installed, installing: false, state: "stopped" });
  let services: Service[] = [
    service("caddy", "Caddy", "2", [80, 443], "Web server", "http://localhost", "2.10.2"),
    service("mysql", "MySQL", "8.4", [3306], "Database", null, "8.4.6"),
    service("mariadb", "MariaDB", "11.8", [3306], "Database", null, null),
    service("mailpit", "Mailpit", "1", [1025, 8025], "Mail catcher", "http://127.0.0.1:8025", null),
  ];
  const serviceHandlers = new Set<(p: ServiceProgress) => void>();
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
  let modules: Module[] = [
    mod("php", "PHP", "runtime", "PHP versions for sites and the shell", true, true),
    mod("node", "Node.js", "runtime", "Node.js versions for sites and the shell", true, true),
    mod("python", "Python", "runtime", "Python versions", false, false),
    mod("caddy", "Caddy", "service", "Web server for local sites, with automatic HTTPS", true, true),
    mod("mysql", "MySQL", "service", "MySQL database server", true, true),
    mod("mariadb", "MariaDB", "service", "MySQL-compatible database (Windows and Linux)", false, true),
    mod("mailpit", "Mailpit", "service", "Catches outgoing mail for testing", false, true),
    mod("redis", "Redis", "service", "Cache and queues", false, false),
    mod("shell", "Stackyard Shell", "feature", "Terminal and PATH shims with each project's versions", true, true),
    mod("logs", "Log viewer", "feature", "Live logs from every service, with search", true, true),
    mod("mcp", "MCP server", "feature", "Lets AI coding tools see and manage sites and services", false, true),
    mod("scheduler", "Task scheduler", "feature", "Run commands on a schedule in a project folder, like cron", false, true),
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
  });
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
      tool("read_logs", "Read a log", "Returns the last lines of a service's log.", false),
      tool("start_service", "Start a service", "Starts an installed service in the background.", true),
      tool("stop_service", "Stop a service", "Stops a running service.", true),
      tool("create_site", "Create a site", "Adds a local site for a project folder.", true),
      tool("set_site_runtime", "Switch a site's version", "Switches the PHP or Node.js version a site runs on.", true),
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
  return {
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
      return updateService(id, (s) => ({ ...s, state: "running", pid: 4242 }) as Service);
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
    listModules: async () => modules,
    setModule: async (id, enabled) => {
      modules = modules.map((m) => (m.id === id && m.available ? { ...m, enabled } : m));
      return modules;
    },
    listSites: async () => sites,
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
    removeSite: async (name) => {
      sites = sites.filter((s) => s.domain !== name && s.name !== name);
      return sites;
    },
    listParked: async () => parked,
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
  };
}

export const api: Api = isTauri() ? tauriApi : browserApi();

export function errorText(e: unknown): string {
  return typeof e === "string" ? e : e instanceof Error ? e.message : String(e);
}
