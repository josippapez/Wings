// The tools Claude, and `wings schedules <tool> key=value`, call. The build writes these descriptions and schemas into the
// manifest's mcpTools. The build loads this file in Node with a wings that does nothing, so nothing here runs at the top level.

import { s, tool, type ToolContext } from "wings-plugin-sdk/tools";
import { DAY_NAMES, needsApproval, whenFrom } from "./lib/approval";
import { DEFAULT_MIN_FREE_GB, DEFAULT_PROMPT, describeCheck, resolveProject } from "./lib/assigned";
import { formatList } from "./lib/format";
import { cleanDraft, describeNext, describeWhen, EVERY_HOURS, nextRun, type Draft, type Schedule } from "./lib/schedule";
import { askApproval, type Answer } from "./main/approvals";
import { addWatch, checkOne, removeWatch, saveWatch, startTicket } from "./main/assigned";
import { removeSchedule, saveSchedule } from "./main/schedules";
import { readSchedules, readWatches } from "./main/store";

const EVERY_HOURS_TEXT = EVERY_HOURS.join(", ");
const timing = {
  every_hours: s.integer({ minimum: 1, maximum: 24 }).optional().describe(`Run every this many hours: ${EVERY_HOURS_TEXT}. Leave out for a time of day.`),
  time: s.string({ pattern: "^([01]\\d|2[0-3]):[0-5]\\d$" }).optional().describe("Run at this time of day, HH:MM in the Mac's local time, like 09:00."),
  days: s.array(s.enum(DAY_NAMES), { maxItems: 7 }).optional().describe("The days time runs on, like [\"mon\",\"tue\",\"wed\",\"thu\",\"fri\"]. Leave out for every day."),
};

const askerOf = (context: ToolContext) => context.session?.name ?? null;

/** Throws when the user didn't allow it. */
function allowed(answer: Answer) {
  if (answer === "deny") throw new Error("The user denied it in Wings. Nothing changed.");
  if (answer === "timeout") throw new Error("The user didn't answer in Wings within 100 seconds. Nothing changed. Ask them before you try again.");
}

const describeSchedule = (s: Schedule) => {
  const at = s.enabled ? nextRun(s) : null;
  return `${describeWhen(s.when)}${at === null ? "" : `, next run ${describeNext(at, Date.now())}`}${s.enabled ? "" : ", paused"}`;
};

export const tools = {
  list: tool({
    description: "Lists every schedule and every assigned entry, one line each: id, kind, project, agent, enabled, and for an assigned entry its source, last check, the states it asks about and the tickets assigned to the user with their state and whether each is asked about. Then the tickets waiting for the user's answer. Read-only.",
    run: async () => {
      const [schedules, watches, projects] = await Promise.all([readSchedules(), readWatches(), wings.projects()]);
      return formatList(schedules, watches, (id) => projects.find((p) => p.id === id)?.name ?? "missing project", Date.now());
    },
  }),

  add: tool({
    description: "Adds a timed schedule: Wings starts an agent with this prompt in a new tab of the project every few hours, or at a time of day on the days given, while Wings runs. It starts by itself, so the user has to allow it in a dialog in Wings first: the call waits up to 100 seconds for their answer and fails, saving nothing, when they deny or don't answer.",
    input: s.object({
      prompt: s.string({ minLength: 1, maxLength: 4000 }).describe("What the agent is told on each run, as one line. It can't start with a dash."),
      project: s.string({ minLength: 1, maxLength: 1000 }).optional().describe("A Wings project's name or its folder path. Leave out for the project of the pane you run in."),
      agent: s.enum(["claude", "opencode"]).optional().describe("claude (default) or opencode."),
      name: s.string({ maxLength: 80 }).optional().describe("What the lists call it. Leave out to show the start of the prompt."),
      ...timing,
    }),
    run: async ({ prompt, project, agent, name, every_hours, time, days }, context) => {
      const projects = await wings.projects();
      const here = context.project ? (context.project.parentId ?? context.project.id) : null;
      if (!project && !here) throw new Error("Give project: this call didn't come from a pane in Wings.");
      const found = resolveProject(projects, project ?? here!);
      const when = whenFrom({ every_hours, time, days });
      if (!when) throw new Error("Give every_hours, or time with days.");
      const draft = cleanDraft({ name: name ?? "", projectId: found.id, agent: agent ?? "claude", prompt, when });
      allowed(await askApproval({ asker: askerOf(context), projectName: found.name, current: null, proposed: draft }, found.id));
      const saved = await saveSchedule(null, draft);
      return `Added ${saved.id} in ${found.name}: ${describeSchedule(saved)}.`;
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
      name: s.string({ maxLength: 80 }).optional().describe('What the lists call the entry instead of "When something is assigned to me".'),
      base: s.string({ maxLength: 200 }).optional().describe("The branch a ticket's branch starts from, like origin/develop. Leave out for the repo's default branch."),
    }),
    run: async ({ project, agent, prompt, min_free_gb, states, title_prefixes, base, name }) => {
      const found = resolveProject(await wings.projects(), project);
      const saved = await addWatch({ name, projectId: found.id, agent: agent ?? "claude", prompt: prompt ?? DEFAULT_PROMPT, minFreeGb: min_free_gb ?? DEFAULT_MIN_FREE_GB, ...(states ? { states } : {}), prefixes: title_prefixes, base }, true);
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
    description: "Changes a schedule or an assigned entry: only the fields given change, and an empty string or list clears one. For a schedule, a new agent, prompt or timing waits for the user to allow it in a dialog in Wings (up to 100 seconds) and fails, changing nothing, when they deny or don't answer; a new name alone doesn't ask. For an assigned entry, the tickets it already asked about or started stay as they are.",
    input: s.object({
      id: s.string({ minLength: 1, maxLength: 100 }).describe("The id of the schedule or assigned entry, from list."),
      agent: s.enum(["claude", "opencode"]).optional().describe("claude or opencode."),
      prompt: s.string({ maxLength: 4000 }).optional().describe("What the agent is told. In an assigned entry, {id}, {title} and {url} become the ticket's number, title and link."),
      name: s.string({ maxLength: 80 }).optional().describe('What the lists call it. An empty string clears it.'),
      ...timing,
      min_free_gb: s.integer({ minimum: 0, maximum: 10000 }).optional().describe("Assigned entry only: Wings won't make a worktree when the disk has less free space than this."),
      states: s.array(s.string({ maxLength: 100 }), { maxItems: 50 }).optional().describe("Assigned entry only: the ticket states to ask about. An empty list asks about every state."),
      title_prefixes: s.array(s.string({ maxLength: 40 }), { maxItems: 20 }).optional().describe("Assigned entry only: ask only about tickets whose title starts with one of these, like FE or BE."),
      base: s.string({ maxLength: 200 }).optional().describe("Assigned entry only: the branch a ticket's branch starts from, like origin/develop."),
    }),
    run: async ({ id, agent, prompt, name, every_hours, time, days, min_free_gb, states, title_prefixes, base }, context) => {
      const schedule = (await readSchedules()).find((x) => x.id === id);
      if (schedule) {
        if (min_free_gb !== undefined || states || title_prefixes || base !== undefined) throw new Error("min_free_gb, states, title_prefixes and base are for assigned entries. A schedule takes agent, prompt, name, every_hours, time and days.");
        const current: Draft = { name: schedule.name, projectId: schedule.projectId, agent: schedule.agent, prompt: schedule.prompt, when: schedule.when };
        const proposed = cleanDraft({
          ...current,
          name: name ?? current.name,
          agent: agent ?? current.agent,
          prompt: prompt ?? current.prompt,
          when: whenFrom({ every_hours, time, days }, current.when) ?? current.when,
        });
        if (needsApproval(current, proposed)) {
          const projectName = (await wings.projects()).find((p) => p.id === schedule.projectId)?.name ?? "missing project";
          allowed(await askApproval({ asker: askerOf(context), projectName, current, proposed }, schedule.projectId));
        }
        const saved = await saveSchedule(id, proposed);
        return `Updated ${id}: ${describeSchedule(saved)}.`;
      }
      const w = (await readWatches()).find((x) => x.id === id);
      if (!w) throw new Error(`No schedule or assigned entry ${id}. Use list to see the ids.`);
      if (every_hours !== undefined || time !== undefined || days) throw new Error("every_hours, time and days are for schedules. An assigned entry checks for tickets by itself.");
      const saved = await saveWatch(id, {
        name: name ?? w.name,
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
