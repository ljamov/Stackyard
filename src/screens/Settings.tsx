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
import { setCodeEditor, useCodeEditors } from "../lib/editors";
import { appLogPath, openAppLog } from "../lib/applog";

const GROUPS: { group: Module["group"]; title: string }[] = [
  { group: "runtime", title: "Runtimes" },
  { group: "service", title: "Services" },
  { group: "feature", title: "Features" },
];

export function Settings() {
  const t = useT();
  const { modules, setModule } = useModules();
  const editors = useCodeEditors();
  const editorInUse = editors?.list.find((e) => e.id === editors.current);
  const tldInfo = useLoad(api.getTld, { tld: ".stack", warning: null });
  const [tldDraft, setTldDraft] = useState(".stack");
  const [error, setError] = useState<string | null>(null);
  const [logPath, setLogPath] = useState<string | null>(null);

  useEffect(() => {
    appLogPath()
      .then(setLogPath)
      .catch(() => setLogPath(null));
  }, []);

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
        <div className="row wrap">
          <button type="button" className="btn" onClick={() => openTab("Keyboard shortcuts")}>
            {t("Keyboard shortcuts")}
          </button>
        </div>
      </section>

      <section aria-labelledby="files-h" className="card stack">
        <h2 id="files-h">{t("Settings files")}</h2>
        <p className="muted">
          {t("Everything on this page is also kept in settings.toml, and your shortcuts in keybindings.toml. Change them in any text editor; Stackyard picks the changes up within a few seconds, and lists anything it can't use under Problems.")}
        </p>
        <div className="row wrap">
          <button type="button" className="btn" onClick={() => void api.showInterfaceFile("settings")}>
            {t("Show {file} in the file manager", { file: "settings.toml" })}
          </button>
          <button type="button" className="btn" onClick={() => void api.showInterfaceFile("keybindings")}>
            {t("Show {file} in the file manager", { file: "keybindings.toml" })}
          </button>
        </div>
      </section>

      {logPath && (
        <section aria-labelledby="log-h" className="card stack">
          <h2 id="log-h">{t("Log file")}</h2>
          <p className="muted">
            {t("Stackyard writes what it does while starting, and any errors, to this file. If the window or a screen misbehaves, it shows what went wrong.")}
          </p>
          <p className="mono small">{logPath}</p>
          <div className="row wrap">
            <button type="button" className="btn" onClick={() => openAppLog().catch((e) => setError(errorText(e)))}>
              {t("Open log file")}
            </button>
          </div>
        </section>
      )}

      <section aria-labelledby="editor-h" className="card stack">
        <h2 id="editor-h">{t("Code editor")}</h2>
        <div className="field narrow">
          <label htmlFor="s-editor">{t("Open in editor on the Sites screen uses")}</label>
          <select
            id="s-editor"
            value={editors?.current ?? "vscode"}
            disabled={!editors}
            onChange={(e) => {
              setCodeEditor(e.target.value)
                .then(() => setError(null))
                .catch((err) => setError(errorText(err)));
            }}
          >
            {editors?.list.map((e) => (
              <option key={e.id} value={e.id}>
                {e.installed ? e.name : t("{name} (not installed)", { name: e.name })}
              </option>
            ))}
          </select>
          {editorInUse && !editorInUse.installed ? (
            <span className="warn small">{t("{name} isn't installed on this computer.", { name: editorInUse.name })}</span>
          ) : (
            <span className="muted small">{t("VS Code is used unless you pick another one.")}</span>
          )}
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
