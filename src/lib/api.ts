// Typed wrappers around the Tauri commands in src-tauri/src/lib.rs.
// Outside Tauri (plain `npm run dev` in a browser) an in-memory stand-in
// is used so the screens can be developed and previewed without the core.

import { invoke, isTauri } from "@tauri-apps/api/core";

export type RuntimeFamily = {
  kind: "php" | "node" | "python" | "go" | "java";
  name: string;
  versions: string[];
  available: boolean;
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
  const runtimes: RuntimeFamily[] = [
    { kind: "php", name: "PHP", versions: ["8.4", "8.3", "8.2", "8.1"], available: true },
    { kind: "node", name: "Node.js", versions: ["24", "22", "20"], available: true },
    { kind: "python", name: "Python", versions: ["3.13", "3.12", "3.11"], available: false },
    { kind: "go", name: "Go", versions: ["1.23", "1.22"], available: false },
    { kind: "java", name: "Java", versions: ["21", "17"], available: false },
  ];
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
