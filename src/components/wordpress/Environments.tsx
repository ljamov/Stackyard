import { useCallback, useEffect, useState } from "react";
import { api, errorText, type Site } from "../../lib/api";
import { useT } from "../../lib/i18n";
import {
  wp,
  type WpCloneProgress,
  type WpEnvironment,
  type WpEnvironments,
  type WpSnapshots,
  type WpStagingFrom,
  type WpTrashed,
} from "../../lib/wordpress";
import { stageText } from "./CloneSite";
import { LivePanel, type LiveAsk } from "./Live";
import { MergePanel } from "./Merge";
import { size } from "./WordPressTools";

type T = ReturnType<typeof useT>;

function when(at: number) {
  return new Date(at * 1000).toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });
}

function madeFromText(t: T, from: string | null) {
  if (!from) return "";
  return from === "live" ? t("made from the live site") : t("made from {site}", { site: from });
}

/** The Environments part of the WordPress tab: the main site, its stagings,
 * the trash, and snapshots of this copy. */
export function EnvironmentsTab({ site, liveAsk }: { site: Site; liveAsk?: LiveAsk | null }) {
  const t = useT();
  const [envs, setEnvs] = useState<WpEnvironments | null>(null);
  const [here, setHere] = useState<WpEnvironment | null>(null);
  const [trash, setTrash] = useState<WpTrashed[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [confirmDestroy, setConfirmDestroy] = useState<string | null>(null);
  const [typed, setTyped] = useState("");

  const reload = useCallback(() => {
    wp.stagings(site.domain)
      .then(setEnvs)
      .catch((e) => setError(errorText(e)));
    wp.environment(site.domain)
      .then(setHere)
      .catch(() => setHere(null));
    wp.trash()
      .then(setTrash)
      .catch(() => setTrash([]));
  }, [site.domain]);
  useEffect(reload, [reload]);

  async function act(label: string, work: () => Promise<unknown>, done?: string) {
    setBusy(label);
    setError(null);
    setNotice(null);
    try {
      await work();
      if (done) setNotice(done);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(null);
      reload();
    }
  }

  if (!envs) {
    return error ? (
      <p className="error small" role="alert">
        {error}
      </p>
    ) : (
      <p className="muted small">{t("Reading the site…")}</p>
    );
  }
  const isStaging = envs.main !== site.domain;
  const mine = trash.filter((x) => x.site.tld === `.${envs.main}`);

  return (
    <div className="stack">
      <p className="muted small">
        {isStaging
          ? t("This is a staging of {site}. Stagings are full copies with their own files and database, for trying changes before they go to the main site.", { site: envs.main })
          : t("Stagings are full copies of this site with their own files and database, for trying changes before they go to the main site.")}
      </p>

      <section className="stack" aria-label={t("Stagings")}>
        <h3 className="small">{t("Stagings of {site}", { site: envs.main })}</h3>
        {envs.stagings.length === 0 && <p className="muted small">{t("No stagings yet.")}</p>}
        {envs.stagings.map((s) => (
          <div key={s.domain} className="list-row">
            <div className="stack tight">
              <span className="mono small">{s.domain}</span>
              <span className="muted small">
                {[madeFromText(t, s.made_from), when(s.created_at), s.shared_media ? t("media shared") : ""].filter(Boolean).join(" · ")}
              </span>
            </div>
            <div className="row wrap">
              <button type="button" className="btn small" onClick={() => api.openLocalPage(s.url)}>
                {t("Open")}
              </button>
              {confirmDestroy === s.domain ? (
                <>
                  <label className="stack tight warn small">
                    {t("Destroy {site}? It goes to the trash for 7 days. Type {name} to confirm.", { site: s.domain, name: s.domain.split(".")[0] })}
                    <input value={typed} onChange={(e) => setTyped(e.target.value)} autoFocus />
                  </label>
                  <button
                    type="button"
                    className="btn small danger"
                    disabled={busy !== null || typed.trim() !== s.domain.split(".")[0]}
                    onClick={() => {
                      setConfirmDestroy(null);
                      act(t("Destroying…"), () => wp.destroyStaging(s.domain), t("{site} is in the trash.", { site: s.domain }));
                    }}
                  >
                    {t("Destroy")}
                  </button>
                  <button type="button" className="btn small ghost" onClick={() => setConfirmDestroy(null)}>
                    {t("Cancel")}
                  </button>
                </>
              ) : (
                s.domain !== site.domain && (
                  <button type="button" className="btn small ghost" disabled={busy !== null} onClick={() => {
                      setTyped("");
                      setConfirmDestroy(s.domain);
                    }}>
                    {t("Destroy…")}
                  </button>
                )
              )}
            </div>
          </div>
        ))}
      </section>

      <NewStaging envs={envs} current={site.domain} disabled={busy !== null} onMade={(domain) => {
        setNotice(t("{site} is ready.", { site: domain }));
        reload();
      }} />

      {here?.live && <LivePanel key={liveAsk?.at ?? 0} domain={site.domain} main={envs.main} live={here.live} ask={liveAsk ?? undefined} />}

      {isStaging && <MergePanel source={site.domain} targets={[envs.main, ...envs.stagings.map((s) => s.domain)].filter((d) => d !== site.domain)} />}

      {mine.length > 0 && (
        <section className="stack" aria-label={t("Trash")}>
          <h3 className="small">{t("Trash")}</h3>
          {mine.map((x) => (
            <div key={x.id} className="list-row">
              <div className="stack tight">
                <span className="mono small">{`${x.site.name}${x.site.tld}`}</span>
                <span className="muted small">
                  {t("destroyed {date}; deleted for good after 7 days", { date: when(x.trashed_at) })}
                </span>
              </div>
              <div className="row wrap">
                <button
                  type="button"
                  className="btn small"
                  disabled={busy !== null}
                  onClick={() => act(t("Bringing it back…"), () => wp.restoreTrashed(x.id), t("{site} is back.", { site: `${x.site.name}${x.site.tld}` }))}
                >
                  {t("Bring back")}
                </button>
                <button type="button" className="btn small ghost" disabled={busy !== null} onClick={() => act(t("Deleting…"), () => wp.deleteTrashed(x.id))}>
                  {t("Delete now")}
                </button>
              </div>
            </div>
          ))}
        </section>
      )}

      <SnapshotList domain={site.domain} />

      {busy && (
        <p className="muted small" role="status">
          {busy}
        </p>
      )}
      {notice && (
        <p className="notice small" role="status">
          {notice}
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

function NewStaging({
  envs,
  current,
  disabled,
  onMade,
}: {
  envs: WpEnvironments;
  current: string;
  disabled: boolean;
  onMade: (domain: string) => void;
}) {
  const t = useT();
  const [name, setName] = useState("");
  const [from, setFrom] = useState<string>(current);
  const [copyMedia, setCopyMedia] = useState(true);
  const [progress, setProgress] = useState<WpCloneProgress | null>(null);
  const [making, setMaking] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let stop: (() => void) | undefined;
    wp.onStagingProgress(setProgress).then((u) => (stop = u));
    return () => stop?.();
  }, []);

  const clean = name.trim().toLowerCase();
  const sources = [envs.main, ...envs.stagings.map((s) => s.domain)];

  async function make() {
    const source: WpStagingFrom = from === "live" ? { kind: "live" } : { kind: "site", domain: from };
    setMaking(true);
    setError(null);
    setProgress(null);
    try {
      await wp.createStaging({ parent: envs.main, name: clean, from: source, copy_media: copyMedia });
      setName("");
      onMade(`${clean}.${envs.main}`);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setMaking(false);
      setProgress(null);
    }
  }

  return (
    <section className="stack" aria-label={t("New staging")}>
      <h3 className="small">{t("New staging")}</h3>
      <div className="row wrap">
        <label className="stack tight small">
          {t("Name")}
          <input
            value={name}
            placeholder="checkout"
            onChange={(e) => setName(e.target.value)}
            pattern="[a-z0-9-]+"
            aria-describedby="staging-address"
          />
        </label>
        <label className="stack tight small">
          {t("Copy of")}
          <select value={from} onChange={(e) => setFrom(e.target.value)}>
            {sources.map((d) => (
              <option key={d} value={d}>
                {d}
              </option>
            ))}
            {envs.from_live && <option value="live">{t("The live site")}</option>}
          </select>
        </label>
      </div>
      <p id="staging-address" className="muted small">
        {clean ? t("Its address: {url}", { url: `https://${clean}.${envs.main}` }) : t("Its address is the name in front of {site}, e.g. checkout.{site}.", { site: envs.main })}
      </p>
      {from !== "live" && (
        <label className="row small">
          <input type="checkbox" checked={copyMedia} onChange={(e) => setCopyMedia(e.target.checked)} />
          {t("Copy the media too (wp-content/uploads)")}
        </label>
      )}
      {from !== "live" && !copyMedia && (
        <p className="muted small">{t("The staging then copies each image from {site} the first time a page shows it. Faster to make and smaller.", { site: from })}</p>
      )}
      {from === "live" && <p className="muted small">{t("Downloaded from the live server like a clone, made safe the same way.")}</p>}
      {making && progress && (
        <div className="stack" role="status">
          <div className="progress" aria-hidden="true">
            <div
              className={`progress-fill${progress.total ? "" : " busy"}`}
              style={{ width: progress.total ? `${Math.round((progress.done / progress.total) * 100)}%` : "30%" }}
            />
          </div>
          <p className="muted small">
            {stageText(t, progress.stage)}: {progress.message}
            {progress.stage === "files" && progress.total > 0 && ` ${size(progress.done)} / ${size(progress.total)}`}
          </p>
        </div>
      )}
      <div className="row">
        {making ? (
          <button type="button" className="btn" onClick={() => wp.cloneStop()}>
            {t("Stop")}
          </button>
        ) : (
          <button type="button" className="btn primary" disabled={disabled || !/^[a-z0-9][a-z0-9-]*$/.test(clean)} onClick={make}>
            {t("Make the staging")}
          </button>
        )}
      </div>
      {error && (
        <p className="error small" role="alert">
          {error}
        </p>
      )}
    </section>
  );
}

function SnapshotList({ domain }: { domain: string }) {
  const t = useT();
  const [snaps, setSnaps] = useState<WpSnapshots | null>(null);
  const [confirm, setConfirm] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<{ ok: boolean; text: string } | null>(null);

  const reload = useCallback(() => {
    wp.snapshots(domain)
      .then(setSnaps)
      .catch(() => setSnaps(null));
  }, [domain]);
  useEffect(reload, [reload]);

  if (!snaps) return null;
  return (
    <section className="stack" aria-label={t("Snapshots")}>
      <h3 className="small">{t("Snapshots of {site}", { site: domain })}</h3>
      <p className="muted small">
        {t("Taken before Stackyard changes this copy (merges, pushes and pulls), so the change can be undone. The last 5 are kept.")}
      </p>
      {snaps.list.length === 0 && <p className="muted small">{t("No snapshots yet.")}</p>}
      {snaps.list.map((s) => (
        <div key={s.id} className="list-row">
          <div className="stack tight">
            <span className="small">{s.reason}</span>
            <span className="muted small">
              {when(s.created_at)} · {s.saved.length + s.added.length === 1 ? t("1 file") : t("{count} files", { count: s.saved.length + s.added.length })} · {size(s.bytes)}
            </span>
          </div>
          <div className="row wrap">
            {confirm === s.id ? (
              <>
                <span className="warn small">{t("Put the database and those files back as they were? A snapshot of how it is now is taken first.")}</span>
                <button
                  type="button"
                  className="btn small primary"
                  disabled={busy}
                  onClick={async () => {
                    setConfirm(null);
                    setBusy(true);
                    setMessage(null);
                    try {
                      await wp.restoreSnapshot(domain, s.id);
                      setMessage({ ok: true, text: t("Put back as it was on {date}.", { date: when(s.created_at) }) });
                    } catch (e) {
                      setMessage({ ok: false, text: errorText(e) });
                    } finally {
                      setBusy(false);
                      reload();
                    }
                  }}
                >
                  {t("Undo")}
                </button>
                <button type="button" className="btn small ghost" onClick={() => setConfirm(null)}>
                  {t("Cancel")}
                </button>
              </>
            ) : (
              <>
                <button type="button" className="btn small" disabled={busy} onClick={() => setConfirm(s.id)}>
                  {t("Undo…")}
                </button>
                <button type="button" className="btn small ghost" disabled={busy} onClick={() => wp.removeSnapshot(domain, s.id).then(reload)}>
                  {t("Remove")}
                </button>
              </>
            )}
          </div>
        </div>
      ))}
      {snaps.base !== null && (
        <p className="muted small">{t("The staging's starting point ({date}) is kept for merging.", { date: when(snaps.base) })}</p>
      )}
      {busy && <p className="muted small">{t("Putting it back…")}</p>}
      {message && <p className={`${message.ok ? "notice" : "error"} small`}>{message.text}</p>}
    </section>
  );
}
