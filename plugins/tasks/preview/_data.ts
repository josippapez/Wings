// Made-up issues for the fixtures.
import type { Issue, LogEntry } from "../src/lib/domain";

const now = Date.now();
const ago = (h: number) => now - h * 3_600_000;

export const project = { id: "p1", name: "web", path: "/work/acme/web", branch: "feature/checkout-summary" } as WingsProject;

const s1 = { id: "6f1c2a90-3b7e-4d51-9a02-5c8e1d4b7a11", name: "checkout summary" };
const s2 = { id: "b04e77d2-91aa-4c3f-8e65-0d2f9c6a3e42", name: "fix the totals" };
const s3 = { id: "e8d3a1c5-27f4-4b90-b1d6-7a5c0e9f2b38" };

const entry = (at: number, by: LogEntry["by"], kind: LogEntry["kind"], text: string): LogEntry => ({ at, by, kind, text });

const longLog: LogEntry[] = [
  entry(ago(52), "you", "created", "Created"),
  entry(ago(51), "you", "status", "To do → In progress"),
  entry(ago(50), s1, "note", "Read the cart code. The summary is built in `CartSummary.tsx` from `useCart()`. Plan: one card with items, shipping, tax and a total, no new data fetching."),
  entry(ago(49), "git", "link", "Branch: feature/checkout-summary"),
  entry(ago(49), s1, "link", "Parent: WEB-10"),
  entry(ago(46), s1, "note", "Card renders with items and total. Shipping and tax still hard-coded.\n\nNext: read the rates from the order response."),
  entry(ago(44), "git", "link", "Commit 3fa91c2: WEB-4 Checkout summary card"),
  entry(ago(40), "you", "note", "Collapse it under 640px, like the delivery card does."),
  entry(ago(36), s2, "note", "Picked this up. The tax line rounds per item, so a basket of three can be a cent off the total. Switching to rounding once on the sum."),
  entry(ago(35), "git", "link", "Commit 8b27de0: WEB-4 Round the totals once, on the sum"),
  entry(ago(30), s2, "note", "Totals match the order response for the 12 baskets in the fixtures.\n\n- Rounding done once on the sum\n- Test added for the three-item case\n\nNext: the narrow layout."),
  entry(ago(26), s3, "note", "Narrow layout done: items fold into a list under 640px. Screenshots in the PR."),
  entry(ago(25), s3, "link", "PR: https://example.com/acme/web/pull/412"),
  entry(ago(25), s3, "status", "In progress → In review"),
  entry(ago(20), "you", "note", "Review: the empty cart still shows a zero total. Please hide the card then."),
  entry(ago(19), "you", "status", "In review → In progress"),
  entry(ago(14), s3, "note", "Empty cart now hides the card. Added a test."),
  entry(ago(13), "git", "link", "Commit c41d5e9: Hide the summary for an empty cart (web-4)"),
  entry(ago(9), s3, "status", "In progress → In review"),
  entry(ago(5), "you", "note", "Approved. Waiting on the release branch."),
  entry(ago(2), s3, "note", "Waiting for review on the release branch, nothing else to do here. If it comes back, the open point is the focus order of the promo field."),
];

type Base = Omit<Issue, "type">;
const base: Base[] = [
  { key: "WEB-1", title: "Show the delivery estimate under the cart total", description: "## Goal\nShow when the order arrives, under the total.\n\n## Scope\n- Use the postcode from the saved address\n- Fall back to the shop default when there is none\n\n## Done when\n- [ ] The estimate shows for a saved address\n- [ ] A cart with no address shows the default", status: "todo", sessions: [], log: [entry(ago(30), "you", "created", "Created")], createdAt: ago(30), updatedAt: ago(30) },
  { key: "WEB-2", title: "Fix the focus ring on the promo code field", status: "todo", links: { branch: "fix/promo-focus" }, sessions: [], log: [entry(ago(28), "you", "created", "Created")], createdAt: ago(28), updatedAt: ago(26) },
  { key: "WEB-3", title: "Write a migration that backfills order totals for rows created before the currency column existed", description: "Run it in batches of 500.", status: "todo", sessions: [{ id: s3.id, seen: ago(18) }], log: [entry(ago(20), "you", "created", "Created")], createdAt: ago(20), updatedAt: ago(20) },
  { key: "WEB-4", title: "Checkout summary card", description: "## Goal\nShow the buyer what they are about to pay for in one place, right above the pay button: the items, shipping, tax and the total.\n\n## Scope\n- One card built from `useCart()`, no new data fetching\n- Shipping and tax come from the order response, not hard-coded\n- Rounding happens once, on the sum, so the lines always add up to the total\n- Under 640px the items fold into a list, like the delivery card does\n\n## Out of scope\nPromo codes and gift cards. They get their own issue once this ships.\n\n## Done when\n- [x] The card shows items, shipping, tax and total\n- [x] Totals match the order response for the fixture baskets\n- [ ] An empty cart hides the card\n- [ ] The focus order through the promo field is checked with a keyboard\n\n```ts\nconst total = roundOnce(items.reduce((sum, i) => sum + i.price * i.qty, 0) + shipping + tax);\n```", status: "in_review", links: { branch: "feature/checkout-summary", pr: "https://example.com/acme/web/pull/412", commits: ["3fa91c2", "8b27de0", "c41d5e9"] }, sessions: [{ ...s1, seen: ago(46) }, { ...s2, seen: ago(30) }, { id: s3.id, seen: ago(2) }], log: longLog, createdAt: ago(52), updatedAt: ago(2) },
  { key: "WEB-5", title: "Retry failed payment webhooks", status: "in_progress", sessions: [{ ...s2, seen: ago(5) }], log: [entry(ago(40), "you", "created", "Created"), entry(ago(5), s2, "status", "To do → In progress")], createdAt: ago(40), updatedAt: ago(5) },
  { key: "WEB-6", title: "Upgrade the date picker", status: "in_progress", links: { branch: "chore/date-picker" }, sessions: [], log: [entry(ago(60), "you", "created", "Created")], createdAt: ago(60), updatedAt: ago(8) },
  { key: "WEB-7", title: "Remove the old banner experiment", description: "Flag deleted, tests updated.", status: "done", sessions: [{ ...s1, seen: ago(50) }], log: [entry(ago(90), "you", "created", "Created"), entry(ago(50), s1, "status", "In progress → Done")], createdAt: ago(90), updatedAt: ago(50) },
  { key: "WEB-8", title: "Switch the image CDN", status: "done", sessions: [], log: [entry(ago(120), "you", "created", "Created")], createdAt: ago(120), updatedAt: ago(100) },
  { key: "WEB-9", title: "Dark mode for the invoice PDF", status: "canceled", sessions: [], log: [entry(ago(200), "you", "created", "Created"), entry(ago(150), "you", "status", "To do → Canceled")], createdAt: ago(200), updatedAt: ago(150) },
];

const extra: Record<string, Partial<Issue>> = {
  "WEB-1": { type: "feature", parent: "WEB-10", priority: "medium" },
  "WEB-2": { type: "bug", parent: "WEB-10", priority: "urgent", blockedBy: ["WEB-5"], related: ["WEB-4"] },
  "WEB-3": { parent: "WEB-10", status: "done" },
  "WEB-4": { type: "feature", parent: "WEB-10", priority: "high", related: ["WEB-2", "WEB-6"] },
  "WEB-5": { parent: "WEB-10", priority: "high" },
  "WEB-6": { related: ["WEB-4"], parent: "WEB-13" },
  "WEB-7": { parent: "WEB-10" },
  "WEB-8": { parent: "WEB-10" },
  "WEB-9": { type: "bug" },
};
const epic: Base = { key: "WEB-10", title: "Checkout redesign", description: "## Goal\nA checkout that fits on one screen and shows the total early.\n\n## Scope\n- The summary card, the delivery estimate and the promo field\n- Payment webhooks keep working throughout\n\n## Done when\n- [ ] Every child issue is done\n- [ ] The checkout passes the keyboard walk-through\n\n## Validation", status: "in_progress", sessions: [], log: [entry(ago(80), "you", "created", "Created"), entry(ago(52), "you", "link", "Child: WEB-4"), entry(ago(28), s2, "link", "Child: WEB-2")], createdAt: ago(80), updatedAt: ago(5) };
const more: Issue[] = [
  { key: "WEB-12", type: "task", parent: "WEB-4", priority: "low", title: "Hide the summary card for an empty cart", status: "todo", sessions: [], log: [entry(ago(20), "you", "created", "Created")], createdAt: ago(20), updatedAt: ago(20) },
  { key: "WEB-13", type: "epic", title: "Faster product pages", status: "todo", sessions: [], log: [entry(ago(70), "you", "created", "Created")], createdAt: ago(70), updatedAt: ago(8) },
  { key: "WEB-14", type: "task", title: "Update the README for the new checkout", status: "todo", sessions: [], log: [entry(ago(3), "you", "created", "Created")], createdAt: ago(3), updatedAt: ago(3) },
];
export const issues: Issue[] = [...[...base, epic].map((i) => ({ type: "task" as const, ...i, ...(i.key === "WEB-10" ? { type: "epic" as const } : {}), ...extra[i.key] })), ...more];

export const emptyIssue: Issue = { key: "WEB-11", type: "task", title: "Look into the slow product list", status: "todo", sessions: [], log: [entry(ago(1), "you", "created", "Created")], createdAt: ago(1), updatedAt: ago(1) };
export const longIssue = issues[3];
export const shortIssue = issues[0];
export const epicIssue = issues[9];
export const bugIssue = issues[1];
