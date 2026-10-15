// A setting kept in `wings.storage`, that every frame of the plugin hears change.

import { channel } from "./messages.js";

/**
 * A setting stored under `key`, read as `default` when it isn't set or `check` doesn't accept what is stored. Make it once
 * in a module that main, the pages and the settings page all import. `set` tells the other frames, which read it again.
 */
export function setting(key, { default: fallback, check, wings }) {
  const w = () => wings ?? globalThis.wings;
  const ch = channel({ wings });
  const name = `setting:${key}`;
  const here = new Set();

  const get = async () => {
    const stored = await w().storage.get(key);
    const value = stored == null ? undefined : check ? check(stored) : stored;
    return value === undefined ? fallback : value;
  };

  return {
    key,
    get,
    async set(value) {
      await w().storage.set(key, value);
      // Each listener runs on its own promise, so one that throws can't fail `set` or stop the rest.
      for (const listener of here) void Promise.resolve(value).then(listener);
      await ch.tell(name);
    },
    onChange(listener) {
      here.add(listener);
      const off = ch.on(name, () => void get().then(listener));
      return () => {
        here.delete(listener);
        off();
      };
    },
  };
}
