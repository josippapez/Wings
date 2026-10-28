// The tools Claude, and `wings schedules <tool> key=value`, call. The build writes these descriptions and schemas into the
// manifest's mcpTools. The build loads this file in Node with a wings that does nothing, so nothing here runs at the top level.

import { s, tool } from "wings-plugin-sdk/tools";
import { DEFAULT_MIN_FREE_GB, DEFAULT_PROMPT, describeCheck, resolveProject } from "./lib/assigned";
import { formatList } from "./lib/format";
import { addWatch, checkOne, removeWatch, saveWatch, startTicket } from "./main/assigned";
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
      title_prefixes: s.array(s.string({ maxLength: 40 }), { maxItems: 20 }).optional().describe("Ask only about tickets whose title starts with one of these, like FE or BE, when the project's tickets carry a prefix for each repo. Leave out for every title."),
      base: s.string({ maxLength: 200 }).optional().describe("The branch a ticket's branch starts from, like origin/develop. Leave out for the repo's default branch."),
    }),
    run: async ({ project, agent, prompt, min_free_gb, states, title_prefixes, base }) => {
      const found = resolveProject(await wings.projects(), project);
      const saved = await addWatch({ projectId: found.id, agent: agent ?? "claude", prompt: prompt ?? DEFAULT_PROMPT, minFreeGb: min_free_gb ?? DEFAULT_MIN_FREE_GB, ...(states ? { states } : {}), prefixes: title_prefixes, base }, true);
      await checkOne(saved.id).catch(console.error);
      const w = (await readWatches()).find((x) => x.id === saved.id);
      const asks = w?.states === undefined ? "" : w.states.length ? ` It asks about: ${w.states.join(", ")}.` : " It asks about every state.";
      return `Added ${saved.id} for ${found.name}. ${describeCheck(w?.lastCheck, w?.source)}.${asks}`;
    },
  }),

  check: tool({
    description: "Checks for assigned tickets now, in one assigned entry or in every enabled one, and returns what each found. Use it after a ticket was assigned, instead of waiting for the next check.",
    input: s.object({ id: s.string({ maxLength: 100 }).optional().describe("The id of an assigned entry, from list. Leave out to check every enabled one.") }),
    run: async ({ id }) => {
      const all = await readWatches();
      const picked = id ? all.filter((w) => w.id === id) : all.filter((w) => w.enabled);
      if (id && !picked.length) throw new Error(`No assigned entry ${id}. Use list to see the ids.`);
      if (id && !picked[0].enabled) throw new Error("That entry is paused. Resume it on the Schedules page first.");
      if (!picked.length) return "No assigned entries are enabled.";
      const projects = await wings.projects();
      const lines: string[] = [];
      for (const w of picked) {
        await checkOne(w.id).catch(console.error);
        const now = (await readWatches()).find((x) => x.id === w.id);
        lines.push(`${w.id} · ${projects.find((p) => p.id === w.projectId)?.name ?? "missing project"} · ${describeCheck(now?.lastCheck, now?.source)}${now?.waiting.length ? ` · ${now.waiting.length} waiting` : ""}`);
      }
      return lines.join("\n");
    },
  }),

  start: tool({
    description: "Starts an agent on one assigned ticket: makes a worktree on a feature branch and opens the agent in it, in the entry's project or another project of the same repo host and project. The ticket must be waiting, or listed by the last check in a state the entry asks about, and not started before. One that already has a branch, or was started before, needs force.",
    input: s.object({
      id: s.string({ minLength: 1, maxLength: 100 }).describe("The id of the assigned entry, from list."),
      ticket: s.string({ minLength: 1, maxLength: 100 }).describe("The ticket's number, as list shows it without the #."),
      force: s.boolean().optional().describe("Start a ticket in a state the entry doesn't ask about, one that already has a branch, or one started before."),
      project: s.string({ minLength: 1, maxLength: 1000 }).optional().describe("Where to start it: a Wings project's name or folder path. Leave out for the entry's project."),
    }),
    run: async ({ id, ticket, force, project }) => {
      const projectId = project ? resolveProject(await wings.projects(), project).id : undefined;
      const { title, target } = await startTicket(id, ticket.replace(/^#/, ""), force, projectId);
      return `Started ${title} on ${target}.`;
    },
  }),

  update: tool({
    description: "Changes an assigned entry: only the fields given change, and an empty string or list clears one. The tickets it already asked about or started stay as they are.",
    input: s.object({
      id: s.string({ minLength: 1, maxLength: 100 }).describe("The id of the assigned entry, from list."),
      agent: s.enum(["claude", "opencode"]).optional().describe("claude or opencode."),
      prompt: s.string({ maxLength: 4000 }).optional().describe("What the agent is told. {id}, {title} and {url} become the ticket's number, title and link."),
      min_free_gb: s.integer({ minimum: 0, maximum: 10000 }).optional().describe("Wings won't make a worktree when the disk has less free space than this."),
      states: s.array(s.string({ maxLength: 100 }), { maxItems: 50 }).optional().describe("The ticket states to ask about. An empty list asks about every state."),
      title_prefixes: s.array(s.string({ maxLength: 40 }), { maxItems: 20 }).optional().describe("Ask only about tickets whose title starts with one of these, like FE or BE."),
      base: s.string({ maxLength: 200 }).optional().describe("The branch a ticket's branch starts from, like origin/develop."),
    }),
    run: async ({ id, agent, prompt, min_free_gb, states, title_prefixes, base }) => {
      const w = (await readWatches()).find((x) => x.id === id);
      if (!w) throw new Error(`No assigned entry ${id}. Use list to see the ids.`);
      const saved = await saveWatch(id, {
        projectId: w.projectId,
        agent: agent ?? w.agent,
        prompt: prompt ?? w.prompt,
        minFreeGb: min_free_gb ?? w.minFreeGb,
        states: states ?? w.states,
        prefixes: title_prefixes ?? w.prefixes,
        base: base ?? w.base,
      });
      return `Updated ${id}: ${saved.agent}, ${saved.states?.length ? `asks about ${saved.states.join(", ")}` : "every state"}${saved.prefixes?.length ? `, titles starting with ${saved.prefixes.join(", ")}` : ""}, branches from ${saved.base ?? "the default branch"}.`;
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
