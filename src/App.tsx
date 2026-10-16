import { useCallback, useEffect, useState } from "react";
import { Sidebar, type Screen } from "./components/Sidebar";
import { Dashboard } from "./screens/Dashboard";
import { Sites } from "./screens/Sites";
import { Runtimes } from "./screens/Runtimes";
import { Services } from "./screens/Services";
import { ComingSoon } from "./screens/ComingSoon";
import { Settings } from "./screens/Settings";
import { McpServer } from "./screens/McpServer";
import { Databases } from "./screens/Databases";
import { Logs } from "./screens/Logs";
import { Scheduler } from "./screens/Scheduler";
import { Images } from "./screens/Images";
import { ModulesProvider } from "./lib/modules";
import { TitleBar } from "./components/TitleBar";
import { Setup } from "./screens/Setup";
import { api, type SetupStatus } from "./lib/api";

export function App() {
  const [screen, setScreen] = useState<Screen>("Dashboard");
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
        <TitleBar onMaximized={onMaximized} />
        {!checked ? null : setup ? (
          <Setup status={setup} onDone={() => window.location.reload()} />
        ) : (
          <div className="shell">
            <Sidebar current={screen} onSelect={setScreen} />
            <main className="content">
              {screen === "Dashboard" && <Dashboard onNavigate={setScreen} />}
              {screen === "Sites" && <Sites />}
              {screen === "Services" && <Services />}
              {screen === "Runtimes" && <Runtimes />}
              {screen === "Settings" && <Settings />}
              {screen === "MCP server" && <McpServer />}
              {screen === "Databases" && <Databases />}
              {screen === "Scheduler" && <Scheduler />}
              {screen === "Logs" && <Logs />}
              {screen === "Images" && <Images />}
              {![
                "Dashboard",
                "Sites",
                "Services",
                "Runtimes",
                "Settings",
                "MCP server",
                "Scheduler",
                "Images",
                "Logs",
                "Databases",
              ].includes(screen) && <ComingSoon screen={screen} />}
            </main>
          </div>
        )}
      </div>
    </ModulesProvider>
  );
}
