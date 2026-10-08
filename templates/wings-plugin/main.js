// @ts-check
/// <reference path="./wings.d.ts" />

// Shows each pane's git branch as a badge. Start here and replace it with your own plugin.

wings.onPanes(async (panes) => {
  for (const pane of panes) {
    const out = await wings.exec("git", ["branch", "--show-current"], { cwd: pane.cwd });
    const branch = out.code === 0 ? out.stdout.trim() : "";
    void wings.setBadge(
      pane.paneId,
      branch
        ? {
            label: branch,
            tone: "info",
            title: pane.project || "Git branch",
            rows: [{ label: "Folder", value: pane.cwd }],
            actions: [{ id: "refresh", label: "Refresh" }],
          }
        : null,
    );
  }
});

wings.onAction(async ({ actionId }) => {
  if (actionId === "refresh") console.log("Refresh clicked");
});
