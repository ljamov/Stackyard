import { useEffect, useState } from "react";
import { api, errorText, type AiTool, type Site } from "../lib/api";
import { useLoad } from "../lib/useLoad";
import { useModules } from "../lib/modules";

const FOLDER_KEY = "stackyard.ai.folder";

function savedFolder(): string {
  try {
    return localStorage.getItem(FOLDER_KEY) ?? "";
  } catch {
    return "";
  }
}

export function AiTools() {
  const { isOn } = useModules();
  const sites = useLoad<Site[]>(api.listSites, []);
  const [tools, setTools] = useState<AiTool[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [errors, setErrors] = useState<Record<string, string>>({});
  const [progress, setProgress] = useState<Record<string, string>>({});
  const [checking, setChecking] = useState(false);
  const [folder, setFolder] = useState(savedFolder);
  const [opened, setOpened] = useState<string | null>(null);

  useEffect(() => {
    // The list first (instant), then the newest versions from npm.
    api
      .aiTools()
      .then(setTools)
      .catch((e) => setError(errorText(e)));
    void check();
    let unsubscribe: (() => void) | undefined;
    let cancelled = false;
    api
      .onAiProgress((p) => setProgress((all) => ({ ...all, [p.id]: p.text })))
      .then((off) => (cancelled ? off() : (unsubscribe = off)));
    return () => {
      cancelled = true;
      unsubscribe?.();
    };
  }, []);

  // Default to the first site once sites are known.
  const choices = sites.data.map((s) => ({ label: s.domain, path: s.path }));
  const chosen = folder || choices[0]?.path || "";
  const chosenLabel = choices.find((c) => c.path === chosen)?.label ?? chosen;

  function choose(path: string) {
    setFolder(path);
    try {
      localStorage.setItem(FOLDER_KEY, path);
    } catch {
      // Remembering the folder is a convenience only.
    }
  }

  async function check() {
    setChecking(true);
    try {
      setTools(await api.checkAiTools());
      setError(null);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setChecking(false);
    }
  }

  async function act(tool: AiTool, action: () => Promise<AiTool[] | void>) {
    setErrors(({ [tool.id]: _, ...rest }) => rest);
    setOpened(null);
    try {
      const next = await action();
      if (next) {
        // Keep the newest versions looked up before; an update clears its badge.
        setTools(
          next.map((t) => {
            const old = tools.find((o) => o.id === t.id);
            const update = !!old?.update && t.installed === old.installed;
            return { ...t, latest: t.latest ?? old?.latest ?? null, update };
          }),
        );
      }
    } catch (e) {
      setErrors((all) => ({ ...all, [tool.id]: errorText(e) }));
    } finally {
      setProgress(({ [tool.id]: _, ...rest }) => rest);
    }
  }

  function install(tool: AiTool) {
    setTools((all) => all.map((t) => (t.id === tool.id ? { ...t, installing: true } : t)));
    return act(tool, () => api.installAiTool(tool.id));
  }

  async function open(tool: AiTool) {
    if (!chosen) {
      setErrors((all) => ({ ...all, [tool.id]: "Pick a site or folder to open it in first." }));
      return;
    }
    await act(tool, () => api.openAiTool(tool.id, chosen));
    setOpened(tool.id);
  }

  async function pickFolder() {
    const picked = await api.pickFolder("Open AI tools in this folder", chosen || undefined);
    if (picked) choose(picked);
  }

  const installed = tools.filter((t) => t.installed);

  return (
    <>
      <header className="page-header">
        <div>
          <h1>AI tools</h1>
          <p className="muted">
            Install AI coding assistants on Stackyard&apos;s own Node.js, keep them up to date, and start one in a
            site&apos;s folder with one click.
          </p>
        </div>
        <button type="button" className="btn small" disabled={checking} onClick={() => void check()}>
          {checking ? "Checking…" : "Check for updates"}
        </button>
      </header>

      {error && (
        <p className="error" role="alert">
          {error}
        </p>
      )}

      <section aria-labelledby="ai-where" className="card stack">
        <h2 id="ai-where">Open in</h2>
        <div className="row wrap">
          <div className="field grow">
            <label htmlFor="ai-folder">Site or folder</label>
            <select id="ai-folder" value={chosen} onChange={(e) => choose(e.target.value)}>
              {!chosen && <option value="">No sites yet: choose a folder</option>}
              {choices.map((c) => (
                <option key={c.path} value={c.path}>
                  {c.label}
                </option>
              ))}
              {chosen && !choices.some((c) => c.path === chosen) && <option value={chosen}>{chosen}</option>}
            </select>
          </div>
          <button type="button" className="btn small" onClick={() => void pickFolder()}>
            Choose folder…
          </button>
        </div>
        <p className="muted small">
          <strong>Open</strong> starts the tool in a new terminal window in <span className="mono">{chosenLabel || "…"}</span>.
          The first time, the tool asks you to sign in.
        </p>
      </section>

      <section aria-label="AI coding tools" className="card">
        {tools.map((tool) => (
          <ToolRow
            key={tool.id}
            tool={tool}
            progress={progress[tool.id]}
            error={errors[tool.id]}
            opened={opened === tool.id ? chosenLabel : null}
            onInstall={() => void install(tool)}
            onRemove={() => void act(tool, () => api.removeAiTool(tool.id))}
            onOpen={() => void open(tool)}
            onDocs={() => void api.openAiToolDocs(tool.id)}
          />
        ))}
      </section>

      <p className="card dashed muted">
        {isOn("shell") ? (
          <>
            With <strong>Use in your terminal</strong> on (Runtimes screen), you can also type{" "}
            {installed.length > 0 ? (
              installed.map((t, i) => (
                <span key={t.id}>
                  {i > 0 && ", "}
                  <span className="mono">{t.command}</span>
                </span>
              ))
            ) : (
              <span className="mono">claude</span>
            )}{" "}
            in any new terminal.
          </>
        ) : (
          <>Turn on Stackyard Shell in Settings to also start these tools by typing their name in any terminal.</>
        )}{" "}
        Each tool runs on the Node.js it was installed with, whatever version a project pins.
      </p>
    </>
  );
}

type RowProps = {
  tool: AiTool;
  progress?: string;
  error?: string;
  opened: string | null;
  onInstall(): void;
  onRemove(): void;
  onOpen(): void;
  onDocs(): void;
};

function ToolRow({ tool, progress, error, opened, onInstall, onRemove, onOpen, onDocs }: RowProps) {
  return (
    <div className="version-row">
      <div className="stack tight">
        <div className="row wrap">
          <span className="strong">{tool.name}</span>
          <span className="muted small">{tool.maker}</span>
          {tool.installing ? (
            <span className="muted small">{progress ?? "Starting…"}</span>
          ) : tool.installed ? (
            <span className="small">
              <span className="ok">Installed</span> <span className="mono muted">{tool.installed}</span>
            </span>
          ) : (
            <span className="muted small">Not installed</span>
          )}
          {tool.update && !tool.installing && <span className="badge">Update {tool.latest}</span>}
        </div>
        <span className="muted small">
          {tool.about} Command: <span className="mono">{tool.command}</span>
          {tool.node && <> · Node.js {tool.node}</>}{" "}
          <button type="button" className="link" onClick={onDocs} aria-label={`${tool.name} guide`}>
            Guide
          </button>
        </span>
        {tool.installing && (
          <div className="progress" role="progressbar" aria-label={`Installing ${tool.name}`}>
            <div className="progress-fill busy" style={{ width: "30%" }} />
          </div>
        )}
        {opened && (
          <p className="small">
            <span className="ok">Opened</span> in a new terminal window in <span className="mono">{opened}</span>.
          </p>
        )}
        {error && (
          <p role="alert" className="error small">
            {error}
          </p>
        )}
      </div>
      <div className="row">
        {tool.installed && !tool.installing && (
          <>
            <button type="button" className="btn small" onClick={onRemove} aria-label={`Remove ${tool.name}`}>
              Remove
            </button>
            {tool.update && (
              <button type="button" className="btn small" onClick={onInstall} aria-label={`Update ${tool.name}`}>
                Update
              </button>
            )}
            <button type="button" className="btn primary small" onClick={onOpen} aria-label={`Open ${tool.name}`}>
              Open
            </button>
          </>
        )}
        {(!tool.installed || tool.installing) && (
          <button
            type="button"
            className="btn primary small"
            disabled={tool.installing}
            onClick={onInstall}
            aria-label={`Install ${tool.name}`}
          >
            {tool.installing ? "Installing…" : "Install"}
          </button>
        )}
      </div>
    </div>
  );
}
