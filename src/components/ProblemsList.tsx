import { api } from "../lib/api";
import type { Problem, ProblemSource } from "../lib/problems";
import { useT } from "../lib/i18n";

/** The Problems panel: what Stackyard couldn't use in the files people edit by hand. */
export function ProblemsList({ problems }: { problems: Problem[] }) {
  const t = useT();
  // keyProblem()'s reasons, in words.
  const why: Record<string, string> = {
    "too-many": t("write one key, or two separated by a space"),
    "no-key": t("a key is missing after the last +"),
    modifier: t("only ctrl, alt and shift can come before +"),
  };
  const files: Partial<Record<ProblemSource, "settings" | "keybindings">> = {
    settings: "settings",
    keybindings: "keybindings",
  };

  if (problems.length === 0) {
    return <p className="muted">{t("No problems with settings.toml, keybindings.toml or language files.")}</p>;
  }
  return (
    <ul className="problems">
      {problems.map((p, i) => {
        const file = files[p.source];
        const vars = p.vars?.why ? { ...p.vars, why: why[p.vars.why] ?? p.vars.why } : p.vars;
        return (
          <li key={i} className="problem">
            <span className="warn" aria-hidden="true">
              ⚠
            </span>
            <div className="grow">
              <div>{t(p.message, vars)}</div>
              {p.detail && <pre className="mono small problem-detail">{p.detail}</pre>}
              <div className="row wrap small">
                <span className="muted mono connect">{p.file}</span>
                {file && (
                  <button type="button" className="link" onClick={() => void api.showInterfaceFile(file)}>
                    {t("Show in the file manager")}
                  </button>
                )}
              </div>
            </div>
          </li>
        );
      })}
    </ul>
  );
}
