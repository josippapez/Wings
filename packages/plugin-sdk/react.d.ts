/// <reference path="./wings.d.ts" />
/**
 * Calls `listener` with each message the plugin's other pages send with `wings.broadcast`, while the component
 * is mounted. The latest `listener` is used, so it can read state without resubscribing.
 */
export function useBroadcast(listener: (message: unknown) => void): void;
