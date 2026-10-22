// The Issues main script: Claude's tools and hooks, the todo card on each pane, the operations the pages ask for, and each project's section in the sidebar.
// Every change goes through `write`, one at a time.

import { registerTools } from "wings-plugin-sdk/tools";
import { ch } from "./lib/messages";
import { registerHooks } from "./main/hooks";
import { resumeCommand } from "./lib/commands";
import { onSectionPanes } from "./main/section";
import { onPaneList, paneOf } from "./main/todo";
import { loadProject, write } from "./main/write";
import { tools } from "./tools";

registerTools(tools);
registerHooks();
wings.onPanes((list) => {
  onPaneList(list);
  onSectionPanes(list);
});

ch.answer("op", async ({ projectId, projectName, op }) => {
  await write(projectId, projectName, op, "you");
});

// A running session is focused, never started a second time.
ch.answer("openSession", async ({ session, cwd }) => {
  const pane = paneOf(session.id);
  if (pane) await wings.focusPane(pane.paneId);
  else await wings.openPane({ command: resumeCommand(session), cwd });
});

/** Picks up every project as soon as Wings tells us which exist, and again when that changes. */
wings.onProjects((list) => {
  for (const { id, name } of list) loadProject(id, name).catch(console.error);
});
