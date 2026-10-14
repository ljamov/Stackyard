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

export type TldInfo = { tld: string; warning: string | null };

export type NewSite = { name: string; path: string; runtime: string; https: boolean };

export interface Api {
  listRuntimes(): Promise<RuntimeFamily[]>;
  listServices(): Promise<ServiceDef[]>;
  getTld(): Promise<TldInfo>;
  setTld(value: string): Promise<TldInfo>;
  listSites(): Promise<Site[]>;
  addSite(site: NewSite): Promise<Site>;
  removeSite(name: string): Promise<void>;
}

const tauriApi: Api = {
  listRuntimes: () => invoke("list_runtimes"),
  listServices: () => invoke("list_services"),
  getTld: () => invoke("get_tld"),
  setTld: (value) => invoke("set_tld", { value }),
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
