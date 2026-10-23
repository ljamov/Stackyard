import type { PackProgress, ShareProgress } from "../lib/api";
import type { Translate } from "../lib/i18n";

/** Where a long action is: the words under the bar, and how full the bar is (`null` = no way to tell yet). */
export type ProgressState = { text: string; percent: number | null };

/** A progress bar with what is happening under it, in words and numbers. */
export function Progress({ label, state }: { label: string; state: ProgressState }) {
  const { text, percent } = state;
  return (
    <div className="progress-line">
      <div
        className="progress"
        role="progressbar"
        aria-label={label}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={percent ?? undefined}
      >
        <div className={percent === null ? "progress-fill busy" : "progress-fill"} style={{ width: `${percent ?? 30}%` }} />
      </div>
      <span className="muted small" role="status">
        {text}
        {percent !== null && <span className="mono"> · {percent}%</span>}
      </span>
    </div>
  );
}

const mb = (bytes: number) => (bytes / 1e6).toFixed(1);

/** A download that is unpacked after: runtimes, services, phpMyAdmin, cloudflared. */
export function downloadState(
  t: Translate,
  p?: { step: string; done?: number; total?: number | null } | null,
): ProgressState {
  if (p?.step === "unpacking") return { text: t("Unpacking…"), percent: 100 };
  if (p?.step === "downloading") {
    const done = p.done ?? 0;
    return p.total
      ? { text: t("Downloading {done} of {total} MB", { done: mb(done), total: mb(p.total) }), percent: Math.min(100, Math.floor((done * 100) / p.total)) }
      : { text: t("Downloading {done} MB", { done: mb(done) }), percent: null };
  }
  return { text: t("Looking up the latest release…"), percent: null };
}

/** cloudflared's download, then the wait for Cloudflare's address. */
export function shareState(t: Translate, p: ShareProgress | null): ProgressState {
  if (p?.step === "connecting") return { text: t("Asking Cloudflare for an address…"), percent: null };
  return downloadState(t, p);
}

/**
 * A site backup, export, restore or import. A restore backs up the site as
 * it is first, so its first stages say so.
 */
export function packState(t: Translate, p: PackProgress | null, restoring = false): ProgressState {
  const pct = (done: number, total: number) => (total ? Math.min(100, Math.floor((done * 100) / total)) : 100);
  const first = (s: ProgressState) => (restoring ? { ...s, text: `${t("Backing up the site as it is now first")} · ${s.text}` } : s);
  if (!p) return { text: t("Starting…"), percent: null };
  switch (p.stage) {
    case "database":
      return first({ text: t("Saving the database…"), percent: null });
    case "files":
      return first({
        text: t("Packing files: {done} of {total}", { done: p.done.toLocaleString(), total: p.total.toLocaleString() }),
        percent: pct(p.done, p.total),
      });
    case "unpacking":
      return {
        text: t("Writing files: {done} of {total}", { done: p.done.toLocaleString(), total: p.total.toLocaleString() }),
        percent: pct(p.done, p.total),
      };
    case "loading":
      return { text: t("Loading the database…"), percent: null };
  }
}
