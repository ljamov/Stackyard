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

export type ServiceDef = {
  id: string;
  name: string;
  version: string;
  ports: number[];
  role: string;
};

export type Site = {
  name: string;
  domain: string;
  url: string;
  path: string;
  runtime: string;
  https: boolean;
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

export type TldInfo = { tld: string; warning: string | null };

export type NewSite = { name: string; path: string; runtime: string; https: boolean };

export interface Api {
  listRuntimes(): Promise<RuntimeFamily[]>;
  installRuntime(kind: RuntimeKind, version: string): Promise<RuntimeFamily[]>;
  uninstallRuntime(kind: RuntimeKind, version: string): Promise<RuntimeFamily[]>;
  setDefaultRuntime(kind: RuntimeKind, version: string): Promise<RuntimeFamily[]>;
  /** Calls `handler` for install progress; resolves to an unsubscribe function. */
  onInstallProgress(handler: (p: InstallProgress) => void): Promise<() => void>;
  listServices(): Promise<ServiceDef[]>;
  getTld(): Promise<TldInfo>;
  setTld(value: string): Promise<TldInfo>;
  listModules(): Promise<Module[]>;
  setModule(id: string, enabled: boolean): Promise<Module[]>;
  listSites(): Promise<Site[]>;
  addSite(site: NewSite): Promise<Site>;
  removeSite(name: string): Promise<void>;
}

const tauriApi: Api = {
  listRuntimes: () => invoke("list_runtimes"),
  installRuntime: (kind, version) => invoke("install_runtime", { kind, version }),
  uninstallRuntime: (kind, version) => invoke("uninstall_runtime", { kind, version }),
  setDefaultRuntime: (kind, version) => invoke("set_default_runtime", { kind, version }),
  onInstallProgress: (handler) => listen<InstallProgress>("runtime-progress", (e) => handler(e.payload)),
  listServices: () => invoke("list_services"),
  getTld: () => invoke("get_tld"),
  setTld: (value) => invoke("set_tld", { value }),
  listModules: () => invoke("list_modules"),
  setModule: (id, enabled) => invoke("set_module", { id, enabled }),
  listSites: () => invoke("list_sites"),
  addSite: (site) => invoke("add_site", site),
  removeSite: (name) => invoke("remove_site", { name }),
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
  const services: ServiceDef[] = [
    { id: "nginx", name: "Nginx", version: "1.27", ports: [8080], role: "Web server" },
    { id: "mariadb", name: "MariaDB", version: "11.4", ports: [3306], role: "Database" },
    { id: "mailpit", name: "Mailpit", version: "1.20", ports: [1025, 8025], role: "Mail catcher" },
  ];
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
    mod("nginx", "Nginx", "service", "Web server for PHP sites", true, true),
    mod("mariadb", "MariaDB", "service", "MySQL-compatible database", true, true),
    mod("mailpit", "Mailpit", "service", "Catches outgoing mail for testing", false, true),
    mod("redis", "Redis", "service", "Cache and queues", false, false),
    mod("shell", "Stackyard Shell", "feature", "Terminal and PATH shims with each project's versions", true, false),
    mod("mcp", "MCP server", "feature", "Lets AI coding tools manage sites and services", false, false),
  ];
  let tld = ".stack";
  let sites: Site[] = [];
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
      const site = { name, domain, url: `${https ? "https" : "http"}://${domain}`, path, runtime, https };
      sites = [...sites, site];
      return site;
    },
    removeSite: async (name) => {
      sites = sites.filter((s) => s.domain !== name && s.name !== name);
    },
  };
}

export const api: Api = isTauri() ? tauriApi : browserApi();

export function errorText(e: unknown): string {
  return typeof e === "string" ? e : e instanceof Error ? e.message : String(e);
}
