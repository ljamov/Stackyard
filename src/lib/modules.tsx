import { createContext, useCallback, useContext, useEffect, useState, type ReactNode } from "react";
import { api, type Module } from "./api";

type ModulesState = {
  modules: Module[];
  isOn: (id: string) => boolean;
  setModule: (id: string, enabled: boolean) => Promise<void>;
};

const ModulesContext = createContext<ModulesState | null>(null);

/** Loads module switches once and shares them, so screens and the sidebar agree. */
export function ModulesProvider({ children }: { children: ReactNode }) {
  const [modules, setModules] = useState<Module[]>([]);
  useEffect(() => {
    api.listModules().then(setModules).catch(() => setModules([]));
  }, []);
  const isOn = useCallback((id: string) => modules.some((m) => m.id === id && m.enabled), [modules]);
  const setModule = useCallback(async (id: string, enabled: boolean) => {
    setModules(await api.setModule(id, enabled));
  }, []);
  return <ModulesContext.Provider value={{ modules, isOn, setModule }}>{children}</ModulesContext.Provider>;
}

export function useModules(): ModulesState {
  const state = useContext(ModulesContext);
  if (!state) throw new Error("useModules must be used inside ModulesProvider");
  return state;
}
