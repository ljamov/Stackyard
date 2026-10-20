import { useEffect, useState } from "react";
import { useCommandList } from "../lib/commands";
import { isChanged, keyLabel, keyOf, keysFor, recording, setKeys, useKeyChanges } from "../lib/keybindings";
import { matchScore } from "../lib/palette";
import { useT } from "../lib/i18n";

/** Every command with its keys; Change waits for the new keys to be pressed. */
export function KeyboardShortcuts() {
  const t = useT();
  const commands = useCommandList();
  const changes = useKeyChanges();
  const [query, setQuery] = useState("");
  const [editing, setEditing] = useState<string | null>(null);

  // While waiting for keys, every key press goes to this screen alone.
  useEffect(() => {
    if (!editing) return;
    recording.on = true;
    function key(e: KeyboardEvent) {
      const k = keyOf(e);
      if (!k) return;
      e.preventDefault();
      e.stopPropagation();
      if (k !== "escape") setKeys(editing!, [k]);
      setEditing(null);
    }
    window.addEventListener("keydown", key, true);
    return () => {
      recording.on = false;
      window.removeEventListener("keydown", key, true);
    };
  }, [editing]);

  const name = (id: string) => {
    const c = commands.find((x) => x.id === id);
    return c ? (c.category ? `${c.category}: ${c.title}` : c.title) : id;
  };
  const shown = commands.filter((c) => matchScore(query, `${c.category ?? ""} ${c.title}`) !== null);

  return (
    <>
      <header className="page-header">
        <div>
          <h1>{t("Keyboard shortcuts")}</h1>
          <p className="muted">{t("Change the keys for any command. Click Change, then press the new keys.")}</p>
        </div>
      </header>

      <div className="field narrow">
        <label htmlFor="ks-search">{t("Search commands")}</label>
        <input id="ks-search" type="search" value={query} onChange={(e) => setQuery(e.target.value)} />
      </div>

      <section className="card table-wrap" aria-label={t("Keyboard shortcuts")}>
        <table>
          <thead>
            <tr>
              <th>{t("Command")}</th>
              <th>{t("Keys")}</th>
              <th>
                <span className="sr-only">{t("Actions")}</span>
              </th>
            </tr>
          </thead>
          <tbody>
            {shown.map((c) => {
              const keys = keysFor(c.id, changes);
              const clash = keys.flatMap((k) =>
                commands.filter((o) => o.id !== c.id && keysFor(o.id, changes).includes(k)).map((o) => o.id),
              );
              return (
                <tr key={c.id}>
                  <td>
                    {c.category && <span className="muted">{c.category}: </span>}
                    {c.title}
                  </td>
                  <td>
                    {editing === c.id ? (
                      <span className="warn">{t("Press the new keys… Escape cancels.")}</span>
                    ) : keys.length ? (
                      <span className="row wrap">
                        {keys.map((k) => (
                          <kbd key={k} className="palette-key">
                            {keyLabel(k)}
                          </kbd>
                        ))}
                      </span>
                    ) : (
                      <span className="muted">{t("None")}</span>
                    )}
                    {clash.length > 0 && (
                      <div className="warn small">{t("Also used by {name}", { name: clash.map(name).join(", ") })}</div>
                    )}
                  </td>
                  <td className="right nowrap">
                    <button
                      type="button"
                      className="btn small"
                      aria-label={t("Change the keys for {name}", { name: name(c.id) })}
                      onClick={() => setEditing(editing === c.id ? null : c.id)}
                    >
                      {t("Change")}
                    </button>{" "}
                    {keys.length > 0 && (
                      <button
                        type="button"
                        className="btn ghost small"
                        aria-label={t("Remove the keys for {name}", { name: name(c.id) })}
                        onClick={() => setKeys(c.id, [])}
                      >
                        {t("Remove")}
                      </button>
                    )}{" "}
                    {isChanged(c.id, changes) && (
                      <button
                        type="button"
                        className="btn ghost small"
                        aria-label={t("Reset the keys for {name}", { name: name(c.id) })}
                        onClick={() => setKeys(c.id, null)}
                      >
                        {t("Reset")}
                      </button>
                    )}
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </section>
    </>
  );
}
