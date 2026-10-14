import { useCallback, useEffect, useState } from "react";
import { api, errorText, type McpActivity, type McpStatus, type McpTool } from "../lib/api";

export function McpServer() {
  const [status, setStatus] = useState<McpStatus | null>(null);
  const [error, setError] = useState<string | null>(null);

  const reload = useCallback(() => {
    api
      .mcpStatus()
      .then((s) => {
        setStatus(s);
        setError(null);
      })
      .catch((e) => setError(errorText(e)));
  }, []);

  // AI tools work while this screen is open; show what they do as they do it.
  useEffect(() => {
    reload();
    const timer = setInterval(reload, 5000);
    return () => clearInterval(timer);
  }, [reload]);

  async function toggle(tool: McpTool) {
    try {
      setStatus(await api.setMcpTool(tool.name, !tool.allowed));
      setError(null);
    } catch (e) {
      setError(errorText(e));
    }
  }

  const reads = status?.tools.filter((t) => !t.action) ?? [];
  const actions = status?.tools.filter((t) => t.action) ?? [];
  const titles = Object.fromEntries((status?.tools ?? []).map((t) => [t.name, t.title]));

  return (
    <>
      <header className="page-header">
        <div>
          <h1>MCP server</h1>
          <p className="muted">
            AI coding tools such as Claude Code and Cursor can see your sites, services and logs. They change something only
            with the actions you allow here.
          </p>
        </div>
      </header>

      {error && (
        <p className="error" role="alert">
          {error}
        </p>
      )}

      <section aria-labelledby="mcp-connect" className="card stack">
        <h2 id="mcp-connect">Connect an AI tool</h2>
        {status && !status.setup && (
          <p className="notice">
            The stackyard command-line program was not found next to the app, so AI tools have nothing to start. Reinstall
            Stackyard to get it back.
          </p>
        )}
        {status?.setup && (
          <>
            <Snippet label="Claude Code: run this in a terminal" text={status.setup.claude_code} />
            <Snippet
              label="Cursor, Claude Desktop, Windsurf and others: add this to the tool's MCP settings"
              text={status.setup.json}
            />
          </>
        )}
      </section>

      <section aria-labelledby="mcp-tools" className="card">
        <h2 id="mcp-tools">What AI tools may do</h2>
        <h3 className="subhead">Always allowed: looking</h3>
        {reads.map((t) => (
          <div key={t.name} className="version-row">
            <div className="stack tight">
              <div className="strong">{t.title}</div>
              <div className="muted small">{t.description}</div>
            </div>
          </div>
        ))}
        <h3 className="subhead">Actions: off until you allow them</h3>
        {actions.map((t) => (
          <div key={t.name} className="version-row">
            <div className="stack tight">
              <div className="strong">{t.title}</div>
              <div className="muted small">{t.description}</div>
            </div>
            <label className="switch">
              <input type="checkbox" checked={t.allowed} onChange={() => toggle(t)} aria-label={`Allow ${t.title}`} />
              <span>{t.allowed ? "On" : "Off"}</span>
            </label>
          </div>
        ))}
        <p className="muted small">Deleting sites, databases or files is never offered to AI tools.</p>
      </section>

      <section aria-labelledby="mcp-activity" className="card">
        <h2 id="mcp-activity">Recent activity</h2>
        {status && status.activity.length === 0 && <p className="muted">No AI tool has used Stackyard yet.</p>}
        {status?.activity.map((a, i) => <ActivityRow key={`${a.at}-${i}`} entry={a} title={titles[a.tool] ?? a.tool} />)}
      </section>
    </>
  );
}

function Snippet({ label, text }: { label: string; text: string }) {
  const [copied, setCopied] = useState(false);
  async function copy() {
    try {
      await navigator.clipboard.writeText(text);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch {
      setCopied(false);
    }
  }
  return (
    <div className="stack tight">
      <div className="row between">
        <span className="small muted">{label}</span>
        <button type="button" className="btn ghost small" onClick={copy}>
          {copied ? "Copied" : "Copy"}
        </button>
      </div>
      <pre className="code mono log">{text}</pre>
    </div>
  );
}

function ActivityRow({ entry, title }: { entry: McpActivity; title: string }) {
  const time = new Date(entry.at).toLocaleString();
  return (
    <div className="version-row">
      <div className="stack tight">
        <div className="row wrap">
          <span className="strong">{title}</span>
          {entry.summary && <span className="mono small">{entry.summary}</span>}
        </div>
        <div className="muted small">
          {entry.client} · <time dateTime={new Date(entry.at).toISOString()}>{time}</time>
        </div>
        {entry.error && <div className="error small">{entry.error}</div>}
      </div>
      <span className={entry.ok ? "status ok" : "status error"}>
        <span className={entry.ok ? "dot ok" : "dot error"} aria-hidden="true" />
        {entry.ok ? "Done" : "Refused or failed"}
      </span>
    </div>
  );
}
