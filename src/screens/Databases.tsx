import { useCallback, useEffect, useState, type FormEvent, type KeyboardEvent } from "react";
import {
  api,
  errorText,
  type Database,
  type DbBackup,
  type DbKind,
  type DbServerStatus,
  type DbTable,
  type ResultSet,
} from "../lib/api";
import { PhpMyAdminCard } from "../components/PhpMyAdminCard";
import { useT } from "../lib/i18n";
import { Progress } from "../components/Progress";

const PAGE = 100;
type Tab = "Tables" | "Query" | "Backups";

const KIND_NAMES: Record<DbKind, string> = { mysql: "MySQL / MariaDB", postgres: "PostgreSQL" };

export function Databases() {
  const t = useT();
  const [servers, setServers] = useState<DbKind[]>([]);
  const [kind, setKind] = useState<DbKind>("mysql");
  const [status, setStatus] = useState<DbServerStatus | null>(null);
  const [dbs, setDbs] = useState<Database[]>([]);
  const [current, setCurrent] = useState<string | null>(null);
  const [tab, setTab] = useState<Tab>("Tables");
  const [newName, setNewName] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [starting, setStarting] = useState(false);

  const running = !!status?.server?.version;

  useEffect(() => {
    api
      .dbServers()
      .then((found) => {
        setServers(found);
        // Only PostgreSQL is set up: show it rather than a MySQL notice.
        if (found.length === 1) setKind(found[0]);
      })
      .catch(() => {});
  }, []);

  const load = useCallback(async () => {
    try {
      const s = await api.dbServer(kind);
      setStatus(s);
      setDbs(s.server?.version ? await api.dbList(kind) : []);
    } catch (e) {
      setError(errorText(e));
    }
  }, [kind]);

  useEffect(() => {
    load();
  }, [load]);

  function switchTo(next: DbKind) {
    if (next === kind) return;
    setStatus(null);
    setDbs([]);
    setCurrent(null);
    setError(null);
    setKind(next);
  }

  const selected = dbs.find((d) => d.name === current)?.name ?? dbs[0]?.name ?? null;

  async function start() {
    if (!status?.server) return;
    setStarting(true);
    try {
      await api.startService(status.server.id);
      await load();
      setError(null);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setStarting(false);
    }
  }

  async function create(e: FormEvent) {
    e.preventDefault();
    try {
      setDbs(await api.dbCreate(newName.trim(), kind));
      setCurrent(newName.trim());
      setNewName("");
      setError(null);
    } catch (err) {
      setError(errorText(err));
    }
  }

  return (
    <>
      <header className="page-header">
        <div>
          <h1>{t("Databases")}</h1>
          <p className="muted">
            {status?.server
              ? `${status.server.name}${status.server.version ? ` · running ${status.server.version}` : " · not running"}. Connect as ${kind === "postgres" ? "postgres" : "root"} with no password on 127.0.0.1.`
              : t("Browse tables, run queries, and back up and restore your MySQL, MariaDB or PostgreSQL databases.")}
          </p>
        </div>
        {servers.length > 1 && (
          <div className="row" role="group" aria-label={t("Database server")}>
            {servers.map((k) => (
              <button key={k} type="button" className="tab" aria-pressed={kind === k} onClick={() => switchTo(k)}>
                {KIND_NAMES[k]}
              </button>
            ))}
          </div>
        )}
      </header>

      {error && (
        <p className="error" role="alert">
          {error}
        </p>
      )}

      {kind === "mysql" && <PhpMyAdminCard />}

      {status?.problem && <p className="notice">{status.problem}</p>}
      {status?.server && !running && (
        <div className="notice row wrap between centered">
          <span>{status.server.name} is not running. Start it to see its databases.</span>
          <button type="button" className="btn primary small" onClick={start} disabled={starting}>
            {starting ? "Starting…" : `Start ${status.server.name.split(" ")[0]}`}
          </button>
        </div>
      )}

      {running && (
        <div className="split">
          <nav className="side card stack db-list" aria-label={t("Databases")}>
            {dbs.length === 0 && <p className="muted small">No databases yet.</p>}
            <div className="stack tight">
              {dbs.map((d) => (
                <button
                  key={d.name}
                  type="button"
                  className="nav-item"
                  aria-current={d.name === selected ? "true" : undefined}
                  onClick={() => setCurrent(d.name)}
                >
                  <span className="mono">{d.name}</span>
                  <span className="muted small">
                    {d.tables} {d.tables === 1 ? "table" : "tables"} · {size(d.size)}
                  </span>
                </button>
              ))}
            </div>
            <form className="stack tight" onSubmit={create}>
              <label htmlFor="db-new" className="small muted">
                New database
              </label>
              <div className="row">
                <input
                  id="db-new"
                  className="mono grow"
                  value={newName}
                  placeholder="e.g. shop"
                  onChange={(e) => setNewName(e.target.value)}
                />
                <button type="submit" className="btn small" disabled={!newName.trim()}>
                  Create
                </button>
              </div>
            </form>
          </nav>

          {selected && (
            <section className="table-wrap card stack db-main" aria-label={selected}>
              <div className="row wrap between centered">
                <h2 className="mono">{selected}</h2>
                <div className="row wrap" role="group" aria-label={t("View")}>
                  {(["Tables", "Query", "Backups"] as Tab[]).map((t) => (
                    <button key={t} type="button" className="tab" aria-pressed={tab === t} onClick={() => setTab(t)}>
                      {t}
                    </button>
                  ))}
                </div>
              </div>
              {tab === "Tables" && <Tables key={`${kind}/${selected}`} kind={kind} database={selected} />}
              {tab === "Query" && <Query key={`${kind}/${selected}`} kind={kind} database={selected} onChange={load} />}
              {tab === "Backups" && (
                <Backups
                  key={`${kind}/${selected}`}
                  kind={kind}
                  database={selected}
                  onChange={(next) => {
                    setDbs(next);
                    if (!next.some((d) => d.name === selected)) setCurrent(null);
                  }}
                  onCopied={setCurrent}
                />
              )}
            </section>
          )}
        </div>
      )}
    </>
  );
}

function Tables({ kind, database }: { kind: DbKind; database: string }) {
  const [tables, setTables] = useState<DbTable[] | null>(null);
  const [table, setTable] = useState<string | null>(null);
  const [offset, setOffset] = useState(0);
  const [page, setPage] = useState<ResultSet | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api.dbTables(database, kind).then(setTables).catch((e) => setError(errorText(e)));
  }, [kind, database]);

  useEffect(() => {
    if (!table) return;
    setPage(null);
    api
      .dbBrowse(database, table, offset, PAGE, kind)
      .then((p) => {
        setPage(p);
        setError(null);
      })
      .catch((e) => setError(errorText(e)));
  }, [kind, database, table, offset]);

  if (error) return <p className="error small">{error}</p>;
  if (!tables) return <p className="muted small">Loading…</p>;
  if (tables.length === 0) return <p className="muted">No tables yet. Create some from the Query tab or your app's migrations.</p>;

  if (table) {
    const rows = page?.rows.length ?? 0;
    return (
      <div className="stack">
        <div className="row wrap between centered">
          <button type="button" className="btn ghost small" onClick={() => setTable(null)}>
            ← All tables
          </button>
          <span className="mono strong">{table}</span>
          <div className="row">
            <button
              type="button"
              className="btn small"
              disabled={offset === 0}
              onClick={() => setOffset(Math.max(0, offset - PAGE))}
            >
              Previous
            </button>
            <span className="muted small" aria-live="polite">
              {rows ? `Rows ${offset + 1}–${offset + rows}` : "No rows"}
            </span>
            <button type="button" className="btn small" disabled={rows < PAGE} onClick={() => setOffset(offset + PAGE)}>
              Next
            </button>
          </div>
        </div>
        {page ? <Grid set={page} /> : <p className="muted small">Loading…</p>}
      </div>
    );
  }

  return (
    <div className="grid-wrap">
      <table>
        <thead>
          <tr>
            <th>Table</th>
            <th className="num">Rows</th>
            <th className="num">Size</th>
            <th>
              <span className="sr-only">Open</span>
            </th>
          </tr>
        </thead>
        <tbody>
          {tables.map((t) => (
            <tr key={t.name}>
              <td className="mono">
                {t.name}
                {t.kind === "VIEW" && <span className="muted small"> view</span>}
              </td>
              <td className="num">{t.kind === "VIEW" ? "–" : `~${t.rows.toLocaleString()}`}</td>
              <td className="num">{size(t.size)}</td>
              <td className="num">
                <button
                  type="button"
                  className="btn ghost small"
                  onClick={() => {
                    setOffset(0);
                    setTable(t.name);
                  }}
                >
                  View rows
                </button>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function Query({ kind, database, onChange }: { kind: DbKind; database: string; onChange: () => void }) {
  const [sql, setSql] = useState("");
  const [results, setResults] = useState<ResultSet[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function run() {
    if (!sql.trim() || busy) return;
    setBusy(true);
    try {
      setResults(await api.dbQuery(database, sql, kind));
      setError(null);
      onChange();
    } catch (e) {
      setError(errorText(e));
      setResults(null);
    } finally {
      setBusy(false);
    }
  }

  function keys(e: KeyboardEvent<HTMLTextAreaElement>) {
    if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
      e.preventDefault();
      run();
    }
  }

  return (
    <div className="stack">
      <div className="field">
        <label htmlFor="db-sql">SQL, run in {database}</label>
        <textarea
          id="db-sql"
          className="mono"
          rows={6}
          value={sql}
          spellCheck={false}
          placeholder="SELECT * FROM users LIMIT 10;"
          onChange={(e) => setSql(e.target.value)}
          onKeyDown={keys}
        />
      </div>
      <div className="row wrap">
        <button type="button" className="btn primary" onClick={run} disabled={busy || !sql.trim()}>
          {busy ? "Running…" : "Run"}
        </button>
        <span className="muted small">Ctrl+Enter or ⌘+Enter runs it. Up to 1,000 rows are shown.</span>
      </div>
      {error && (
        <p className="error small" role="alert">
          {error}
        </p>
      )}
      {results && results.length === 0 && <p className="status ok">Done. Nothing was returned.</p>}
      {results?.map((set, i) => (
        <Grid key={i} set={set} />
      ))}
    </div>
  );
}

function Backups({
  kind,
  database,
  onChange,
  onCopied,
}: {
  kind: DbKind;
  database: string;
  onChange: (dbs: Database[]) => void;
  onCopied: (name: string) => void;
}) {
  const t = useT();
  const [all, setAll] = useState<DbBackup[]>([]);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [done, setDone] = useState<string | null>(null);
  /** The file the last backup made. */
  const [saved, setSaved] = useState<string | null>(null);
  const [confirm, setConfirm] = useState<string | null>(null);
  const [copyName, setCopyName] = useState(`${database}_copy`);
  const [dropText, setDropText] = useState("");

  useEffect(() => {
    api.dbBackups().then(setAll).catch(() => {});
  }, []);

  const mine = all.filter((b) => b.database === database && b.server === kind);

  async function attempt(label: string, action: () => Promise<void>, message: string) {
    setBusy(label);
    setError(null);
    setDone(null);
    setSaved(null);
    try {
      await action();
      setDone(message);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(null);
      setConfirm(null);
    }
  }

  return (
    <div className="stack">
      <div className="row wrap between centered">
        <p className="muted small">
          Backups are gzipped SQL files in Stackyard's backups folder. Schedule one with the Scheduler:{" "}
          <code className="mono">
            stackyard db {kind === "postgres" ? "--postgres " : ""}backup {database}
          </code>
        </p>
        <button
          type="button"
          className="btn primary"
          disabled={busy !== null}
          onClick={() =>
            attempt(
              "backup",
              async () => {
                const before = new Set(all.map((b) => b.file));
                const after = await api.dbBackup(database, kind);
                setAll(after);
                setSaved(after.find((b) => b.database === database && !before.has(b.file))?.file ?? null);
              },
              "Backup saved.",
            )
          }
        >
          {busy === "backup" ? "Backing up…" : "Back up now"}
        </button>
      </div>
      <div className="row">
        <button type="button" className="btn ghost small" onClick={() => api.openDbBackups(database).catch((e) => setError(errorText(e)))}>
          {t("Show backups folder")}
        </button>
      </div>
      {busy === "backup" && <Progress label={t("Backing up {name}", { name: database })} state={{ text: t("Saving {name}…", { name: database }), percent: null }} />}
      {error && (
        <p className="error small" role="alert">
          {error}
        </p>
      )}
      {done && (
        <p className="status ok" role="status">
          {done}
          {saved && (
            <>
              {" "}
              {t("Saved as")} <span className="mono">{saved}</span>
            </>
          )}
        </p>
      )}

      {mine.length === 0 ? (
        <p className="muted">No backups of {database} yet.</p>
      ) : (
        mine.map((b) => (
          <div key={b.file} className="version-row">
            <div className="stack tight">
              <span className="mono small">{b.file}</span>
              <span className="muted small">
                {new Date(b.created).toLocaleString()} · {size(b.size)}
              </span>
            </div>
            {confirm === b.file ? (
              <div className="row wrap">
                <button
                  type="button"
                  className="btn small danger"
                  disabled={busy !== null}
                  onClick={() =>
                    attempt(
                      b.file,
                      async () => onChange(await api.dbRestore(database, b.file, null, kind)),
                      `Restored ${b.file} into ${database}.`,
                    )
                  }
                >
                  {busy === b.file ? "Restoring…" : `Replace tables in ${database}`}
                </button>
                <span className="row">
                  <input
                    aria-label="Name of the copy"
                    className="mono"
                    value={copyName}
                    onChange={(e) => setCopyName(e.target.value)}
                  />
                  <button
                    type="button"
                    className="btn small"
                    disabled={busy !== null || !copyName.trim()}
                    onClick={() =>
                      attempt(
                        b.file,
                        async () => {
                          onChange(await api.dbRestore(database, b.file, copyName.trim(), kind));
                          onCopied(copyName.trim());
                        },
                        `Restored ${b.file} into ${copyName.trim()}.`,
                      )
                    }
                  >
                    Restore as copy
                  </button>
                </span>
                <button type="button" className="btn ghost small" onClick={() => setConfirm(null)}>
                  Cancel
                </button>
              </div>
            ) : (
              <div className="row">
                <button type="button" className="btn small" onClick={() => setConfirm(b.file)}>
                  Restore…
                </button>
                <button
                  type="button"
                  className="btn ghost small"
                  onClick={() =>
                    attempt(b.file, async () => setAll(await api.dbRemoveBackup(database, b.file)), "Backup deleted.")
                  }
                >
                  Delete
                </button>
              </div>
            )}
          </div>
        ))
      )}

      <div className="stack tight danger-zone">
        <h3 className="subhead">Drop {database}</h3>
        <p className="muted small">Deletes the database and everything in it. Type its name to confirm.</p>
        <div className="row wrap">
          <input
            aria-label={`Type ${database} to confirm`}
            className="mono"
            value={dropText}
            placeholder={database}
            onChange={(e) => setDropText(e.target.value)}
          />
          <button
            type="button"
            className="btn small danger"
            disabled={dropText !== database || busy !== null}
            onClick={() => attempt("drop", async () => onChange(await api.dbDrop(database, kind)), `Dropped ${database}.`)}
          >
            Drop database
          </button>
        </div>
      </div>
    </div>
  );
}

function Grid({ set }: { set: ResultSet }) {
  if (set.columns.length === 0) return <p className="muted small">No rows.</p>;
  return (
    <div className="stack tight">
      <div className="grid-wrap">
        <table className="result-grid">
          <thead>
            <tr>
              {set.columns.map((c, i) => (
                <th key={i} className="mono">
                  {c}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {set.rows.map((row, r) => (
              <tr key={r}>
                {row.map((cell, i) => (
                  <td key={i} className="mono" title={cell ?? "NULL"}>
                    {cell === null ? <span className="muted null">NULL</span> : cell}
                  </td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <span className="muted small">
        {set.rows.length.toLocaleString()} {set.rows.length === 1 ? "row" : "rows"}
        {set.truncated && " (more were left out)"}
      </span>
    </div>
  );
}

function size(bytes: number) {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${Math.ceil(bytes / 1024)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}
