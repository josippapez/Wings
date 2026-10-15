// The Issues main script: Claude's tools, the operations the pages ask for, and each project's section in the sidebar.
// Every change goes through `write`, one at a time.

import { registerTools } from "wings-plugin-sdk/tools";
import { ch } from "./lib/messages";
import { loadProject, write } from "./main/write";
import { tools } from "./tools";

registerTools(tools);

ch.answer("op", async ({ projectId, projectName, op }) => {
  await write(projectId, projectName, op, "you");
});

/** Picks up every project as soon as Wings tells us which exist, and again when that changes. */
wings.onProjects((list) => {
  for (const { id, name } of list) loadProject(id, name).catch(console.error);
});
