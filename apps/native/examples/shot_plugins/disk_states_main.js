// A stand-in for Disk usage's main that tells its page what the real one tells it, for the states a scan of a few files
// is too quick to show: a scan in progress (the "Slow" project), one that failed ("Broken"), and one that is asked for and
// not begun ("Quiet"), which main neither shows nor answers.
const tell = (name, payload) => wings.broadcast({ "wings:sdk": 1, kind: "tell", name, payload });
let scans = [];
wings.onBroadcast((message) => {
  if (message["wings:sdk"] !== 1 || message.kind !== "tell") return;
  if (message.name === "disk-want") tell("disk-state", { scans, collector: { items: [], total: 0 } });
  if (message.name !== "disk-ask" || message.payload.op !== "scan") return;
  const { id, root } = message.payload;
  const now = Date.now();
  if (root.endsWith("/slow")) {
    scans = [{ root, state: "scanning", progress: { entries: 48213, bytes: 3100000000, path: `${root}/node_modules/react/cjs` }, expected: 8000000000, startedAt: now - 83000, stamp: now }];
  } else if (root.endsWith("/broken")) {
    scans = [{ root, state: "error", message: `There's no file or folder at ${root}`, stamp: now }];
  } else {
    return;
  }
  tell("disk-state", { scans, collector: { items: [], total: 0 } });
  tell("disk-done", { id, result: null });
});
