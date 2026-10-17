import { useState } from "react";
import { api, errorText } from "../lib/api";
import { ACCENTS, setAppearance, THEMES, useAppearance } from "../lib/appearance";
import { useI18n } from "../lib/i18n";

/** Light or dark, the accent color, and the language of the app. */
export function AppearanceSettings() {
  const { theme, accent } = useAppearance();
  const { t, choice, languages, setLanguage, reload } = useI18n();
  const [error, setError] = useState<string | null>(null);

  async function openFolder() {
    try {
      await api.openLanguagesFolder();
      setError(null);
    } catch (e) {
      setError(errorText(e));
    }
  }

  return (
    <section aria-labelledby="look-h" className="card stack">
      <h2 id="look-h">{t("Appearance")}</h2>

      <div className="field">
        <span className="label" id="theme-l">
          {t("Theme")}
        </span>
        <div className="choices" role="group" aria-labelledby="theme-l">
          {THEMES.map((th) => (
            <button
              key={th.id}
              type="button"
              className="tab"
              aria-pressed={theme === th.id}
              onClick={() => setAppearance({ theme: th.id })}
            >
              {t(th.label)}
            </button>
          ))}
        </div>
      </div>

      <div className="field">
        <span className="label" id="accent-l">
          {t("Accent color")}
        </span>
        <div className="swatches" role="group" aria-labelledby="accent-l">
          {ACCENTS.map((a) => (
            <button
              key={a.id}
              type="button"
              className="swatch"
              aria-pressed={accent === a.id}
              aria-label={t(a.label)}
              title={t(a.label)}
              onClick={() => setAppearance({ accent: a.id })}
            >
              <span style={{ background: a.swatch }} />
            </button>
          ))}
        </div>
      </div>

      <div className="field narrow">
        <label htmlFor="s-lang">{t("Language")}</label>
        <select id="s-lang" value={choice} onChange={(e) => setLanguage(e.target.value)}>
          <option value="system">{t("Match my computer")}</option>
          {languages.map((l) => (
            <option key={l.code} value={l.code}>
              {l.name}
            </option>
          ))}
        </select>
        <span className="muted small">{t("Anything not translated yet stays in English.")}</span>
        <div className="row wrap">
          <button type="button" className="link" onClick={openFolder}>
            {t("Add a language…")}
          </button>
          <button type="button" className="link" onClick={() => reload()}>
            {t("Reload language files")}
          </button>
        </div>
        {error && <span className="error small">{error}</span>}
      </div>
    </section>
  );
}
