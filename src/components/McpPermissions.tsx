import type { McpTool } from "../lib/api";

/** What AI tools may do through the MCP server, with a switch for each tool that is off until allowed. */
export function McpPermissions({ tools, onToggle }: { tools: McpTool[]; onToggle: (tool: McpTool) => void }) {
  const reads = tools.filter((t) => !t.action);
  const allowed = tools.filter((t) => t.action);
  return (
    <>
      <h3 className="subhead">Always allowed: looking</h3>
      {reads.map((t) => (
        <div key={t.name} className="version-row">
          <div className="stack tight">
            <div className="strong">{t.title}</div>
            <div className="muted small">{t.description}</div>
          </div>
        </div>
      ))}
      <h3 className="subhead">Off until you allow them</h3>
      {allowed.map((t) => (
        <div key={t.name} className="version-row">
          <div className="stack tight">
            <div className="strong">{t.title}</div>
            <div className="muted small">{t.description}</div>
          </div>
          <label className="switch">
            <input
              type="checkbox"
              role="switch"
              checked={t.allowed}
              onChange={() => onToggle(t)}
              aria-label={`Allow ${t.title}`}
            />
            <span>{t.allowed ? "On" : "Off"}</span>
          </label>
        </div>
      ))}
      <p className="muted small">
        Deleting sites, whole databases or files is never offered to AI tools. Change a database can change and delete
        rows and tables, so turn it on only if your databases hold nothing you can't lose.
      </p>
    </>
  );
}
