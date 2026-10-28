// The three tools Claude, and `wings schedules <tool> key=value`, call. The build writes these descriptions and schemas into the
// manifest's mcpTools. The build loads this file in Node with a wings that does nothing, so nothing here runs at the top level.

import { s, tool } from "wings-plugin-sdk/tools";
import { DEFAULT_MIN_FREE_GB, DEFAULT_PROMPT, describeCheck, resolveProject } from "./lib/assigned";
import { formatList } from "./lib/format";
import { addWatch, checkOne, removeWatch } from "./main/assigned";
import { removeSchedule } from "./main/schedules";
import { readSchedules, readWatches } from "./main/store";

export const tools = {
  list: tool({
    description: "Lists every schedule and every assigned entry, one line each: id, kind, project, agent, enabled, and for an assigned entry its source, last check, the states it asks about and the tickets assigned to the user with their state and whether each is asked about. Then the tickets waiting for the user's answer. Read-only.",
    run: async () => {
      const [schedules, watches, projects] = await Promise.all([readSchedules(), readWatches(), wings.projects()]);
      return formatList(schedules, watches, (id) => projects.find((p) => p.id === id)?.name ?? "missing project", Date.now());
    },
  }),

  add_assigned: tool({
    description: "Adds an entry that watches a project's Azure DevOps, GitHub or GitLab for tickets assigned to the user and offers to start an agent on each in a new worktree. It checks once and returns what it found. One entry per project. Nothing starts until the user presses Start on the Schedules page.",
    input: s.object({
      project: s.string({ minLength: 1, maxLength: 1000 }).describe("A Wings project's name or its folder path."),
      agent: s.enum(["claude", "opencode"]).optional().describe("claude (default) or opencode."),
      prompt: s.string({ maxLength: 4000 }).optional().describe("What the agent is told. {id}, {title} and {url} become the ticket's number, title and link. Leave out for the default."),
      min_free_gb: s.integer({ minimum: 0, maximum: 10_000 }).optional().describe("Wings won't make a worktree when the disk has less free space than this. 10 by default."),
      states: s.array(s.string({ maxLength: 100 }), { maxItems: 50 }).optional().describe("The ticket states to ask about, like New or Ready for development. Leave out to pick the not-started ones after the first check."),
    }),
    run: async ({ project, agent, prompt, min_free_gb, states }) => {
      const found = resolveProject(await wings.projects(), project);
      const saved = await addWatch({ projectId: found.id, agent: agent ?? "claude", prompt: prompt ?? DEFAULT_PROMPT, minFreeGb: min_free_gb ?? DEFAULT_MIN_FREE_GB, ...(states ? { states } : {}) }, true);
      await checkOne(saved.id).catch(console.error);
      const w = (await readWatches()).find((x) => x.id === saved.id);
      const asks = w?.states === undefined ? "" : w.states.length ? ` It asks about: ${w.states.join(", ")}.` : " It asks about every state.";
      return `Added ${saved.id} for ${found.name}. ${describeCheck(w?.lastCheck, w?.source)}.${asks}`;
    },
  }),

  remove: tool({
    description: "Removes a schedule or an assigned entry by its id from list. What it was waiting to ask about goes with it. Tabs it already started stay open.",
    input: s.object({ id: s.string({ minLength: 1, maxLength: 100 }).describe("The id of the entry, from list.") }),
    run: async ({ id }) => {
      const [schedules, watches] = await Promise.all([readSchedules(), readWatches()]);
      if (schedules.some((x) => x.id === id)) await removeSchedule(id);
      else if (watches.some((x) => x.id === id)) await removeWatch(id);
      else throw new Error(`No entry ${id}. Use list to see the ids.`);
      return `Removed ${id}.`;
    },
  }),
};
