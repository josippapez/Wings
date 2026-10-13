// Made-up tasks for the fixtures.
import type { Task } from "../src/lib/tasks";

const now = Date.now();
const ago = (h: number) => now - h * 3_600_000;

export const project = { id: "p1", name: "web", path: "/work/acme/web", branch: "feature/checkout-summary" } as WingsProject;

export const tasks: Task[] = [
  { id: "t1", title: "Show the delivery estimate under the cart total", notes: "Use the postcode from the saved address. Fall back to the shop default when there is none.", status: "todo", createdAt: ago(30), updatedAt: ago(30) },
  { id: "t2", title: "Fix the focus ring on the promo code field", status: "todo", links: { branch: "fix/promo-focus" }, createdAt: ago(28), updatedAt: ago(26) },
  { id: "t3", title: "Write a migration that backfills order totals for rows created before the currency column existed", notes: "Run it in batches of 500.", status: "todo", createdAt: ago(20), updatedAt: ago(20) },
  { id: "t4", title: "Checkout summary card", notes: "Items, shipping, tax and total in one card. Collapses under 640px.", status: "doing", links: { branch: "feature/checkout-summary" }, createdAt: ago(48), updatedAt: ago(2) },
  { id: "t5", title: "Retry failed payment webhooks", status: "doing", createdAt: ago(40), updatedAt: ago(5) },
  { id: "t6", title: "Upgrade the date picker", status: "done", links: { branch: "chore/date-picker" }, createdAt: ago(100), updatedAt: ago(24) },
  { id: "t7", title: "Remove the old banner experiment", notes: "Flag deleted, tests updated.", status: "done", createdAt: ago(90), updatedAt: ago(50) },
];
