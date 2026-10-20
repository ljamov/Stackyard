import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "@fontsource/jetbrains-mono/400.css";
import "./styles.css";
import { App } from "./App";
import { startAppearance } from "./lib/appearance";
import { I18nProvider, loadLanguage, savedLanguage } from "./lib/i18n";

startAppearance();

// The language's words load before the first paint, so English never flashes.
loadLanguage(savedLanguage()).then((initial) =>
  createRoot(document.getElementById("root")!).render(
    <StrictMode>
      <I18nProvider initial={initial}>
        <App />
      </I18nProvider>
    </StrictMode>,
  ),
);
