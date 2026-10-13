// A stand-in for `window.wings` in `pnpm shot`'s page: enough that a plugin page mounts and draws, and nothing it asks
// of Wings does anything. A fixture can say more through its exports (see shot.mjs): `project`, `args`, `respond`.

/** Installs the stub. `fx` is the fixture module's exports. */
export function installWings(fx) {
  const listeners = new Set();
  const state = { tab: null, block: null };
  const ok = () => Promise.resolve();
  const known = {
    project: () => Promise.resolve(fx.project === undefined ? { id: "p1", name: "web", path: "/work/acme/web", branch: "main" } : fx.project),
    projects: () => Promise.resolve([]),
    viewArgs: () => fx.args ?? null,
    placement: () => "tab",
    onBroadcast: (l) => (listeners.add(l), () => listeners.delete(l)),
    // A message goes nowhere, unless the fixture's `respond` has an answer for it: that is delivered like main's would be.
    broadcast: (message) => {
      const answer = fx.respond?.(message);
      if (answer !== undefined) setTimeout(() => listeners.forEach((l) => l(answer)), 50);
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
    storage: { get: () => Promise.resolve(undefined), set: ok, delete: ok, keys: () => Promise.resolve([]) },
    secrets: { has: () => Promise.resolve(false), set: ok, delete: ok },
  };
  // Anything else a page calls answers with a resolved promise, so a missing stub is a quiet no-op, not a thrown page.
  window.wings = new Proxy(known, { get: (t, k) => (k in t ? t[k] : () => Promise.resolve(undefined)) });
}
