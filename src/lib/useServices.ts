import { useCallback, useEffect, useState } from "react";
import { api, errorText, type Service, type ServiceProgress } from "./api";

/**
 * Services with their live state. Refreshes every few seconds so a crash or
 * restart shows up without a click, and tracks install progress and errors.
 */
export function useServices() {
  const [services, setServices] = useState<Service[]>([]);
  const [progress, setProgress] = useState<Record<string, ServiceProgress>>({});
  const [errors, setErrors] = useState<Record<string, string>>({});
  const [busy, setBusy] = useState<Record<string, boolean>>({});

  const reload = useCallback(() => {
    api.listServices().then(setServices).catch(() => {});
  }, []);

  useEffect(() => {
    reload();
    const timer = setInterval(reload, 3000);
    return () => clearInterval(timer);
  }, [reload]);

  useEffect(() => {
    let off: (() => void) | undefined;
    let cancelled = false;
    api
      .onServiceProgress((p) => setProgress((all) => ({ ...all, [p.id]: p })))
      .then((unsubscribe) => (cancelled ? unsubscribe() : (off = unsubscribe)));
    return () => {
      cancelled = true;
      off?.();
    };
  }, []);

  async function run(id: string, action: () => Promise<Service[]>) {
    setErrors(({ [id]: _, ...rest }) => rest);
    setBusy((all) => ({ ...all, [id]: true }));
    try {
      setServices(await action());
    } catch (e) {
      setErrors((all) => ({ ...all, [id]: errorText(e) }));
      reload();
    } finally {
      setBusy(({ [id]: _, ...rest }) => rest);
      setProgress(({ [id]: _, ...rest }) => rest);
    }
  }

  return {
    services,
    progress,
    errors,
    busy,
    install: (id: string) => {
      setServices((all) => all.map((s) => (s.id === id ? { ...s, installing: true } : s)));
      return run(id, () => api.installService(id));
    },
    uninstall: (id: string) => run(id, () => api.uninstallService(id)),
    start: (id: string) => run(id, () => api.startService(id)),
    stop: (id: string) => run(id, () => api.stopService(id)),
  };
}

export type ServicesHook = ReturnType<typeof useServices>;
