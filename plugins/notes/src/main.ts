// The notes tools Claude calls, and the only writer of the notes. Pages send saves here and tools append here, one
// at a time, each against what storage holds at that moment.

import { appendSection, mergeSave, readNotes, writeNotes, type NotesMessage, type SaveMessage } from "./lib/notes";

let panes: WingsPane[] = [];
wings.onPanes((list) => (panes = list));

/** Writes run one after another, so a read-then-write never interleaves with another. */
let queue: Promise<unknown> = Promise.resolve();
const serial = <T>(job: () => Promise<T>): Promise<T> => {
  const run = queue.then(job, job);
  queue = run.catch(() => {});
  return run;
};

const notify = (message: NotesMessage) => wings.broadcast(message).catch(console.error);

wings.onBroadcast((message) => {
  const m = message as Partial<SaveMessage> | null;
  if (m?.type !== "save" || typeof m.projectId !== "string" || typeof m.base !== "string" || typeof m.text !== "string" || typeof m.rid !== "string") return;
  const { projectId, base, text, rid } = m;
  void serial(async () => {
    const name = panes.find((p) => p.projectId === projectId)?.project ?? "this project";
    try {
      await writeNotes(projectId, mergeSave((await readNotes(projectId)).text, base, text), name);
      await notify({ type: "notes", projectId, rid });
    } catch (error) {
      await notify({ type: "notes", projectId, rid, error: error instanceof Error ? error.message : String(error) });
    }
  });
});

/** The project of the calling terminal, or of the terminal inside the `project` folder when one is given. */
function resolve(input: { project?: unknown }, paneId: string | null): { id: string; name: string } {
  const folder = typeof input.project === "string" && input.project ? input.project.replace(/\/+$/, "") : null;
  const within = (inner: string, outer: string) => inner === outer || inner.startsWith(`${outer}/`);
  const pane = folder ? panes.find((p) => within(p.cwd, folder) || within(folder, p.cwd)) : panes.find((p) => p.paneId === paneId);
  if (!pane) {
    throw new Error(folder ? `No terminal in Wings is open in ${folder}. Open a terminal in that project, or leave out project.` : "Can't tell which project this call comes from. Pass project, an absolute path inside the project.");
  }
  return { id: pane.projectId, name: pane.project };
}

wings.onTool("read", async (input: { project?: unknown }, { paneId }) => {
  const project = resolve(input, paneId);
  const { text } = await readNotes(project.id);
  return text.trim() ? text : `No notes yet for ${project.name}.`;
});

wings.onTool("write", (input: { project?: unknown; text?: unknown }, { paneId }) => {
  if (typeof input.text !== "string" || !input.text.trim()) throw new Error("text is required.");
  const text = input.text;
  const project = resolve(input, paneId);
  return serial(async () => {
    await writeNotes(project.id, appendSection((await readNotes(project.id)).text, text), project.name);
    await notify({ type: "notes", projectId: project.id });
    return `Added to the notes for ${project.name}.`;
  });
});
