import { useEffect, useState } from "react";
import { DiskSpace } from "../components/DiskSpace";
import { McpPermissions } from "../components/McpPermissions";
import { Updates } from "../components/Updates";
import { api, errorText, type McpStatus, type McpTool, type Module } from "../lib/api";
import { useModules } from "../lib/modules";
import { useLoad } from "../lib/useLoad";
import { useT } from "../lib/i18n";
import { AppearanceSettings } from "../components/AppearanceSettings";
import { openTab } from "../lib/layout";

const GROUPS: { group: Module["group"]; title: string }[] = [
  { group: "runtime", title: "Runtimes" },
  { group: "service", title: "Services" },
  { group: "feature", title: "Features" },
];

export function Settings() {
  const t = useT();
  const { modules, setModule } = useModules();
  const tldInfo = useLoad(api.getTld, { tld: ".stack", warning: null });
  const [tldDraft, setTldDraft] = useState(".stack");
  const [error, setError] = useState<string | null>(null);

  useEffect(() => setTldDraft(tldInfo.data.tld), [tldInfo.data.tld]);

  const mcpOn = modules.some((m) => m.id === "mcp" && m.enabled);
  const [mcp, setMcp] = useState<McpStatus | null>(null);
  useEffect(() => {
    if (!mcpOn) return;
    api
      .mcpStatus()
      .then(setMcp)
      .catch((e) => setError(errorText(e)));
  }, [mcpOn]);

  async function toggleMcpTool(tool: McpTool) {
    try {
      setMcp(await api.setMcpTool(tool.name, !tool.allowed));
      setError(null);
    } catch (e) {
      setError(errorText(e));
    }
  }

  async function saveTld() {
    if (tldDraft === tldInfo.data.tld) return;
    try {
      await api.setTld(tldDraft);
      setError(null);
    } catch (e) {
      setError(errorText(e));
    }
    tldInfo.reload();
  }

  async function toggle(m: Module) {
    try {
      await setModule(m.id, !m.enabled);
      setError(null);
    } catch (e) {
      setError(errorText(e));
    }
  }

  return (
    <>
      <header className="page-header">
        <div>
          <h1>{t("Settings")}</h1>
          <p className="muted">{t("Switch off anything you don't use. Modules that are off download and run nothing.")}</p>
        </div>
      </header>

      {error && (
        <p className="error" role="alert">
          {error}
        </p>
      )}

      <Updates />

      <AppearanceSettings />

      <section aria-labelledby="keys-h" className="card stack">
        <h2 id="keys-h">{t("Keyboard shortcuts")}</h2>
        <p className="muted">{t("Change the keys for any command. Click Change, then press the new keys.")}</p>
        <div>
          <button type="button" className="btn" onClick={() => openTab("Keyboard shortcuts")}>
            {t("Keyboard shortcuts")}
          </button>
        </div>
      </section>

      <section aria-labelledby="dom-h" className="card stack">
        <h2 id="dom-h">{t("Local domains")}</h2>
        <div className="field narrow">
          <label htmlFor="s-tld">{t("Domain ending for new sites")}</label>
          <input
            id="s-tld"
            className="mono"
            value={tldDraft}
            onChange={(e) => setTldDraft(e.target.value)}
            onBlur={saveTld}
          />
          <span className="muted small">{t("Default is .stack. If .stack ever becomes a public domain, use .test.")}</span>
          {tldInfo.data.warning && <span className="warn small">{tldInfo.data.warning}</span>}
        </div>
      </section>

      {GROUPS.map(({ group, title }) => (
        <section key={group} aria-labelledby={`mod-${group}`} className="card">
          <h2 id={`mod-${group}`}>{t(title)}</h2>
          {modules
            .filter((m) => m.group === group)
            .map((m) => (
              <div key={m.id} className="version-row">
                <div>
                  <div className="strong">{t(m.name)}</div>
                  <div className="muted small">{t(m.description)}</div>
                </div>
                {m.available ? (
                  <label className="switch">
                    <input
                      type="checkbox"
                      role="switch"
                      checked={m.enabled}
                      onChange={() => toggle(m)}
                      aria-label={t("{name} module", { name: t(m.name) })}
                    />
                    <span>{m.enabled ? t("On") : t("Off")}</span>
                  </label>
                ) : (
                  <span className="muted small">{t("Coming soon")}</span>
                )}
              </div>
            ))}
        </section>
      ))}

      {mcpOn && mcp && (
        <section aria-labelledby="ai-h" className="card">
          <h2 id="ai-h">{t("AI tools")}</h2>
          <p className="muted small">
            {t(
              "What Claude Code, Cursor and other AI tools may do through the MCP server. See how to connect one, and what they did, on the MCP server screen.",
            )}
          </p>
          <McpPermissions tools={mcp.tools} onToggle={toggleMcpTool} />
        </section>
      )}

      <DiskSpace />
    </>
  );
}
