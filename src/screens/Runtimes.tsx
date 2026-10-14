import { useEffect, useState } from "react";
import { api, errorText, type InstallProgress, type RuntimeFamily, type RuntimeVersion } from "../lib/api";
import { useLoad } from "../lib/useLoad";
import { useModules } from "../lib/modules";

const key = (kind: string, version: string) => `${kind}/${version}`;

export function Runtimes() {
  const runtimes = useLoad(api.listRuntimes, []);
  const { isOn } = useModules();
  const [data, setData] = useState<RuntimeFamily[]>([]);
  const [selected, setSelected] = useState("php");
  const [progress, setProgress] = useState<Record<string, InstallProgress>>({});
  const [errors, setErrors] = useState<Record<string, string>>({});

  useEffect(() => setData(runtimes.data), [runtimes.data]);

  useEffect(() => {
    let unsubscribe: (() => void) | undefined;
    let cancelled = false;
    api
      .onInstallProgress((p) => setProgress((all) => ({ ...all, [key(p.kind, p.version)]: p })))
      .then((off) => (cancelled ? off() : (unsubscribe = off)));
    return () => {
      cancelled = true;
      unsubscribe?.();
    };
  }, []);

  const shown = data.filter((r) => isOn(r.kind));
  const family = shown.find((r) => r.kind === selected) ?? shown[0];

  async function run(version: string, action: () => Promise<RuntimeFamily[]>) {
    if (!family) return;
    const k = key(family.kind, version);
    setErrors(({ [k]: _, ...rest }) => rest);
    try {
      setData(await action());
    } catch (e) {
      setErrors((all) => ({ ...all, [k]: errorText(e) }));
      runtimes.reload();
    } finally {
      setProgress(({ [k]: _, ...rest }) => rest);
    }
  }

  function install(version: string) {
    if (!family) return;
    const kind = family.kind;
    setData((all) =>
      all.map((f) =>
        f.kind !== kind
          ? f
          : { ...f, versions: f.versions.map((v) => (v.version === version ? { ...v, installing: true } : v)) },
      ),
    );
    return run(version, () => api.installRuntime(kind, version));
  }

  return (
    <>
      <header className="page-header">
        <div>
          <h1>Runtimes</h1>
          <p className="muted">Install several versions side by side. Each site can pin its own version.</p>
        </div>
      </header>

      <div role="group" aria-label="Language" className="row wrap">
        {shown.length === 0 && <p className="muted">All runtimes are switched off in Settings.</p>}
        {shown.map((r) => (
          <button
            key={r.kind}
            type="button"
            className="tab"
            aria-pressed={r.kind === family?.kind}
            onClick={() => setSelected(r.kind)}
          >
            {r.name}
          </button>
        ))}
      </div>

      {runtimes.error && <p className="notice error">{runtimes.error}</p>}

      {family && (
        <section aria-label={`${family.name} versions`} className="card">
          {!family.available && <p className="notice inset">{family.name} support is coming after the prototype.</p>}
          {family.versions.map((v) => (
            <VersionRow
              key={v.version}
              name={family.name}
              available={family.available}
              v={v}
              progress={progress[key(family.kind, v.version)]}
              error={errors[key(family.kind, v.version)]}
              onInstall={() => install(v.version)}
              onRemove={() => run(v.version, () => api.uninstallRuntime(family.kind, v.version))}
              onDefault={() => run(v.version, () => api.setDefaultRuntime(family.kind, v.version))}
            />
          ))}
        </section>
      )}

      <p className="card dashed muted">
        Per-project override: add a <code className="mono">.stackyard</code> file to a project folder, or pick a
        version on the Sites screen.
      </p>
    </>
  );
}

type RowProps = {
  name: string;
  available: boolean;
  v: RuntimeVersion;
  progress?: InstallProgress;
  error?: string;
  onInstall(): void;
  onRemove(): void;
  onDefault(): void;
};

function VersionRow({ name, available, v, progress, error, onInstall, onRemove, onDefault }: RowProps) {
  const label = `${name} ${v.version}`;
  return (
    <div className="version-row">
      <div className="stack tight">
        <div className="row">
          <span className="mono strong">{v.version}</span>
          {v.installing ? (
            <ProgressText progress={progress} />
          ) : v.installed ? (
            <span className="small">
              <span className="ok">Installed</span> <span className="mono muted">{v.installed}</span>
            </span>
          ) : (
            <span className="muted small">Not installed</span>
          )}
          {v.default && v.installed && <span className="badge">Default</span>}
        </div>
        {v.installing && <ProgressBar label={label} progress={progress} />}
        {error && (
          <p role="alert" className="error small">
            {error}
          </p>
        )}
      </div>
      <div className="row">
        {v.installed && !v.default && !v.installing && (
          <button type="button" className="btn ghost small" onClick={onDefault} aria-label={`Make ${label} the default`}>
            Set default
          </button>
        )}
        {v.installed && !v.installing && (
          <button type="button" className="btn small" onClick={onRemove} aria-label={`Remove ${label}`}>
            Remove
          </button>
        )}
        {!v.installed && (
          <button
            type="button"
            className="btn primary small"
            disabled={!available || v.installing}
            onClick={onInstall}
            aria-label={`Install ${label}`}
          >
            {v.installing ? "Installing…" : "Install"}
          </button>
        )}
      </div>
    </div>
  );
}

function percent(p?: InstallProgress) {
  return p?.step === "downloading" && p.total ? Math.floor(((p.done ?? 0) * 100) / p.total) : null;
}

function ProgressText({ progress }: { progress?: InstallProgress }) {
  const pct = percent(progress);
  const text =
    progress?.step === "unpacking"
      ? "Unpacking…"
      : progress?.step === "downloading"
        ? pct === null
          ? `Downloading ${((progress.done ?? 0) / 1e6).toFixed(0)} MB…`
          : `Downloading ${pct}%`
        : "Looking up the latest release…";
  return <span className="muted small">{text}</span>;
}

function ProgressBar({ label, progress }: { label: string; progress?: InstallProgress }) {
  const pct = progress?.step === "unpacking" ? 100 : percent(progress);
  return (
    <div
      className="progress"
      role="progressbar"
      aria-label={`Installing ${label}`}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={pct ?? undefined}
    >
      <div className={pct === null ? "progress-fill busy" : "progress-fill"} style={{ width: `${pct ?? 30}%` }} />
    </div>
  );
}
