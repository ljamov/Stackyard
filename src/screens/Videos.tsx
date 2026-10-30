import { useEffect, useRef, useState } from "react";
import {
  api,
  errorText,
  type FfmpegProgress,
  type FfmpegStatus,
  type ImagesFound,
  type VideoFormat,
  type VideoOptions,
  type VideoQuality,
  type VideoResult,
  type VideosProgress,
} from "../lib/api";
import { useLoad } from "../lib/useLoad";
import { useT } from "../lib/i18n";
import { useBusy, useShown } from "../lib/tabLife";

const FORMATS: { id: VideoFormat; label: string; hint: string }[] = [
  { id: "mp4", label: "MP4", hint: "H.264, plays everywhere" },
  { id: "webm", label: "WebM", hint: "VP9, smaller; every current browser" },
  { id: "gif", label: "GIF", hint: "No sound; much larger than video" },
];

const QUALITIES: { id: VideoQuality; label: string }[] = [
  { id: "small", label: "Smallest files (backgrounds, previews)" },
  { id: "balanced", label: "Balanced" },
  { id: "high", label: "Looks like the original" },
];

const WIDTHS = [0, 1920, 1280, 960, 720, 480];

function size(bytes: number) {
  if (bytes >= 1_000_000_000) return `${(bytes / 1_000_000_000).toFixed(2)} GB`;
  if (bytes >= 1_000_000) return `${(bytes / 1_000_000).toFixed(1)} MB`;
  if (bytes >= 1_000) return `${Math.round(bytes / 1_000)} KB`;
  return `${bytes} B`;
}

function saved(before: number, after: number) {
  if (!before) return "";
  const pct = Math.round(100 - (after * 100) / before);
  return pct >= 0 ? `−${pct}%` : `+${-pct}%`;
}

function shortName(path: string) {
  return path.split(/[\\/]/).slice(-2).join("/");
}

const VIDEO_FILE = /\.(mp4|m4v|mov|webm|mkv|avi|gif)$/i;

function folderOf(path: string) {
  return VIDEO_FILE.test(path) ? path.replace(/[\\/][^\\/]*$/, "") : path;
}

type Finished = { ok: boolean; stopped: boolean; message: string | null };

export function Videos() {
  const t = useT();
  const sites = useLoad(api.listSites, []);
  const sitesFolder = useLoad(api.sitesFolder, "");
  const [ffmpeg, setFfmpeg] = useState<FfmpegStatus | null>(null);
  const [download, setDownload] = useState<FfmpegProgress | null>(null);
  const [paths, setPaths] = useState<string[]>([]);
  const [found, setFound] = useState<ImagesFound | null>(null);
  const [formats, setFormats] = useState<VideoFormat[]>(["mp4", "webm"]);
  const [quality, setQuality] = useState<VideoQuality>("balanced");
  const [maxWidth, setMaxWidth] = useState<number | "custom">(0);
  const [customWidth, setCustomWidth] = useState("");
  const [mute, setMute] = useState(false);
  const [toFolder, setToFolder] = useState(false);
  const [outDir, setOutDir] = useState("");
  const [running, setRunning] = useState(false);
  useBusy(running);
  const shown = useShown();
  const shownNow = useRef(shown);
  shownNow.current = shown;
  const [progress, setProgress] = useState<VideosProgress | null>(null);
  const [results, setResults] = useState<VideoResult[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [finished, setFinished] = useState<Finished | null>(null);
  const banner = useRef<HTMLDivElement>(null);

  useEffect(() => {
    api
      .ffmpegStatus()
      .then(setFfmpeg)
      .catch((e) => setError(errorText(e)));
  }, []);

  useEffect(() => {
    let offs: (() => void)[] = [];
    let cancelled = false;
    Promise.all([
      api.onVideosProgress((p) => {
        setProgress(p);
        const done = p.result;
        if (done) setResults((r) => [...r, done]);
      }),
      api.onFfmpegProgress(setDownload),
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
      .findVideos(paths)
      .then((f) => current && setFound(f))
      .catch((e) => current && setError(errorText(e)));
    return () => {
      current = false;
    };
  }, [paths]);

  useEffect(() => {
    if (finished) banner.current?.scrollIntoView({ behavior: "smooth", block: "nearest" });
  }, [finished]);

  async function getFfmpeg() {
    setError(null);
    setDownload({ step: "resolving" });
    try {
      setFfmpeg(await api.installFfmpeg());
    } catch (e) {
      setError(errorText(e));
    } finally {
      setDownload(null);
    }
  }

  async function removeFfmpeg() {
    try {
      setFfmpeg(await api.removeFfmpeg());
    } catch (e) {
      setError(errorText(e));
    }
  }

  function toggle(format: VideoFormat) {
    setFormats((f) => (f.includes(format) ? f.filter((x) => x !== format) : [...f, format]));
  }

  const start = () => (paths[0] ? folderOf(paths[0]) : sitesFolder.data);

  async function chooseVideos() {
    const picked = await api.pickVideos(start());
    if (picked.length) setPaths((current) => [...new Set([...current, ...picked])]);
  }

  async function chooseFolder() {
    const folder = await api.pickFolder("Select a folder with videos", start());
    if (folder) setPaths((current) => [...new Set([...current, folder])]);
  }

  async function chooseOut() {
    const folder = await api.pickFolder("Save the new videos in", outDir || start());
    if (folder) setOutDir(folder);
  }

  const width = maxWidth === "custom" ? Number(customWidth) || null : maxWidth || null;

  async function run() {
    const options: VideoOptions = {
      formats,
      quality,
      max_width: width,
      mute,
      out_dir: toFolder && outDir.trim() ? outDir.trim() : null,
    };
    setRunning(true);
    setResults([]);
    setProgress(null);
    setError(null);
    setFinished(null);
    try {
      const done = await api.optimizeVideos(paths, options);
      setResults(done);
      const stopped = done.length < (found?.count ?? 0) || done.some((r) => r.error === "stopped");
      setFinished({ ok: done.every((r) => !r.error), stopped, message: null });
    } catch (e) {
      setFinished({ ok: false, stopped: false, message: errorText(e) });
    } finally {
      setRunning(false);
    }
  }

  const made = results.flatMap((r) => r.made.filter((m) => !m.skipped).map((m) => ({ before: r.size, after: m.size })));
  const after = made.reduce((sum, m) => sum + m.after, 0);
  const failed = results.filter((r) => r.error && r.error !== "stopped");
  const pickedSite = sites.data.find((s) => paths.includes(s.project.folder));
  const ready = Boolean(ffmpeg?.path);
  const canRun =
    ready &&
    !running &&
    (found?.count ?? 0) > 0 &&
    formats.length > 0 &&
    (maxWidth !== "custom" || width !== null) &&
    (!toFolder || outDir.trim() !== "");
  const downloadPct =
    download?.step === "downloading" && download.total ? Math.round(((download.done ?? 0) * 100) / download.total) : null;
  // Each video counts once; within it, each format gets an equal share.
  const step = progress?.format ? formats.indexOf(progress.format) + (progress.percent ?? 0) / 100 : 0;
  const overall =
    progress && progress.total
      ? Math.min(((progress.done + (progress.current ? step / Math.max(formats.length, 1) : 0)) * 100) / progress.total, 100)
      : 0;

  return (
    <>
      <header className="page-header">
        <div>
          <h1>{t("Videos")}</h1>
          <p className="muted">
            {t(
              "Make web-ready MP4, WebM and GIF copies of videos and GIFs, smaller and optionally narrower or without sound. Your originals stay as they are.",
            )}
          </p>
        </div>
      </header>

      {error && (
        <p className="error small" role="alert">
          {error}
        </p>
      )}

      {ffmpeg && !ready && (
        <section className="card stack" aria-labelledby="ff-h">
          <div className="row between wrap centered">
            <h2 id="ff-h">Get FFmpeg first</h2>
            <button type="button" className="btn primary" disabled={download !== null} onClick={getFfmpeg}>
              {download
                ? download.step === "unpacking"
                  ? "Unpacking…"
                  : downloadPct !== null
                    ? `Downloading ${downloadPct}%`
                    : "Finding the latest…"
                : "Download FFmpeg"}
            </button>
          </div>
          <p className="muted small">
            Videos are made with FFmpeg, the free video tool most sites and apps use. It is not part of Stackyard, to
            keep Stackyard small: the download is about 150 MB and takes about 180 MB of disk in Stackyard's tools
            folder. If FFmpeg is already installed on this computer, Stackyard uses that one instead.
          </p>
        </section>
      )}

      {finished && !running && (
        <div ref={banner} className={`banner ${finished.ok ? "ok" : "error"}`} role={finished.ok ? "status" : "alert"}>
          <div className="row between centered">
            <strong>
              {finished.message
                ? "Optimizing stopped with an error"
                : finished.stopped
                  ? "Stopped"
                  : finished.ok
                    ? `Done: ${results.length} video${results.length === 1 ? "" : "s"} optimized`
                    : `Done, but ${failed.length} of ${results.length} could not be optimized`}
            </strong>
            <button type="button" className="btn ghost small" onClick={() => setFinished(null)}>
              Dismiss
            </button>
          </div>
          {finished.message && <p className="small">{finished.message}</p>}
          {made.length > 0 && (
            <p className="small">
              {made.length} new file{made.length === 1 ? "" : "s"}, {size(after)} in all. The table below compares
              each one with its original.
            </p>
          )}
          {failed.length > 0 && (
            <ul className="small banner-errors">
              {failed.slice(0, 5).map((r) => (
                <li key={r.source}>
                  <span className="mono">{shortName(r.source)}</span>: {r.error}
                </li>
              ))}
            </ul>
          )}
        </div>
      )}

      <div className="images-top">
        <section className="card stack" aria-labelledby="vpick-h">
          <h2 id="vpick-h">Videos</h2>
          <div className="drop-zone">
            <p className="muted">Drop videos or folders anywhere on this window, or</p>
            <div className="row">
              <button type="button" className="btn" onClick={chooseVideos}>
                Choose videos…
              </button>
              <button type="button" className="btn" onClick={chooseFolder}>
                Choose a folder…
              </button>
            </div>
            {sites.data.length > 0 && (
              <div className="field">
                <label htmlFor="v-site">Or a site's folder</label>
                <select
                  id="v-site"
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
                ? "No videos there (MP4, MOV, WebM, MKV, AVI or GIF). Folders like node_modules and vendor are skipped."
                : `${found.count} video${found.count === 1 ? "" : "s"}, ${size(found.size)} in all.`}
            </p>
          )}
          {ffmpeg?.path && (
            <p className="muted small">
              Using {ffmpeg.version ?? "FFmpeg"}
              {ffmpeg.downloaded ? " (downloaded by Stackyard). " : " from this computer. "}
              {ffmpeg.downloaded && (
                <button type="button" className="link small" disabled={running} onClick={removeFfmpeg}>
                  Remove it
                </button>
              )}
            </p>
          )}
        </section>

        <section className="card stack form" aria-labelledby="vopt-h">
          <h2 id="vopt-h">Make</h2>
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
              <label htmlFor="v-quality">Quality</label>
              <select id="v-quality" value={quality} onChange={(e) => setQuality(e.target.value as VideoQuality)}>
                {QUALITIES.map((q) => (
                  <option key={q.id} value={q.id}>
                    {q.label}
                  </option>
                ))}
              </select>
            </div>
            <div className="field">
              <label htmlFor="v-width">Largest width</label>
              <select
                id="v-width"
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
            {maxWidth === "custom" && (
              <div className="field">
                <label htmlFor="v-custom-w">Width in pixels</label>
                <input
                  id="v-custom-w"
                  className="mono"
                  inputMode="numeric"
                  value={customWidth}
                  placeholder="e.g. 1600"
                  title={t("Wider videos are made this wide; the height follows, so nothing is stretched.")}
                  onChange={(e) => setCustomWidth(e.target.value.replace(/\D/g, "").slice(0, 4))}
                />
              </div>
            )}
          </div>
          <span className="muted small">
            The height always follows the width. GIFs are made at most 480 px wide unless you pick a width.
          </span>
          <label className="check">
            <input type="checkbox" checked={mute} onChange={(e) => setMute(e.target.checked)} />
            Remove the sound (for background videos)
          </label>
          <div className="stack tight">
            <label className="check">
              <input type="radio" name="v-out" checked={!toFolder} onChange={() => setToFolder(false)} />
              Next to the originals (clip.webm, clip.gif, clip.min.mp4)
            </label>
            <label className="check">
              <input type="radio" name="v-out" checked={toFolder} onChange={() => setToFolder(true)} />
              In another folder, keeping the folder layout
            </label>
            {toFolder && (
              <div className="field">
                <label htmlFor="v-out-dir">Folder for the new videos</label>
                <div className="row folder-input">
                  <input
                    id="v-out-dir"
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
          </div>
          <div className="row between centered">
            {running ? (
              <div className="stack tight grow">
                <span className="muted small">
                  {progress?.current
                    ? `${shortName(progress.current)} → ${progress.format?.toUpperCase()}${
                        progress.percent !== null ? ` ${Math.round(progress.percent)}%` : ""
                      }`
                    : "Starting…"}
                  {progress && ` · ${progress.done} of ${progress.total} done`}
                </span>
                <div className="progress" aria-hidden="true">
                  <div className="progress-fill" style={{ width: `${overall}%` }} />
                </div>
              </div>
            ) : (
              <span />
            )}
            {running ? (
              <button type="button" className="btn" onClick={() => api.stopVideos()}>
                Stop
              </button>
            ) : (
              <button type="button" className="btn primary" disabled={!canRun} onClick={run}>
                {`Optimize ${found?.count ? `${found.count} ` : ""}video${found?.count === 1 ? "" : "s"}`}
              </button>
            )}
          </div>
        </section>
      </div>

      {results.length > 0 && (
        <section aria-label={t("Results")} className="card table-wrap">
          <table>
            <thead>
              <tr>
                <th>Video</th>
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
