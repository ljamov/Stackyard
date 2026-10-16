import { useEffect, useLayoutEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { api, errorText, type LogFile } from "../lib/api";

/** Lines kept in view; older ones scroll away. */
const KEEP = 5000;
const FIRST_READ = 500;

type Matcher = { test: (line: string) => boolean; mark: (line: string) => ReactNode } | null;

export function Logs() {
  const [files, setFiles] = useState<LogFile[]>([]);
  const [current, setCurrent] = useState<string | null>(null);
  const [lines, setLines] = useState<string[]>([]);
  const [paused, setPaused] = useState(false);
  const [follow, setFollow] = useState(true);
  const [query, setQuery] = useState("");
  const [regex, setRegex] = useState(false);
  const [matchCase, setMatchCase] = useState(false);
  const [onlyMatches, setOnlyMatches] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [confirming, setConfirming] = useState(false);
  const [clearing, setClearing] = useState(false);
  // The log whose "Clear" was pressed once and waits for the second press.
  const [confirmingOne, setConfirmingOne] = useState<string | null>(null);
  // Bumped after clearing, so the open log is read again from the start.
  const [fresh, setFresh] = useState(0);
  const view = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const load = () => api.listLogs().then(setFiles).catch(() => {});
    load();
    const timer = setInterval(load, 5000);
    return () => clearInterval(timer);
  }, [fresh]);

  async function clearAll() {
    setClearing(true);
    setError(null);
    try {
      await api.clearLogs();
      setFresh((n) => n + 1);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setClearing(false);
      setConfirming(false);
    }
  }

  async function clearOne(id: string) {
    setClearing(true);
    setError(null);
    try {
      await api.clearLog(id);
      setFresh((n) => n + 1);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setClearing(false);
      setConfirmingOne(null);
    }
  }

  const selected = current ?? files[0]?.id ?? null;

  // Read the end of the log, then whatever is written to it, once a second.
  useEffect(() => {
    if (!selected) return;
    let offset: number | null = null;
    let stopped = false;
    setLines([]);
    setError(null);
    const read = async () => {
      try {
        const chunk = await api.readLog(selected, offset, FIRST_READ);
        if (stopped) return;
        const first = offset === null;
        offset = chunk.offset;
        if (chunk.reset) {
          setLines(["--- the log started over ---", ...chunk.lines]);
        } else if (first || chunk.lines.length > 0) {
          setLines((old) => (first ? chunk.lines : [...old, ...chunk.lines].slice(-KEEP)));
        }
        setError(null);
      } catch (e) {
        if (!stopped) setError(errorText(e));
      }
    };
    read();
    const timer = setInterval(() => {
      if (!paused) read();
    }, 1000);
    return () => {
      stopped = true;
      clearInterval(timer);
    };
  }, [selected, paused, fresh]);

  const matcher = useMemo(() => buildMatcher(query, regex, matchCase), [query, regex, matchCase]);
  const live = matcher && !("error" in matcher) ? matcher : null;
  const shown = live && onlyMatches ? lines.filter((l) => live.test(l)) : lines;
  const matches = live ? lines.filter((l) => live.test(l)).length : null;

  useLayoutEffect(() => {
    if (follow && view.current) view.current.scrollTop = view.current.scrollHeight;
  }, [shown.length, follow, selected]);

  function onScroll() {
    const el = view.current;
    if (!el) return;
    const atEnd = el.scrollHeight - el.scrollTop - el.clientHeight < 24;
    if (atEnd !== follow) setFollow(atEnd);
  }

  const name = files.find((f) => f.id === selected)?.name ?? selected;

  return (
    <>
      <header className="page-header">
        <div>
          <h1>Logs</h1>
          <p className="muted">Every service's log in one place, updated live. Search with plain text or a regular expression.</p>
        </div>
        {files.length > 0 &&
          (confirming ? (
            <div className="row centered">
              <span className="muted small">Empty every log and start from zero?</span>
              <button type="button" className="btn small" disabled={clearing} onClick={() => setConfirming(false)}>
                Cancel
              </button>
              <button type="button" className="btn danger small" disabled={clearing} onClick={clearAll}>
                {clearing ? "Clearing…" : "Clear all logs"}
              </button>
            </div>
          ) : (
            <button type="button" className="btn small" onClick={() => setConfirming(true)}>
              Clear all logs
            </button>
          ))}
      </header>

      {files.length === 0 ? (
        <section className="card empty">
          <p>No logs yet. They appear here once a service has run.</p>
        </section>
      ) : (
        <div className="split">
          <nav className="side card log-list" aria-label="Logs">
            {files.map((f) => (
              <button
                key={f.id}
                type="button"
                className="nav-item"
                aria-current={f.id === selected ? "true" : undefined}
                onClick={() => setCurrent(f.id)}
              >
                <span>{f.name}</span>
                <span className="muted small">{size(f.size)}</span>
              </button>
            ))}
          </nav>

          <section className="card stack log-pane" aria-label={`${name} log`}>
            <div className="row wrap log-tools">
              <div className="field grow">
                <label htmlFor="log-search">Search</label>
                <input
                  id="log-search"
                  className="mono"
                  value={query}
                  placeholder={regex ? "e.g. (error|warn).*php" : "e.g. 404"}
                  onChange={(e) => setQuery(e.target.value)}
                />
              </div>
              <label className="check">
                <input type="checkbox" checked={regex} onChange={(e) => setRegex(e.target.checked)} />
                Regular expression
              </label>
              <label className="check">
                <input type="checkbox" checked={matchCase} onChange={(e) => setMatchCase(e.target.checked)} />
                Match case
              </label>
              <label className="check">
                <input type="checkbox" checked={onlyMatches} onChange={(e) => setOnlyMatches(e.target.checked)} />
                Only matching lines
              </label>
            </div>

            <div className="row wrap between centered">
              <span className="muted small" aria-live="polite">
                {matcher && "error" in matcher ? (
                  <span className="error">{matcher.error}</span>
                ) : matches !== null ? (
                  `${matches} of ${lines.length} lines match`
                ) : (
                  `${lines.length} lines`
                )}
              </span>
              <div className="row">
                {!follow && (
                  <button type="button" className="btn ghost small" onClick={() => setFollow(true)}>
                    Jump to latest
                  </button>
                )}
                <button type="button" className="btn small" onClick={() => setPaused(!paused)} aria-pressed={paused}>
                  {paused ? "Resume" : "Pause"}
                </button>
                {selected &&
                  (confirmingOne === selected ? (
                    <>
                      <button type="button" className="btn small" disabled={clearing} onClick={() => setConfirmingOne(null)}>
                        Cancel
                      </button>
                      <button
                        type="button"
                        className="btn danger small"
                        disabled={clearing}
                        onClick={() => clearOne(selected)}
                      >
                        {clearing ? "Clearing…" : `Empty the ${name} log`}
                      </button>
                    </>
                  ) : (
                    <button
                      type="button"
                      className="btn small"
                      title={`Empties only the ${name} log; other logs stay as they are`}
                      onClick={() => setConfirmingOne(selected)}
                    >
                      Clear this log
                    </button>
                  ))}
              </div>
            </div>

            {error && (
              <p className="error small" role="alert">
                {error}
              </p>
            )}

            <div ref={view} className="code mono log log-view" onScroll={onScroll} tabIndex={0} aria-label="Log lines">
              {shown.length === 0 ? (
                <span className="muted">{lines.length === 0 ? "This log is empty." : "No lines match."}</span>
              ) : (
                shown.map((line, i) => (
                  <div key={i} className={`log-line ${level(line)}`}>
                    {live ? live.mark(line) : line}
                  </div>
                ))
              )}
            </div>
          </section>
        </div>
      )}
    </>
  );
}

function buildMatcher(query: string, regex: boolean, matchCase: boolean): Matcher | { error: string } {
  if (!query) return null;
  let pattern: RegExp;
  try {
    const source = regex ? query : query.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
    pattern = new RegExp(source, matchCase ? "g" : "gi");
  } catch {
    return { error: "That is not a valid regular expression." };
  }
  return {
    test: (line) => {
      pattern.lastIndex = 0;
      return pattern.test(line);
    },
    mark: (line) => {
      const parts: ReactNode[] = [];
      let last = 0;
      pattern.lastIndex = 0;
      for (const m of line.matchAll(pattern)) {
        if (m[0] === "") break;
        parts.push(line.slice(last, m.index), <mark key={m.index}>{m[0]}</mark>);
        last = m.index + m[0].length;
      }
      parts.push(line.slice(last));
      return parts;
    },
  };
}

/** Colours a line by how serious it looks. */
function level(line: string) {
  const l = line.toLowerCase();
  if (/\b(error|fatal|panic|critical|emerg)\b/.test(l)) return "error";
  if (/\bwarn(ing)?\b/.test(l)) return "warn";
  return "";
}

function size(bytes: number) {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${Math.ceil(bytes / 1024)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}
