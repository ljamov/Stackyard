import { useCallback, useEffect, useRef, useState, type KeyboardEvent, type PointerEvent, type ReactNode } from "react";
import { api, errorText, type ListKind, type Site } from "../lib/api";
import { useT } from "../lib/i18n";
import {
  arrange,
  isGroup,
  flatten,
  keyOf,
  mergeShown,
  move,
  newGroupName,
  shownIn,
  step,
  type Moved,
  type Place,
  type SiteGroup,
  type SiteItem,
} from "../lib/siteGroups";
import { Icon } from "./Icon";
import { IconButton } from "./IconButton";

/** What a site's row gets to place itself: the drag handle and the drop target. */
export type RowParts = {
  handle: ReactNode;
  row: { "data-drop": string; "data-in-group"?: true; "data-drop-at"?: string; "data-dragged"?: true };
};

type Where = "before" | "after" | "into";
const id = (m: Moved) => ("site" in m ? `site:${m.site}` : `group:${m.group}`);
const parse = (s: string): Moved => (s.startsWith("site:") ? { site: s.slice(5) } : { group: s.slice(6) });

/**
 * The rows of a Projects, Apps or Sites table in the user's order, with
 * that list's groups. Each row has a handle: drag it to move the site or
 * group, or focus it and use the arrow keys. Dropped on a group's heading,
 * a site goes into the group. One order is kept for everything; each list
 * changes only its own part of it.
 */
export function SiteList({
  sites,
  list,
  columns,
  onError,
  renderSite,
}: {
  sites: Site[];
  list: ListKind;
  columns: number;
  onError: (message: string | null) => void;
  renderSite: (site: Site, parts: RowParts) => ReactNode;
}) {
  const t = useT();
  const [saved, setSaved] = useState<SiteItem[]>([]);
  const domains = sites.map((s) => s.domain);
  const shown = new Set(domains);
  // Everything, the other lists' sites too, so saving keeps them.
  const whole = arrange(saved, [...flatten(saved), ...domains]);
  const items = shownIn(whole, list, shown);
  const [renaming, setRenaming] = useState<string | null>(null);
  const [drag, setDrag] = useState<{ moved: Moved; target: Moved; where: Where } | null>(null);
  const start = useRef<{ moved: Moved; y: number; active: boolean } | null>(null);
  const reload = useCallback(() => {
    api.siteLayout().then(setSaved).catch((e) => onError(errorText(e)));
  }, [onError]);
  useEffect(reload, [reload]);

  // Sites added, removed or renamed elsewhere: the order is read again.
  useEffect(() => {
    let off: (() => void) | undefined;
    let cancelled = false;
    api.onSitesChanged(reload).then((u) => (cancelled ? u() : (off = u)));
    return () => {
      cancelled = true;
      off?.();
    };
  }, [reload]);

  /** Shows the change at once, then keeps what the app saved. */
  async function save(shownNext: SiteItem[]) {
    const next = mergeShown(whole, list, shown, shownNext);
    setSaved(next);
    try {
      setSaved(await api.setSiteLayout(next));
      onError(null);
    } catch (e) {
      onError(errorText(e));
      reload();
    }
  }

  function newGroup() {
    // Named unlike every group, the other lists' too.
    const name = newGroupName(whole, t("New group"));
    void save([{ group: name, sites: [], ...(list === "all" ? {} : { list }) }, ...items]);
    setRenaming(name);
  }

  /** A group of the other list can't hold this site (on Projects, where both show). */
  function fits(group: string, domain: string) {
    const g = items.find((i) => isGroup(i) && i.group === group) as SiteGroup | undefined;
    const site = sites.find((s) => s.domain === domain);
    return !g?.list || !site || site.project.category === g.list;
  }

  function rename(group: SiteGroup, name: string) {
    setRenaming(null);
    const typed = name.trim();
    if (!typed || typed === group.group) return;
    void save(items.map((i) => (i === group ? { ...i, group: typed } : i)));
  }

  /** The group goes; its sites stay where it was. */
  function ungroup(group: SiteGroup) {
    void save(items.flatMap((i) => (i === group ? i.sites.map((site) => ({ site })) : [i])));
  }

  function fold(group: SiteGroup) {
    void save(items.map((i) => (i === group ? { ...i, collapsed: !i.collapsed } : i)));
  }

  /** Arrow keys on a handle move by one; Home and End to the ends. */
  function keys(e: KeyboardEvent, moved: Moved) {
    let next: SiteItem[] | null = null;
    if (e.key === "ArrowUp" || e.key === "ArrowDown") next = step(items, moved, e.key === "ArrowUp" ? -1 : 1);
    if (e.key === "Home" && items[0]) next = move(items, moved, { before: keyOf(items[0]) });
    if (e.key === "End" && items.length) next = move(items, moved, { after: keyOf(items[items.length - 1]) });
    if (!next) return;
    e.preventDefault();
    const holder = "site" in moved ? next.find((i) => isGroup(i) && i.sites.includes(moved.site)) : undefined;
    if (holder && isGroup(holder) && "site" in moved && !fits(holder.group, moved.site)) {
      onError(t("{group} is a group of the other list.", { group: holder.group }));
      return;
    }
    void save(next);
    // The handle moved with its row; keep the focus on it.
    requestAnimationFrame(() => document.querySelector<HTMLElement>(`[data-handle="${CSS.escape(id(moved))}"]`)?.focus());
  }

  function pointerDown(e: PointerEvent<HTMLElement>, moved: Moved) {
    if (e.button !== 0) return;
    e.currentTarget.setPointerCapture(e.pointerId);
    start.current = { moved, y: e.clientY, active: false };
  }

  function pointerMove(e: PointerEvent<HTMLElement>) {
    const s = start.current;
    if (!s) return;
    if (!s.active && Math.abs(e.clientY - s.y) < 4) return;
    s.active = true;
    const el = document.elementFromPoint(e.clientX, e.clientY)?.closest<HTMLElement>("[data-drop]");
    if (!el) return;
    const target = parse(el.dataset.drop!);
    const box = el.getBoundingClientRect();
    const y = (e.clientY - box.top) / box.height;
    // A site over a group's heading: its lower part puts it inside.
    const where: Where = "group" in target && "site" in s.moved && y > 0.4 ? "into" : y < 0.5 ? "before" : "after";
    setDrag({ moved: s.moved, target, where });
    // Near the window's top or bottom, the page scrolls along.
    const scroller = el.closest(".content");
    if (scroller && e.clientY < 60) scroller.scrollTop -= 12;
    else if (scroller && e.clientY > window.innerHeight - 60) scroller.scrollTop += 12;
  }

  function pointerUp() {
    const d = drag;
    const moved = start.current?.active;
    start.current = null;
    setDrag(null);
    if (!d || !moved) return;
    if (d.where === "into" && "group" in d.target && "site" in d.moved && !fits(d.target.group, d.moved.site)) {
      onError(t("{group} is a group of the other list.", { group: d.target.group }));
      return;
    }
    const place: Place =
      d.where === "into" && "group" in d.target
        ? { into: d.target.group }
        : d.where === "before"
          ? { before: d.target }
          : { after: d.target };
    const next = move(items, d.moved, place);
    if (next !== items) void save(next);
  }

  function cancel(e: KeyboardEvent) {
    if (e.key === "Escape" && start.current) {
      start.current = null;
      setDrag(null);
    }
  }

  const handle = (moved: Moved, name: string) => (
    <button
      type="button"
      className="drag-handle"
      data-handle={id(moved)}
      aria-label={t("Move {name}", { name })}
      title={t("Drag to move {name}, or use the arrow keys", { name })}
      onPointerDown={(e) => pointerDown(e, moved)}
      onPointerMove={pointerMove}
      onPointerUp={pointerUp}
      onPointerCancel={() => {
        start.current = null;
        setDrag(null);
      }}
      onKeyDown={(e) => {
        cancel(e);
        keys(e, moved);
      }}
    >
      <Icon name="grip" />
    </button>
  );

  const dropProps = (m: Moved) => {
    const at = drag && id(drag.target) === id(m) ? drag.where : undefined;
    return {
      "data-drop": id(m),
      "data-drop-at": at,
      "data-dragged": drag && id(drag.moved) === id(m) ? (true as const) : undefined,
    };
  };

  const siteRow = (domain: string, inGroup: boolean) => {
    const site = sites.find((s) => s.domain === domain);
    if (!site) return null;
    const m = { site: domain };
    return renderSite(site, {
      handle: handle(m, domain),
      row: { ...dropProps(m), "data-in-group": inGroup ? true : undefined },
    });
  };

  return (
    <>
      <tr className="site-list-tools">
        <td colSpan={columns}>
          <div className="row between centered">
            <span className="muted small">{t("Drag by the handle to change the order or put it in a group.")}</span>
            <button type="button" className="btn small" onClick={newGroup}>
              <Icon name="plus" />
              {t("New group")}
            </button>
          </div>
        </td>
      </tr>
      {items.map((item) => {
        if (!isGroup(item)) return siteRow(item.site, false);
        const m = { group: item.group };
        const count = item.sites.length;
        return [
          <tr key={`group:${item.group}`} className="group-row" {...dropProps(m)}>
            <td colSpan={columns}>
              <div className="group-head">
                {handle(m, item.group)}
                {renaming === item.group ? (
                  <input
                    className="group-name-input"
                    defaultValue={item.group}
                    aria-label={t("Group name")}
                    autoFocus
                    maxLength={60}
                    onFocus={(e) => e.currentTarget.select()}
                    onBlur={(e) => rename(item, e.currentTarget.value)}
                    onKeyDown={(e) => {
                      if (e.key === "Enter") e.currentTarget.blur();
                      if (e.key === "Escape") setRenaming(null);
                    }}
                  />
                ) : (
                  <button
                    type="button"
                    className="disclosure group-name"
                    aria-expanded={!item.collapsed}
                    onClick={() => fold(item)}
                    onDoubleClick={() => setRenaming(item.group)}
                  >
                    <Icon name="chevron" />
                    <strong>{item.group}</strong>
                    <span className="muted small">{countText(t, list, count)}</span>
                  </button>
                )}
                <span className="grow" />
                <IconButton icon="edit" label={t("Rename {name}", { name: item.group })} onClick={() => setRenaming(item.group)} />
                <IconButton icon="close" label={t("Ungroup {name}", { name: item.group })} onClick={() => ungroup(item)} />
              </div>
              {count === 0 && !item.collapsed && (
                <div className="muted small group-empty">{t("Drag items here to add them to this group.")}</div>
              )}
            </td>
          </tr>,
          ...(item.collapsed ? [] : item.sites.map((d) => siteRow(d, true))),
        ];
      })}
    </>
  );
}

/** "3 apps", "1 site", "2 projects": how many a group holds, in the list's words. */
export function countText(t: ReturnType<typeof useT>, list: ListKind, count: number) {
  if (list === "app") return count === 1 ? t("1 app") : t("{count} apps", { count });
  if (list === "site") return count === 1 ? t("1 site") : t("{count} sites", { count });
  return count === 1 ? t("1 project") : t("{count} projects", { count });
}
