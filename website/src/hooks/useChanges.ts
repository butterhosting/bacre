import { useEffect, useRef } from "react";
import { ChangeClient } from "../clients/ChangeClient";
import { useRegistry } from "./useRegistry";

/** Also when the tab comes back into view: a hidden tab's stream may have been put to sleep */
export function useChanges(onChange: () => void): void {
  const changeClient = useRegistry(ChangeClient);
  const latest = useRef(onChange);
  latest.current = onChange;

  useEffect(() => {
    const run = () => latest.current();
    const visible = () => document.visibilityState === "visible" && run();
    const unsubscribe = changeClient.subscribe(run);
    document.addEventListener("visibilitychange", visible);
    return () => {
      unsubscribe();
      document.removeEventListener("visibilitychange", visible);
    };
  }, []);
}
