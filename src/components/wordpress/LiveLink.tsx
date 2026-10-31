import { useCallback, useEffect, useState } from "react";
import { api, errorText } from "../../lib/api";
import { useT } from "../../lib/i18n";
import { wp, type WpLiveLink, type WpWay } from "../../lib/wordpress";
import { ConnectionCheck } from "../ConnectionCheck";
import { useBusy } from "../../lib/tabLife";

/** A push or pull error that a new connection code would fix. */
export function codeProblem(error: string) {
  return error.includes("not valid any more") || error.includes("can copy the site but not change it");
}

/**
 * The top of the Live site tab: how this copy reaches the live site, and
 * whether it still can. A Stackyard Connect code that stopped working, or
 * one that can't push, gets a box to paste a new one right here.
 */
export function LiveLinkStatus({ domain, way, round }: { domain: string; way: WpWay; round: number }) {
  const t = useT();
  const [link, setLink] = useState<WpLiveLink | null>(null);
  const [checking, setChecking] = useState(true);
  const [pasting, setPasting] = useState(false);
  const [code, setCode] = useState("");
  const [saving, setSaving] = useState(false);
  useBusy(saving);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);
  const check = useCallback(() => {
    setChecking(true);
    wp.liveLink(domain)
      .then(setLink)
      .catch((e) => setError(errorText(e)))
      .finally(() => setChecking(false));
  }, [domain]);
  useEffect(check, [check, round]);

  async function save() {
    setSaving(true);
    setError(null);
    setSaved(false);
    try {
      setLink(await wp.liveSetCode(domain, code.trim()));
      setCode("");
      setPasting(false);
      setSaved(true);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setSaving(false);
    }
  }

  if (!link) {
    return checking ? <p className="muted small">{t("Checking the connection to the live site…")}</p> : error ? <p className="error small">{error}</p> : null;
  }
  if (link.kind === "files") return null;
  const plugin = link.kind === "plugin";
  const copyOnly = plugin && link.works === true && link.can_push === false;
  const broken = plugin && link.works === false;
  const askCode = broken || (copyOnly && way === "push") || pasting;
  const how = plugin
    ? link.works
      ? [t("Through the Stackyard Connect plugin {version}", { version: link.plugin ?? "" }), link.can_push ? t("can pull and push") : t("can pull, not push")].join(" · ")
      : t("Through the Stackyard Connect plugin")
    : t("With {protocol} as {user}@{host}", { protocol: link.kind.toUpperCase(), user: link.user, host: link.host });
  return (
    <div className="stack tight live-link">
      <div className="row wrap centered">
        <span className={`dot ${broken ? "error" : link.works ? "ok" : ""}`} aria-hidden="true" />
        <span className="small">{how}</span>
        {checking ? (
          <span className="muted small">{t("Checking…")}</span>
        ) : (
          plugin && (
            <>
              <button type="button" className="btn small ghost" onClick={check}>
                {t("Check again")}
              </button>
              {!askCode && (
                <button type="button" className="btn small ghost" onClick={() => setPasting(true)}>
                  {t("Use a new code")}
                </button>
              )}
            </>
          )
        )}
      </div>
      {saved && link.works && <p className="notice small">{t("The new code works. Find the changes again to go on.")}</p>}
      {broken && (
        <p className="warn small">
          {link.expired
            ? t("The connection code Stackyard has stopped working: it was stopped on the live site, a newer code replaced it, or it ran out after a day without use or a week after it was made.")
            : t("The live site did not answer through the plugin: {problem}", { problem: link.problem ?? "" })}
        </p>
      )}
      {copyOnly && way === "push" && (
        <p className="warn small">{t("This code can copy the site but not push to it. Make a new code with “Also let Stackyard push changes” ticked.")}</p>
      )}
      {askCode && (
        <div className="stack tight">
          <label className="small" htmlFor={`code-${domain}`}>
            {t("On the live site, open Tools › Stackyard Connect, click Make a new code (tick “Also let Stackyard push changes” to push), copy it and paste it here:")}
          </label>
          <div className="row">
            <input
              id={`code-${domain}`}
              className="mono grow"
              value={code}
              placeholder="stackyard-connect:…"
              spellCheck={false}
              onChange={(e) => setCode(e.target.value)}
            />
            <button type="button" className="btn primary" disabled={saving || !code.trim().startsWith("stackyard-connect:")} onClick={save}>
              {saving ? t("Checking the code…") : t("Save the code")}
            </button>
            {pasting && !broken && (
              <button type="button" className="btn ghost" onClick={() => setPasting(false)}>
                {t("Cancel")}
              </button>
            )}
          </div>
          <span className="muted small">{t("Stackyard keeps it in the system's password store, in place of the old one.")}</span>
        </div>
      )}
      {error && (
        <p className="error small" role="alert">
          {error}
        </p>
      )}
      {!plugin && link.connection && <ConnectionCheck run={() => api.remoteCheck(link.connection!)} />}
    </div>
  );
}
