import type { McpTool } from "../lib/api";
import { useT } from "../lib/i18n";

/** What AI tools may do through the MCP server, with a switch for each tool that is off until allowed. */
export function McpPermissions({ tools, onToggle }: { tools: McpTool[]; onToggle: (tool: McpTool) => void }) {
  const t = useT();
  const reads = tools.filter((x) => !x.action);
  const allowed = tools.filter((x) => x.action);
  return (
    <>
      <h3 className="subhead">{t("Always allowed: looking")}</h3>
      {reads.map((tool) => (
        <div key={tool.name} className="version-row">
          <div className="stack tight">
            <div className="strong">{t(tool.title)}</div>
            <div className="muted small">{tool.description}</div>
          </div>
        </div>
      ))}
      <h3 className="subhead">{t("Off until you allow them")}</h3>
      {allowed.map((tool) => (
        <div key={tool.name} className="version-row">
          <div className="stack tight">
            <div className="strong">{t(tool.title)}</div>
            <div className="muted small">{tool.description}</div>
          </div>
          <label className="switch">
            <input
              type="checkbox"
              role="switch"
              checked={tool.allowed}
              onChange={() => onToggle(tool)}
              aria-label={t("Allow {name}", { name: t(tool.title) })}
            />
            <span>{tool.allowed ? t("On") : t("Off")}</span>
          </label>
        </div>
      ))}
      <p className="muted small">
        {t(
          "Deleting sites, whole databases or files is never offered to AI tools. Change a database can change and delete rows and tables, so turn it on only if your databases hold nothing you can't lose.",
        )}
      </p>
    </>
  );
}
