// Fixture main for the real native Sprint block: it answers the block's sprint query with made-up data only. Run as the
// plugin's main, in place of dist/native/main.js, by the native app's tests and screenshots.
const items = [
  { id: 1432, title: "Sync client gives up after the first failed request", type: "Bug", state: "Active", assignee: "Ada Lovelace", mine: true },
  { id: 1433, title: "Add retry with backoff", type: "Task", state: "Active", assignee: "Ada Lovelace", mine: true },
  { id: 1434, title: "Show a notice while retrying", type: "Task", state: "New", assignee: "Grace Hopper" },
  { id: 1440, title: "Settings page uses the new form components", type: "Product Backlog Item", state: "New" },
  { id: 1441, title: "Date picker overflows on small screens", type: "Bug", state: "Resolved", assignee: "Alan Turing" },
  { id: 1443, title: "Write the sync retry runbook", type: "Task", state: "Closed", assignee: "Ada Lovelace", mine: true },
];
const sprint = ({ mine }) => {
  const shown = items.filter((i) => !mine || i.mine).map(({ mine: _, ...item }) => item);
  return { org: "https://dev.azure.com/acme", project: "Web", sprints: ["Sprint 14"], items: shown, total: shown.length };
};
const respond = (message) => wings.broadcast({ "wings:sdk": 1, ...message });
wings.onBroadcast((m) => {
  if (m?.["wings:sdk"] !== 1 || m.kind !== "sub") return;
  for (const [name, key] of m.subs) if (name === "sprint") void respond({ kind: "state", name, key, data: sprint(key), at: Date.now() });
});
