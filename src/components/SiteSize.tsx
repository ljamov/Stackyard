import { useEffect, useState } from "react";
import { api, errorText, type Site, type SiteSize as Size } from "../lib/api";
import { useT } from "../lib/i18n";
import { useBusy } from "../lib/tabLife";

function size(bytes: number) {
  if (bytes < 1024 ** 2) return `${Math.max(1, Math.round(bytes / 1024))} KB`;
  if (bytes < 1024 ** 3) return `${(bytes / 1024 ** 2).toFixed(1)} MB`;
  return `${(bytes / 1024 ** 3).toFixed(2)} GB`;
}

/** A measurement older than this is taken again when the site's tab opens. */
const STALE_SECS = 60 * 60;

/**
 * How much room the site takes: total, database and files (for WordPress by
 * part). The last measurement shows at once; a missing or old one is taken
 * again in the background, since walking a big folder takes a while.
 */
export function SiteSizeFacts({ site }: { site: Site }) {
  const t = useT();
  const [sizes, setSizes] = useState<Size | null>(null);
  const [busy, setBusy] = useState(false);
  useBusy(busy);
  const [error, setError] = useState<string | null>(null);

  async function measure() {
    setBusy(true);
    setError(null);
    try {
      setSizes(await api.measureSiteSize(site.domain));
    } catch (e) {
      setError(errorText(e));
    }
    setBusy(false);
  }

  useEffect(() => {
    let gone = false;
    setSizes(null);
    api
      .siteSize(site.domain)
      .catch(() => null)
      .then((cached) => {
        if (gone) return;
        setSizes(cached);
        if (!cached || Date.now() / 1000 - cached.measured > STALE_SECS) void measure();
      });
    return () => {
      gone = true;
    };
  }, [site.domain]);

  const f = sizes?.files;
  const n = (count: number) => count.toLocaleString();
  const parts: [string, string][] = [];
  if (sizes && f) {
    if (sizes.database) parts.push([t("Database"), sizes.database_bytes === null ? t("unknown (start its server)") : size(sizes.database_bytes)]);
    if (sizes.wordpress) {
      parts.push([t("WordPress core"), size(f.core.bytes)]);
      parts.push([t("Themes"), `${size(f.themes.bytes)} (${t("{count} themes", { count: n(f.themes.items) })})`]);
      parts.push([t("Plugins"), `${size(f.plugins.bytes)} (${t("{count} plugins", { count: n(f.plugins.items) })})`]);
      parts.push([t("Media (uploads)"), size(f.uploads.bytes)]);
      if (f.other.files > 0) parts.push([t("Other files"), size(f.other.bytes)]);
      if (f.skipped.files > 0) parts.push([t("Caches and backups"), size(f.skipped.bytes)]);
    } else {
      parts.push([t("Files"), `${size(f.other.bytes)} (${t("{count} files", { count: n(f.other.files) })})`]);
    }
  }
  const files = f ? [f.core, f.themes, f.plugins, f.uploads, f.other, f.skipped].reduce((sum, p) => sum + p.files, 0) : 0;

  return (
    <>
      <dt>{t("Size")}</dt>
      <dd className="stack site-size">
        <span className="row wrap centered">
          {sizes ? (
            <strong>
              {size(sizes.total)}
              <span className="muted small"> · {t("{count} files", { count: n(files) })}</span>
            </strong>
          ) : (
            <span className="muted">{busy ? t("Measuring…") : t("Not measured yet")}</span>
          )}
          <button type="button" className="link small" onClick={() => void measure()} disabled={busy}>
            {busy && sizes ? t("Measuring…") : t("Measure again")}
          </button>
        </span>
        {parts.length > 0 && (
          <span className="small site-size-parts">
            {parts.map(([label, value]) => (
              <span key={label}>
                <span className="muted">{label}</span> {value}
              </span>
            ))}
          </span>
        )}
        {error && (
          <span className="error small" role="alert">
            {error}
          </span>
        )}
      </dd>
    </>
  );
}
