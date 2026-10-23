import { useEffect, useMemo, useState } from "react";
import { errorText } from "../../lib/api";
import { useT } from "../../lib/i18n";
import {
  wp,
  type WpChange,
  type WpChangeList,
  type WpExisting,
  type WpMergeLevel,
  type WpMerged,
  type WpMoveChoice,
  type WpMovePlan,
  type WpMoved,
  type WpPostSummary,
} from "../../lib/wordpress";

type T = ReturnType<typeof useT>;

function when(at: number) {
  return new Date(at * 1000).toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });
}

function changeText(t: T, change: WpChange) {
  return change === "added" ? t("added") : change === "removed" ? t("removed") : t("changed");
}

function groupText(t: T, group: string) {
  const [kind, name] = group.split(":");
  if (kind === "plugin") return t("Plugin {name}", { name });
  if (kind === "theme") return t("Theme {name}", { name });
  return (
    {
      media: t("Media"),
      "mu-plugins": t("Must-use plugins"),
      languages: t("Translations"),
      content: t("Other wp-content files"),
      core: t("WordPress itself"),
    }[group] ?? t("Other files")
  );
}

function Undo({ target, snapshot, onDone }: { target: string; snapshot: string; onDone: () => void }) {
  const t = useT();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  return (
    <>
      <button
        type="button"
        className="btn small"
        disabled={busy}
        onClick={async () => {
          setBusy(true);
          setError(null);
          try {
            await wp.restoreSnapshot(target, snapshot);
            onDone();
          } catch (e) {
            setError(errorText(e));
          } finally {
            setBusy(false);
          }
        }}
      >
        {busy ? t("Putting it back…") : t("Undo")}
      </button>
      {error && (
        <p className="error small" role="alert">
          {error}
        </p>
      )}
    </>
  );
}

/** Taking a staging's work into another copy of the site: whole changes
 * at one of three levels, or single posts and pages. */
export function MergePanel({ source, targets }: { source: string; targets: string[] }) {
  const t = useT();
  const [target, setTarget] = useState(targets[0] ?? "");
  const [mode, setMode] = useState<"merge" | "posts">("merge");
  useEffect(() => {
    if (!targets.includes(target)) setTarget(targets[0] ?? "");
  }, [targets, target]);
  if (!target) return null;
  return (
    <section className="stack" aria-label={t("Merge")}>
      <h3 className="small">{t("Merge into another copy")}</h3>
      <div className="row wrap">
        <label className="stack tight small">
          {t("Into")}
          <select value={target} onChange={(e) => setTarget(e.target.value)}>
            {targets.map((d) => (
              <option key={d} value={d}>
                {d}
              </option>
            ))}
          </select>
        </label>
        <div className="row" role="radiogroup" aria-label={t("What to take")}>
          <label className="row small">
            <input type="radio" checked={mode === "merge"} onChange={() => setMode("merge")} />
            {t("Changes since the staging was made")}
          </label>
          <label className="row small">
            <input type="radio" checked={mode === "posts"} onChange={() => setMode("posts")} />
            {t("Single posts and pages")}
          </label>
        </div>
      </div>
      {mode === "merge" ? <MergeChanges key={target} source={source} target={target} /> : <MovePosts key={target} source={source} target={target} />}
    </section>
  );
}

function MergeChanges({ source, target }: { source: string; target: string }) {
  const t = useT();
  const [list, setList] = useState<WpChangeList | null>(null);
  const [files, setFiles] = useState<Set<string>>(new Set());
  const [switches, setSwitches] = useState<Set<string>>(new Set());
  const [settings, setSettings] = useState<Set<string>>(new Set());
  const [level, setLevel] = useState<WpMergeLevel>("code");
  const [typed, setTyped] = useState("");
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [done, setDone] = useState<WpMerged | null>(null);
  const [undone, setUndone] = useState(false);

  async function find() {
    setBusy(t("Comparing the copies…"));
    setError(null);
    setDone(null);
    setUndone(false);
    try {
      const l = await wp.mergeChanges(source, target);
      setList(l);
      // What only the staging changed is picked; conflicts wait for the user.
      setFiles(new Set(l.files.filter((f) => !f.conflict).map((f) => f.path)));
      setSwitches(new Set(l.switches.filter((s) => !s.conflict).map((s) => s.id)));
      setSettings(new Set(l.settings.filter((s) => !s.conflict).map((s) => s.name)));
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(null);
    }
  }

  const groups = useMemo(() => {
    const m = new Map<string, WpChangeList["files"]>();
    for (const f of list?.files ?? []) m.set(f.group, [...(m.get(f.group) ?? []), f]);
    return [...m.entries()];
  }, [list]);

  function toggle(set: Set<string>, update: (s: Set<string>) => void, keys: string[], on: boolean) {
    const next = new Set(set);
    for (const k of keys) {
      if (on) next.add(k);
      else next.delete(k);
    }
    update(next);
  }

  const name = target.split(".")[0];
  const nothing = list && list.files.length === 0 && list.switches.length === 0 && list.settings.length === 0;

  async function run() {
    setBusy(t("Merging…"));
    setError(null);
    try {
      const merged = await wp.merge(source, target, {
        level,
        files: [...files],
        switches: level === "everything" ? [] : [...switches],
        settings: level === "settings" ? [...settings] : [],
      });
      setDone(merged);
      setList(null);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(null);
    }
  }

  return (
    <div className="stack">
      <p className="muted small">
        {t("Lists what changed on {source} since it was made. Anything {target} also changed since then is marked; you decide. A snapshot of {target} is taken first, so the merge can be undone.", { source, target })}
      </p>
      {!list && (
        <div className="row">
          <button type="button" className="btn primary" disabled={busy !== null} onClick={find}>
            {t("Find the changes")}
          </button>
        </div>
      )}
      {list && nothing && <p className="muted small">{t("Nothing changed on {site} since it was made ({date}).", { site: source, date: when(list.base_at) })}</p>}
      {list && !nothing && (
        <>
          <p className="muted small">{t("Changes since {date}.", { date: when(list.base_at) })}</p>
          {groups.length > 0 && (
            <div className="stack tight" aria-label={t("Files")}>
              <h4 className="small">{t("Files")}</h4>
              {groups.map(([group, items]) => {
                const all = items.every((f) => files.has(f.path));
                return (
                  <details key={group}>
                    <summary className="small">
                      <label className="small" style={{ display: "inline-flex", gap: 8, alignItems: "center" }} onClick={(e) => e.stopPropagation()}>
                        <input type="checkbox" checked={all} onChange={(e) => toggle(files, setFiles, items.map((f) => f.path), e.target.checked)} />
                        {groupText(t, group)} · {items.length === 1 ? t("1 file") : t("{count} files", { count: items.length })}
                        {items.some((f) => f.conflict) && <span className="warn">{t("changed on both")}</span>}
                      </label>
                    </summary>
                    {items.map((f) => (
                      <label key={f.path} className="row small">
                        <input type="checkbox" checked={files.has(f.path)} onChange={(e) => toggle(files, setFiles, [f.path], e.target.checked)} />
                        <span className="mono">{f.path}</span>
                        <span className="muted">{changeText(t, f.change)}</span>
                        {f.conflict && f.target && <span className="warn">{t("also {change} on {site}", { change: changeText(t, f.target), site: target })}</span>}
                      </label>
                    ))}
                  </details>
                );
              })}
            </div>
          )}

          <fieldset className="stack tight">
            <legend className="small">{t("How much to merge")}</legend>
            <label className="row small">
              <input type="radio" checked={level === "code"} onChange={() => setLevel("code")} />
              <span>
                <strong>{t("Code")}</strong> {t("Files, and which plugins and theme are on. {site} keeps its whole database.", { site: target })}
              </span>
            </label>
            <label className="row small">
              <input type="radio" checked={level === "settings"} onChange={() => setLevel("settings")} />
              <span>
                <strong>{t("Settings")}</strong> {t("Code, plus the settings you pick below.")}
              </span>
            </label>
            <label className="row small">
              <input type="radio" checked={level === "everything"} onChange={() => setLevel("everything")} />
              <span>
                <strong>{t("Everything")}</strong> {t("Code, plus the staging's whole database: posts, pages, products and settings. {site}'s users, orders and customers are kept.", { site: target })}
              </span>
            </label>
          </fieldset>

          {level !== "everything" && list.switches.length > 0 && (
            <div className="stack tight">
              <h4 className="small">{t("Plugins and theme")}</h4>
              {list.switches.map((s) => (
                <label key={s.id} className="row small">
                  <input type="checkbox" checked={switches.has(s.id)} onChange={(e) => toggle(switches, setSwitches, [s.id], e.target.checked)} />
                  {s.on ? t("Switch {name} on", { name: s.title }) : t("Switch {name} off", { name: s.title })}
                  {s.conflict && <span className="warn">{t("changed on both")}</span>}
                </label>
              ))}
            </div>
          )}

          {level === "settings" && (
            <div className="stack tight">
              <h4 className="small">{t("Settings")}</h4>
              {list.settings.length === 0 && <p className="muted small">{t("No settings changed.")}</p>}
              {list.settings.map((s) => (
                <label key={s.name} className="row small">
                  <input type="checkbox" checked={settings.has(s.name)} onChange={(e) => toggle(settings, setSettings, [s.name], e.target.checked)} />
                  <span className="mono">{s.name}</span>
                  <span className="muted">
                    {s.change === "removed" ? changeText(t, s.change) : `${s.target ?? "–"} → ${s.staging ?? "–"}`}
                  </span>
                  {s.conflict && <span className="warn">{t("changed on both")}</span>}
                </label>
              ))}
            </div>
          )}

          {level === "everything" && (
            <div className="stack tight">
              <ul className="small warn">
                {list.target_new_posts > 0 && (
                  <li>
                    {t("Posts or pages made on {site} since the staging was made ({count}) are replaced by the staging's content. Move them to the staging first, or pick Settings.", {
                      count: list.target_new_posts,
                      site: target,
                    })}
                  </li>
                )}
                {list.store && <li>{t("Orders on {site} ({count}) are kept, with their customers.", { count: list.store.orders, site: target })}</li>}
                {list.staging_new_users > 0 && (
                  <li>{t("Users added on the staging ({count}) are not taken; {site}'s users are kept.", { count: list.staging_new_users, site: target })}</li>
                )}
              </ul>
              <label className="stack tight small">
                {t("Type {name} to confirm.", { name })}
                <input value={typed} onChange={(e) => setTyped(e.target.value)} />
              </label>
            </div>
          )}

          <div className="row">
            <button
              type="button"
              className="btn primary"
              disabled={busy !== null || (level === "everything" && typed.trim() !== name)}
              onClick={run}
            >
              {t("Merge into {site}", { site: target })}
            </button>
            <button type="button" className="btn ghost" disabled={busy !== null} onClick={() => setList(null)}>
              {t("Cancel")}
            </button>
          </div>
        </>
      )}
      {done && !undone && (
        <div className="stack tight" role="status">
          <p className="notice small">
            {t("Merged into {site}: {files} files, {switches} plugin or theme switches, {settings} settings.", {
              site: target,
              files: done.files,
              switches: done.switches,
              settings: done.settings,
            })}
          </p>
          {done.notes.map((n) => (
            <p key={n} className="warn small">
              {n}
            </p>
          ))}
          <div className="row">
            <Undo target={target} snapshot={done.snapshot} onDone={() => setUndone(true)} />
          </div>
        </div>
      )}
      {undone && <p className="notice small">{t("{site} is back as it was before the merge.", { site: target })}</p>}
      {busy && (
        <p className="muted small" role="status">
          {busy}
        </p>
      )}
      {error && (
        <p className="error small" role="alert">
          {error}
        </p>
      )}
    </div>
  );
}

function typeText(t: T, type: string) {
  return { post: t("Post"), page: t("Page"), attachment: t("Image or file"), product: t("Product"), wp_block: t("Pattern") }[type] ?? type;
}

function MovePosts({ source, target }: { source: string; target: string }) {
  const t = useT();
  const [search, setSearch] = useState("");
  const [found, setFound] = useState<WpPostSummary[] | null>(null);
  const [picked, setPicked] = useState<Map<number, WpPostSummary>>(new Map());
  const [existing, setExisting] = useState<Record<number, WpExisting>>({});
  const [comments, setComments] = useState(false);
  const [plan, setPlan] = useState<WpMovePlan | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [moved, setMoved] = useState<WpMoved | null>(null);
  const [undone, setUndone] = useState(false);

  useEffect(() => {
    const id = setTimeout(() => {
      wp.postsList(source, search.trim())
        .then(setFound)
        .catch((e) => setError(errorText(e)));
    }, 250);
    return () => clearTimeout(id);
  }, [source, search]);

  const choice: WpMoveChoice = { ids: [...picked.keys()], existing, comments };

  async function preview(next: WpMoveChoice = choice) {
    setBusy(t("Working out what moves…"));
    setError(null);
    try {
      setPlan(await wp.postsPlan(source, target, next));
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(null);
    }
  }

  async function move() {
    setBusy(t("Moving…"));
    setError(null);
    try {
      setMoved(await wp.postsMove(source, target, choice));
      setUndone(false);
      setPlan(null);
      setPicked(new Map());
      setExisting({});
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(null);
    }
  }

  function pick(p: WpPostSummary, on: boolean) {
    const next = new Map(picked);
    if (on) next.set(p.id, p);
    else next.delete(p.id);
    setPicked(next);
    setPlan(null);
  }

  return (
    <div className="stack">
      <p className="muted small">
        {t("Copies the picked posts and pages to {target} with their images, categories, tags and custom fields. Numbers that point to other posts are changed to match {target}. A snapshot of {target} is taken first.", { target })}
      </p>
      {!plan && (
        <>
          <input type="search" value={search} placeholder={t("Search by title or slug")} aria-label={t("Search by title or slug")} onChange={(e) => setSearch(e.target.value)} />
          <div className="stack tight" aria-label={t("Posts and pages")}>
            {found?.length === 0 && <p className="muted small">{t("Nothing found.")}</p>}
            {found?.map((p) => (
              <label key={p.id} className="row small">
                <input type="checkbox" checked={picked.has(p.id)} onChange={(e) => pick(p, e.target.checked)} />
                <span>{p.title || t("(no title)")}</span>
                <span className="muted">
                  {typeText(t, p.post_type)} · {p.status} · {p.modified}
                </span>
              </label>
            ))}
          </div>
          <label className="row small">
            <input type="checkbox" checked={comments} onChange={(e) => setComments(e.target.checked)} />
            {t("Take their comments too")}
          </label>
          <div className="row">
            <button type="button" className="btn primary" disabled={busy !== null || picked.size === 0} onClick={() => preview()}>
              {picked.size === 0
                ? t("Pick posts or pages to move")
                : picked.size === 1
                  ? t("Preview moving 1 item")
                  : t("Preview moving {count} items", { count: picked.size })}
            </button>
          </div>
        </>
      )}
      {plan && (
        <div className="stack">
          <div className="table-wrap">
            <table>
              <thead>
                <tr>
                  <th>{t("Title")}</th>
                  <th>{t("Type")}</th>
                  <th>{t("On {site}", { site: target })}</th>
                </tr>
              </thead>
              <tbody>
                {plan.posts.map((p) => (
                  <tr key={p.source_id}>
                    <td>
                      {p.title || t("(no title)")}
                      {!p.picked && <span className="muted"> · {t("comes along")}</span>}
                    </td>
                    <td>{typeText(t, p.post_type)}</td>
                    <td>
                      {p.exists && p.picked ? (
                        <select
                          aria-label={t("{title} is already on {site}", { title: p.title, site: target })}
                          value={existing[p.source_id] ?? "update"}
                          onChange={(e) => {
                            const next = { ...existing, [p.source_id]: e.target.value as WpExisting };
                            setExisting(next);
                            preview({ ...choice, existing: next });
                          }}
                        >
                          <option value="update">{t("Replace the one there")}</option>
                          <option value="new">{t("Add as a new one")}</option>
                          <option value="skip">{t("Leave it out")}</option>
                        </select>
                      ) : (
                        {
                          add: t("added as #{id}", { id: p.target_id }),
                          update: t("replaces #{id}", { id: p.target_id }),
                          skip: t("left out"),
                          reuse: t("already there as #{id}", { id: p.target_id }),
                        }[p.action]
                      )}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
          <p className="muted small">
            {t("{files} media files, {terms} categories and tags ({new} new), {comments} comments.", {
              files: plan.counts.files,
              terms: plan.counts.terms,
              new: plan.counts.new_terms,
              comments: plan.counts.comments,
            })}
          </p>
          {plan.warnings.length > 0 && (
            <div className="stack tight">
              <h4 className="small warn">{t("Can't be changed safely")}</h4>
              <ul className="small">
                {plan.warnings.map((w, i) => (
                  <li key={i}>
                    <strong>{w.title}</strong>: {w.text}
                  </li>
                ))}
              </ul>
              <p className="muted small">{t("These are copied as they are. Check them on {site} after moving.", { site: target })}</p>
            </div>
          )}
          <div className="row">
            <button type="button" className="btn primary" disabled={busy !== null} onClick={move}>
              {t("Move to {site}", { site: target })}
            </button>
            <button type="button" className="btn ghost" disabled={busy !== null} onClick={() => setPlan(null)}>
              {t("Back")}
            </button>
          </div>
        </div>
      )}
      {moved && !undone && (
        <div className="stack tight" role="status">
          <p className="notice small">
            {t("Moved {posts} items and {files} media files to {site}.", { posts: moved.posts, files: moved.files, site: target })}
          </p>
          <div className="row">
            <Undo target={target} snapshot={moved.snapshot} onDone={() => setUndone(true)} />
          </div>
        </div>
      )}
      {undone && <p className="notice small">{t("{site} is back as it was before the move.", { site: target })}</p>}
      {busy && (
        <p className="muted small" role="status">
          {busy}
        </p>
      )}
      {error && (
        <p className="error small" role="alert">
          {error}
        </p>
      )}
    </div>
  );
}
