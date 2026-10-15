import { useEffect, useState } from "react";
import { api, type UpdateStatus } from "./api";

/** The app's update status, kept current when a background check finds one. */
export function useUpdate() {
  const [status, setStatus] = useState<UpdateStatus | null>(null);
  useEffect(() => {
    let stop = () => {};
    let gone = false;
    api.updateStatus().then((s) => !gone && setStatus(s), () => {});
    api.onUpdateAvailable((s) => setStatus(s)).then((unlisten) => {
      if (gone) unlisten();
      else stop = unlisten;
    });
    return () => {
      gone = true;
      stop();
    };
  }, []);
  return [status, setStatus] as const;
}
