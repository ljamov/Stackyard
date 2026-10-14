import { useState } from "react";
import { Sidebar, type Screen } from "./components/Sidebar";
import { Dashboard } from "./screens/Dashboard";
import { Sites } from "./screens/Sites";
import { Runtimes } from "./screens/Runtimes";
import { ComingSoon } from "./screens/ComingSoon";
import { Settings } from "./screens/Settings";
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
          {screen === "Runtimes" && <Runtimes />}
          {screen === "Settings" && <Settings />}
          {!["Dashboard", "Sites", "Runtimes", "Settings"].includes(screen) && <ComingSoon screen={screen} />}
        </main>
      </div>
    </ModulesProvider>
  );
}
