import { useEffect, useState, type FormEvent } from "react";
import { api, errorText, type Site, type WpBuilt, type WpPackage } from "../lib/api";
import { useT } from "../lib/i18n";
import { useBusy } from "../lib/tabLife";

function size(bytes: number) {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

/**
 * A WordPress theme's or plugin's own tools: its version, as its header
 * says, and the .zip WordPress installs it from.
 */
export function WpPackageTab({ site }: { site: Site }) {
  const t = useT();
  const theme = site.project.kind === "wordpress-theme";
  const [pkg, setPkg] = useState<WpPackage | null>(null);
  const [version, setVersion] = useState("");
  const [busy, setBusy] = useState<"version" | "zip" | "old" | null>(null);
  useBusy(busy !== null);
  const [built, setBuilt] = useState<WpBuilt | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  function show(p: WpPackage) {
    setPkg(p);
    setVersion(p.version ?? "");
  }

  useEffect(() => {
    setBuilt(null);
    setNote(null);
    api
      .wpPackage(site.domain)
      .then((p) => {
        show(p);
        setError(null);
      })
      .catch((e) => setError(errorText(e)));
  }, [site.domain, site.project.folder]);

  async function run(what: "version" | "zip" | "old", work: () => Promise<void>) {
    setBusy(what);
    setNote(null);
    try {
      await work();
      setError(null);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(null);
    }
  }

  function saveVersion(e: FormEvent) {
    e.preventDefault();
    void run("version", async () => {
      show(await api.wpPackageSetVersion(site.domain, version));
      setNote(t("Version set to {version}.", { version: version.trim() }));
    });
  }

  const header = pkg?.main_file ?? (theme ? "style.css" : t("the plugin's main .php file"));
  return (
    <div className="stack">
      <div className="stack">
        <h3>{theme ? t("Theme") : t("Plugin")}</h3>
        <p className="muted small">
          {theme
            ? t("Built in its own folder. Set its version and build the .zip you upload in WordPress under Appearance › Themes › Add New.")
            : t("Built in its own folder. Set its version and build the .zip you upload in WordPress under Plugins › Add New.")}
        </p>
      </div>
      {pkg && !pkg.main_file && (
        <p className="warn small">
          {theme
            ? t("There is no style.css with a Theme Name header in the folder yet; WordPress needs one to install a theme.")
            : t("There is no .php file with a Plugin Name header in the folder yet; WordPress needs one to install a plugin.")}
        </p>
      )}
      <form className="stack form" onSubmit={saveVersion} aria-label={t("Version")}>
        <div className="form-grid">
          <div className="field">
            <label htmlFor={`wp-pkg-version-${site.domain}`}>{t("Version")}</label>
            <div className="row">
              <input
                id={`wp-pkg-version-${site.domain}`}
                className="mono grow"
                value={version}
                placeholder="1.0.0"
                disabled={!pkg?.main_file || busy !== null}
                onChange={(e) => setVersion(e.target.value)}
              />
              <button
                type="submit"
                className="btn"
                disabled={!pkg?.main_file || busy !== null || !version.trim() || version.trim() === (pkg?.version ?? "")}
              >
                {busy === "version" ? t("Saving…") : t("Set version")}
              </button>
            </div>
            <span className="muted small">
              {t("Written to the Version line in {file}, and to Stable tag in readme.txt when there is one.", { file: header })}
            </span>
          </div>
          <div className="field">
            <span className="label">{t("Package")}</span>
            <div className="row">
              <button
                type="button"
                className="btn primary"
                disabled={!pkg?.main_file || busy !== null}
                onClick={() =>
                  void run("zip", async () => {
                    setBuilt(await api.wpPackageBuild(site.domain));
                  })
                }
              >
                {busy === "zip" ? t("Building…") : t("Build .zip")}
              </button>
              <button
                type="button"
                className="btn"
                onClick={() => void run("zip", () => api.openWpBuilds(site.domain))}
                disabled={busy !== null}
              >
                {t("Open folder")}
              </button>
            </div>
            {pkg && (
              <span className="muted small">
                {t("Saved in {folder}.", { folder: pkg.builds })}
              </span>
            )}
            <span className="muted small">
              {t("Git, node_modules, editor settings, other zips and what .distignore lists are left out.")}
            </span>
          </div>
        </div>
      </form>
      {built && (
        <div className="row">
          <p className="small grow">
            {t("Built {file}: {count} files, {size}.", { file: built.path, count: String(built.files), size: size(built.bytes) })}
          </p>
          <button type="button" className="btn" onClick={() => void run("zip", () => api.openWpBuilds(site.domain))} disabled={busy !== null}>
            {t("Open folder")}
          </button>
        </div>
      )}
      {pkg?.old_wordpress && (
        <div className="stack">
          <p className="muted small">
            {t("Stackyard made a whole WordPress for this {what} before, at {path}. It is not used any more and can go; the {what}'s own folder is not touched.", {
              what: theme ? t("theme") : t("plugin"),
              path: pkg.old_wordpress,
            })}
          </p>
          <div className="row">
            <button
              type="button"
              className="btn"
              disabled={busy !== null}
              onClick={() =>
                void run("old", async () => {
                  show(await api.wpRemoveOldWordPress(site.domain));
                  setNote(t("The old WordPress is removed. Its database, if you no longer need it, can be dropped on the Databases screen."));
                })
              }
            >
              {busy === "old" ? t("Removing…") : t("Remove old WordPress")}
            </button>
          </div>
        </div>
      )}
      {note && <p className="muted small">{note}</p>}
      {error && <p className="error small">{error}</p>}
    </div>
  );
}
