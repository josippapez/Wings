// Made-up issues for the native app's screenshots of the real Issues bundles (apps/native/examples/shot_plugins.rs
// --tasks): run as a page of the plugin before its main starts, it writes them to wings.storage the way
// src/lib/storage.ts keeps them, for the project `__PROJECT__`, then shows "Seeded".
const project = "__PROJECT__";
const now = Date.now();
const ago = (h) => now - h * 3_600_000;

/** src/lib/storage.ts's slotOf. */
function slotOf(projectId) {
  let h1 = 0xdeadbeef;
  let h2 = 0x41c6ce57;
  for (let i = 0; i < projectId.length; i++) {
    const c = projectId.charCodeAt(i);
    h1 = Math.imul(h1 ^ c, 2654435761);
    h2 = Math.imul(h2 ^ c, 1597334677);
  }
  h1 = Math.imul(h1 ^ (h1 >>> 16), 2246822507) ^ Math.imul(h2 ^ (h2 >>> 13), 3266489909);
  h2 = Math.imul(h2 ^ (h2 >>> 16), 2246822507) ^ Math.imul(h1 ^ (h1 >>> 13), 3266489909);
  return (h2 >>> 0).toString(16).padStart(8, "0") + (h1 >>> 0).toString(16).padStart(8, "0");
}

const agent = { id: "6f1c2a90-3b7e-4d51-9a02-5c8e1d4b7a11", name: "checkout summary" };
const created = (h) => ({ at: ago(h), by: "you", kind: "created", text: "Created" });
const issue = (key, title, status, type, h, more = {}) => ({ key, title, status, type, sessions: [], log: [created(h)], createdAt: ago(h), updatedAt: ago(h / 2), ...more });
const issues = [
  issue("WEB-1", "Show the delivery estimate under the cart total", "todo", "task", 30, { parent: "WEB-10", priority: "medium", description: "## Goal\nShow when the order arrives, under the total.\n\n## Done when\n- [ ] The estimate shows for a saved address\n- [ ] A cart with no address shows the default" }),
  issue("WEB-2", "Fix the focus ring on the promo code field", "todo", "bug", 28, { priority: "high", links: { branch: "fix/promo-focus" } }),
  issue("WEB-3", "Backfill order totals for rows created before the currency column existed", "in_progress", "task", 20, { parent: "WEB-10", sessions: [{ ...agent, seen: ago(2) }] }),
  issue("WEB-4", "Checkout summary card", "in_review", "feature", 52, {
    parent: "WEB-10",
    priority: "urgent",
    description: "## Goal\nShow the buyer what they are about to pay for in one place, right above the pay button.\n\n## Scope\n- One card built from `useCart()`, no new data fetching\n- Rounding happens once, on the sum\n\n## Done when\n- [x] The card shows items, shipping, tax and total\n- [ ] An empty cart hides the card",
    links: { branch: "feature/checkout-summary", pr: "https://example.com/acme/web/pull/412", commits: ["3fa91c2", "8b27de0"] },
    sessions: [{ ...agent, seen: ago(3) }],
    log: [
      created(52),
      { at: ago(50), by: agent, kind: "note", text: "Read the cart code. Plan: one card with items, shipping, tax and a total, no new data fetching." },
      { at: ago(26), by: agent, kind: "status", text: "In progress → In review" },
      { at: ago(5), by: "you", kind: "note", text: "Approved. Waiting on the release branch." },
    ],
  }),
  issue("WEB-5", "Rename the cart store to match the checkout naming", "done", "task", 60),
  issue("WEB-6", "Upgrade the date picker", "done", "task", 70, { parent: "WEB-10" }),
  issue("WEB-10", "Checkout redesign", "in_progress", "epic", 80, { description: "The checkout pages on the new design system." }),
];

const slot = slotOf(project);
Promise.all([
  ...issues.map((i) => wings.storage.set(`issues/${slot}/${i.key}`, i)),
  wings.storage.set(`issues/${slot}`, { v: 3, path: project, prefix: "WEB", next: 11 }),
]).then(() => wings.render(JSON.stringify({ id: 0, type: "#root", props: {}, children: [{ id: 1, type: "#text", text: "Seeded" }] })));
