import { useEffect, useState } from "react";
import { api, errorText, type LivePathsFixed, type LivePathsReport, type PageCheck, type Site } from "../lib/api";
import { useT } from "../lib/i18n";
import { useBusy } from "../lib/tabLife";

/** Home page checks already made this session, by domain: one per site is enough. */
const checked = new Map<string, PageCheck>();

function seconds(ms: number) {
  return (ms / 1000).toFixed(1);
}

/**
 * Live-server paths in a copied PHP site: a `.user.ini` (or `php.ini`,
 * `.htaccess`) setting such as Wordfence's `auto_prepend_file` naming a
 * file only the live server has, which stops PHP before the site runs.
 * Shows a banner with Fix it when Stackyard finds one, or when the home
 * page fails to open a file this computer doesn't have; then the result
 * and Undo. Shows nothing for a site without such leftovers.
 */
export function LivePaths({ site }: { site: Site }) {
  const t = useT();
  const php = site.runtime.startsWith("PHP") && !site.problem;
  const [report, setReport] = useState<LivePathsReport | null>(null);
  const [check, setCheck] = useState<PageCheck | null>(checked.get(site.domain) ?? null);
  const [fixed, setFixed] = useState<LivePathsFixed | null>(null);
  const [undone, setUndone] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  useBusy(busy !== null);

  async function look(recheck: boolean) {
    const found = await api.siteLivePaths(site.domain);
    setReport(found);
    if (recheck || !checked.has(site.domain)) {
      // A stopped web server or a site without index.php just isn't checked.
      const page = await api.checkSitePage(site.domain).catch(() => null);
      if (page) checked.set(site.domain, page);
      setCheck(page);
    }
  }

  useEffect(() => {
    setFixed(null);
    setUndone(false);
    setCheck(checked.get(site.domain) ?? null);
    if (php) look(false).catch((e) => setError(errorText(e)));
    // Only when the site changes; Check again is the button's job.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [site.domain, php]);

  if (!php || !report) return null;

  async function run(label: string, work: () => Promise<void>) {
    setBusy(label);
    setError(null);
    try {
      await work();
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(null);
    }
  }

  const fix = () =>
    run(t("Switching off the live-server paths, restarting PHP and opening the home page…"), async () => {
      const done = await api.fixLivePaths(site.domain);
      setFixed(done);
      setUndone(false);
      if (done.check) {
        checked.set(site.domain, done.check);
        setCheck(done.check);
      }
      setReport(await api.siteLivePaths(site.domain));
    });

  const undo = () =>
    run(t("Putting the files back…"), async () => {
      await api.undoLivePaths(site.domain);
      setFixed(null);
      setUndone(true);
      checked.delete(site.domain);
      setCheck(null);
      setReport(await api.siteLivePaths(site.domain));
    });

  const findings = report.findings;
  const pageMissing = check && !check.ok ? check.missing : null;
  const pageError = check && !check.ok ? check.error : null;
  const leftovers = report.leftovers;
  const changed = report.changed;
  const fixable = findings.length > 0;

  const details = (findings.length > 0 || leftovers.length > 0) && (
    <details>
      <summary>{t("What Stackyard found")}</summary>
      <ul className="banner-errors small">
        {findings.map((f) => (
          <li key={`${f.file}:${f.line}`}>
            <span className="mono">
              {f.file}:{f.line}
            </span>{" "}
            <span className="mono">
              {f.directive} = {f.value}
            </span>
            <br />
            <span className="muted">
              {f.apache_only
                ? t("{path} isn't on this computer. Only Apache reads this file; it is switched off too, so the copy stays tidy.", { path: f.missing })
                : t("{path} isn't on this computer, so PHP stops before the site runs.", { path: f.missing })}
            </span>
          </li>
        ))}
        {leftovers.map((l, i) => (
          <li key={`left-${l.file}-${i}`}>
            <span className="mono">{l.file}</span> · <span className="muted">{t("left as it is")}</span>
            <br />
            <span className="muted">{l.text}</span>
          </li>
        ))}
      </ul>
    </details>
  );

  const status = busy ? (
    <p className="small" role="status">
      {busy}
    </p>
  ) : error ? (
    <p className="error small" role="alert">
      {error}
    </p>
  ) : null;

  // Found: the banner with Fix it.
  if (fixable || pageMissing) {
    return (
      <section className="banner warn" aria-labelledby={`live-paths-${site.domain}`}>
        <p id={`live-paths-${site.domain}`}>
          <strong>{t("This site references a file from its live server.")}</strong>
        </p>
        <p className="small">
          {fixable
            ? t("{file} sets {directive} to {path}, which only the live server has. Fix it keeps the file as it was ({file}.live, put back by Undo), switches the line off, restarts PHP and opens the home page.", {
                file: findings[0].file,
                directive: findings[0].directive,
                path: findings[0].missing,
              })
            : t("The home page stops with: {error}. Stackyard didn't find the setting naming it in the site's .user.ini, php.ini or .htaccess; look in the PHP settings of the site's folders.", { error: pageError ?? "" })}
        </p>
        {fixable && pageError && (
          <p className="small mono">
            {t("Home page now: {error}", { error: pageError })}
          </p>
        )}
        {details}
        <div className="row wrap">
          {fixable && (
            <button type="button" className="btn small primary" disabled={busy !== null} onClick={() => void fix()}>
              {t("Fix it")}
            </button>
          )}
          <button type="button" className="btn small" disabled={busy !== null} onClick={() => void run(t("Checking…"), () => look(true))}>
            {t("Check again")}
          </button>
        </div>
        {status}
      </section>
    );
  }

  // Just fixed: what changed and how the home page answers now.
  if (fixed) {
    const page = fixed.check;
    const good = page?.ok ?? false;
    return (
      <section className={`banner ${good ? "ok" : "warn"}`} aria-live="polite">
        <p>
          <strong>{good ? t("Fixed: the home page works.") : t("The live-server paths are switched off.")}</strong>
        </p>
        <p className="small">
          {t("Changed {files}; the files as they were are kept as {backups}.", {
            files: fixed.changed.map((c) => c.file).join(", "),
            backups: fixed.changed.map((c) => c.backup).join(", "),
          })}{" "}
          {fixed.restarted ? t("PHP restarted, so the change counts now.") : t("PHP wasn't running; the change counts when the web server starts.")}{" "}
          {page
            ? page.ok
              ? t("The home page answered {status} in {seconds} s.", { status: String(page.status), seconds: seconds(page.millis) })
              : t("The home page still answers {status}: {error}", { status: String(page.status), error: page.error ?? "" })
            : fixed.check_error
              ? t("The home page wasn't checked: {error}.", { error: fixed.check_error })
              : ""}
        </p>
        <p className="small muted">{t("Push and pull leave these files out, so the live server keeps its own settings and its firewall.")}</p>
        <div className="row wrap">
          <button type="button" className="btn small" disabled={busy !== null} onClick={() => void undo()}>
            {t("Undo")}
          </button>
        </div>
        {status}
      </section>
    );
  }

  // Fixed before, or a PHP error on the home page: a quieter note.
  if (changed.length > 0 || pageError || undone) {
    return (
      <section className="notice inset small" aria-live="polite">
        {pageError && (
          <p className="error small">
            {t("The home page answers {status}: {error}", { status: String(check?.status ?? ""), error: pageError })}
          </p>
        )}
        {changed.length > 0 && (
          <p>
            {t("Live-server paths are switched off in {files} for this computer (originals kept as {backups}). Push and pull leave them out, so the live server keeps its own.", {
              files: changed.map((c) => c.file).join(", "),
              backups: changed.map((c) => c.backup).join(", "),
            })}
          </p>
        )}
        {undone && changed.length === 0 && <p>{t("Put back as it was. PHP restarted.")}</p>}
        {details}
        <div className="row wrap">
          {changed.length > 0 && (
            <button type="button" className="btn small" disabled={busy !== null} onClick={() => void undo()}>
              {t("Undo")}
            </button>
          )}
          {pageError && (
            <button type="button" className="btn small" disabled={busy !== null} onClick={() => void run(t("Checking…"), () => look(true))}>
              {t("Check again")}
            </button>
          )}
        </div>
        {status}
      </section>
    );
  }

  // Only harmless leftovers.
  if (leftovers.length > 0) {
    return <div className="small">{details}</div>;
  }
  return null;
}
