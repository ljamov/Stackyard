import { useCallback, useEffect, useState } from "react";
import { ModulesProvider } from "./lib/modules";
import { TitleBar } from "./components/TitleBar";
import { Workbench } from "./components/Workbench";
import { Setup } from "./screens/Setup";
import { api, type SetupStatus } from "./lib/api";

export function App() {
  const [maximized, setMaximized] = useState(false);
  const onMaximized = useCallback((m: boolean) => setMaximized(m), []);
  const [setup, setSetup] = useState<SetupStatus | null>(null);
  const [checked, setChecked] = useState(false);
  useEffect(() => {
    api
      .setupStatus()
      .then((s) => setSetup(s.needed ? s : null))
      .catch(() => {})
      .finally(() => setChecked(true));
  }, []);
  return (
    <ModulesProvider>
      <div className={maximized ? "frame maximized" : "frame"}>
        <TitleBar onMaximized={onMaximized} workbench={checked && !setup} />
        {!checked ? null : setup ? (
          <Setup status={setup} onDone={() => window.location.reload()} />
        ) : (
          <Workbench />
        )}
      </div>
    </ModulesProvider>
  );
}
