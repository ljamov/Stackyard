import { useState } from "react";
import { Sidebar, type Screen } from "./components/Sidebar";
import { Dashboard } from "./screens/Dashboard";
import { Sites } from "./screens/Sites";
import { Runtimes } from "./screens/Runtimes";
import { Services } from "./screens/Services";
import { ComingSoon } from "./screens/ComingSoon";
import { Settings } from "./screens/Settings";
import { McpServer } from "./screens/McpServer";
import { Logs } from "./screens/Logs";
import { Scheduler } from "./screens/Scheduler";
import { ModulesProvider } from "./lib/modules";

export function App() {
  const [screen, setScreen] = useState<Screen>("Dashboard");
  return (
    <ModulesProvider>
      <div className="shell">
        <Sidebar current={screen} onSelect={setScreen} />
        <main className="content">
          {screen === "Dashboard" && <Dashboard onNavigate={setScreen} />}
          {screen === "Sites" && <Sites />}
          {screen === "Services" && <Services />}
          {screen === "Runtimes" && <Runtimes />}
          {screen === "Settings" && <Settings />}
          {screen === "MCP server" && <McpServer />}
          {screen === "Scheduler" && <Scheduler />}
          {screen === "Logs" && <Logs />}
          {!["Dashboard", "Sites", "Services", "Runtimes", "Settings", "MCP server", "Scheduler", "Logs"].includes(screen) && (
            <ComingSoon screen={screen} />
          )}
        </main>
      </div>
    </ModulesProvider>
  );
}
