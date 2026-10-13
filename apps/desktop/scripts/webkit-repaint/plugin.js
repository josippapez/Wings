// The test plugin: one badge on every pane. Served for each plugin id in stub.js, so each id adds one badge.
const id = document.currentScript.src.split("/")[3];
const badge = {
  changes: { label: "main", tone: "neutral", counts: [{ icon: "changed", value: 4 }] },
  pr: { label: "#12 Merged", tone: "merged", icon: "pr-merged" },
  context: { label: "Model · 187k", tone: "neutral" },
}[id];
wings.onPanes((panes) => {
  for (const pane of panes) wings.setBadge(pane.paneId, badge);
});
