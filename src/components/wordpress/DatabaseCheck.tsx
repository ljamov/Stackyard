import { useEffect, useState } from "react";
import { errorText, type Site } from "../../lib/api";
import { useT } from "../../lib/i18n";
import { useBusy } from "../../lib/tabLife";
import { wp, type WpDbFinding, type WpDbReport } from "../../lib/wordpress";

const DOT: Record<WpDbFinding["status"], string> = { ok: "ok", fixed: "ok", fix: "warn", problem: "error" };

/**
 * "Check database": what is wrong with a WordPress site's database and its
 * wp-config.php (a copy still logging in to the live server, a missing
 * database, the wrong table prefix, IDs without AUTO_INCREMENT, roles under
 * another prefix, the live address…), and a Repair button that backs up and
 * fixes what it can. `auto` checks as soon as it shows, for a site that
 * can't be read.
 */
export function DatabaseCheck({ site, auto = false, onRepaired }: { site: Site; auto?: boolean; onRepaired?: () => void }) {
  const t = useT();
  const [report, setReport] = useState<WpDbReport | null>(null);
  const [repaired, setRepaired] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);
  useBusy(busy !== null);
  const [error, setError] = useState<string | null>(null);

  async function run(label: string, work: () => Promise<WpDbReport>, after?: () => void) {
    setBusy(label);
    setError(null);
    try {
      setReport(await work());
      after?.();
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(null);
    }
  }

  const check = () => run(t("Checking the database…"), () => wp.checkDatabase(site.domain), () => setRepaired(false));
  const repair = () =>
    run(t("Backing up, then repairing…"), () => wp.repairDatabase(site.domain), () => {
      setRepaired(true);
      onRepaired?.();
    });

  useEffect(() => {
    if (auto) check();
    // Only when the site changes; a re-check is the button's job.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [auto, site.domain]);

  const fixes = report?.findings.filter((f) => f.status === "fix").length ?? 0;
  const problems = report?.findings.filter((f) => f.status === "problem").length ?? 0;

  return (
    <section className="wp-card db-check" aria-labelledby={`db-check-h-${site.domain}`}>
      <div className="row between centered wrap">
        <h3 id={`db-check-h-${site.domain}`}>{t("Check and repair")}</h3>
        <div className="row wrap">
          <button type="button" className="btn small" disabled={busy !== null} onClick={check}>
            {report ? t("Check again") : t("Check database")}
          </button>
          {fixes > 0 && (
            <button type="button" className="btn small primary" disabled={busy !== null} onClick={repair}>
              {t("Repair {count} problems", { count: fixes })}
            </button>
          )}
        </div>
      </div>
      {!report && !busy && (
        <p className="muted small">
          {t(
            "For a copied or cloned site that shows database errors: checks that wp-config.php logs in to this computer's database, that the database and its tables are there with the right prefix, that IDs count up, that roles and user rights match, the site's address, damaged tables and old character sets. Nothing changes until you press Repair.",
          )}
        </p>
      )}
      {busy && (
        <p className="muted small" role="status">
          {busy}
        </p>
      )}
      {error && (
        <p className="error small" role="alert">
          {error}
        </p>
      )}
      {report && (
        <>
          <ul className="db-check-list" aria-label={t("What was checked")}>
            {report.findings.map((f, i) => (
              <li key={`${f.check}-${i}`} className="row tight-row">
                <span className={`dot ${DOT[f.status]}`} aria-hidden="true" />
                <span className="small">
                  <strong>{t(f.check)}</strong>
                  {f.status === "fixed" && <span className="muted"> · {t("fixed")}</span>}
                  {f.status === "problem" && <span className="muted"> · {t("needs you")}</span>}
                  <br />
                  <span className={f.status === "ok" ? "muted" : undefined}>{f.text}</span>
                </span>
              </li>
            ))}
          </ul>
          {!busy && (
            <p className={fixes + problems === 0 ? "notice small" : "muted small"} role="status">
              {repaired
                ? t("Repaired.") +
                  (report.backup ? " " + t("The database as it was is backed up as {file} (Databases screen).", { file: report.backup }) : "") +
                  (report.config_backup ? " " + t("The old wp-config.php is saved as {file}.", { file: report.config_backup }) : "")
                : fixes > 0
                  ? t("Repair backs up the database and wp-config.php first.")
                  : problems === 0
                    ? t("Everything checks out.")
                    : ""}
            </p>
          )}
        </>
      )}
    </section>
  );
}
