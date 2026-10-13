import { createContext, useContext, useEffect, useState, type ReactNode } from "react";

const NowContext = createContext<number | null>(null);

/** One timer for the whole page: the time to the minute, so every "2 minutes ago" under it ticks together. Idle while `paused`. */
export function NowProvider({ paused, children }: { paused?: boolean; children: ReactNode }) {
  const [now, setNow] = useState(Date.now);
  useEffect(() => {
    if (paused) return;
    setNow(Date.now());
    const timer = setInterval(() => setNow(Date.now()), 60_000);
    return () => clearInterval(timer);
  }, [paused]);
  return <NowContext.Provider value={now}>{children}</NowContext.Provider>;
}

/** The provider's time, or the time of this render outside one. */
export const useNow = () => useContext(NowContext) ?? Date.now();
