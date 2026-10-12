import { useEffect, useRef } from "react";

/**
 * Calls `listener` with each message the plugin's other pages send with `wings.broadcast`, while the component
 * is mounted. The latest `listener` is used, so it can read state without resubscribing.
 */
export function useBroadcast(listener) {
  const latest = useRef(listener);
  latest.current = listener;
  useEffect(() => wings.onBroadcast((message) => latest.current(message)), []);
}
