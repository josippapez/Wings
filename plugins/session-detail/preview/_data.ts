// Made-up sessions for the Session sidebar.
import type { Item, ItemView, SessionView } from "../src/lib/types";

const now = Date.now();
const at = (minutesAgo: number) => now - minutesAgo * 60_000;

const items: ItemView[] = [
  { kind: "prompt", key: "p1", summary: "Rework the checkout page so the coupon field sits under the order summary, and keep the totals in sync", at: at(42) },
  { kind: "tool", key: "t1", name: "Read", summary: "src/features/checkout/CheckoutPage.tsx", at: at(41), ms: 120, done: true },
  { kind: "tool", key: "t2", name: "Grep", summary: "useCoupon in src/", at: at(40), ms: 340, done: true },
  { kind: "tool", key: "t3", name: "Edit", summary: "src/features/checkout/CheckoutSummary.tsx", at: at(38), ms: 42, done: true },
  { kind: "tool", key: "t4", name: "Bash", summary: "git commit -m 'Add a coupon field to the checkout summary'", at: at(30), ms: 880, done: true, tags: [{ kind: "commit", label: "commit" }] },
  { kind: "tool", key: "t5", name: "Bash", summary: "git push origin feature/checkout-redesign", at: at(29), ms: 2400, done: true, tags: [{ kind: "push", label: "push" }] },
  { kind: "tool", key: "t6", name: "Bash", summary: "gh pr create --fill", at: at(28), ms: 1900, done: true, tags: [{ kind: "pr", label: "pr" }], outcome: "opened #482" },
  { kind: "tool", key: "t7", name: "Bash", summary: "pnpm test -- checkout", at: at(12), ms: 18_400, done: true, error: true },
  { kind: "tool", key: "t8", name: "Bash", summary: "pnpm test -- checkout --update-snapshots", at: at(6), ms: 21_000, done: true, tags: [{ kind: "ok", label: "tests passed" }] },
  { kind: "prompt", key: "p2", summary: "Also make the coupon discount show in the cart", at: at(5) },
  { kind: "tool", key: "t9", name: "Edit", summary: "src/routes/cart.tsx", at: at(1), done: false },
];

/** What some of the rows open to. */
const opens: Item[] = [
  { ...items[0], text: "Rework the checkout page so the coupon field sits under the order summary, and keep the totals in sync with the cart. The discount comes from useCoupon: don't change its API." },
  { ...items[3], input: [["file_path", "src/features/checkout/CheckoutSummary.tsx"], ["old_string", "<Total value={subtotal} />"], ["new_string", "<Total value={subtotal - discount} />"]], result: "The file src/features/checkout/CheckoutSummary.tsx has been updated." },
  {
    ...items[7],
    input: [["command", "pnpm test -- checkout"]],
    result: "FAIL src/features/checkout/CheckoutSummary.test.tsx\n  ● coupon › keeps the totals in sync\n\n    expect(received).toBe(expected)\n    Expected: 41.2\n    Received: 45",
  },
  { ...items[10], input: [["file_path", "src/routes/cart.tsx"], ["old_string", "<Total value={subtotal} />"], ["new_string", "<Total value={subtotal - discount} />"]], result: null },
];

/** What main answers a row's detail request with. */
export const detail = ({ key }: { sessionId: string; key: string }) => {
  const item = opens.find((i) => i.key === key);
  if (!item) throw new Error("That row isn't kept any more.");
  return item;
};

export const working: SessionView = {
  id: "s1",
  project: "web",
  name: "checkout redesign",
  state: "working",
  model: "Sonnet 5.5",
  context: 116_000,
  tokens: "1.2M in · 84k out",
  running: [
    { name: "Edit", summary: "src/routes/cart.tsx", at: at(1) },
    { name: "Bash", summary: "pnpm --dir packages/shared build && pnpm test -- cart", at: at(0) },
  ],
  items,
  todos: [
    { text: "Read the checkout page", active: "Reading the checkout page", status: "completed" },
    { text: "Add the coupon field", active: "Adding the coupon field", status: "completed" },
    { text: "Show the discount in the cart and keep the totals in sync with the order summary", active: "Showing the discount in the cart", status: "in_progress" },
    { text: "Update the snapshots", active: null, status: "pending" },
    { text: "Open a pull request", active: null, status: "pending" },
  ],
  files: [
    ["src/features/checkout/CheckoutSummary.tsx", 4],
    ["src/routes/cart.tsx", 2],
    ["src/features/checkout/hooks/useCoupon.ts", 1],
    ["tests/checkout/coupon.spec.ts", 1],
  ],
};

export const idle: SessionView = { ...working, id: "s2", name: null, project: "api", state: "idle", running: [], todos: [], files: [], items: items.slice(0, 3), tokens: "12k in · 1k out" };
