import { useEffect, useRef, useState } from "react";
import {
  api,
  errorText,
  type ImageFormat,
  type ImageOptions,
  type ImageResult,
  type ImagesFound,
} from "../lib/api";
import { useLoad } from "../lib/useLoad";
import { useT } from "../lib/i18n";
import { useBusy, useShown } from "../lib/tabLife";

const FORMATS: { id: ImageFormat; label: string; hint: string }[] = [
  { id: "webp", label: "WebP", hint: "Works in every browser" },
  { id: "avif", label: "AVIF", hint: "Smallest; slower to make" },
  { id: "same", label: "Smaller JPG/PNG", hint: "Same format, fewer bytes" },
];

const WIDTHS = [0, 3840, 2560, 1920, 1280, 800];

function size(bytes: number) {
  if (bytes >= 1_000_000) return `${(bytes / 1_000_000).toFixed(1)} MB`;
  if (bytes >= 1_000) return `${Math.round(bytes / 1_000)} KB`;
  return `${bytes} B`;
}

function saved(before: number, after: number) {
  if (!before) return "";
  const pct = Math.round(100 - (after * 100) / before);
  return pct >= 0 ? `−${pct}%` : `+${-pct}%`;
}

/** The last part of a path, for a compact list. */
function shortName(path: string) {
  const parts = path.split(/[\\/]/);
  return parts.slice(-2).join("/");
}

const IMAGE_FILE = /\.(jpe?g|png)$/i;

/** The folder a chosen path is in: the path itself for a folder, its parent for an image. */
function folderOf(path: string) {
  return IMAGE_FILE.test(path) ? path.replace(/[\\/][^\\/]*$/, "") : path;
}

type Finished = { ok: boolean; message: string | null };

export function Images() {
  const t = useT();
  const sites = useLoad(api.listSites, []);
  const sitesFolder = useLoad(api.sitesFolder, "");
  const [paths, setPaths] = useState<string[]>([]);
  const [found, setFound] = useState<ImagesFound | null>(null);
  const [formats, setFormats] = useState<ImageFormat[]>(["webp", "avif"]);
  const [quality, setQuality] = useState(80);
  const [maxWidth, setMaxWidth] = useState<number | "custom">(0);
  const [customWidth, setCustomWidth] = useState("");
  const [keepShape, setKeepShape] = useState(true);
  const [customHeight, setCustomHeight] = useState("");
  const [toFolder, setToFolder] = useState(false);
  const [outDir, setOutDir] = useState("");
  const [replace, setReplace] = useState(false);
  const [running, setRunning] = useState(false);
  useBusy(running);
  const shown = useShown();
  const shownNow = useRef(shown);
  shownNow.current = shown;
  const [progress, setProgress] = useState<{ done: number; total: number } | null>(null);
  const [results, setResults] = useState<ImageResult[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [finished, setFinished] = useState<Finished | null>(null);
  const banner = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let offs: (() => void)[] = [];
    let cancelled = false;
    Promise.all([
      api.onImagesProgress((p) => {
        setProgress({ done: p.done, total: p.total });
        setResults((r) => [...r, p.result]);
      }),
      // Files dropped on the window go to the screen that is shown.
      api.onDrop((dropped) => shownNow.current && setPaths((current) => [...new Set([...current, ...dropped])])),
    ]).then((list) => (cancelled ? list.forEach((off) => off()) : (offs = list)));
    return () => {
      cancelled = true;
      offs.forEach((off) => off());
    };
  }, []);

  useEffect(() => {
    if (paths.length === 0) {
      setFound(null);
      return;
    }
    let current = true;
    api
      .findImages(paths)
      .then((f) => current && setFound(f))
      .catch((e) => current && setError(errorText(e)));
    return () => {
      current = false;
    };
  }, [paths]);

  // The button is at the bottom of the form; bring the result into view.
  useEffect(() => {
    if (finished) banner.current?.scrollIntoView({ behavior: "smooth", block: "nearest" });
  }, [finished]);

  function toggle(format: ImageFormat) {
    setFormats((f) => (f.includes(format) ? f.filter((x) => x !== format) : [...f, format]));
  }

  async function chooseImages() {
    const picked = await api.pickImages(paths[0] ? folderOf(paths[0]) : sitesFolder.data);
    if (picked.length) setPaths((current) => [...new Set([...current, ...picked])]);
  }

  async function chooseFolder() {
    const folder = await api.pickFolder("Select a folder with images", paths[0] ? folderOf(paths[0]) : sitesFolder.data);
    if (folder) setPaths((current) => [...new Set([...current, folder])]);
  }

  async function chooseOut() {
    const folder = await api.pickFolder("Save the new images in", outDir || (paths[0] ? folderOf(paths[0]) : sitesFolder.data));
    if (folder) setOutDir(folder);
  }

  async function run() {
    const options: ImageOptions = {
      formats,
      quality,
      max_width: width,
      height,
      out_dir: toFolder && outDir.trim() ? outDir.trim() : null,
      replace: !toFolder && replace && formats.includes("same"),
    };
    setRunning(true);
    setResults([]);
    setProgress({ done: 0, total: found?.count ?? 0 });
    setError(null);
    setFinished(null);
    try {
      const done = await api.optimizeImages(paths, options);
      setResults(done);
      setFinished({ ok: done.every((r) => !r.error), message: null });
    } catch (e) {
      setFinished({ ok: false, message: errorText(e) });
    } finally {
      setRunning(false);
    }
  }

  const made = results.flatMap((r) => r.made.filter((m) => !m.skipped).map((m) => ({ before: r.size, after: m.size })));
  const before = made.reduce((sum, m) => sum + m.before, 0);
  const after = made.reduce((sum, m) => sum + m.after, 0);
  const failed = results.filter((r) => r.error);
  const skipped = results.reduce((n, r) => n + r.made.filter((m) => m.skipped).length, 0);
  const custom = maxWidth === "custom";
  const width = custom ? Number(customWidth) || null : maxWidth || null;
  const height = custom && !keepShape ? Number(customHeight) || null : null;
  const sizeReady = !custom || (width !== null && (keepShape || height !== null));
  const pickedSite = sites.data.find((s) => paths.includes(s.project.folder));
  const canRun =
    !running && (found?.count ?? 0) > 0 && formats.length > 0 && sizeReady && (!toFolder || outDir.trim() !== "");

  return (
    <>
      <header className="page-header">
        <div>
          <h1>{t("Images")}</h1>
          <p className="muted">
            {t("Make WebP and AVIF copies of JPG and PNG images, or smaller JPGs and PNGs. Your originals stay as they are.")}
          </p>
        </div>
      </header>

      {error && (
        <p className="error small" role="alert">
          {error}
        </p>
      )}

      {finished && !running && (
        <div ref={banner} className={`banner ${finished.ok ? "ok" : "error"}`} role={finished.ok ? "status" : "alert"}>
          <div className="row between centered">
            <strong>
              {finished.message
                ? "Optimizing stopped with an error"
                : finished.ok
                  ? `Done: ${results.length} image${results.length === 1 ? "" : "s"} optimized`
                  : `Done, but ${failed.length} of ${results.length} image${results.length === 1 ? "" : "s"} could not be optimized`}
            </strong>
            <button type="button" className="btn ghost small" onClick={() => setFinished(null)}>
              Dismiss
            </button>
          </div>
          {finished.message && <p className="small">{finished.message}</p>}
          {made.length > 0 ? (
            <p className="small">
              {made.length} new file{made.length === 1 ? "" : "s"}: {size(after)} instead of {size(before)}. You save{" "}
              <span className="strong">{size(Math.max(before - after, 0))}</span> ({saved(before, after)}).
            </p>
          ) : (
            !finished.message && <p className="small">No new files were made.</p>
          )}
          {skipped > 0 && (
            <p className="small">
              {skipped} file{skipped === 1 ? " was" : "s were"} skipped, usually because the result would not be
              smaller. The table below says why.
            </p>
          )}
          {failed.length > 0 && (
            <ul className="small banner-errors">
              {failed.slice(0, 5).map((r) => (
                <li key={r.source}>
                  <span className="mono">{shortName(r.source)}</span>: {r.error}
                </li>
              ))}
              {failed.length > 5 && <li>and {failed.length - 5} more, listed in the table below.</li>}
            </ul>
          )}
        </div>
      )}

      <div className="images-top">
        <section className="card stack" aria-labelledby="pick-h">
          <h2 id="pick-h">Images</h2>
          <div className="drop-zone">
            <p className="muted">Drop images or folders anywhere on this window, or</p>
            <div className="row">
              <button type="button" className="btn" onClick={chooseImages}>
                Choose images…
              </button>
              <button type="button" className="btn" onClick={chooseFolder}>
                Choose a folder…
              </button>
            </div>
            {sites.data.length > 0 && (
              <div className="field">
                <label htmlFor="i-site">Or a site's folder</label>
                <select
                  id="i-site"
                  value={pickedSite?.path ?? ""}
                  onChange={(e) => e.target.value && setPaths((c) => [...new Set([...c, e.target.value])])}
                >
                  <option value="">Pick a site…</option>
                  {sites.data.map((s) => (
                    <option key={s.domain} value={s.project.folder}>
                      {s.domain}
                    </option>
                  ))}
                </select>
              </div>
            )}
          </div>
          {paths.map((p) => (
            <div key={p} className="list-row">
              <span className="mono small">{p}</span>
              <button
                type="button"
                className="btn ghost small"
                aria-label={`Remove ${p}`}
                onClick={() => setPaths((c) => c.filter((x) => x !== p))}
              >
                Remove
              </button>
            </div>
          ))}
          {found && (
            <p className="muted small">
              {found.count === 0
                ? "No JPG or PNG images there. Folders like node_modules and vendor are skipped."
                : `${found.count} image${found.count === 1 ? "" : "s"}, ${size(found.size)} in all.`}
            </p>
          )}
        </section>

        <section className="card stack form" aria-labelledby="opt-h">
          <h2 id="opt-h">Make</h2>
          <div className="stack tight">
            {FORMATS.map((f) => (
              <label key={f.id} className="check">
                <input type="checkbox" checked={formats.includes(f.id)} onChange={() => toggle(f.id)} />
                {f.label} <span className="muted small">{f.hint}</span>
              </label>
            ))}
          </div>
          <div className="form-grid">
            <div className="field">
              <label htmlFor="i-quality">Quality: {quality}</label>
              <input
                id="i-quality"
                type="range"
                min={40}
                max={100}
                value={quality}
                onChange={(e) => setQuality(Number(e.target.value))}
              />
              <span className="muted small">80 looks the same as the original on most photos.</span>
            </div>
            <div className="field">
              <label htmlFor="i-width">Largest width</label>
              <select
                id="i-width"
                value={maxWidth}
                onChange={(e) => setMaxWidth(e.target.value === "custom" ? "custom" : Number(e.target.value))}
              >
                {WIDTHS.map((w) => (
                  <option key={w} value={w}>
                    {w ? `${w} px` : "Keep the size"}
                  </option>
                ))}
                <option value="custom">Custom width…</option>
              </select>
            </div>
          </div>
          {custom && (
            <div className="stack tight">
              <div className="form-grid">
                <div className="field">
                  <label htmlFor="i-custom-w">Width in pixels</label>
                  <input
                    id="i-custom-w"
                    className="mono"
                    inputMode="numeric"
                    value={customWidth}
                    placeholder="e.g. 1200"
                    title={t("Wider images are made this wide. Narrower ones keep their size unless you set a height.")}
                    onChange={(e) => setCustomWidth(e.target.value.replace(/\D/g, "").slice(0, 5))}
                  />
                </div>
                {!keepShape && (
                  <div className="field">
                    <label htmlFor="i-custom-h">Height in pixels</label>
                    <input
                      id="i-custom-h"
                      className="mono"
                      inputMode="numeric"
                      value={customHeight}
                      placeholder="e.g. 630"
                      title={t("Every image comes out exactly this high, cut from the middle so nothing is stretched.")}
                      onChange={(e) => setCustomHeight(e.target.value.replace(/\D/g, "").slice(0, 5))}
                    />
                  </div>
                )}
              </div>
              <label className="check">
                <input type="checkbox" checked={keepShape} onChange={(e) => setKeepShape(e.target.checked)} />
                Keep the aspect ratio (the height follows the width)
              </label>
              <span className="muted small">
                {keepShape
                  ? "Images wider than this are scaled down; the height of each one is worked out for you."
                  : "Every image comes out exactly this size. Edges are cut off evenly so nothing looks stretched."}
              </span>
            </div>
          )}
          <div className="stack tight">
            <label className="check">
              <input type="radio" name="i-out" checked={!toFolder} onChange={() => setToFolder(false)} />
              Next to the originals (photo.webp, photo.avif, photo.min.jpg)
            </label>
            <label className="check">
              <input type="radio" name="i-out" checked={toFolder} onChange={() => setToFolder(true)} />
              In another folder, keeping the folder layout
            </label>
            {toFolder && (
              <div className="field">
                <label htmlFor="i-out-dir">Folder for the new images</label>
                <div className="row folder-input">
                  <input
                    id="i-out-dir"
                    className="mono grow"
                    value={outDir}
                    placeholder="Choose a folder"
                    onChange={(e) => setOutDir(e.target.value)}
                  />
                  <button type="button" className="btn" onClick={chooseOut}>
                    Browse…
                  </button>
                </div>
              </div>
            )}
            {!toFolder && formats.includes("same") && (
              <label className="check">
                <input type="checkbox" checked={replace} onChange={(e) => setReplace(e.target.checked)} />
                Replace the original JPGs and PNGs with the smaller ones
              </label>
            )}
          </div>
          <div className="row between centered">
            {progress && running ? (
              <div className="stack tight grow">
                <span className="muted small">
                  {progress.done} of {progress.total} done
                </span>
                <div className="progress" aria-hidden="true">
                  <div
                    className="progress-fill"
                    style={{ width: `${progress.total ? (progress.done * 100) / progress.total : 0}%` }}
                  />
                </div>
              </div>
            ) : (
              <span />
            )}
            <button type="button" className="btn primary" disabled={!canRun} onClick={run}>
              {running ? "Optimizing…" : `Optimize ${found?.count ? `${found.count} ` : ""}images`}
            </button>
          </div>
        </section>
      </div>

      {results.length > 0 && (
        <section aria-label={t("Results")} className="card table-wrap">
          <p className="muted small results-summary">
            {before > 0 && `New files: ${size(after)} instead of ${size(before)} (${saved(before, after)}).`}
            {failed.length > 0 && ` ${failed.length} could not be read.`}
          </p>
          <table>
            <thead>
              <tr>
                <th>Image</th>
                <th>Original</th>
                {formats.map((f) => (
                  <th key={f}>{FORMATS.find((x) => x.id === f)?.label}</th>
                ))}
              </tr>
            </thead>
            <tbody>
              {results.map((r) => (
                <tr key={r.source}>
                  <td className="mono small" title={r.source}>
                    {shortName(r.source)}
                    {r.error && <div className="warn small">{r.error}</div>}
                  </td>
                  <td className="small">{size(r.size)}</td>
                  {formats.map((f) => {
                    const m = r.made.find((x) => x.format === f);
                    return (
                      <td key={f} className="small" title={m?.path}>
                        {!m ? "—" : m.skipped ? <span className="muted">{m.skipped}</span> : (
                          <>
                            {size(m.size)} <span className="muted">{saved(r.size, m.size)}</span>
                          </>
                        )}
                      </td>
                    );
                  })}
                </tr>
              ))}
            </tbody>
          </table>
        </section>
      )}
    </>
  );
}
