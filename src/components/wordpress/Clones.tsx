import { useEffect, useRef, useState } from "react";
import { useT } from "../../lib/i18n";
import { cloneScreen, clonePercent, closeClone, pauseClone, retryClone, showClone, stopClone, useClones, type Clone } from "../../lib/clones";
import { stageText } from "./CloneSite";
import { size } from "./WordPressTools";
import type { WpCloneProgress } from "../../lib/wordpress";

/** A running clone's bar: the stage, what it is doing, and for files how much is downloaded. */
export function CloneProgress({ clone }: { clone: Clone }) {
  const t = useT();
  return <StepProgress progress={clone.progress} label={t("Cloning {domain}", { domain: `${clone.name}.stack` })} />;
}

/** A clone's or a Look for WordPress's bar, from its progress events. */
export function StepProgress({ progress: p, label }: { progress: WpCloneProgress | null; label: string }) {
  const t = useT();
  const percent = clonePercent(p);
  // Files and the database being loaded count bytes.
  const bytes = (p?.stage === "files" || p?.stage === "loading") && p.total > 0 ? `${size(p.done)} / ${size(p.total)}` : null;
  const message = p ? p.message : t("Starting…");
  // The amount sits at a fixed spot above the bar; the changing file name goes
  // on its own line below, cut to one line, so a long path never moves it.
  return (
    <div className="progress-line clone-progress">
      <div className="clone-progress-head small">
        <span>{p ? stageText(t, p.stage) : t("Starting…")}</span>
        {(bytes || percent !== null) && (
          <span className="mono clone-progress-amount">{[bytes, percent !== null && `${percent}%`].filter(Boolean).join(" · ")}</span>
        )}
      </div>
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
      {p && (
        <span className="muted small clone-progress-file" title={message}>
          {message}
        </span>
      )}
    </div>
  );
}

/** Every clone started in this window, with where each one is. */
export function ClonesList({ onShow, minimized = false }: { onShow: (id: string) => void; minimized?: boolean }) {
  const t = useT();
  const started = useClones().filter((c) => c.state !== "new" && !(minimized && c.open));
  if (!started.length) return <p className="muted small">{t("No clones running.")}</p>;
  return (
    <ul className="clone-list">
      {started.map((c) => (
        <li key={c.id} className="clone-item">
          <div className="row between centered">
            <div className="clone-name">
              <strong className="mono">{c.name}.stack</strong>
              {c.from && <span className="muted small mono"> · {c.from}</span>}
            </div>
            <div className="row">
              <button type="button" className="btn small" onClick={() => onShow(c.id)}>
                {t("Show")}
              </button>
              {c.state === "running" ? (
                <>
                  <button type="button" className="btn small" onClick={() => pauseClone(c.id)}>
                    {t("Pause")}
                  </button>
                  <button type="button" className="btn small" onClick={() => stopClone(c.id)}>
                    {t("Stop")}
                  </button>
                </>
              ) : c.state === "paused" ? (
                <button type="button" className="btn small primary" onClick={() => retryClone(c.id)}>
                  {t("Resume")}
                </button>
              ) : (
                <button type="button" className="btn small ghost" onClick={() => closeClone(c.id)}>
                  {t("Close")}
                </button>
              )}
            </div>
          </div>
          {c.state === "running" && <CloneProgress clone={c} />}
          {c.state === "paused" && <span className="small muted">{t("Paused")}</span>}
          {c.state === "done" && <span className="small ok">{t("Ready")}</span>}
          {c.state === "failed" && <span className="small error clone-progress-text">{c.error}</span>}
        </li>
      ))}
    </ul>
  );
}

/** The status bar's clones item: how many run, and a list of all of them over the bar. */
export function ClonesStatus({ onOpenSites }: { onOpenSites: (screen: "Sites" | "Import backup") => void }) {
  const t = useT();
  const clones = useClones().filter((c) => c.state !== "new");
  const [open, setOpen] = useState(false);
  const box = useRef<HTMLDivElement>(null);
  const running = clones.filter((c) => c.state === "running");
  const ready = clones.filter((c) => c.state === "done").length;
  const failed = clones.filter((c) => c.state === "failed").length;
  const paused = clones.filter((c) => c.state === "paused").length;

  useEffect(() => {
    if (!open) return;
    const away = (e: MouseEvent) => !box.current?.contains(e.target as Node) && setOpen(false);
    const esc = (e: KeyboardEvent) => e.key === "Escape" && setOpen(false);
    document.addEventListener("mousedown", away);
    document.addEventListener("keydown", esc);
    return () => {
      document.removeEventListener("mousedown", away);
      document.removeEventListener("keydown", esc);
    };
  }, [open]);
  useEffect(() => {
    if (!clones.length) setOpen(false);
  }, [clones.length]);
  if (!clones.length) return null;

  // One number for all running clones: the mean of the ones that can tell.
  const known = running.map((c) => clonePercent(c.progress)).filter((p): p is number => p !== null);
  const mean = known.length ? Math.floor(known.reduce((a, b) => a + b, 0) / known.length) : null;
  const words = [
    running.length && (running.length === 1 ? t("Cloning 1 site") : t("Cloning {count} sites", { count: running.length })),
    paused && (paused === 1 ? t("1 clone paused") : t("{count} clones paused", { count: paused })),
    ready && (ready === 1 ? t("1 clone ready") : t("{count} clones ready", { count: ready })),
    failed && (failed === 1 ? t("1 clone stopped") : t("{count} clones stopped", { count: failed })),
  ].filter(Boolean);

  return (
    <div className="status-pop" ref={box}>
      <button
        type="button"
        className={`status-item${ready && !running.length ? " highlight" : ""}`}
        aria-expanded={open}
        aria-haspopup="dialog"
        onClick={() => setOpen((o) => !o)}
      >
        {running.length > 0 && <span className="dot ok" aria-hidden="true" />}
        {words.join(" · ")}
        {mean !== null && <span className="mono">{mean}%</span>}
      </button>
      {open && (
        <div className="status-popover card stack" role="dialog" aria-label={t("Clones from live servers")}>
          <strong className="small">{t("Clones from live servers")}</strong>
          <ClonesList
            onShow={(id) => {
              showClone(id);
              setOpen(false);
              onOpenSites(cloneScreen(id));
            }}
          />
        </div>
      )}
    </div>
  );
}
