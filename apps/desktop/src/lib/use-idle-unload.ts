import { useEffect, useRef, useState } from "react";

import { hiddenLoaded, stillUnloaded } from "@/lib/idle-unload";

/**
 * Which of `keys` are unloaded: hidden (not `shown`) for `after` ms. Showing one again, or dropping it from `keys`,
 * loads it. The countdown starts when it becomes hidden and isn't restarted by other pages changing.
 */
export function useIdleUnload(keys: string[], shown: string | null, after: number): ReadonlySet<string> {
  const [unloaded, setUnloaded] = useState<ReadonlySet<string>>(new Set());
  const timers = useRef(new Map<string, ReturnType<typeof setTimeout>>());
  const id = keys.join("\n");

  useEffect(() => {
    const keep = stillUnloaded(unloaded, keys, shown);
    if (keep.length !== unloaded.size) setUnloaded(new Set(keep));
    const waiting = new Set(hiddenLoaded(keys, shown, new Set(keep)));
    for (const [key, timer] of timers.current) {
      if (waiting.has(key)) continue;
      clearTimeout(timer);
      timers.current.delete(key);
    }
    for (const key of waiting) {
      if (timers.current.has(key)) continue;
      timers.current.set(
        key,
        setTimeout(() => {
          timers.current.delete(key);
          setUnloaded((all) => new Set(all).add(key));
        }, after),
      );
    }
  }, [id, shown, unloaded, after]);

  useEffect(() => () => timers.current.forEach(clearTimeout), []);
  return unloaded;
}
