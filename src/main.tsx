import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "@fontsource/jetbrains-mono/400.css";
import "./styles.css";
import { App } from "./App";
import { startAppearance } from "./lib/appearance";
import { startViewOptions } from "./lib/viewOptions";
import { I18nProvider, loadLanguage, savedLanguage } from "./lib/i18n";
import { catchPageErrors } from "./lib/applog";
import { Crashed } from "./components/Crashed";

catchPageErrors();
startAppearance();
startViewOptions();

// The language's words load before the first paint, so English never flashes.
loadLanguage(savedLanguage()).then((initial) =>
  createRoot(document.getElementById("root")!).render(
    <StrictMode>
      <I18nProvider initial={initial}>
        <Crashed where="the window" full>
          <App />
        </Crashed>
      </I18nProvider>
    </StrictMode>,
  ),
);
