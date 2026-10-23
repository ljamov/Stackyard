import { useEffect, useState } from "react";
import { useT } from "../lib/i18n";
import { setAppearance } from "../lib/appearance";
import { Icon } from "./Icon";

/** Switches between the light and dark themes; from any other theme it goes to the opposite kind. */
export function ThemeSwitch({ className }: { className: string }) {
  const t = useT();
  const [light, setLight] = useState(() => document.documentElement.dataset.theme === "light");
  useEffect(() => {
    const root = document.documentElement;
    const watch = new MutationObserver(() => setLight(root.dataset.theme === "light"));
    watch.observe(root, { attributes: true, attributeFilter: ["data-theme"] });
    return () => watch.disconnect();
  }, []);
  const label = light ? t("Switch to the dark theme") : t("Switch to the light theme");
  return (
    <button
      type="button"
      className={className}
      aria-label={label}
      title={label}
      onClick={() => setAppearance({ theme: light ? "dark" : "light" })}
    >
      <Icon name={light ? "moon" : "sun"} />
    </button>
  );
}
