import { useCallback, useEffect, useState } from "react";
import { errorText } from "./api";

/** Loads data once on mount; `reload` fetches it again. */
export function useLoad<T>(load: () => Promise<T>, initial: T) {
  const [data, setData] = useState<T>(initial);
  const [error, setError] = useState<string | null>(null);
  const reload = useCallback(() => {
    load()
      .then((value) => {
        setData(value);
        setError(null);
      })
      .catch((e) => setError(errorText(e)));
  }, [load]);
  useEffect(reload, [reload]);
  return { data, error, reload };
}
