import { useEffect, useState } from "react";
import {
  api,
  errorText,
  type ImageFormat,
  type ImageOptions,
  type ImageResult,
  type ImagesFound,
} from "../lib/api";
import { useLoad } from "../lib/useLoad";

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

export function Images() {
  const sites = useLoad(api.listSites, []);
  const [paths, setPaths] = useState<string[]>([]);
  const [found, setFound] = useState<ImagesFound | null>(null);
  const [formats, setFormats] = useState<ImageFormat[]>(["webp", "avif"]);
  const [quality, setQuality] = useState(80);
  const [maxWidth, setMaxWidth] = useState(0);
  const [toFolder, setToFolder] = useState(false);
  const [outDir, setOutDir] = useState("");
  const [replace, setReplace] = useState(false);
  const [running, setRunning] = useState(false);
  const [progress, setProgress] = useState<{ done: number; total: number } | null>(null);
  const [results, setResults] = useState<ImageResult[]>([]);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let offs: (() => void)[] = [];
    let cancelled = false;
    Promise.all([
      api.onImagesProgress((p) => {
        setProgress({ done: p.done, total: p.total });
        setResults((r) => [...r, p.result]);
      }),
      api.onDrop((dropped) => setPaths((current) => [...new Set([...current, ...dropped])])),
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

  function toggle(format: ImageFormat) {
    setFormats((f) => (f.includes(format) ? f.filter((x) => x !== format) : [...f, format]));
  }

  async function chooseImages() {
    const picked = await api.pickImages();
    if (picked.length) setPaths((current) => [...new Set([...current, ...picked])]);
  }

  async function chooseFolder() {
    const folder = await api.pickFolder("Select a folder with images");
    if (folder) setPaths((current) => [...new Set([...current, folder])]);
  }

  async function chooseOut() {
    const folder = await api.pickFolder("Save the new images in", outDir);
    if (folder) setOutDir(folder);
  }

  async function run() {
    const options: ImageOptions = {
      formats,
      quality,
      max_width: maxWidth || null,
      out_dir: toFolder && outDir.trim() ? outDir.trim() : null,
      replace: !toFolder && replace && formats.includes("same"),
    };
    setRunning(true);
    setResults([]);
    setProgress({ done: 0, total: found?.count ?? 0 });
    setError(null);
    try {
      setResults(await api.optimizeImages(paths, options));
    } catch (e) {
      setError(errorText(e));
    } finally {
      setRunning(false);
    }
  }

  const made = results.flatMap((r) => r.made.filter((m) => !m.skipped).map((m) => ({ before: r.size, after: m.size })));
  const before = made.reduce((sum, m) => sum + m.before, 0);
  const after = made.reduce((sum, m) => sum + m.after, 0);
  const failed = results.filter((r) => r.error).length;
  const canRun = !running && (found?.count ?? 0) > 0 && formats.length > 0 && (!toFolder || outDir.trim() !== "");

  return (
    <>
      <header className="page-header">
        <div>
          <h1>Images</h1>
          <p className="muted">
            Make WebP and AVIF copies of JPG and PNG images, or smaller JPGs and PNGs. Your originals stay as they are.
          </p>
        </div>
      </header>

      {error && (
        <p className="error small" role="alert">
          {error}
        </p>
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
                  value=""
                  onChange={(e) => e.target.value && setPaths((c) => [...new Set([...c, e.target.value])])}
                >
                  <option value="">Pick a site…</option>
                  {sites.data.map((s) => (
                    <option key={s.domain} value={s.path}>
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
              <select id="i-width" value={maxWidth} onChange={(e) => setMaxWidth(Number(e.target.value))}>
                {WIDTHS.map((w) => (
                  <option key={w} value={w}>
                    {w ? `${w} px` : "Keep the size"}
                  </option>
                ))}
              </select>
            </div>
          </div>
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
              <div className="row folder-input">
                <input
                  aria-label="Folder for the new images"
                  className="mono grow"
                  value={outDir}
                  placeholder="Choose a folder"
                  onChange={(e) => setOutDir(e.target.value)}
                />
                <button type="button" className="btn" onClick={chooseOut}>
                  Browse…
                </button>
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
        <section aria-label="Results" className="card table-wrap">
          <p className="muted small results-summary">
            {before > 0 && `New files: ${size(after)} instead of ${size(before)} (${saved(before, after)}).`}
            {failed > 0 && ` ${failed} could not be read.`}
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
