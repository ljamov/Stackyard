// What a new app or site can start from, as the New project form lists
// them: an empty folder first (or one with files in it already), then the
// starters Stackyard makes, and for sites the other ways to bring one in.

import type { Category, ProjectKind, Starter } from "./api";

export type Template = { id: string; name: string; description: string; brand: string } & (
  /** An empty or existing folder, with a runtime to pick. */
  | { via: "folder"; kind: ProjectKind }
  /** A starter the app makes (WordPress, Astro, a Node.js server…). */
  | { via: "starter"; starter: Starter }
  /** A WordPress theme or plugin. */
  | { via: "wordpress"; kind: ProjectKind }
  /** A copy of a live WordPress site, made in the Clone form. */
  | { via: "clone" }
  /** A site from a Stackyard export file or a backup. */
  | { via: "import" }
);

/** Starters' logos, where the starter's own id is not a logo's. */
const STARTER_BRAND: Record<string, string> = { react: "createreactapp", vue: "vite", svelte: "sveltekit", "node-server": "node", "python-scripts": "python", static: "" };

/** Each kind's own icon, for one with no logo (an empty app or site). */
export const CATEGORY_ICON = { app: "apps", site: "sites" } as const;

/** The order sites' starters are offered in; ones not named go after. */
const SITE_ORDER = ["wordpress", "woocommerce", "laravel", "astro", "nextjs", "react", "vue", "svelte", "static"];

export function templatesFor(
  category: Category,
  starters: Starter[],
  can: { clone: boolean; import: boolean },
): Template[] {
  const fromStarters = starters
    .filter((s) => s.category === category)
    .map((s): Template => ({ id: s.id, name: s.name, description: s.description, brand: STARTER_BRAND[s.id] ?? s.id, via: "starter", starter: s }));
  if (category === "app") {
    return [
      { id: "empty-app", name: "Empty app", description: "A new folder, or one with your files in it already; pick what it runs on. It needs no address", brand: "", via: "folder", kind: "website" },
      { id: "wordpress-plugin", name: "WordPress plugin", description: "A plugin folder with a starting plugin to build on", brand: "wordpress", via: "wordpress", kind: "wordpress-plugin" },
      { id: "wordpress-theme", name: "WordPress theme", description: "A theme folder with a starting theme to build on", brand: "wordpress", via: "wordpress", kind: "wordpress-theme" },
      { id: "php-package", name: "PHP package", description: "A PHP library, with PHP to try it in a page", brand: "php", via: "folder", kind: "php-package" },
      ...fromStarters,
    ];
  }
  const rank = (t: Template) => {
    const at = SITE_ORDER.indexOf(t.id);
    return at < 0 ? SITE_ORDER.length : at;
  };
  return [
    { id: "empty-site", name: "Empty site", description: "A new folder, or one with your site's files in it already; pick what it runs on", brand: "", via: "folder", kind: "website" },
    ...fromStarters.sort((a, b) => rank(a) - rank(b)),
    ...(can.clone
      ? [{ id: "clone", name: "Clone a live WordPress", description: "Copy a WordPress site from its server (FTP or SFTP) with its database", brand: "wordpress", via: "clone" } as Template]
      : []),
    ...(can.import
      ? [{ id: "import", name: "Import a site file", description: "A site packed by Stackyard (Backups › Export), with its files and database", brand: "", via: "import" } as Template]
      : []),
  ];
}
