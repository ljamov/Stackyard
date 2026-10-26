import { useEffect, useRef, useState } from "react";
import { useT } from "../../lib/i18n";
import { clonePercent, closeClone, showClone, stopClone, useClones, type Clone } from "../../lib/clones";
import { stageText } from "./CloneSite";
import { size } from "./WordPressTools";

/** A running clone's bar: the stage, what it is doing, and for files how much is downloaded. */
export function CloneProgress({ clone }: { clone: Clone }) {
  const t = useT();
  const p = clone.progress;
  const percent = clonePercent(p);
  const bytes = p?.stage === "files" && p.total > 0 ? `${size(p.done)} / ${size(p.total)}` : null;
  return (
    <div className="progress-line">
      <div
        className="progress"
        role="progressbar"
        aria-label={t("Cloning {domain}", { domain: `${clone.name}.stack` })}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={percent ?? undefined}
      >
        <div className={percent === null ? "progress-fill busy" : "progress-fill"} style={{ width: `${percent ?? 30}%` }} />
      </div>
      <span className="muted small clone-progress-text">
        {p ? `${stageText(t, p.stage)}: ${p.message}` : t("Starting…")}
        {bytes && <span className="mono"> · {bytes}</span>}
        {percent !== null && <span className="mono"> · {percent}%</span>}
      </span>
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
                <button type="button" className="btn small" onClick={() => stopClone(c.id)}>
                  {t("Stop")}
                </button>
              ) : (
                <button type="button" className="btn small ghost" onClick={() => closeClone(c.id)}>
                  {t("Close")}
                </button>
              )}
            </div>
          </div>
          {c.state === "running" && <CloneProgress clone={c} />}
          {c.state === "done" && <span className="small ok">{t("Ready")}</span>}
          {c.state === "failed" && <span className="small error clone-progress-text">{c.error}</span>}
        </li>
      ))}
    </ul>
  );
}

/** The status bar's clones item: how many run, and a list of all of them over the bar. */
export function ClonesStatus({ onOpenSites }: { onOpenSites: () => void }) {
  const t = useT();
  const clones = useClones().filter((c) => c.state !== "new");
  const [open, setOpen] = useState(false);
  const box = useRef<HTMLDivElement>(null);
  const running = clones.filter((c) => c.state === "running");
  const ready = clones.filter((c) => c.state === "done").length;
  const failed = clones.filter((c) => c.state === "failed").length;

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
              onOpenSites();
            }}
          />
        </div>
      )}
    </div>
  );
}
