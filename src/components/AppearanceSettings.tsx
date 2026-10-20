import { useState, type ReactNode } from "react";
import { api, errorText } from "../lib/api";
import { ACCENTS, isBuiltInTheme, setAppearance, THEMES, useAddedThemes, useAppearance } from "../lib/appearance";
import { importThemeFromFile, removeAddedTheme, useThemeImport } from "../lib/themeImport";
import {
  DENSITIES,
  ICON_THEMES,
  PANEL_POSITIONS,
  setViewOptions,
  SIDE_BAR_POSITIONS,
  useViewOptions,
  ZOOM_LEVELS,
} from "../lib/viewOptions";
import { useI18n } from "../lib/i18n";
import { Icon } from "./Icon";

/** A row of buttons of which one is pressed. */
function Choices<T extends string>({
  id,
  label,
  options,
  value,
  onPick,
  translate,
}: {
  id: string;
  label: ReactNode;
  options: { id: T; label: string }[];
  value: T;
  onPick: (id: T) => void;
  translate: (s: string) => string;
}) {
  return (
    <div className="field">
      <span className="label" id={id}>
        {label}
      </span>
      <div className="choices" role="group" aria-labelledby={id}>
        {options.map((o) => (
          <button key={o.id} type="button" className="tab" aria-pressed={value === o.id} onClick={() => onPick(o.id)}>
            {translate(o.label)}
          </button>
        ))}
      </div>
    </div>
  );
}

/** The theme, accent color, icons, density, zoom, layout and language of the app. */
export function AppearanceSettings() {
  const { theme, accent } = useAppearance();
  const view = useViewOptions();
  const added = useAddedThemes();
  const importing = useThemeImport();
  const { t, choice, languages, setLanguage, reload } = useI18n();
  const [error, setError] = useState<string | null>(null);
  const addedShown = !isBuiltInTheme(theme);

  async function openFolder() {
    try {
      await api.openLanguagesFolder();
      setError(null);
    } catch (e) {
      setError(errorText(e));
    }
  }

  async function openThemes() {
    try {
      await api.openThemesFolder();
      setError(null);
    } catch (e) {
      setError(errorText(e));
    }
  }

  return (
    <section aria-labelledby="look-h" className="card stack">
      <h2 id="look-h">{t("Appearance")}</h2>

      <Choices
        id="theme-l"
        label={t("Theme")}
        options={THEMES}
        value={theme}
        onPick={(id) => setAppearance({ theme: id })}
        translate={t}
      />

      <div className="field">
        <span className="label" id="added-l">
          {t("Added themes")}
        </span>
        {added.length > 0 && (
          <ul className="theme-list" aria-labelledby="added-l" role="list">
            {added.map((th) => (
              <li key={th.id} className="theme-row">
                <button type="button" className="tab" aria-pressed={theme === th.id} onClick={() => setAppearance({ theme: th.id })}>
                  {th.name}
                </button>
                <button
                  type="button"
                  className="icon-btn"
                  aria-label={t("Remove {name}", { name: th.name })}
                  title={t("Remove {name}", { name: th.name })}
                  onClick={() => void removeAddedTheme(th.id)}
                >
                  <Icon name="close" />
                </button>
              </li>
            ))}
          </ul>
        )}
        <span className="muted small">{t("Import a color theme from VS Code: a theme's .json file or a .vsix extension.")}</span>
        <div className="row wrap">
          <button
            type="button"
            className="btn small"
            disabled={importing.busy}
            onClick={() => void importThemeFromFile(t("Import a VS Code theme"))}
          >
            {importing.busy ? t("Importing…") : t("Import a VS Code theme…")}
          </button>
          <button type="button" className="link" onClick={openThemes}>
            {t("Open the themes folder")}
          </button>
        </div>
        {importing.error && (
          <span className="error small" role="alert">
            {importing.error}
          </span>
        )}
      </div>

      <div className="field">
        <span className="label" id="accent-l">
          {t("Accent color")}
        </span>
        {addedShown ? (
          <span className="muted small">{t("The theme you picked brings its own accent color.")}</span>
        ) : (
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
        )}
      </div>

      <Choices
        id="icons-l"
        label={t("Icons")}
        options={ICON_THEMES}
        value={view.iconTheme}
        onPick={(iconTheme) => setViewOptions({ iconTheme })}
        translate={t}
      />

      <Choices
        id="density-l"
        label={t("Density")}
        options={DENSITIES}
        value={view.density}
        onPick={(density) => setViewOptions({ density })}
        translate={t}
      />

      <div className="field narrow">
        <label htmlFor="s-zoom">{t("Zoom")}</label>
        <select id="s-zoom" value={view.zoom} onChange={(e) => setViewOptions({ zoom: Number(e.target.value) })}>
          {ZOOM_LEVELS.map((z) => (
            <option key={z} value={z}>
              {z}%
            </option>
          ))}
        </select>
      </div>

      <Choices
        id="side-l"
        label={t("Side bar position")}
        options={SIDE_BAR_POSITIONS}
        value={view.sideBarPosition}
        onPick={(sideBarPosition) => setViewOptions({ sideBarPosition })}
        translate={t}
      />

      <Choices
        id="panel-l"
        label={t("Panel position")}
        options={PANEL_POSITIONS}
        value={view.panelPosition}
        onPick={(panelPosition) => setViewOptions({ panelPosition })}
        translate={t}
      />

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
      </div>
      {error && <span className="error small">{error}</span>}
    </section>
  );
}
