// The code editor a site's folder opens in, shared by the Sites screen and
// Settings so a change in one shows in the other straight away.

import { useEffect, useState } from "react";
import { api, type CodeEditors } from "./api";

let current: CodeEditors | null = null;
const listeners = new Set<(e: CodeEditors) => void>();

function publish(editors: CodeEditors) {
  current = editors;
  for (const l of listeners) l(editors);
}

/** The editors, which are installed, and the one in use; null until read. */
export function useCodeEditors() {
  const [editors, setEditors] = useState<CodeEditors | null>(current);
  useEffect(() => {
    listeners.add(setEditors);
    // Read again on each mount: an editor may have been installed since.
    api.codeEditors().then(publish).catch(() => {});
    return () => {
      listeners.delete(setEditors);
    };
  }, []);
  return editors;
}

/** Picks the editor (null for the default) and tells every screen. */
export async function setCodeEditor(id: string | null) {
  publish(await api.setCodeEditor(id));
}

/** The name of the editor in use, e.g. "VS Code". */
export function editorName(editors: CodeEditors | null) {
  return editors?.list.find((e) => e.id === editors.current)?.name ?? "VS Code";
}
