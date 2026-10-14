// The issue tools Claude calls, and the only writer of the issues. Pages send operations here and tools run the same
// ones, one at a time, each against what storage holds at that moment. After each change it refreshes the project's
// section in the sidebar. Which session wrote a tool call's entry comes from the pane the call was made in.

import { pollWhileFocused } from "wings-plugin-sdk/poll";
import { scanProject } from "./lib/autolink";
import { applyOp, canParent, childrenOf, migrateLegacy, openIssues, progress, readStore, resolvePane, STATUS_LABEL, TYPE_LABEL, writeStore, type Actor, type Issue, type IssuesMessage, type Op, type OpMessage } from "./lib/issues";
import { formatIssue, formatList, parseTool } from "./lib/tools";

const SECTION = "tasks";
/** The project section shows this many rows. */
const SECTION_ROWS = 50;

let panes: WingsPane[] = [];
wings.onPanes((list) => (panes = list));

/** Writes run one after another, so a read-then-write never interleaves with another. */
let queue: Promise<unknown> = Promise.resolve();
const serial = <T>(job: () => Promise<T>): Promise<T> => {
  const run = queue.then(job, job);
  queue = run.catch(() => {});
  return run;
};

const notify = (message: IssuesMessage) => wings.broadcast(message).catch(console.error);

/** What the project section last got, per project, so an unchanged list sends nothing. */
const pushed = new Map<string, string>();

/** A muted line under a row for what isn't a plain task: its type, and an epic's or feature's progress. */
function detail(issues: Issue[], i: Issue): string {
  if (i.type === "task") return "";
  const { done, total } = progress(childrenOf(issues, i.key));
  return canParent(i.type) && total ? `${TYPE_LABEL[i.type]} · ${done} of ${total} done` : TYPE_LABEL[i.type];
}

function pushSection(projectId: string, issues: Issue[]): Promise<void> {
  const rows = openIssues(issues)
    .slice(0, SECTION_ROWS)
    .map((i) => ({
      id: i.key,
      label: `${i.key} ${i.title}`.slice(0, 120),
      ...(detail(issues, i) ? { detail: detail(issues, i) } : {}),
      ...(i.status === "todo" ? {} : { badge: { text: STATUS_LABEL[i.status], tone: i.status === "in_progress" ? ("info" as const) : ("warning" as const) } }),
      open: { view: "issue", args: { key: i.key, projectId } },
    }));
  const signature = JSON.stringify(rows);
  if (pushed.get(projectId) === signature) return Promise.resolve();
  pushed.set(projectId, signature);
  return wings.setProjectSection(projectId, SECTION, rows.length ? rows : null).catch((error) => {
    pushed.delete(projectId);
    console.error(error);
  });
}

/** The first look for links goes back this far, for a project never scanned. */
const FIRST_SCAN_MS = 14 * 24 * 3_600_000;
/** Each scan starts this much before the last one ended, so a clock a little off loses no commit. Commits already linked are skipped. */
const OVERLAP_MS = 10 * 60_000;
const SCAN_MS = 60_000;

let projects: WingsProject[] = [];
let scanning = false;

/**
 * Finds commits and branches that name an issue's key, in each project's repository, and links them. The last scan time
 * of each project is kept in storage, so a restart picks up where it stopped. Runs on start and every minute while Wings is focused.
 */
async function scan(): Promise<void> {
  if (scanning) return;
  scanning = true;
  try {
    for (const project of projects) {
      try {
        const started = Date.now();
        const store = await readStore(project.id, project.name);
        const last = await wings.storage.get(`scan:${project.id}`);
        const since = (typeof last === "number" ? last : started - FIRST_SCAN_MS) - OVERLAP_MS;
        const { ops, calls } = await scanProject((program, args, options) => wings.exec(program, args, options), project, panes.filter((p) => p.projectId === project.id).map((p) => p.cwd), store, since);
        if (calls) console.log(`[tasks] ${project.name}: ${calls} git ${calls === 1 ? "call" : "calls"}, ${ops.length} links found`);
        if (ops.length) {
          await serial(async () => {
            let fresh = await readStore(project.id, project.name);
            let changed = false;
            for (const op of ops) {
              try {
                const applied = applyOp(fresh, op, "git");
                if (applied.changed) ((fresh = applied.store), (changed = true));
              } catch (error) {
                console.error(error);
              }
            }
            if (!changed) return;
            await writeStore(project.id, fresh);
            await notify({ type: "issues", projectId: project.id });
            await pushSection(project.id, fresh.issues);
          });
        }
        await wings.storage.set(`scan:${project.id}`, started);
      } catch (error) {
        // Not a git repository, git missing or a folder gone: the next scan tries again.
        console.error(error);
      }
    }
  } finally {
    scanning = false;
  }
}
pollWhileFocused(() => void scan(), SCAN_MS);

/** Moves old tasks into issues, once per project, and fills every project's section, as soon as Wings tells us which exist. */
wings.onProjects((list) => {
  const first = projects.length === 0;
  projects = list;
  for (const { id, name } of list) {
    void serial(async () => {
      try {
        await migrateLegacy(id, name);
      } catch (error) {
        console.error(error);
      }
      await pushSection(id, (await readStore(id, name)).issues);
    });
  }
  // Once on start, after the stores are migrated.
  if (first && list.length) void serial(async () => {}).then(scan);
});

/** Applies `op` as `by` to a project's issues, saves, and tells the pages. */
function run(projectId: string, projectName: string, op: Op, by: Actor, rid?: string): Promise<{ issue: Issue | null; changed: boolean }> {
  return serial(async () => {
    try {
      const applied = applyOp(await readStore(projectId, projectName), op, by);
      if (applied.changed) {
        await writeStore(projectId, applied.store);
        await notify({ type: "issues", projectId, rid });
        await pushSection(projectId, applied.store.issues);
      } else if (rid) await notify({ type: "issues", projectId, rid });
      return { issue: applied.issue, changed: applied.changed };
    } catch (error) {
      if (rid) await notify({ type: "issues", projectId, rid, error: error instanceof Error ? error.message : String(error) });
      throw error;
    }
  });
}

wings.onBroadcast((message) => {
  const m = message as Partial<OpMessage> | null;
  if (m?.type !== "op" || typeof m.projectId !== "string" || typeof m.projectName !== "string" || typeof m.rid !== "string" || !m.op || typeof m.op !== "object") return;
  run(m.projectId, m.projectName, m.op, "you", m.rid).catch(() => {});
});

const resolve = (paneId: string | null) => resolvePane(panes, paneId);

wings.onTool("list", async (input, { paneId }) => {
  const call = parseTool("list", input);
  const project = resolve(paneId);
  const store = await readStore(project.projectId, project.projectName);
  return call.tool === "list" ? formatList(store.issues, call.status, project.projectName, call.type) : "";
});

wings.onTool("get", async (input, { paneId }) => {
  const call = parseTool("get", input);
  const project = resolve(paneId);
  const store = await readStore(project.projectId, project.projectName);
  const issue = call.tool === "get" ? store.issues.find((i) => i.key === call.key) : undefined;
  if (!issue) throw new Error(`No issue ${call.tool === "get" ? call.key : ""} in ${project.projectName}. Use list to see the keys.`);
  return formatIssue(issue, store.issues);
});

for (const name of ["create", "update", "log"] as const) {
  wings.onTool(name, async (input, { paneId }) => {
    const call = parseTool(name, input);
    const project = resolve(paneId);
    if (call.tool !== "write") throw new Error("Unexpected tool input.");
    const { issue, changed } = await run(project.projectId, project.projectName, call.op, project.actor);
    if (!issue) return "Done.";
    if (name === "create") return `Created ${issue.key} in ${project.projectName}. Log progress with log, and set the status with update as it moves.`;
    if (name === "log") return `Logged on ${issue.key}.`;
    return changed ? `Updated ${issue.key}: ${issue.title} [${issue.status}].` : `${issue.key} already has that. Nothing changed.`;
  });
}
