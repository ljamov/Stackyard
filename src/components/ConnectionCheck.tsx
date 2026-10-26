import { useState } from "react";
import { errorText, type ConnectionReport, type ConnectionStage, type ConnectionWhose } from "../lib/api";
import { useT } from "../lib/i18n";

const MARK = { ok: "✓", failed: "✕", warning: "!", skipped: "–" } as const;

/**
 * "Check connection": connects one step at a time and shows where it
 * stops, whose side the problem is on, and what to try.
 */
export function ConnectionCheck({
  run,
  onTrust,
  onForgetKey,
}: {
  run: () => Promise<ConnectionReport>;
  /** Trusts an FTPS certificate the system does not. */
  onTrust?: (fingerprint: string) => void;
  onForgetKey?: () => void;
}) {
  const t = useT();
  const stages: Record<ConnectionStage, string> = {
    find: t("Find the server"),
    reach: t("Reach the port"),
    handshake: t("Secure handshake"),
    login: t("Log in"),
    session: t("Open a session"),
    files: t("Open the files"),
    commands: t("Run commands"),
  };
  const whose: Record<ConnectionWhose, string> = {
    internet: t("Internet or firewall"),
    server: t("Server"),
    access: t("Login or settings"),
    stackyard: "Stackyard",
  };
  const [report, setReport] = useState<ConnectionReport | "checking" | null>(null);
  const [error, setError] = useState<string | null>(null);

  async function check() {
    setReport("checking");
    setError(null);
    try {
      setReport(await run());
    } catch (e) {
      setReport(null);
      setError(errorText(e));
    }
  }

  const failed = report && report !== "checking" ? report.steps.find((s) => s.status === "failed") : undefined;
  return (
    <div className="stack tight">
      <div className="row wrap centered">
        <button type="button" className="btn small" disabled={report === "checking"} onClick={check}>
          {report === "checking" ? t("Checking…") : t("Check connection")}
        </button>
        {!report && <span className="muted small">{t("Tries each step and shows where it stops and what to do.")}</span>}
      </div>
      {error && <p className="error small">{error}</p>}
      {report && report !== "checking" && (
        <>
          <ol className="conn-check" aria-label={t("Connection check")}>
            {report.steps.map((s) => (
              <li key={s.stage} className={`conn-step ${s.status}`}>
                <span className="conn-mark" aria-hidden="true">
                  {MARK[s.status]}
                </span>
                <div className="stack tight grow">
                  <span>
                    <strong>{stages[s.stage]}</strong>
                    {s.whose && s.status !== "ok" && <span className="conn-whose">{whose[s.whose]}</span>}
                  </span>
                  {s.detail && <span className="small">{s.detail}</span>}
                  {s.fix && <span className="muted small">{s.fix}</span>}
                </div>
              </li>
            ))}
          </ol>
          {!failed && <p className="ok small">{t("Everything works.")}</p>}
          {report.certificate && onTrust && (
            <div className="row wrap centered">
              <button type="button" className="btn small" onClick={() => onTrust(report.certificate!)}>
                {t("Trust this certificate")}
              </button>
            </div>
          )}
          {report.key_changed && onForgetKey && (
            <div className="row wrap centered">
              <span className="muted small">{t("Only do this if you know the server was rebuilt or moved.")}</span>
              <button type="button" className="btn small danger" onClick={onForgetKey}>
                {t("Trust the new key")}
              </button>
            </div>
          )}
        </>
      )}
    </div>
  );
}
