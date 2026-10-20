// Problems with files people edit by hand: settings.toml, keybindings.toml,
// language files and theme files. The Problems panel lists them; the status bar counts them.

import { useEffect, useState } from "react";

export type ProblemSource = "settings" | "keybindings" | "languages" | "themes";

export type Problem = {
  source: ProblemSource;
  /** The file's full path. */
  file: string;
  /** English text with {placeholders}, shown through t(). */
  message: string;
  vars?: Record<string, string>;
  /** Shown as it is, e.g. the TOML reader's own description of a mistake. */
  detail?: string;
};

let all: Problem[] = [];
const listeners = new Set<(p: Problem[]) => void>();

/** Replaces the problems one source found; an empty list clears them. */
export function setProblems(source: ProblemSource, problems: Omit<Problem, "source">[]) {
  const next = [...all.filter((p) => p.source !== source), ...problems.map((p) => ({ ...p, source }))];
  if (JSON.stringify(next) === JSON.stringify(all)) return;
  all = next;
  listeners.forEach((l) => l(all));
}

export function useProblems(): Problem[] {
  const [value, setValue] = useState(all);
  useEffect(() => {
    listeners.add(setValue);
    return () => void listeners.delete(setValue);
  }, []);
  return value;
}
