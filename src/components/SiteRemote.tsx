import { useCallback, useEffect, useState, type FormEvent } from "react";
import {
  api,
  errorText,
  type GitStatus,
  type PlannedFile,
  type RemoteConnection,
  type RemoteConnections,
  type RemoteInput,
  type RemoteListing,
  type RemoteProgress,
  type RemoteProtocol,
  type RemoteTest,
  type Site,
  type TransferDirection,
  type TransferOutcome,
  type TransferPlan,
} from "../lib/api";
import { useModules } from "../lib/modules";

const PROTOCOLS: { value: RemoteProtocol; label: string }[] = [
  { value: "sftp", label: "SFTP (SSH)" },
  { value: "ftps", label: "FTPS (FTP with TLS)" },
  { value: "ftp", label: "FTP (not encrypted)" },
];

const DEFAULT_SKIP = [".git", "node_modules", ".DS_Store", "Thumbs.db", ".env"];

function size(bytes: number) {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 ** 2) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 ** 3) return `${(bytes / 1024 ** 2).toFixed(1)} MB`;
  return `${(bytes / 1024 ** 3).toFixed(2)} GB`;
}

function plural(n: number, one: string, many = `${one}s`) {
  return `${n} ${n === 1 ? one : many}`;
}

/** Remote servers and Git for one site, shown under its row on the Sites screen. */
export function SiteRemote({ site }: { site: Site }) {
  const { isOn } = useModules();
  const tabs = [isOn("remote") && "Servers", isOn("git") && "Git"].filter(Boolean) as string[];
  const [tab, setTab] = useState(tabs[0] ?? "Servers");
  if (tabs.length === 0) {
    return <p className="muted small">Turn on Remote servers or Git in Settings › Modules.</p>;
  }
  const shown = tabs.includes(tab) ? tab : tabs[0];
  return (
    <div className="stack remote-panel">
      {tabs.length > 1 && (
        <div className="row wrap" role="group" aria-label="Connect with">
          {tabs.map((t) => (
            <button key={t} type="button" className="tab" aria-pressed={shown === t} onClick={() => setTab(t)}>
              {t}
            </button>
          ))}
        </div>
      )}
      {shown === "Servers" ? <Servers site={site} /> : <Git site={site} />}
    </div>
  );
}

function Servers({ site }: { site: Site }) {
  const [data, setData] = useState<RemoteConnections | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [editing, setEditing] = useState<RemoteConnection | "new" | null>(null);

  const reload = useCallback(() => {
    api
      .remoteConnections(site.domain)
      .then((d) => {
        setData(d);
        setError(null);
      })
      .catch((e) => setError(errorText(e)));
  }, [site.domain]);
  useEffect(reload, [reload]);

  if (!data) return error ? <p className="error small">{error}</p> : <p className="muted small">Loading…</p>;
  return (
    <div className="stack">
      {error && (
        <p className="error small" role="alert">
          {error}
        </p>
      )}
      {data.connections.length === 0 && !editing && (
        <p className="muted">
          Connect this site to a server to upload and download its files. SFTP works with Cloudways, most hosts and
          any VPS; FTP works with almost every shared host.
        </p>
      )}
      {data.connections.map((c) =>
        editing !== "new" && editing?.id === c.id ? (
          <ConnectionForm
            key={c.id}
            site={site}
            existing={c}
            secretStore={data.secret_store}
            onSaved={(d) => {
              setData(d);
              setEditing(null);
            }}
            onCancel={() => setEditing(null)}
          />
        ) : (
          <ConnectionCard
            key={c.id}
            connection={c}
            onEdit={() => setEditing(c)}
            onChanged={setData}
            onError={setError}
          />
        ),
      )}
      {editing === "new" ? (
        <ConnectionForm
          site={site}
          existing={null}
          secretStore={data.secret_store}
          onSaved={(d) => {
            setData(d);
            setEditing(null);
          }}
          onCancel={() => setEditing(null)}
        />
      ) : (
        <div>
          <button type="button" className="btn" onClick={() => setEditing("new")}>
            Add a server
          </button>
        </div>
      )}
    </div>
  );
}

function ConnectionForm({
  site,
  existing,
  secretStore,
  onSaved,
  onCancel,
}: {
  site: Site;
  existing: RemoteConnection | null;
  secretStore: string;
  onSaved: (d: RemoteConnections) => void;
  onCancel: () => void;
}) {
  const [form, setForm] = useState<RemoteInput>(() => ({
    site: site.domain,
    name: existing?.name ?? "Production",
    protocol: existing?.protocol ?? "sftp",
    host: existing?.host ?? "",
    port: existing?.port ?? null,
    user: existing?.user ?? "",
    key_file: existing?.key_file ?? null,
    remote_dir: existing?.remote_dir ?? "",
    local_dir: existing?.local_dir ?? "",
    skip: existing?.skip ?? DEFAULT_SKIP,
    password: null,
  }));
  const [useKey, setUseKey] = useState(Boolean(existing?.key_file));
  const [skipText, setSkipText] = useState((existing?.skip ?? DEFAULT_SKIP).join(", "));
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const set = <K extends keyof RemoteInput>(key: K, value: RemoteInput[K]) => setForm((f) => ({ ...f, [key]: value }));
  const id = existing?.id ?? "new";
  const defaultPort = form.protocol === "sftp" ? 22 : 21;

  async function browseKey() {
    const [file] = await api.pickFiles("Select your private key", form.key_file ?? undefined, false);
    if (file) set("key_file", file);
  }

  async function save(e: FormEvent) {
    e.preventDefault();
    setSaving(true);
    try {
      const input: RemoteInput = {
        ...form,
        key_file: form.protocol === "sftp" && useKey ? form.key_file : null,
        skip: skipText
          .split(/[,\n]/)
          .map((s) => s.trim())
          .filter(Boolean),
      };
      onSaved(await api.remoteSave(existing?.id ?? null, input));
    } catch (err) {
      setError(errorText(err));
    } finally {
      setSaving(false);
    }
  }

  return (
    <form className="card stack form remote-form" onSubmit={save} aria-label={existing ? `Edit ${existing.name}` : "New server"}>
      <h3>{existing ? `Edit ${existing.name}` : "New server"}</h3>
      <div className="form-grid">
        <div className="field">
          <label htmlFor={`r-name-${id}`}>Name</label>
          <input id={`r-name-${id}`} value={form.name} required onChange={(e) => set("name", e.target.value)} />
        </div>
        <div className="field">
          <label htmlFor={`r-proto-${id}`}>Connect with</label>
          <select
            id={`r-proto-${id}`}
            value={form.protocol}
            onChange={(e) => {
              const protocol = e.target.value as RemoteProtocol;
              setForm((f) => ({ ...f, protocol, port: null }));
            }}
          >
            {PROTOCOLS.map((p) => (
              <option key={p.value} value={p.value}>
                {p.label}
              </option>
            ))}
          </select>
        </div>
        <div className="field">
          <label htmlFor={`r-host-${id}`}>Server address</label>
          <input
            id={`r-host-${id}`}
            className="mono"
            value={form.host}
            placeholder="203.0.113.10 or ftp.example.com"
            required
            onChange={(e) => set("host", e.target.value)}
          />
        </div>
        <div className="field">
          <label htmlFor={`r-port-${id}`}>Port</label>
          <input
            id={`r-port-${id}`}
            className="mono"
            inputMode="numeric"
            value={form.port ?? ""}
            placeholder={String(defaultPort)}
            onChange={(e) => {
              const digits = e.target.value.replace(/\D/g, "");
              set("port", digits ? Number(digits) : null);
            }}
          />
        </div>
        <div className="field">
          <label htmlFor={`r-user-${id}`}>User name</label>
          <input
            id={`r-user-${id}`}
            className="mono"
            value={form.user}
            required
            autoComplete="off"
            onChange={(e) => set("user", e.target.value)}
          />
        </div>
        <div className="field">
          <label htmlFor={`r-pass-${id}`}>{useKey && form.protocol === "sftp" ? "Key passphrase" : "Password"}</label>
          <input
            id={`r-pass-${id}`}
            type="password"
            autoComplete="new-password"
            value={form.password ?? ""}
            placeholder={existing?.has_password ? "Saved; type to change" : useKey ? "Only if the key has one" : ""}
            onChange={(e) => set("password", e.target.value)}
          />
        </div>
        <div className="field">
          <label htmlFor={`r-rdir-${id}`}>Folder on the server</label>
          <input
            id={`r-rdir-${id}`}
            className="mono"
            value={form.remote_dir}
            placeholder="/home/master/applications/abc/public_html"
            onChange={(e) => set("remote_dir", e.target.value)}
          />
        </div>
        <div className="field">
          <label htmlFor={`r-ldir-${id}`}>Part of the site folder</label>
          <input
            id={`r-ldir-${id}`}
            className="mono"
            value={form.local_dir}
            placeholder="The whole folder"
            onChange={(e) => set("local_dir", e.target.value)}
          />
        </div>
      </div>
      {form.protocol === "sftp" && (
        <div className="stack tight">
          <label className="check">
            <input type="checkbox" checked={useKey} onChange={(e) => setUseKey(e.target.checked)} />
            Log in with an SSH key instead of a password
          </label>
          {useKey && (
            <div className="row folder-input">
              <input
                aria-label="Private key file"
                className="mono grow"
                value={form.key_file ?? ""}
                placeholder="~/.ssh/id_ed25519"
                onChange={(e) => set("key_file", e.target.value)}
              />
              <button type="button" className="btn" onClick={browseKey}>
                Browse…
              </button>
            </div>
          )}
        </div>
      )}
      <div className="field">
        <label htmlFor={`r-skip-${id}`}>Never transfer</label>
        <input id={`r-skip-${id}`} className="mono" value={skipText} onChange={(e) => setSkipText(e.target.value)} />
        <span className="muted small">
          Names anywhere (<span className="mono">*.log</span> works) or paths from the top (
          <span className="mono">storage/logs</span>), separated by commas.
        </span>
      </div>
      {form.protocol === "ftp" && (
        <p className="warn small">Plain FTP sends your password and files unencrypted. Use SFTP or FTPS if the host offers them.</p>
      )}
      <p className="muted small">
        Cloudways: use the server's Public IP, your application's SFTP user, and{" "}
        <span className="mono">/home/master/applications/&lt;app folder&gt;/public_html</span>. Passwords are kept in{" "}
        {secretStore}, not in Stackyard's files.
      </p>
      {error && (
        <p className="error small" role="alert">
          {error}
        </p>
      )}
      <div className="row">
        <button type="submit" className="btn primary" disabled={saving}>
          {saving ? "Saving…" : "Save"}
        </button>
        <button type="button" className="btn ghost" onClick={onCancel}>
          Cancel
        </button>
      </div>
    </form>
  );
}

type Job =
  | { kind: "planning"; direction: TransferDirection; del: boolean }
  | { kind: "plan"; plan: TransferPlan; del: boolean }
  | { kind: "running"; total: number }
  | { kind: "done"; outcome: TransferOutcome; direction: TransferDirection };

function ConnectionCard({
  connection: c,
  onEdit,
  onChanged,
  onError,
}: {
  connection: RemoteConnection;
  onEdit: () => void;
  onChanged: (d: RemoteConnections) => void;
  onError: (e: string | null) => void;
}) {
  const [test, setTest] = useState<RemoteTest | "testing" | null>(null);
  const [job, setJob] = useState<Job | null>(null);
  const [progress, setProgress] = useState<RemoteProgress | null>(null);
  const [browsing, setBrowsing] = useState(false);
  const [confirmRemove, setConfirmRemove] = useState(false);
  const label = PROTOCOLS.find((p) => p.value === c.protocol)?.label ?? c.protocol;

  useEffect(() => {
    let off: (() => void) | undefined;
    let cancelled = false;
    api
      .onRemoteProgress((p) => {
        if (p.id === c.id) setProgress(p);
      })
      .then((unsubscribe) => (cancelled ? unsubscribe() : (off = unsubscribe)));
    return () => {
      cancelled = true;
      off?.();
    };
  }, [c.id]);

  async function runTest() {
    setTest("testing");
    try {
      setTest(await api.remoteTest(c.id));
    } catch (e) {
      setTest({ start_dir: null, error: errorText(e), certificate: null, key_changed: false });
    }
  }

  async function trust(fingerprint: string | null) {
    try {
      onChanged(await api.remoteTrust(c.id, fingerprint));
      await runTest();
    } catch (e) {
      onError(errorText(e));
    }
  }

  async function plan(direction: TransferDirection, del: boolean) {
    setJob({ kind: "planning", direction, del });
    setProgress(null);
    try {
      const result = await api.remotePlan(c.id, direction, del, null);
      setJob({ kind: "plan", plan: result, del });
    } catch (e) {
      setJob(null);
      onError(errorText(e) === "stopped" ? null : errorText(e));
    }
  }

  async function run(files: PlannedFile[], direction: TransferDirection) {
    setJob({ kind: "running", total: files.length });
    setProgress(null);
    try {
      const outcome = await api.remoteRun(c.id, files);
      setJob({ kind: "done", outcome, direction });
    } catch (e) {
      setJob(null);
      onError(errorText(e));
    }
  }

  async function remove() {
    try {
      onChanged(await api.remoteRemove(c.id));
      onError(null);
    } catch (e) {
      onError(errorText(e));
    }
  }

  const busy = job?.kind === "planning" || job?.kind === "running";
  return (
    <section className="card stack remote-card" aria-label={c.name}>
      <div className="row between wrap">
        <div className="stack tight">
          <h3>{c.name}</h3>
          <span className="muted small">
            {label} · <span className="mono">{`${c.user}@${c.host}:${c.port}`}</span>
          </span>
          <span className="mono muted small">
            {c.remote_dir || "The server's start folder"}
            {c.local_dir && ` ⇄ ${c.local_dir}/`}
          </span>
        </div>
        <div className="row wrap">
          <button type="button" className="btn small primary" disabled={busy} onClick={() => plan("upload", false)}>
            Upload changes
          </button>
          <button type="button" className="btn small" disabled={busy} onClick={() => plan("download", false)}>
            Download changes
          </button>
          <button type="button" className="btn small" aria-pressed={browsing} onClick={() => setBrowsing(!browsing)}>
            {browsing ? "Hide files" : "Browse files"}
          </button>
          <button type="button" className="btn small ghost" disabled={test === "testing"} onClick={runTest}>
            {test === "testing" ? "Testing…" : "Test"}
          </button>
          <button type="button" className="btn small ghost" disabled={busy} onClick={onEdit}>
            Edit
          </button>
          {confirmRemove ? (
            <button type="button" className="btn small danger" onClick={remove}>
              Remove {c.name}?
            </button>
          ) : (
            <button type="button" className="btn small ghost" disabled={busy} onClick={() => setConfirmRemove(true)}>
              Remove
            </button>
          )}
        </div>
      </div>

      {test && test !== "testing" && (
        <TestResult test={test} onTrust={(fp) => trust(fp)} onForgetKey={() => trust(null)} />
      )}

      {job?.kind === "planning" && (
        <div className="row between wrap centered">
          <span className="muted small">
            Comparing files{progress?.step === "scanning" && progress.folder ? ` in ${progress.folder}` : ""}…
            {progress?.step === "scanning" && ` ${plural(progress.found, "file")} so far`}
          </span>
          <button type="button" className="btn small ghost" onClick={() => api.remoteStop(c.id)}>
            Stop
          </button>
        </div>
      )}

      {job?.kind === "plan" && (
        <PlanView
          plan={job.plan}
          del={job.del}
          onDelete={(del) => plan(job.plan.direction, del)}
          onRun={() => run(job.plan.files, job.plan.direction)}
          onCancel={() => setJob(null)}
        />
      )}

      {job?.kind === "running" && (
        <div className="stack tight">
          <div className="row between wrap centered">
            <span className="small">
              {progress?.step === "transfer"
                ? `${progress.index + 1} of ${progress.total}: ${progress.path}`
                : `Starting ${plural(job.total, "file")}…`}
            </span>
            <button type="button" className="btn small ghost" onClick={() => api.remoteStop(c.id)}>
              Stop
            </button>
          </div>
          <div className="progress" aria-hidden="true">
            <div
              className="progress-fill"
              style={{
                width:
                  progress?.step === "transfer"
                    ? `${Math.round(((progress.index + (progress.size ? progress.done / progress.size : 1)) / progress.total) * 100)}%`
                    : "0%",
              }}
            />
          </div>
        </div>
      )}

      {job?.kind === "done" && <OutcomeView outcome={job.outcome} direction={job.direction} onClose={() => setJob(null)} />}

      {browsing && <Browser connection={c} disabled={busy} onRun={run} />}
    </section>
  );
}

function TestResult({
  test,
  onTrust,
  onForgetKey,
}: {
  test: RemoteTest;
  onTrust: (fingerprint: string) => void;
  onForgetKey: () => void;
}) {
  if (test.start_dir) {
    return (
      <p className="ok small" role="status">
        Connected. The server starts in <span className="mono">{test.start_dir}</span>.
      </p>
    );
  }
  return (
    <div className="stack tight" role="alert">
      <p className="error small">{test.error}</p>
      {test.certificate && (
        <div className="row wrap centered">
          <span className="muted small">
            Hosts often use a certificate made for their server name, not yours. Trust it only if this is your host.
          </span>
          <button type="button" className="btn small" onClick={() => onTrust(test.certificate!)}>
            Trust this certificate
          </button>
        </div>
      )}
      {test.key_changed && (
        <div className="row wrap centered">
          <span className="muted small">Only do this if you know the server was rebuilt or moved.</span>
          <button type="button" className="btn small danger" onClick={onForgetKey}>
            Trust the new key
          </button>
        </div>
      )}
    </div>
  );
}

const ACTION_LABEL: Record<PlannedFile["action"], string> = {
  upload: "Upload",
  download: "Download",
  "delete-remote": "Delete on server",
  "delete-local": "Delete here",
};

const REASON_LABEL: Record<PlannedFile["reason"], string> = {
  new: "new",
  changed: "changed",
  extra: "not on the other side",
  picked: "picked",
};

function PlanView({
  plan,
  del,
  onDelete,
  onRun,
  onCancel,
}: {
  plan: TransferPlan;
  del: boolean;
  onDelete: (del: boolean) => void;
  onRun: () => void;
  onCancel: () => void;
}) {
  const upload = plan.direction === "upload";
  const copies = plan.files.filter((f) => !f.action.startsWith("delete")).length;
  const deletes = plan.files.length - copies;
  const [showAll, setShowAll] = useState(false);
  const shown = showAll ? plan.files : plan.files.slice(0, 12);
  return (
    <div className="stack">
      {plan.files.length === 0 ? (
        <p className="ok small" role="status">
          Everything is the same on both sides ({plural(plan.unchanged, "file")} checked).
        </p>
      ) : (
        <>
          <p className="small">
            {upload ? "Upload" : "Download"} {plural(copies, "file")} ({size(plan.bytes)})
            {deletes > 0 && `, delete ${plural(deletes, "file")} ${upload ? "on the server" : "in the site folder"}`}.{" "}
            <span className="muted">{plural(plan.unchanged, "file")} are already the same.</span>
          </p>
          <div className="grid-wrap remote-files">
            <table className="result-grid">
              <tbody>
                {shown.map((f) => (
                  <tr key={f.path}>
                    <td className={f.action.startsWith("delete") ? "error" : undefined}>{ACTION_LABEL[f.action]}</td>
                    <td>{f.path}</td>
                    <td className="muted">{REASON_LABEL[f.reason]}</td>
                    <td className="num">{f.action.startsWith("delete") ? "" : size(f.size)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
          {plan.files.length > shown.length && (
            <button type="button" className="link small" onClick={() => setShowAll(true)}>
              Show all {plan.files.length}
            </button>
          )}
        </>
      )}
      <label className="check small">
        <input type="checkbox" checked={del} onChange={(e) => onDelete(e.target.checked)} />
        Also delete files {upload ? "on the server that are not in the site folder" : "in the site folder that are not on the server"}
      </label>
      <div className="row">
        {plan.files.length > 0 && (
          <button type="button" className={deletes > 0 ? "btn danger" : "btn primary"} onClick={onRun}>
            {upload ? "Upload" : "Download"} now
          </button>
        )}
        <button type="button" className="btn ghost" onClick={onCancel}>
          {plan.files.length > 0 ? "Cancel" : "Close"}
        </button>
      </div>
    </div>
  );
}

function OutcomeView({
  outcome,
  direction,
  onClose,
}: {
  outcome: TransferOutcome;
  direction: TransferDirection;
  onClose: () => void;
}) {
  return (
    <div className="stack tight" role="status">
      <p className={outcome.failed.length ? "warn small" : "ok small"}>
        {direction === "upload" ? "Uploaded" : "Downloaded"} {plural(outcome.done, "file")} ({size(outcome.bytes)})
        {outcome.failed.length > 0 && `; ${plural(outcome.failed.length, "file")} failed`}
        {outcome.stopped && "; stopped before the end"}.
      </p>
      {outcome.failed.slice(0, 20).map((f) => (
        <p key={f.path} className="error small">
          <span className="mono">{f.path}</span>: {f.error}
        </p>
      ))}
      <div>
        <button type="button" className="btn small ghost" onClick={onClose}>
          Close
        </button>
      </div>
    </div>
  );
}

/** `/a/b/c` inside `/a` → `b/c`; null when outside. */
function relativeTo(root: string, path: string): string | null {
  const base = root.replace(/\/+$/, "");
  if (path === base || (base === "" && path === "/")) return "";
  if (base === "" ? path.startsWith("/") : path.startsWith(`${base}/`)) return path.slice(base.length + 1);
  return null;
}

function joinPath(dir: string, name: string) {
  return dir.endsWith("/") ? `${dir}${name}` : `${dir}/${name}`;
}

function Browser({
  connection,
  disabled,
  onRun,
}: {
  connection: RemoteConnection;
  disabled: boolean;
  onRun: (files: PlannedFile[], direction: TransferDirection) => Promise<void>;
}) {
  const [listing, setListing] = useState<RemoteListing | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  const open = useCallback(
    async (dir: string | null) => {
      setLoading(true);
      try {
        setListing(await api.remoteBrowse(connection.id, dir));
        setError(null);
      } catch (e) {
        setError(errorText(e));
      } finally {
        setLoading(false);
      }
    },
    [connection.id],
  );
  useEffect(() => {
    open(null);
  }, [open]);

  async function transfer(direction: TransferDirection, paths: string[]) {
    try {
      const plan = await api.remotePlan(connection.id, direction, false, paths);
      await onRun(plan.files, direction);
      if (direction === "upload" && listing) await open(listing.dir);
    } catch (e) {
      setError(errorText(e));
    }
  }

  async function uploadHere() {
    if (!listing) return;
    const inside = relativeTo(listing.root, listing.dir);
    if (inside === null) return;
    const localRoot = await api.remoteLocalFolder(connection.id);
    const sep = localRoot.includes("\\") ? "\\" : "/";
    const start = inside ? `${localRoot}${sep}${inside.split("/").join(sep)}` : localRoot;
    const picked = await api.pickFiles("Pick files to upload", start);
    if (picked.length === 0) return;
    const norm = (p: string) => p.replace(/\\/g, "/").replace(/\/+$/, "");
    const base = norm(localRoot);
    const rel = picked.map((p) => {
      const n = norm(p);
      const same = sep === "\\" ? n.toLowerCase().startsWith(`${base.toLowerCase()}/`) : n.startsWith(`${base}/`);
      return same ? n.slice(base.length + 1) : null;
    });
    if (rel.some((r) => r === null)) {
      setError(`Pick files inside the site folder (${localRoot}); they go to the same place on the server.`);
      return;
    }
    await transfer("upload", rel as string[]);
  }

  if (!listing) {
    return error ? <p className="error small">{error}</p> : <p className="muted small">Opening the server…</p>;
  }
  const inside = relativeTo(listing.root, listing.dir);
  const parent = listing.dir === "/" ? null : listing.dir.replace(/\/[^/]+\/?$/, "") || "/";
  return (
    <div className="stack tight remote-browser">
      <div className="row between wrap centered">
        <div className="row wrap centered">
          <button type="button" className="btn small" disabled={!parent || loading} onClick={() => parent && open(parent)}>
            Up
          </button>
          <span className="mono small">{listing.dir}</span>
          {listing.dir !== listing.root && (
            <button type="button" className="link small" onClick={() => open(listing.root)}>
              Back to the site's folder
            </button>
          )}
        </div>
        {inside !== null && (
          <button type="button" className="btn small" disabled={disabled} onClick={uploadHere}>
            Upload files here…
          </button>
        )}
      </div>
      {error && (
        <p className="error small" role="alert">
          {error}
        </p>
      )}
      {inside === null && (
        <p className="muted small">This folder is outside the site's folder on the server, so files here can only be viewed.</p>
      )}
      <div className="grid-wrap">
        <table className="result-grid">
          <tbody>
            {listing.entries.length === 0 && (
              <tr>
                <td className="muted">This folder is empty.</td>
              </tr>
            )}
            {listing.entries.map((entry) => {
              const path = joinPath(listing.dir, entry.name);
              const rel = relativeTo(listing.root, path);
              return (
                <tr key={entry.name}>
                  <td>
                    {entry.dir ? (
                      <button type="button" className="link mono" onClick={() => open(path)}>
                        {entry.name}/
                      </button>
                    ) : (
                      entry.name
                    )}
                  </td>
                  <td className="num muted">{entry.dir ? "" : size(entry.size)}</td>
                  <td className="muted">{entry.modified ? new Date(entry.modified * 1000).toLocaleString() : ""}</td>
                  <td className="right">
                    {rel !== null && (
                      <button
                        type="button"
                        className="link small"
                        disabled={disabled}
                        onClick={() => transfer("download", [rel])}
                      >
                        Download
                      </button>
                    )}
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
    </div>
  );
}

const CHANGE_LABEL: Record<string, string> = {
  added: "Added",
  modified: "Changed",
  deleted: "Deleted",
  renamed: "Renamed",
  new: "New",
  conflict: "Conflict",
};

function Git({ site }: { site: Site }) {
  const [status, setStatus] = useState<GitStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [url, setUrl] = useState("");
  const [relinking, setRelinking] = useState(false);
  const [message, setMessage] = useState("");
  const [name, setName] = useState("");
  const [email, setEmail] = useState("");
  const [showAll, setShowAll] = useState(false);

  const act = useCallback(async (label: string | null, work: () => Promise<GitStatus>) => {
    setBusy(label);
    try {
      setStatus(await work());
      setError(null);
      return true;
    } catch (e) {
      setError(errorText(e));
      return false;
    } finally {
      setBusy(null);
    }
  }, []);

  useEffect(() => {
    act(null, () => api.gitStatus(site.domain));
  }, [act, site.domain]);

  async function link(e: FormEvent) {
    e.preventDefault();
    if (await act("Linking…", () => api.gitLink(site.domain, url))) {
      setRelinking(false);
      setUrl("");
    }
  }

  async function commit(e: FormEvent) {
    e.preventDefault();
    const identity = status?.identity ? [null, null] : [name, email];
    if (await act("Committing…", () => api.gitCommit(site.domain, message, identity[0], identity[1]))) setMessage("");
  }

  if (!status) return error ? <p className="error small">{error}</p> : <p className="muted small">Loading…</p>;
  if (!status.installed) {
    return (
      <p className="muted">
        Git isn't installed on this computer. Install it from <span className="mono">git-scm.com</span> (on Windows,
        "Git for Windows" with its default options), then come back here.
      </p>
    );
  }
  const errorLine = error && (
    <p className="error small" role="alert" style={{ whiteSpace: "pre-wrap" }}>
      {error}
    </p>
  );
  if (!status.repo || !status.remote || relinking) {
    return (
      <form className="stack" onSubmit={link}>
        <p className="muted small">
          Link this site to a repository on GitHub, GitLab, Bitbucket or any Git host. An empty site folder gets a copy
          of the repository; a folder that already has files keeps them, and anything that differs shows up as changes
          to commit.
        </p>
        <div className="field">
          <label htmlFor={`g-url-${site.domain}`}>Repository address</label>
          <input
            id={`g-url-${site.domain}`}
            className="mono"
            value={url}
            placeholder="https://github.com/you/app.git"
            required
            onChange={(e) => setUrl(e.target.value)}
          />
        </div>
        {errorLine}
        <div className="row">
          <button type="submit" className="btn primary" disabled={busy !== null}>
            {busy ?? "Link repository"}
          </button>
          {relinking && (
            <button type="button" className="btn ghost" onClick={() => setRelinking(false)}>
              Cancel
            </button>
          )}
        </div>
      </form>
    );
  }
  const changes = showAll ? status.changes : status.changes.slice(0, 10);
  const total = status.changes.length + status.more_changes;
  return (
    <div className="stack">
      <div className="row between wrap">
        <div className="stack tight">
          <span>
            <span className="strong">{status.branch ?? "No branch"}</span>
            <span className="muted"> · </span>
            <span className="mono small">{status.remote}</span>
          </span>
          <span className="muted small">
            {[
              status.ahead > 0 && `${plural(status.ahead, "commit")} to push`,
              status.behind > 0 && `${plural(status.behind, "commit")} to pull`,
              total > 0 ? `${plural(total, "changed file")}` : "No changes",
              !status.upstream && "not pushed yet",
            ]
              .filter(Boolean)
              .join(" · ")}
          </span>
          {status.root && <span className="muted small">Part of the repository in {status.root}</span>}
        </div>
        <div className="row wrap">
          <button type="button" className="btn small" disabled={busy !== null} onClick={() => act("Pulling…", () => api.gitPull(site.domain))}>
            {busy === "Pulling…" ? busy : "Pull"}
          </button>
          <button type="button" className="btn small primary" disabled={busy !== null} onClick={() => act("Pushing…", () => api.gitPush(site.domain))}>
            {busy === "Pushing…" ? busy : "Push"}
          </button>
          <button type="button" className="btn small ghost" disabled={busy !== null} onClick={() => act("Checking…", () => api.gitFetch(site.domain))}>
            {busy === "Checking…" ? busy : "Check the server"}
          </button>
          <button type="button" className="btn small ghost" onClick={() => setRelinking(true)}>
            Change address
          </button>
        </div>
      </div>
      {errorLine}
      {total > 0 && (
        <form className="stack" onSubmit={commit}>
          <div className="grid-wrap remote-files">
            <table className="result-grid">
              <tbody>
                {changes.map((c) => (
                  <tr key={c.path}>
                    <td className={c.kind === "conflict" ? "error" : "muted"}>{CHANGE_LABEL[c.kind]}</td>
                    <td>{c.path}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
          {total > changes.length && (
            <button type="button" className="link small" onClick={() => setShowAll(true)}>
              {status.changes.length > changes.length ? `Show all ${status.changes.length}` : `and ${status.more_changes} more`}
            </button>
          )}
          {!status.identity && (
            <div className="form-grid">
              <div className="field">
                <label htmlFor={`g-name-${site.domain}`}>Your name (for commits)</label>
                <input id={`g-name-${site.domain}`} value={name} required onChange={(e) => setName(e.target.value)} />
              </div>
              <div className="field">
                <label htmlFor={`g-email-${site.domain}`}>Your email</label>
                <input
                  id={`g-email-${site.domain}`}
                  type="email"
                  value={email}
                  required
                  onChange={(e) => setEmail(e.target.value)}
                />
              </div>
            </div>
          )}
          <div className="row folder-input">
            <input
              aria-label="What changed"
              className="grow"
              value={message}
              placeholder="What changed, e.g. New contact page"
              required
              onChange={(e) => setMessage(e.target.value)}
            />
            <button type="submit" className="btn" disabled={busy !== null}>
              {busy === "Committing…" ? busy : "Commit all changes"}
            </button>
          </div>
        </form>
      )}
    </div>
  );
}
