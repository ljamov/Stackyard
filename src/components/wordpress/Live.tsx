import { useCallback, useEffect, useRef, useState } from "react";
import { errorText } from "../../lib/api";
import { useT } from "../../lib/i18n";
import {
  wp,
  type WpEnvironment,
  type WpLiveBackup,
  type WpMergeLevel,
  type WpPulled,
  type WpPushed,
  type WpSyncChanges,
  type WpSyncProgress,
  type WpWay,
} from "../../lib/wordpress";
import { stageText } from "./CloneSite";
import { FileGroups, MovePosts, SettingList, SwitchList, Undo, UndoWith, when } from "./Merge";
import { size } from "./WordPressTools";
import { useBusy } from "../../lib/tabLife";

type Live = NonNullable<WpEnvironment["live"]>;

/** A request from the WordPress view's Pull and Push buttons to open the
 * live panel in Servers & Git one way; `at` tells two presses apart. */
export type LiveAsk = { way: WpWay; at: number };

/** Progress of the push, pull or live read running for this copy. */
function useSyncProgress(domain: string) {
  const [progress, setProgress] = useState<WpSyncProgress | null>(null);
  useEffect(() => {
    let off: (() => void) | undefined;
    let gone = false;
    wp.onSyncProgress((p) => {
      if (p.domain === domain) setProgress(p);
    }).then((f) => {
      if (gone) f();
      else off = f;
    });
    return () => {
      gone = true;
      off?.();
    };
  }, [domain]);
  return [progress, setProgress] as const;
}

function Progress({ progress, onStop }: { progress: WpSyncProgress | null; onStop: () => void }) {
  const t = useT();
  return (
    <div className="stack" role="status">
      {progress && (
        <>
          <div className="progress" aria-hidden="true">
            <div
              className={`progress-fill${progress.total ? "" : " busy"}`}
              style={{ width: progress.total ? `${Math.round((progress.done / progress.total) * 100)}%` : "30%" }}
            />
          </div>
          <p className="muted small">
            {stageText(t, progress.stage)}: {progress.message}
            {progress.stage === "files" && progress.total > 1000 && ` ${size(progress.done)} / ${size(progress.total)}`}
          </p>
        </>
      )}
      <div className="row">
        <button type="button" className="btn small" onClick={onStop}>
          {t("Stop")}
        </button>
      </div>
    </div>
  );
}

/** The Live site part of a WordPress copy's Servers & Git tab; it waits
 * to learn which site is the main one (this, or the one it is a staging of). */
export function LiveSiteTab({ domain, live, ask }: { domain: string; live: Live; ask?: LiveAsk | null }) {
  const [main, setMain] = useState<string | null>(null);
  useEffect(() => {
    wp.stagings(domain)
      .then((envs) => setMain(envs.main))
      .catch(() => setMain(domain));
  }, [domain]);
  if (!main) return null;
  return <LivePanel key={ask?.at ?? 0} domain={domain} main={main} live={live} ask={ask ?? undefined} />;
}

/** Push to and pull from the live site this copy was cloned from: changes
 * since they were last in step, or single posts and pages. */
export function LivePanel({ domain, main, live, ask }: { domain: string; main: string; live: Live; ask?: LiveAsk }) {
  const t = useT();
  const [way, setWay] = useState<WpWay>(ask?.way ?? "pull");
  const panel = useRef<HTMLElement>(null);
  useEffect(() => {
    if (ask) panel.current?.scrollIntoView({ block: "start", behavior: "smooth" });
  }, [ask]);
  const [mode, setMode] = useState<"changes" | "posts">("changes");
  const [backupsShown, setBackupsShown] = useState(false);
  if (live.route === "files") {
    return (
      <section className="stack" aria-label={t("Live site")}>
        <h3 className="small">{t("Live site")}</h3>
        <p className="muted small">{t("This copy was cloned from backup files, so Stackyard can't reach the live server. Clone it again from the server to push and pull.")}</p>
      </section>
    );
  }
  return (
    <section className="stack" aria-label={t("Live site")} ref={panel}>
      <h3 className="small">{t("Live site")}</h3>
      <p className="muted small">
        <span className="mono">{live.url}</span> · {live.synced_at ? t("last in step {date}", { date: when(live.synced_at) }) : t("never in step yet")}
      </p>
      <div className="stack tight">
        <div className="row" role="radiogroup" aria-label={t("Which way")}>
          <label className="row small">
            <input type="radio" checked={way === "pull"} onChange={() => setWay("pull")} />
            {t("Pull from live")}
          </label>
          <label className="row small">
            <input type="radio" checked={way === "push"} onChange={() => setWay("push")} />
            {t("Push to live")}
          </label>
        </div>
        <div className="row" role="radiogroup" aria-label={t("What to take")}>
          <label className="row small">
            <input type="radio" checked={mode === "changes"} onChange={() => setMode("changes")} />
            {t("Changes since last in step")}
          </label>
          <label className="row small">
            <input type="radio" checked={mode === "posts"} onChange={() => setMode("posts")} />
            {t("Single posts and pages")}
          </label>
        </div>
      </div>
      {mode === "changes" ? <SyncChanges key={way} domain={domain} main={main} way={way} /> : <LivePosts key={way} domain={domain} way={way} />}
      <details open={backupsShown} onToggle={(e) => setBackupsShown((e.target as HTMLDetailsElement).open)}>
        <summary className="small">{t("Backups of the live site")}</summary>
        {backupsShown && <LiveBackups domain={domain} />}
      </details>
    </section>
  );
}

function SyncChanges({ domain, main, way }: { domain: string; main: string; way: WpWay }) {
  const t = useT();
  const [list, setList] = useState<WpSyncChanges | null>(null);
  const [files, setFiles] = useState<Set<string>>(new Set());
  const [switches, setSwitches] = useState<Set<string>>(new Set());
  const [settings, setSettings] = useState<Set<string>>(new Set());
  const [level, setLevel] = useState<WpMergeLevel>("code");
  const [typed, setTyped] = useState("");
  const [busy, setBusy] = useState<string | null>(null);
  useBusy(busy !== null);
  const [error, setError] = useState<string | null>(null);
  const [pushed, setPushed] = useState<WpPushed | null>(null);
  const [pulled, setPulled] = useState<WpPulled | null>(null);
  const [undone, setUndone] = useState(false);
  const [progress, setProgress] = useSyncProgress(domain);
  const push = way === "push";
  const name = main.split(".")[0];

  async function work<R>(label: string, job: () => Promise<R>): Promise<R | undefined> {
    setBusy(label);
    setError(null);
    setProgress(null);
    try {
      return await job();
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(null);
      setProgress(null);
    }
  }

  async function find() {
    setPushed(null);
    setPulled(null);
    setUndone(false);
    const l = await work(t("Comparing with the live site…"), () => wp.syncChanges(domain, way));
    if (!l) return;
    setList(l);
    setFiles(new Set(l.files.filter((f) => !f.conflict).map((f) => f.path)));
    setSwitches(new Set(l.switches.filter((s) => !s.conflict).map((s) => s.id)));
    setSettings(new Set(l.settings.filter((s) => !s.conflict).map((s) => s.name)));
    // Posts and pages are in the database: when only the live side changed
    // them, a pull brings the live database too, as nothing here is lost.
    if (!push && l.live_content_changed && !l.local_content_changed) setLevel("everything");
  }

  async function run() {
    const choice = {
      level,
      files: [...files],
      switches: level === "everything" ? [] : [...switches],
      settings: level === "settings" ? [...settings] : [],
    };
    if (push) {
      const done = await work(t("Pushing…"), () => wp.push(domain, choice, typed.trim()));
      if (done) {
        setPushed(done);
        setList(null);
      }
    } else {
      const done = await work(t("Pulling…"), () => wp.pull(domain, choice));
      if (done) {
        setPulled(done);
        setList(null);
      }
    }
  }

  const nothing = list && list.files.length === 0 && list.switches.length === 0 && list.settings.length === 0;
  const contentBlocks = push && level === "everything" && list?.live_content_changed;
  const other = push ? t("the live site") : domain;

  return (
    <div className="stack">
      <p className="muted small">
        {push
          ? t("Lists what changed on {site} since it was last in step with the live site. A backup of the live site is taken first, and the live site shows a maintenance page while it changes.", { site: domain })
          : t("Lists what changed on the live site since {site} was last in step with it. A snapshot of {site} is taken first, so the pull can be undone.", { site: domain })}
      </p>
      {!list && !busy && (
        <div className="row">
          <button type="button" className="btn primary" onClick={find}>
            {t("Find the changes")}
          </button>
        </div>
      )}
      {list && (
        <>
          {list.base_at > 0 && <p className="muted small">{t("Changes since {date}.", { date: when(list.base_at) })}</p>}
          {list.base_at === 0 && <p className="warn small">{t("Stackyard has no record of when this copy was last in step, so every difference is marked; you decide.")}</p>}
          {list.live_wordpress && list.local_wordpress && list.live_wordpress !== list.local_wordpress && (
            <p className="warn small">{t("WordPress is {live} on the live site and {local} here.", { live: list.live_wordpress, local: list.local_wordpress })}</p>
          )}
          {list.notes.map((n) => (
            <p key={n} className="warn small">
              {n}
            </p>
          ))}
          {nothing && level !== "everything" && <p className="muted small">{t("No files, plugins or settings changed.")}</p>}
          {!push && level !== "everything" && list.live_content_changed && (
            <p className="warn small">
              {t("Posts, pages or comments changed on the live site. They are in its database: pick Everything to bring them, or Single posts and pages above to take only some.")}
            </p>
          )}
          <FileGroups files={list.files} picked={files} onChange={setFiles} other={other} />

          <fieldset className="stack tight">
            <legend className="small">{push ? t("How much to push") : t("How much to pull")}</legend>
            <label className="row small">
              <input type="radio" checked={level === "code"} onChange={() => setLevel("code")} />
              <span>
                <strong>{t("Code")}</strong>{" "}
                {push ? t("Files, and which plugins and theme are on. The live database stays as it is.") : t("Files, and which plugins and theme are on. {site} keeps its whole database.", { site: domain })}
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
                <strong>{t("Everything")}</strong>{" "}
                {push
                  ? t("Code, plus this copy's whole database. The live site's orders, customers and payment settings are kept.")
                  : t("Code, plus the live site's whole database, with local addresses and payments in test mode.")}
              </span>
            </label>
          </fieldset>

          {level !== "everything" && <SwitchList switches={list.switches} picked={switches} onChange={setSwitches} />}
          {level === "settings" && <SettingList settings={list.settings} picked={settings} onChange={setSettings} />}

          {level === "everything" && push && (
            <div className="stack tight">
              {contentBlocks ? (
                <p className="warn small">
                  {t("Posts or comments changed on the live site since {date}. Pushing everything would lose them: pull from live first, or move single posts instead.", { date: when(list.base_at) })}
                </p>
              ) : (
                <label className="stack tight small">
                  {t("Type {name} to confirm.", { name })}
                  <input value={typed} onChange={(e) => setTyped(e.target.value)} />
                </label>
              )}
            </div>
          )}
          {level === "everything" && !push && list.local_content_changed && (
            <p className="warn small">{t("Posts or comments changed on {site} since it was last in step. Pulling everything replaces them with the live site's.", { site: domain })}</p>
          )}

          {!busy && (
            <div className="row">
              <button
                type="button"
                className="btn primary"
                disabled={(level === "everything" && push && (contentBlocks || typed.trim() !== name)) || (nothing === true && level !== "everything")}
                onClick={run}
              >
                {push ? t("Push to live") : t("Pull into {site}", { site: domain })}
              </button>
              <button type="button" className="btn ghost" onClick={() => setList(null)}>
                {t("Cancel")}
              </button>
            </div>
          )}
        </>
      )}
      {busy && (
        <>
          <p className="muted small">{busy}</p>
          <Progress progress={progress} onStop={() => wp.syncStop()} />
        </>
      )}
      {pushed && !undone && (
        <div className="stack tight" role="status">
          <p className="notice small">
            {t("Pushed to the live site: {files} files, {switches} plugin or theme switches, {settings} settings.", {
              files: pushed.files,
              switches: pushed.switches,
              settings: pushed.settings,
            })}
          </p>
          {!pushed.site_answers && <p className="warn small">{t("The live site did not answer as expected afterwards. Open it; if it is broken, put the backup back.")}</p>}
          {pushed.notes.map((n) => (
            <p key={n} className="warn small">
              {n}
            </p>
          ))}
          <div className="row">
            <UndoWith undo={() => wp.restoreLiveBackup(domain, pushed.backup)} onDone={() => setUndone(true)} />
          </div>
        </div>
      )}
      {pulled && !undone && (
        <div className="stack tight" role="status">
          <p className="notice small">
            {t("Pulled into {site}: {files} files, {switches} plugin or theme switches, {settings} settings.", {
              site: domain,
              files: pulled.files,
              switches: pulled.switches,
              settings: pulled.settings,
            })}
          </p>
          {pulled.notes.map((n) => (
            <p key={n} className="warn small">
              {n}
            </p>
          ))}
          <div className="row">
            <Undo target={domain} snapshot={pulled.snapshot} onDone={() => setUndone(true)} />
          </div>
        </div>
      )}
      {undone && <p className="notice small">{push ? t("The live site is back as it was before the push.") : t("{site} is back as it was before the pull.", { site: domain })}</p>}
      {error && (
        <p className="error small" role="alert">
          {error}
        </p>
      )}
    </div>
  );
}

/** Single posts to or from the live site. The live database is read into
 * a local copy first, so searching and previewing need no connection. */
function LivePosts({ domain, way }: { domain: string; way: WpWay }) {
  const t = useT();
  const [readAt, setReadAt] = useState<number | null | undefined>(undefined);
  const [busy, setBusy] = useState(false);
  useBusy(busy);
  const [error, setError] = useState<string | null>(null);
  const [progress, setProgress] = useSyncProgress(domain);
  const [round, setRound] = useState(0);
  const push = way === "push";

  useEffect(() => {
    wp.liveReadAt(domain)
      .then(setReadAt)
      .catch(() => setReadAt(null));
  }, [domain]);

  async function read() {
    setBusy(true);
    setError(null);
    setProgress(null);
    try {
      setReadAt(await wp.readLive(domain));
      setRound((r) => r + 1);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
      setProgress(null);
    }
  }

  if (readAt === undefined) return null;
  const live = t("the live site");
  return (
    <div className="stack">
      <div className="row wrap">
        <p className="muted small">
          {readAt ? t("The live posts as read {date}.", { date: when(readAt) }) : t("Read the live database first to pick posts from it or see what is already there.")}
        </p>
        {!busy && (
          <button type="button" className={`btn small${readAt ? "" : " primary"}`} onClick={read}>
            {readAt ? t("Read again") : t("Read the live site")}
          </button>
        )}
      </div>
      {busy && <Progress progress={progress} onStop={() => wp.syncStop()} />}
      {error && (
        <p className="error small" role="alert">
          {error}
        </p>
      )}
      {readAt && !busy && (
        <MovePosts
          key={round}
          source={push ? domain : live}
          target={push ? live : domain}
          intro={
            push
              ? t("Copies the picked posts and pages to the live site with their images, categories, tags and custom fields. The live database is read again first, and a backup of it is taken.")
              : t("Copies the picked posts and pages from the live site into {site} with their images, categories, tags and custom fields. A snapshot of {site} is taken first.", { site: domain })
          }
          ops={{
            list: (search) => (push ? wp.postsList(domain, search) : wp.livePosts(domain, search)),
            plan: (choice) => wp.livePostsPlan(domain, way, choice),
            move: async (choice) => {
              const moved = await wp.livePostsMove(domain, way, choice);
              return {
                ...moved,
                undo: () => (push ? wp.restoreLiveBackup(domain, moved.undo) : wp.restoreSnapshot(domain, moved.undo)),
              };
            },
          }}
        />
      )}
    </div>
  );
}

function LiveBackups({ domain }: { domain: string }) {
  const t = useT();
  const [list, setList] = useState<WpLiveBackup[] | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  useBusy(busy !== null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [progress, setProgress] = useSyncProgress(domain);

  const reload = useCallback(() => {
    wp.liveBackups(domain)
      .then(setList)
      .catch((e) => setError(errorText(e)));
  }, [domain]);
  useEffect(reload, [reload]);

  async function act(id: string, job: () => Promise<void>, done: string) {
    setBusy(id);
    setError(null);
    setNotice(null);
    setProgress(null);
    try {
      await job();
      setNotice(done);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(null);
      reload();
    }
  }

  if (!list) return null;
  return (
    <div className="stack tight">
      <p className="muted small">{t("Taken before each push: the live database and the files the push changed.")}</p>
      {list.length === 0 && <p className="muted small">{t("No backups yet.")}</p>}
      {list.map((b) => (
        <div key={b.id} className="list-row">
          <div className="stack tight">
            <span className="small">{when(b.created_at)}</span>
            <span className="muted small">
              {size(b.bytes)} · {b.saved.length === 1 ? t("1 file") : t("{count} files", { count: b.saved.length })}
            </span>
          </div>
          <div className="row">
            <button
              type="button"
              className="btn small"
              disabled={busy !== null}
              onClick={() => act(b.id, () => wp.restoreLiveBackup(domain, b.id), t("The live site is back as it was on {date}.", { date: when(b.created_at) }))}
            >
              {busy === b.id ? t("Putting it back…") : t("Put back on live")}
            </button>
            <button type="button" className="btn small ghost" disabled={busy !== null} onClick={() => act(b.id, () => wp.removeLiveBackup(domain, b.id), t("Backup removed."))}>
              {t("Remove")}
            </button>
          </div>
        </div>
      ))}
      {busy && <Progress progress={progress} onStop={() => wp.syncStop()} />}
      {notice && <p className="notice small">{notice}</p>}
      {error && (
        <p className="error small" role="alert">
          {error}
        </p>
      )}
    </div>
  );
}
