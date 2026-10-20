// Keeps the window in step with the two files people may edit by hand:
//
//   settings.toml     [interface]  theme, accent, language
//   keybindings.toml  [keys]       "command.id" = ["ctrl+shift+e"]
//
// Changes made in the app are written to the files; changes made in an editor
// are picked up within a couple of seconds. A value Stackyard can't use is
// listed in the Problems panel and the default is used instead, so a typo never
// breaks the app.

import { useEffect, useRef } from "react";
import { api, type InterfaceFile, type InterfaceFileName } from "./api";
import {
  ACCENTS,
  currentAppearance,
  DEFAULT_APPEARANCE,
  onAppearanceChange,
  setAppearance,
  THEMES,
  type Accent,
  type Theme,
} from "./appearance";
import { currentKeyChanges, keyProblem, onKeyChange, replaceKeyChanges, type Changes } from "./keybindings";
import { setProblems, type Problem } from "./problems";
import { useI18n } from "./i18n";

/** How often the files are read again while the window is showing. */
const EVERY_MS = 2000;
const DEFAULT_LANGUAGE = "en";

type Found = Omit<Problem, "source">;

/** The [interface] settings: what each may hold. Anything else is reported. */
export const INTERFACE_SETTINGS = {
  theme: THEMES.map((t) => t.id),
  accent: ACCENTS.map((a) => a.id),
  language: null, // a language code, or "system"
} as const;

const known = new Set(Object.keys(INTERFACE_SETTINGS));
const list = (xs: readonly string[]) => xs.map((x) => `"${x}"`).join(", ");

function readSettings(file: InterfaceFile, languages: string[]) {
  const problems: Found[] = [];
  const v = file.values;
  const pick = <T extends string>(key: string, allowed: readonly T[], fallback: T): T => {
    if (!(key in v)) return fallback;
    if (typeof v[key] === "string" && allowed.includes(v[key] as T)) return v[key] as T;
    problems.push({
      file: file.path,
      message: "{name} must be one of {values}; using {fallback}.",
      vars: { name: `interface.${key}`, values: list(allowed), fallback: `"${fallback}"` },
    });
    return fallback;
  };
  const theme = pick<Theme>("theme", INTERFACE_SETTINGS.theme, DEFAULT_APPEARANCE.theme);
  const accent = pick<Accent>("accent", INTERFACE_SETTINGS.accent, DEFAULT_APPEARANCE.accent);
  let language = DEFAULT_LANGUAGE;
  if ("language" in v) {
    if (typeof v.language === "string" && (v.language === "system" || languages.includes(v.language))) {
      language = v.language;
    } else {
      problems.push({
        file: file.path,
        message: "{name} must be one of {values}; using {fallback}.",
        vars: { name: "interface.language", values: list(["system", ...languages]), fallback: `"${DEFAULT_LANGUAGE}"` },
      });
    }
  }
  for (const key of Object.keys(v)) {
    if (!known.has(key)) {
      problems.push({ file: file.path, message: "{name} isn't a setting Stackyard knows.", vars: { name: `interface.${key}` } });
    }
  }
  return { theme, accent, language, problems };
}

function readKeybindings(file: InterfaceFile) {
  const problems: Found[] = [];
  const changes: Changes = {};
  const keys = file.values.keys;
  if (keys !== undefined && (typeof keys !== "object" || keys === null || Array.isArray(keys))) {
    problems.push({ file: file.path, message: "{name} must be a table of commands and their keys.", vars: { name: "keys" } });
    return { changes, problems };
  }
  for (const [id, value] of Object.entries((keys ?? {}) as Record<string, unknown>)) {
    if (!Array.isArray(value) || !value.every((k) => typeof k === "string")) {
      problems.push({
        file: file.path,
        message: "{name} must be a list of keys, like {example}.",
        vars: { name: `keys."${id}"`, example: '["ctrl+shift+e"]' },
      });
      continue;
    }
    const bad = value.map((k) => [k, keyProblem(k)] as const).filter(([, p]) => p);
    for (const [k, p] of bad) {
      problems.push({ file: file.path, message: "{name}: {key} can't be used ({why}).", vars: { name: `keys."${id}"`, key: `"${k}"`, why: p! } });
    }
    changes[id] = value.filter((k) => !keyProblem(k)).map((k) => k.trim().toLowerCase());
  }
  for (const key of Object.keys(file.values)) {
    if (key !== "keys") problems.push({ file: file.path, message: "{name} isn't a setting Stackyard knows.", vars: { name: key } });
  }
  return { changes, problems };
}

/** JSON with keys in order, so the same values always read the same. */
function stable(value: unknown): string {
  return JSON.stringify(value, (_, v) =>
    v && typeof v === "object" && !Array.isArray(v) ? Object.fromEntries(Object.entries(v).sort(([a], [b]) => (a < b ? -1 : 1))) : v,
  );
}

function invalid(file: InterfaceFile): Found {
  return { file: file.path, message: "This file isn't valid TOML, so the last good values stay in use.", detail: file.error ?? "" };
}

/** Sets or removes one key, keeping everything else someone wrote in the file. */
function withValue(values: Record<string, unknown>, key: string, value: unknown, isDefault: boolean) {
  const { [key]: _, ...rest } = values;
  return isDefault ? rest : { ...rest, [key]: value };
}

export function useUserFiles() {
  const { choice, languages, setLanguage } = useI18n();
  const codes = languages.map((l) => l.code);
  const seen = useRef<Partial<Record<InterfaceFileName, string>>>({});
  const files = useRef<Partial<Record<InterfaceFileName, InterfaceFile>>>({});
  // A language set from the file, so the change it causes isn't written back.
  const fromFile = useRef<string | null>(null);
  const state = useRef({ choice, codes, setLanguage });
  state.current = { choice, codes, setLanguage };

  // Read both files now and every couple of seconds while the window shows.
  useEffect(() => {
    let stopped = false;
    async function check(first: boolean) {
      if (stopped || (!first && document.visibilityState !== "visible")) return;
      const [settings, keybindings] = await Promise.all([
        api.interfaceSettings().catch(() => null),
        api.keybindings().catch(() => null),
      ]);
      if (stopped) return;
      if (settings) applySettings(settings, first);
      if (keybindings) applyKeybindings(keybindings, first);
    }

    function changed(name: InterfaceFileName, file: InterfaceFile) {
      const text = stable([file.values, file.error]);
      if (seen.current[name] === text) return false;
      seen.current[name] = text;
      files.current[name] = file;
      return true;
    }

    function applySettings(file: InterfaceFile, first: boolean) {
      if (!changed("settings", file)) return;
      if (file.error) return setProblems("settings", [invalid(file)]);
      const { codes, choice, setLanguage } = state.current;
      // First run with this version: keep the look chosen before, by writing it to the file.
      if (first && Object.keys(file.values).length === 0) {
        const a = currentAppearance();
        if (a.theme !== DEFAULT_APPEARANCE.theme || a.accent !== DEFAULT_APPEARANCE.accent || choice !== DEFAULT_LANGUAGE) {
          void saveSettings({ theme: a.theme, accent: a.accent, language: choice });
        }
        return setProblems("settings", []);
      }
      const read = readSettings(file, codes);
      setProblems("settings", read.problems);
      setAppearance({ theme: read.theme, accent: read.accent }, true);
      if (read.language !== choice) {
        fromFile.current = read.language;
        void setLanguage(read.language);
      }
    }

    function applyKeybindings(file: InterfaceFile, first: boolean) {
      if (!changed("keybindings", file)) return;
      if (file.error) return setProblems("keybindings", [invalid(file)]);
      if (first && Object.keys(file.values).length === 0) {
        const mine = currentKeyChanges();
        if (Object.keys(mine).length) void saveKeys(mine);
        return setProblems("keybindings", []);
      }
      const read = readKeybindings(file);
      setProblems("keybindings", read.problems);
      replaceKeyChanges(read.changes, true);
    }

    void check(true);
    const timer = setInterval(() => void check(false), EVERY_MS);
    const onFocus = () => void check(false);
    window.addEventListener("focus", onFocus);
    return () => {
      stopped = true;
      clearInterval(timer);
      window.removeEventListener("focus", onFocus);
    };
  }, []);

  async function saveSettings(change: { theme?: Theme; accent?: Accent; language?: string }) {
    let values = files.current.settings?.error ? null : (files.current.settings?.values ?? {});
    if (!values) return; // Never overwrite a file someone is in the middle of fixing.
    if (change.theme) values = withValue(values, "theme", change.theme, change.theme === DEFAULT_APPEARANCE.theme);
    if (change.accent) values = withValue(values, "accent", change.accent, change.accent === DEFAULT_APPEARANCE.accent);
    if (change.language) values = withValue(values, "language", change.language, change.language === DEFAULT_LANGUAGE);
    await api.saveInterfaceSettings(values).catch(() => {});
    remember("settings", values);
  }

  async function saveKeys(changes: Changes) {
    const file = files.current.keybindings;
    if (file?.error) return;
    const values = withValue(file?.values ?? {}, "keys", changes, Object.keys(changes).length === 0);
    await api.saveKeybindings(values).catch(() => {});
    remember("keybindings", values);
  }

  /** What was just written, so the next read doesn't count as an outside change. */
  function remember(name: InterfaceFileName, values: Record<string, unknown>) {
    const file = { path: files.current[name]?.path ?? "", values, error: null };
    files.current[name] = file;
    seen.current[name] = stable([values, null]);
  }

  // Changes made in the app go to the files.
  useEffect(
    () => onAppearanceChange((a, fromSettings) => void (fromSettings || saveSettings({ theme: a.theme, accent: a.accent }))),
    [],
  );
  useEffect(() => onKeyChange((c, fromKeysFile) => void (fromKeysFile || saveKeys(c))), []);
  useEffect(() => {
    if (fromFile.current === choice) {
      fromFile.current = null;
      return;
    }
    // Only once the file has been read, and only a real change from what it holds.
    const file = files.current.settings;
    if (!file || file.error) return;
    const inFile = typeof file.values.language === "string" ? file.values.language : DEFAULT_LANGUAGE;
    if (inFile !== choice) void saveSettings({ language: choice });
  }, [choice]);

  // Problems with language files, found when the languages load.
  useEffect(() => {
    api
      .languageFiles()
      .then((f) =>
        setProblems(
          "languages",
          f.problems.map((p) => ({ file: `${f.folder}/${p.file}`, message: "This language file can't be used.", detail: p.problem })),
        ),
      )
      .catch(() => {});
  }, [languages]);
}
