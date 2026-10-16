// A stand-in for `window.wings` in `pnpm shot`'s page: enough that a plugin page mounts and draws, and nothing it asks
// of Wings does anything. A fixture can say more through its exports (see shot.mjs): `project`, `args`, `storage`, and
// `answers` and `queries` for pages that use wings-plugin-sdk/messages.

import { channel } from "wings-plugin-sdk/messages";

/** Installs the stub. `fx` is the fixture module's exports. */
export function installWings(fx) {
  const listeners = new Set();
  const main = new Set();
  const state = { tab: null, block: null };
  const ok = () => Promise.resolve();
  // What main sends reaches the page a moment later, after its first effects have run.
  const toPage = (message) => setTimeout(() => listeners.forEach((l) => l(message)), 50);
  // Wings hands messages over as JSON, so a fixture's data is held to that too.
  const copy = (message) => JSON.parse(JSON.stringify(message ?? null));
  // The fixture's `answers` and `queries` are served by the SDK's own channel, the code a plugin's main script runs, so a
  // page's `ask` and `useQuery` take their real path, errors and all. A name the fixture leaves out stays unanswered.
  const mainCh = channel({
    wings: {
      onBroadcast: (l) => (main.add(l), () => main.delete(l)),
      broadcast: (message) => (toPage(copy(message)), ok()),
    },
  });
  for (const [name, answer] of Object.entries(fx.answers ?? {})) mainCh.answer(name, answer);
  // A query is a loader, `(key) => value`, or the value itself for every key.
  for (const [name, query] of Object.entries(fx.queries ?? {})) mainCh.serve(name, typeof query === "function" ? query : () => query);
  // What `wings.storage` holds: the fixture's `storage` to start with, then whatever the page sets.
  const stored = new Map(Object.entries(fx.storage ?? {}));
  const known = {
    project: () => Promise.resolve(fx.project === undefined ? { id: "p1", name: "web", path: "/work/acme/web", branch: "main" } : fx.project),
    projects: () => Promise.resolve([]),
    viewArgs: () => fx.args ?? null,
    placement: () => "tab",
    onBroadcast: (l) => (listeners.add(l), () => listeners.delete(l)),
    // A message goes to main's listeners, which are the channel above.
    broadcast: (message) => {
      setTimeout(() => main.forEach((l) => l(copy(message))));
      return ok();
    },
    onProject: (l) => (Promise.resolve().then(() => known.project()).then(l), () => {}),
    onProjects: () => () => {},
    onPanes: () => () => {},
    onFocus: () => () => {},
    tabState: () => Promise.resolve(state.tab),
    blockState: () => Promise.resolve(state.block),
    setTabState: (s) => ((state.tab = s), ok()),
    setBlockState: (s) => ((state.block = s), ok()),
    setHeight: () => {},
    openUrl: ok,
    openView: ok,
    closeView: ok,
    exec: () => Promise.resolve({ code: 0, stdout: "", stderr: "" }),
    storage: {
      get: (key) => Promise.resolve(copy(stored.get(key))),
      set: (key, value) => (stored.set(key, copy(value)), ok()),
      delete: (key) => (stored.delete(key), ok()),
      keys: () => Promise.resolve([...stored.keys()].sort()),
      entries: (prefix = "") => Promise.resolve([...stored].filter(([k]) => k.startsWith(prefix)).sort(([a], [b]) => (a < b ? -1 : 1))),
    },
    secrets: { has: () => Promise.resolve(false), set: ok, delete: ok },
  };
  // Anything else a page calls answers with a resolved promise, so a missing stub is a quiet no-op, not a thrown page.
  window.wings = new Proxy(known, { get: (t, k) => (k in t ? t[k] : () => Promise.resolve(undefined)) });
}
