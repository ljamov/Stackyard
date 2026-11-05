import { useEffect, useState, type ReactNode } from "react";
import { errorText } from "../../lib/api";
import { useT } from "../../lib/i18n";
import { wp, type WpTreeEntry, type WpTreeFolder } from "../../lib/wordpress";
import { Icon } from "../Icon";
import { size } from "./WordPressTools";

/** What is known of the live site's folders: each folder opened so far. */
type Loaded = Record<string, WpTreeFolder | { error: string } | "loading">;

const parentOf = (path: string) => (path.includes("/") ? path.slice(0, path.lastIndexOf("/")) : "");
const within = (path: string, folder: string) => path.startsWith(`${folder}/`);
function ancestors(path: string) {
  const out: string[] = [];
  for (let p = parentOf(path); p !== ""; p = parentOf(p)) out.push(p);
  return out;
}

/** Whether `path` is left out: unticked itself, or inside an unticked folder. */
export function isLeftOut(skip: string[], path: string) {
  return skip.includes(path) || ancestors(path).some((a) => skip.includes(a));
}

/** Unticks `path`: it goes in the list, and anything under it comes out (it goes with it). */
export function untick(skip: string[], path: string) {
  return [...skip.filter((p) => !within(p, path) && p !== path), path];
}

/**
 * Ticks `path`. Under an unticked folder, that folder is ticked again and
 * its other entries on the way down are unticked instead, so only `path`
 * comes back. `children` gives a folder's entries, as loaded.
 */
export function tick(skip: string[], path: string, children: (folder: string) => WpTreeEntry[]) {
  let out = skip.filter((p) => p !== path);
  const above = ancestors(path).reverse();
  const top = above.findIndex((a) => out.includes(a));
  if (top < 0) return out;
  const chain = [...above.slice(top), path];
  out = out.filter((p) => p !== chain[0]);
  for (let i = 0; i < chain.length - 1; i++) {
    for (const child of children(chain[i])) {
      if (child.path !== chain[i + 1] && !child.skipped) out.push(child.path);
    }
  }
  return out;
}

/** Bytes left out by `skip`, from the sizes of the entries loaded. */
export function leftOutBytes(skip: string[], loaded: Loaded) {
  const entries = new Map<string, number>();
  for (const folder of Object.values(loaded)) {
    if (typeof folder === "object" && "entries" in folder) for (const e of folder.entries) entries.set(e.path, e.bytes);
  }
  return skip.reduce((sum, p) => sum + (entries.get(p) ?? 0), 0);
}

/**
 * The live site's folders and files with a box to tick for each and its
 * size, a folder's entries loaded when it is opened (through the
 * connection code). Unticked ones are left out of the clone, e.g. big
 * videos the host won't send, to copy by hand later.
 */
export function FileTree({
  code,
  skip,
  onSkip,
  onLeftOutBytes,
}: {
  code: string;
  skip: string[];
  onSkip: (skip: string[]) => void;
  onLeftOutBytes: (bytes: number) => void;
}) {
  const t = useT();
  const [loaded, setLoaded] = useState<Loaded>({});
  const [open, setOpen] = useState<string[]>([]);

  async function load(path: string) {
    if (loaded[path]) return;
    setLoaded((l) => ({ ...l, [path]: "loading" }));
    try {
      const folder = await wp.cloneTree(code, path);
      setLoaded((l) => ({ ...l, [path]: folder }));
    } catch (e) {
      setLoaded((l) => ({ ...l, [path]: { error: errorText(e) } }));
    }
  }

  useEffect(() => {
    load("");
    // Loaded once for the code it was opened with.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [code]);
  useEffect(() => onLeftOutBytes(leftOutBytes(skip, loaded)), [skip, loaded, onLeftOutBytes]);

  const children = (folder: string) => {
    const f = loaded[folder];
    return typeof f === "object" && "entries" in f ? f.entries : [];
  };
  const root = loaded[""];
  const total = typeof root === "object" && "entries" in root ? root.entries.filter((e) => !e.skipped).reduce((s, e) => s + e.bytes, 0) : 0;
  const left = leftOutBytes(skip, loaded);

  function toggleOpen(path: string) {
    if (open.includes(path)) setOpen(open.filter((p) => p !== path));
    else {
      setOpen([...open, path]);
      load(path);
    }
  }

  function rows(folder: string, depth: number): ReactNode {
    const f = loaded[folder];
    if (!f || f === "loading") return <li className="file-tree-note muted small" style={{ paddingLeft: depth * 18 + 24 }}>{t("Adding up the folders…")}</li>;
    if ("error" in f)
      return (
        <li className="file-tree-note error small" style={{ paddingLeft: depth * 18 + 24 }}>
          {f.error}
        </li>
      );
    const notes = [
      f.more > 0 ? t("{count} smaller entries not shown; they go in with their folder.", { count: f.more }) : "",
      f.complete ? "" : t("Adding up took too long, so some folder sizes are what was counted so far."),
    ].filter(Boolean);
    return (
      <>
        {f.entries.map((e) => {
          const off = isLeftOut(skip, e.path);
          const partly = !off && skip.some((p) => within(p, e.path));
          const isOpen = open.includes(e.path);
          return (
            <li key={e.path} role="treeitem" aria-expanded={e.dir && !e.skipped ? isOpen : undefined}>
              <div className={`file-tree-row${off ? " off" : ""}${e.skipped ? " anyway" : ""}`} style={{ paddingLeft: depth * 18 }}>
                {e.dir && !e.skipped ? (
                  <button
                    type="button"
                    className={`file-tree-toggle${isOpen ? " open" : ""}`}
                    aria-label={isOpen ? t("Close {name}", { name: e.name }) : t("Open {name}", { name: e.name })}
                    onClick={() => toggleOpen(e.path)}
                  >
                    <Icon name="chevron" />
                  </button>
                ) : (
                  <span className="file-tree-toggle" />
                )}
                <label className="file-tree-label" title={e.path}>
                  {!e.skipped && (
                    <input
                      type="checkbox"
                      checked={!off}
                      ref={(box) => {
                        if (box) box.indeterminate = partly;
                      }}
                      disabled={e.path === "wp-config.php"}
                      onChange={(ev) => onSkip(ev.target.checked ? tick(skip, e.path, children) : untick(skip, e.path))}
                    />
                  )}
                  {e.dir && (
                    <span className="file-tree-icon">
                      <Icon name="folder" />
                    </span>
                  )}
                  <span className="file-tree-name mono">{e.name}</span>
                  {e.skipped && <span className="muted"> · {t("left out anyway (cache or backup)")}</span>}
                </label>
                <span className="file-tree-size mono muted">
                  {e.dir ? `${t("{count} files", { count: e.files.toLocaleString() })} · ` : ""}
                  {size(e.bytes)}
                </span>
              </div>
              {e.dir && isOpen && <ul role="group">{rows(e.path, depth + 1)}</ul>}
            </li>
          );
        })}
        {notes.length > 0 && (
          <li className="file-tree-note muted small" style={{ paddingLeft: depth * 18 + 24 }}>
            {notes.join(" ")}
          </li>
        )}
      </>
    );
  }

  return (
    <div className="file-tree">
      <p className="small">
        {total > 0 && t("Picked: {picked} of {total}", { picked: size(Math.max(0, total - left)), total: size(total) })}
        {left > 0 && <span className="muted"> · {t("Left out: {size}", { size: size(left) })}</span>}
        {skip.length > 0 && (
          <>
            {" "}
            <button type="button" className="btn small ghost" onClick={() => onSkip([])}>
              {t("Tick everything")}
            </button>
          </>
        )}
      </p>
      <ul className="file-tree-list" role="tree">
        {rows("", 0)}
      </ul>
    </div>
  );
}
