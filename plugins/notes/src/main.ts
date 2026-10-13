// Project info: the tools Claude calls, and the only writer of the info. Pages send their changes here and tools
// apply theirs here, one at a time, each against what storage holds at that moment.

import { applyOp, renderInfo, emptyInfo, type Op, type Who } from "./lib/model";
import type { InfoMessage, OpMessage } from "./lib/messages";
import { resolveProject } from "./lib/resolve";
import { legacyProjects, loadInfo, saveInfo } from "./lib/store";

let panes: WingsPane[] = [];
wings.onPanes((list) => (panes = list));

/** Writes run one after another, so a read-then-write never interleaves with another. */
let queue: Promise<unknown> = Promise.resolve();
const serial = <T>(job: () => Promise<T>): Promise<T> => {
  const run = queue.then(job, job);
  queue = run.catch(() => {});
  return run;
};

const notify = (message: InfoMessage) => wings.broadcast(message).catch(console.error);

/** Applies `op` to a project's info and saves it. */
const change = (projectId: string, op: Op, by: Who) =>
  serial(async () => {
    const info = (await loadInfo(projectId)) ?? emptyInfo();
    const next = applyOp(info, op, by, Date.now());
    if (next !== info) await saveInfo(projectId, next);
  });

// Old notes move into the new shape once, in the same queue as every write.
void legacyProjects()
  .then((ids) => ids.forEach((id) => void serial(() => loadInfo(id)).catch(console.error)))
  .catch(console.error);

wings.onBroadcast((message) => {
  const m = message as Partial<OpMessage> | null;
  if (m?.type !== "op" || typeof m.projectId !== "string" || typeof m.rid !== "string" || !m.op || typeof m.op !== "object") return;
  const { projectId, rid, op } = m as OpMessage;
  change(projectId, op, "you").then(
    () => notify({ type: "info", projectId, rid }),
    (error) => notify({ type: "info", projectId, rid, error: error instanceof Error ? error.message : String(error) }),
  );
});

const resolve = (input: { project?: unknown }, paneId: string | null) => resolveProject(panes, typeof input.project === "string" ? input.project : null, paneId);

type Input = { project?: unknown; [key: string]: unknown };

/** Runs an agent's change and tells pages. */
async function agentChange(input: Input, paneId: string | null, op: Op, done: (name: string) => string) {
  const project = resolve(input, paneId);
  await change(project.id, op, "agent");
  await notify({ type: "info", projectId: project.id });
  return done(project.name);
}

wings.onTool("read", async (input: Input, { paneId }) => {
  const project = resolve(input, paneId);
  return renderInfo((await serial(() => loadInfo(project.id))) ?? emptyInfo(), project.name);
});

wings.onTool("set_fact", (input: Input, { paneId }) =>
  agentChange(input, paneId, { kind: "setFact", key: input.key as string, value: input.value as string }, (name) => `Saved the fact for ${name}.`),
);

wings.onTool("remove_fact", (input: Input, { paneId }) =>
  agentChange(input, paneId, { kind: "removeFact", key: input.key as string }, (name) => `Removed the fact from ${name}.`),
);

wings.onTool("write_section", (input: Input, { paneId }) =>
  agentChange(input, paneId, { kind: "writeSection", title: input.title as string, body: input.markdown as string }, (name) => `Wrote the section in the info for ${name}.`),
);

wings.onTool("append", (input: Input, { paneId }) =>
  agentChange(input, paneId, { kind: "appendSection", title: input.title as string, body: input.markdown as string }, (name) => `Added to the section in the info for ${name}.`),
);
