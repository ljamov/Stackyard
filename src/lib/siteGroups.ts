// The order of sites and the groups they are in, as the Sites screen arranges
// them. The app keeps it (site-order.toml) and lists sites in this order
// everywhere; these helpers make the changes a drag or an arrow key asks for.

/** A top-level entry: a site, or a group holding sites in order. */
export type SiteItem = { site: string } | { group: string; sites: string[]; collapsed?: boolean };
export type SiteGroup = Extract<SiteItem, { group: string }>;

export const isGroup = (i: SiteItem): i is SiteGroup => "group" in i;

/** What is moved: a site by its domain, or a group by its name. */
export type Moved = { site: string } | { group: string };

/** Where it goes: before or after a site or a group, or first in a group. */
export type Place =
  | { before: Moved }
  | { after: Moved }
  | { into: string };

/** Every site once, gone ones left out, new ones last: like the app does. */
export function arrange(items: SiteItem[], domains: string[]): SiteItem[] {
  const known = new Set(domains);
  const placed = new Set<string>();
  const take = (d: string) => known.has(d) && !placed.has(d) && (placed.add(d), true);
  const out: SiteItem[] = [];
  for (const i of items) {
    if (isGroup(i)) out.push({ ...i, sites: i.sites.filter(take) });
    else if (take(i.site)) out.push({ site: i.site });
  }
  for (const d of domains) if (take(d)) out.push({ site: d });
  return out;
}

/** The domains in the order they are listed, groups unfolded. */
export function flatten(items: SiteItem[]): string[] {
  return items.flatMap((i) => (isGroup(i) ? i.sites : [i.site]));
}

/** The group a site is in, if any. */
export function groupOf(items: SiteItem[], domain: string): SiteGroup | undefined {
  return items.filter(isGroup).find((g) => g.sites.includes(domain));
}

export const keyOf = (i: SiteItem): Moved => (isGroup(i) ? { group: i.group } : { site: i.site });

const same = (a: Moved, b: Moved) =>
  "site" in a ? "site" in b && a.site === b.site : "group" in b && a.group === b.group;

/** Takes `moved` out of the list; a removed group's sites are returned too. */
function without(items: SiteItem[], moved: Moved): SiteItem[] {
  return items
    .filter((i) => !("group" in moved && isGroup(i) && i.group === moved.group))
    .filter((i) => !("site" in moved && !isGroup(i) && i.site === moved.site))
    .map((i) => ("site" in moved && isGroup(i) ? { ...i, sites: i.sites.filter((s) => s !== moved.site) } : i));
}

/**
 * Puts a site or group at a place. A group can't go inside a group: placed
 * next to a site in a group, it goes next to that group instead.
 */
export function move(items: SiteItem[], moved: Moved, place: Place): SiteItem[] {
  const target = "into" in place ? { group: place.into } : "before" in place ? place.before : place.after;
  if (same(moved, target)) return items;
  const rest = without(items, moved);
  const entry: SiteItem =
    "site" in moved ? { site: moved.site } : (items.find((i) => isGroup(i) && i.group === moved.group) as SiteItem);
  if (!entry) return items;

  if ("into" in place) {
    if (!("site" in moved)) return items;
    return rest.map((i) => (isGroup(i) && i.group === place.into ? { ...i, sites: [moved.site, ...i.sites] } : i));
  }
  const after = "after" in place;
  // Next to a site inside a group.
  if ("site" in target) {
    const holder = groupOf(rest, target.site);
    if (holder) {
      if ("group" in moved) return move(items, moved, after ? { after: { group: holder.group } } : { before: { group: holder.group } });
      return rest.map((i) => {
        if (i !== holder) return i;
        const sites = i.sites.filter((s) => s !== moved.site);
        sites.splice(sites.indexOf(target.site) + (after ? 1 : 0), 0, moved.site);
        return { ...i, sites };
      });
    }
  }
  const at = rest.findIndex((i) => same(target, keyOf(i)));
  if (at < 0) return items;
  const next = [...rest];
  next.splice(at + (after ? 1 : 0), 0, entry);
  return next;
}

/**
 * One step up (-1) or down (1), for the arrow keys. A site at the edge of its
 * group steps out of it; a site next to an open group steps into it.
 */
export function step(items: SiteItem[], moved: Moved, dir: -1 | 1): SiteItem[] {
  if ("site" in moved) {
    const holder = groupOf(items, moved.site);
    if (holder) {
      const at = holder.sites.indexOf(moved.site);
      const neighbor = holder.sites[at + dir];
      if (neighbor) return move(items, moved, dir < 0 ? { before: { site: neighbor } } : { after: { site: neighbor } });
      return move(items, moved, dir < 0 ? { before: { group: holder.group } } : { after: { group: holder.group } });
    }
  }
  const key = keyOf;
  const at = items.findIndex((i) => same(key(i), moved));
  const neighbor = items[at + dir];
  if (at < 0 || !neighbor) return items;
  if ("site" in moved && isGroup(neighbor) && !neighbor.collapsed) {
    if (dir > 0) return move(items, moved, { into: neighbor.group });
    const last = neighbor.sites[neighbor.sites.length - 1];
    return last ? move(items, moved, { after: { site: last } }) : move(items, moved, { into: neighbor.group });
  }
  return move(items, moved, dir < 0 ? { before: key(neighbor) } : { after: key(neighbor) });
}

/** A name for a new group that no group has: "Group", "Group 2"… */
export function newGroupName(items: SiteItem[], base: string): string {
  const taken = new Set(items.filter(isGroup).map((g) => g.group.toLowerCase()));
  for (let n = 1; ; n++) {
    const name = n === 1 ? base : `${base} ${n}`;
    if (!taken.has(name.toLowerCase())) return name;
  }
}
