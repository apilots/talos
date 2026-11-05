import { useEffect, useState } from "react";

/** Refresh displayed wall time only while a visible timer needs it. */
export function useNowMs(enabled: boolean) {
  const [nowMs, setNowMs] = useState(() => Date.now());
  useEffect(() => {
    if (!enabled) return;
    setNowMs(Date.now());
    const timer = window.setInterval(() => setNowMs(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, [enabled]);
  return enabled ? Date.now() : nowMs;
}
