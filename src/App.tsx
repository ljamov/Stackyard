import { useState } from "react";
import { Sidebar, type Screen } from "./components/Sidebar";
import { Dashboard } from "./screens/Dashboard";
import { Sites } from "./screens/Sites";
import { Runtimes } from "./screens/Runtimes";
import { ComingSoon } from "./screens/ComingSoon";

export function App() {
  const [screen, setScreen] = useState<Screen>("Dashboard");
  return (
    <div className="shell">
      <Sidebar current={screen} onSelect={setScreen} />
      <main className="content">
        {screen === "Dashboard" && <Dashboard onNavigate={setScreen} />}
        {screen === "Sites" && <Sites />}
        {screen === "Runtimes" && <Runtimes />}
        {!["Dashboard", "Sites", "Runtimes"].includes(screen) && <ComingSoon screen={screen} />}
      </main>
    </div>
  );
}
