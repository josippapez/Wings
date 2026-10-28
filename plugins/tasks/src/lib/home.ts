// Where a project's issues live. A worktree Wings made for an agent is a project of its own, but its issues belong to the
// project it is a worktree of: one store, one set of keys, one sidebar section.

/** What a project says about itself, as `wings.projects()` and a tool call's context give it. */
export type ProjectRef = { id: string; name: string; parentId?: string };

/** The project whose store holds `project`'s issues: its main project when it is a worktree Wings made, else itself. */
export function homeIn(project: ProjectRef, projects: readonly ProjectRef[]): { id: string; name: string } {
  if (!project.parentId) return { id: project.id, name: project.name };
  const parent = projects.find((p) => p.id === project.parentId);
  // A parent Wings no longer lists leaves the worktree its own store: nothing else could name it.
  return parent ? { id: parent.id, name: parent.name } : { id: project.id, name: project.name };
}

/** `homeIn` against the projects Wings lists now. The one place every tool, hook and page asks. */
export async function homeOf(project: ProjectRef): Promise<{ id: string; name: string }> {
  return project.parentId ? homeIn(project, await wings.projects()) : { id: project.id, name: project.name };
}

/** The id of the project whose store holds the issues and todo lists of project `id` (a pane's project, say). */
export async function homeIdOf(id: string): Promise<string> {
  const projects = await wings.projects();
  const project = projects.find((p) => p.id === id);
  return project ? homeIn(project, projects).id : id;
}

/** The project `ref` names, by folder path, id or name (any case), for reading another project's issues. */
export function pickProject<P extends ProjectRef & { path: string }>(projects: readonly P[], ref: string): P {
  const want = ref.trim().replace(/\/+$/, "");
  const found = projects.find((p) => p.path.replace(/\/+$/, "") === want || p.id === want);
  if (found) return found;
  const named = projects.filter((p) => !p.parentId && p.name.toLowerCase() === want.toLowerCase());
  if (named.length === 1) return named[0];
  if (named.length > 1) throw new Error(`${named.length} projects are named "${want}". Pass the folder path of one: ${named.map((p) => p.path).join(", ")}.`);
  throw new Error(`No project "${want}" in Wings. The projects are: ${projects.filter((p) => !p.parentId).map((p) => p.name).join(", ") || "none yet"}.`);
}
