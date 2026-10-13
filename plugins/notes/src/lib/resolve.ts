// Which project a tool call is for.

/**
 * The project of the calling terminal, or, when `project` names a folder, of the terminal open in that folder or
 * below it. A terminal above the folder (one in `~`) says nothing about it. Terminals of several projects below the
 * folder are ambiguous unless one sits exactly in it, so that is an error rather than a guess.
 */
export function resolveProject(panes: Pick<WingsPane, "paneId" | "cwd" | "project" | "projectId">[], project: string | null, paneId: string | null): { id: string; name: string } {
  const folder = project ? project.replace(/\/+$/, "") : null;
  const within = (inner: string, outer: string) => inner === outer || inner.startsWith(`${outer}/`);
  if (!folder) {
    const pane = panes.find((p) => p.paneId === paneId);
    if (!pane) throw new Error("Can't tell which project this call comes from. Pass project, an absolute path inside the project.");
    return { id: pane.projectId, name: pane.project };
  }
  const inside = panes.filter((p) => within(p.cwd, folder));
  const exact = inside.filter((p) => p.cwd === folder);
  const candidates = exact.length ? exact : inside;
  const projects = new Map(candidates.map((p) => [p.projectId, p.project]));
  if (!projects.size) throw new Error(`No terminal in Wings is open in ${folder} or below it. Open a terminal in that project, or leave out project.`);
  if (projects.size > 1) throw new Error(`Terminals of several projects are open under ${folder} (${[...projects.values()].join(", ")}). Pass the project's own folder.`);
  const [id, name] = [...projects][0];
  return { id, name };
}
