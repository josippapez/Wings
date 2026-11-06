// Fixture main for the real native bundles. Only fixture data, never a user's repo.
const root = "__ROOT__";
const repo = { root, name: "web", branch: "feature/checkout" };
const files = [
  ["M", "package.json", 2, 1, null],
  ["A", "src/checkout/Coupon.tsx", 96, 0, null],
  ["M", "src/checkout/Summary.tsx", 42, 18, null],
  ["D", "src/checkout/Legacy.tsx", 0, 212, null],
  ["?", "src/checkout/useCoupon.ts", 24, 0, null],
  ["R", "src/hooks/useCart.ts", 18, 6, "src/cart.ts"],
];
const rows = Array.from({ length: 12 }, (_, i) => [String(i).padStart(40, "a"), ["Add coupon validation", "Round shipping rates", "Merge checkout redesign"][i % 3], ["Ada Lovelace", "Grace Hopper"][i % 2], 1770000000 - i * 3600, i === 0 ? ["H:feature/checkout", "r:origin/feature/checkout"] : []]);
const respond = (message) => wings.broadcast({ "wings:sdk": 1, ...message });
wings.onBroadcast((m) => {
  if (m?.["wings:sdk"] !== 1) return;
  if (m.kind === "ask") {
    const response = m.name === "graph-repo" ? { root, name: "web" } : null;
    void respond({ kind: "answer", id: m.id, response });
  }
  if (m.kind === "sub") for (const [name, key] of m.subs) {
    const data = name === "changes" ? { repo, files, total: files.length, added: 182, removed: 237, partial: false, against: key.mode === "branch" ? "against main" : "Working tree and staged changes" }
      : { repo, rows: key.query ? rows.filter((r) => r[1].toLowerCase().includes(key.query.toLowerCase())) : rows, more: false };
    void respond({ kind: "state", name, key, data, at: Date.now() });
  }
});
