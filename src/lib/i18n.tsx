// Translations. Text is written in English in the code and wrapped in t();
// a language file maps that English text to its translation, and anything a
// file leaves out stays in English. Built-in files live in src/locales and
// load only when picked; files added to the languages folder in Stackyard's
// home add a language or override words of a built-in one.

import { createContext, useCallback, useContext, useEffect, useState, type ReactNode } from "react";
import { api, type LanguageFile } from "./api";

type Strings = Record<string, string>;

/** Built-in languages, by their own name. Each but English has src/locales/<code>.json. */
export const BUILT_IN: { code: string; name: string }[] = [
  { code: "en", name: "English" },
  { code: "de", name: "Deutsch" },
  { code: "es", name: "Español" },
  { code: "fr", name: "Français" },
  { code: "it", name: "Italiano" },
  { code: "mk", name: "Македонски" },
  { code: "pt-BR", name: "Português (Brasil)" },
];

const FILES = import.meta.glob<{ default: Strings }>("../locales/*.json");

const KEY = "stackyard.language";
/** "system" follows the computer's language when Stackyard has it. */
export type LanguageChoice = string;

export function savedLanguage(): LanguageChoice {
  try {
    return localStorage.getItem(KEY) ?? "en";
  } catch {
    return "en";
  }
}

/** The language a choice comes to, given what is available. */
export function resolveLanguage(choice: LanguageChoice, available: string[]): string {
  if (choice !== "system") return available.includes(choice) ? choice : "en";
  for (const wanted of navigator.languages ?? [navigator.language]) {
    const exact = available.find((c) => c.toLowerCase() === wanted.toLowerCase());
    if (exact) return exact;
    const base = available.find((c) => c.split("-")[0] === wanted.split("-")[0].toLowerCase());
    if (base) return base;
  }
  return "en";
}

async function builtIn(code: string): Promise<Strings> {
  const load = FILES[`../locales/${code}.json`];
  if (!load) return {};
  try {
    const { _name, ...strings } = (await load()).default;
    void _name;
    return strings;
  } catch {
    return {};
  }
}

export type Language = { code: string; name: string; added: boolean };

type LoadedState = { code: string; strings: Strings; languages: Language[]; added: LanguageFile[] };

/** Loads the picked language's words; used before the first paint and on every change. */
export async function loadLanguage(choice: LanguageChoice): Promise<LoadedState> {
  const added = await api
    .languageFiles()
    .then((f) => f.languages)
    .catch(() => [] as LanguageFile[]);
  const languages: Language[] = BUILT_IN.map((l) => ({ ...l, added: false }));
  for (const file of added) {
    if (!languages.some((l) => l.code === file.code)) languages.push({ code: file.code, name: file.name, added: true });
  }
  const code = resolveLanguage(
    choice,
    languages.map((l) => l.code),
  );
  const own = added.find((f) => f.code === code)?.strings ?? {};
  const strings = code === "en" ? own : { ...(await builtIn(code)), ...own };
  document.documentElement.lang = code;
  return { code, strings, languages, added };
}

/** Replaces {name} placeholders with values. */
function fill(text: string, vars?: Record<string, string | number>): string {
  if (!vars) return text;
  return text.replace(/\{(\w+)\}/g, (all, name: string) => (name in vars ? String(vars[name]) : all));
}

export type Translate = (text: string, vars?: Record<string, string | number>) => string;

type I18nState = {
  t: Translate;
  choice: LanguageChoice;
  code: string;
  languages: Language[];
  setLanguage: (choice: LanguageChoice) => Promise<void>;
  reload: () => Promise<void>;
};

const english: I18nState = {
  t: fill,
  choice: "en",
  code: "en",
  languages: BUILT_IN.map((l) => ({ ...l, added: false })),
  setLanguage: async () => {},
  reload: async () => {},
};

const I18nContext = createContext<I18nState>(english);

export function I18nProvider({ initial, children }: { initial: LoadedState; children: ReactNode }) {
  const [choice, setChoice] = useState(savedLanguage);
  const [state, setState] = useState(initial);
  useEffect(() => setState(initial), [initial]);

  const setLanguage = useCallback(async (next: LanguageChoice) => {
    try {
      localStorage.setItem(KEY, next);
    } catch {
      // Not saved; it still applies until the app closes.
    }
    setChoice(next);
    setState(await loadLanguage(next));
  }, []);
  const reload = useCallback(async () => setState(await loadLanguage(choice)), [choice]);

  const t = useCallback<Translate>((text, vars) => fill(state.strings[text] ?? text, vars), [state.strings]);
  return (
    <I18nContext.Provider value={{ t, choice, code: state.code, languages: state.languages, setLanguage, reload }}>
      {children}
    </I18nContext.Provider>
  );
}

export function useI18n(): I18nState {
  return useContext(I18nContext);
}

/** The translate function for the current language. */
export function useT(): Translate {
  return useContext(I18nContext).t;
}
