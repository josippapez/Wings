// The Issues main script: Claude's tools and hooks, the todo card on each pane, the operations the pages ask for, and each project's section in the sidebar.
// Every change goes through `write`, one at a time.

import { registerTools } from "wings-plugin-sdk/tools";
import { ch } from "./lib/messages";
import { registerHooks } from "./main/hooks";
import { onPaneList } from "./main/todo";
import { loadProject, write } from "./main/write";
import { tools } from "./tools";

registerTools(tools);
registerHooks();
wings.onPanes(onPaneList);

ch.answer("op", async ({ projectId, projectName, op }) => {
  await write(projectId, projectName, op, "you");
});

/** Picks up every project as soon as Wings tells us which exist, and again when that changes. */
wings.onProjects((list) => {
  for (const { id, name } of list) loadProject(id, name).catch(console.error);
});
