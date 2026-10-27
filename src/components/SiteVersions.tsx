import { useState } from "react";
import { api, errorText, type RuntimeFamily, type Site, type SiteVersion } from "../lib/api";
import { useBusy } from "../lib/tabLife";

const NAMES: Record<SiteVersion["kind"], string> = {
  php: "PHP",
  node: "Node.js",
  python: "Python",
  go: "Go",
  java: "Java",
};

/** Where the version comes from, in a few words. */
function source(v: SiteVersion) {
  switch (v.from) {
    case "pinned":
      return `from ${v.file ?? ".stackyard"}`;
    case "site":
      return null;
    default:
      return "default";
  }
}

/**
 * A version picker per runtime the site's project uses. Picking a version
 * that is not installed installs it first, then switches the site to it.
 */
export function SiteVersions({
  site,
  families,
  onChanged,
}: {
  site: Site;
  families: RuntimeFamily[];
  /** Called after a version is installed or switched, to reload the lists. */
  onChanged: () => void;
}) {
  const [busy, setBusy] = useState<string | null>(null);
  useBusy(busy !== null);
  const [error, setError] = useState<string | null>(null);

  async function pick(v: SiteVersion, value: string) {
    const name = NAMES[v.kind];
    const version = value === "" ? null : value;
    const family = families.find((f) => f.kind === v.kind);
    const missing = version !== null && !family?.versions.find((x) => x.version === version)?.installed;
    setError(null);
    try {
      if (missing) {
        setBusy(`Installing ${name} ${version}…`);
        await api.installRuntime(v.kind, version);
        onChanged();
      }
      setBusy(`Switching to ${name} ${version ?? "default"}…`);
      await api.setSiteVersion(site.domain, v.kind, version);
      onChanged();
    } catch (e) {
      setError(errorText(e));
      onChanged();
    } finally {
      setBusy(null);
    }
  }

  if (site.versions.length === 0) return null;
  return (
    <div className="site-versions">
      {site.versions.map((v) => {
        const family = families.find((f) => f.kind === v.kind);
        const offered = family?.versions ?? [];
        const isDefault = v.from === "default" || v.from === "newest";
        const id = `ver-${site.domain}-${v.kind}`;
        const hint = source(v);
        return (
          <label key={v.kind} className="site-version" htmlFor={id}>
            <span className="muted small">{NAMES[v.kind]}</span>
            <select
              id={id}
              value={isDefault && v.can_unset ? "" : v.version}
              disabled={busy !== null}
              title={hint ? `${NAMES[v.kind]} ${v.version}, ${hint}` : undefined}
              onChange={(e) => pick(v, e.target.value)}
            >
              {v.can_unset && <option value="">Default ({v.version})</option>}
              {offered.map((o) => (
                <option key={o.version} value={o.version}>
                  {o.version}
                  {o.installed ? "" : " (install)"}
                </option>
              ))}
              {!offered.some((o) => o.version === v.version) && <option value={v.version}>{v.version}</option>}
            </select>
            {v.from === "pinned" && <span className="muted small">{hint}</span>}
          </label>
        );
      })}
      {busy && (
        <span className="muted small" role="status">
          {busy}
        </span>
      )}
      {error && (
        <span className="error small" role="alert">
          {error}
        </span>
      )}
    </div>
  );
}
