// The "When something is assigned to me" entries: one CLI call per entry every few minutes, a notification for each ticket
// that is new, and Start, which makes the worktree and opens the agent in it.
// Every read-then-write of the entries goes through `queue`, so a tick, a manual check and an edit never interleave.

import { errorMessage, mapLimit, serial, singleFlight } from "wings-plugin-sdk/async";
import { cli, CliError, lastLine, type Cli } from "wings-plugin-sdk/cli";
import { gitOk } from "wings-plugin-sdk/git";
import { ch } from "../lib/messages";
import { AGENTS, type Run } from "../lib/schedule";
import {
  applyCheck, asks, branchMatching, createdKey, issueDescription, issueTypeOf, mentionsTicket, openIssueKeys, withIssueKey, startedElsewhere, branchName, cleanWatchDraft, diskProblem, fillPrompt, keyOf, listCommand, parseTickets, sameSource, sourceOf, SOURCE_LABELS, UNSUPPORTED, withStarted,
  type AssignedTicket, type Check, type Source, type Ticket, type Watch, type WatchDraft,
} from "../lib/assigned";
import { readWatches, writeWatches } from "./store";

/** A check is one CLI call, so it runs while Wings is in the background too: that is when a new ticket is wanted. */
export const CHECK_MS = 5 * 60_000;

const queue = serial();

const hostOf = (key: string | undefined) => (key ?? "").slice((key ?? "").indexOf(":") + 1);

const CLIS: Record<"az" | "gh" | "glab", Cli> = {
  az: cli("az", { signedOut: /login/i, signIn: ["login", "--allow-no-subscriptions", "--output", "none"] }),
  gh: cli("gh", {
    // No token prints `gh auth login` or GH_TOKEN; an expired one gh can renew prints "Try authenticating with: gh auth refresh".
    signedOut: /gh auth login|GH_TOKEN|Try authenticating with:\s+gh auth refresh\b/,
    signIn: (key) => ["auth", "login", "--web", "--clipboard", "--hostname", hostOf(key)],
  }),
  glab: cli("glab", {
    // A private project looks missing (404) to a signed-out user, so ask glab whether it is signed in.
    signedOut: async ({ stderr }, key) => /\b401\b/.test(stderr) || (/\b404\b/.test(stderr) && (await wings.exec("glab", ["auth", "status", "--hostname", hostOf(key)])).code !== 0),
    signIn: (key) => ["auth", "login", "--web", "--hostname", hostOf(key)],
  }),
};

/** The remote of each project, read once: a check is then the one call. Forgotten on Try again. */
const sources = new Map<string, Source>();

async function sourceFor(project: { id: string; path: string }): Promise<Source> {
  const key = `${project.id}\n${project.path}`;
  let source = sources.get(key);
  if (!source) {
    source = sourceOf(await gitOk(project.path, ["remote", "get-url", "origin"]));
    sources.set(key, source);
  }
  return source;
}

/** The projects an entry's tickets can start in: the ones with its source, its own first, without the worktrees Wings made (`<repo>-worktrees/<branch>`). */
export async function targetsOf(w: Pick<Watch, "projectId" | "source">, projects: { id: string; name: string; path: string }[]): Promise<{ id: string; name: string }[]> {
  const own = projects.filter((p) => p.id === w.projectId);
  const others = [];
  if (w.source) for (const p of projects) if (p.id !== w.projectId && !p.path.includes("-worktrees/") && sameSource(await sourceFor(p), w.source)) others.push(p);
  return [...own, ...others].map(({ id, name }) => ({ id, name }));
}

/** The tickets, with the branch of each that has one in a project of the same source: one for-each-ref in each such project. */
async function markBranches(source: Source, tickets: AssignedTicket[], projects: { id: string; name: string; path: string }[], firstId: string): Promise<AssignedTicket[]> {
  const sorted = [...projects].sort((a, b) => Number(b.id === firstId) - Number(a.id === firstId));
  const found = await mapLimit(sorted, 4, async (p) => {
    if (!sameSource(await sourceFor(p), source)) return null;
    const refs = (await gitOk(p.path, ["for-each-ref", "--format=%(refname:short)", "refs/heads", "refs/remotes/origin"])) ?? "";
    return { project: p.name, branches: refs.split("\n").filter(Boolean) };
  });
  const sets = found.filter((x) => x !== null);
  return tickets.map((t) => {
    for (const { project, branches } of sets) {
      const name = branchMatching(t.id, branches);
      if (name) return { ...t, branch: { name, project } };
    }
    return t;
  });
}

/** An Issues list line for an issue that was started or finished: its key and status. */
const STARTED_ISSUE = /^(\S+) \[(in_progress|in_review|done)\]/;
/** The most issues a search finds that are read for one ticket in one project: a short id like 12 finds many. */
const ISSUES_READ = 5;

/**
 * The tickets without a branch, with a Wings issue that names one, in a project of the same source: an Issues search
 * for the id, then a read of each issue it finds, since the search also finds 12 in WIN-120. Nothing when Issues is off.
 */
async function markIssues(tickets: AssignedTicket[], projects: { id: string; name: string }[]): Promise<AssignedTicket[]> {
  const ask = (name: string, args: Record<string, unknown>, project: string) => wings.callTool(name, args, { project }).then((r) => (r.isError ? "" : r.text), () => "");
  return mapLimit(tickets, 4, async (t) => {
    if (t.branch) return t;
    for (const p of projects) {
      const found = (await ask("tasks__list", { query: t.id }, p.id)).split("\n").map((line) => STARTED_ISSUE.exec(line)).filter((m) => m !== null).slice(0, ISSUES_READ);
      for (const [, key, status] of found) {
        if (mentionsTicket(t.id, await ask("tasks__get", { key }, p.id))) return { ...t, issue: { key, status, project: p.name } };
      }
    }
    return t;
  });
}

/**
 * Opens the Wings issue for a ticket that is being started, in the project the ticket starts in (not the worktree, whose
 * issues are kept there anyway): an open issue that names the ticket is set in progress on the branch, else one is created.
 * Never throws: Issues being off, or any of its calls failing, comes back as `problem` and the agent starts without a key.
 */
async function trackIssue(projectId: string, ticket: Ticket & { type?: string }, branch: string): Promise<{ key?: string; problem?: string }> {
  const call = async (name: string, args: Record<string, unknown>) => {
    const r = await wings.callTool(name, args, { project: projectId });
    if (r.isError) throw new Error(lastLine(r.text) || `${name} failed`);
    return r.text;
  };
  try {
    let key: string | undefined;
    for (const k of openIssueKeys(await call("tasks__list", { query: ticket.id })).slice(0, ISSUES_READ)) {
      if (mentionsTicket(ticket.id, await call("tasks__get", { key: k }))) {
        key = k;
        break;
      }
    }
    if (key) {
      await call("tasks__update", { key, status: "in_progress", branch });
    } else {
      key = createdKey(await call("tasks__create", { title: ticket.title, type: issueTypeOf(ticket), status: "in_progress", description: issueDescription(ticket) }));
      if (!key) throw new Error("Issues didn't say which key it made.");
      await call("tasks__update", { key, branch });
    }
    return { key };
  } catch (e) {
    return { problem: `Couldn't open a Wings issue (${lastLine(errorMessage(e)) || "Issues is off"})` };
  }
}

const problemOf = (e: unknown, source: Source, program: string): Pick<Check, "error" | "problem"> => {
  if (e instanceof CliError && e.kind === "missing") return { error: `The ${program} command isn't installed.`, problem: "missing" };
  if (e instanceof CliError && e.kind === "signed-out") return { error: `Sign in to ${SOURCE_LABELS[source.kind]} to see what is assigned to you.`, problem: "signed-out" };
  return { error: lastLine(errorMessage(e)) || "The check failed." };
};

/** Looks for what is assigned to you in one entry's project and records what it found. */
export async function checkOne(id: string): Promise<void> {
  const w = (await readWatches()).find((x) => x.id === id);
  if (!w) return;
  const projects = await wings.projects();
  const project = projects.find((p) => p.id === w.projectId);
  const at = Date.now();
  let check: Check;
  let source = w.source;
  let tickets: AssignedTicket[] | null = null;
  if (!project) {
    check = { at, ok: false, error: "The project is no longer in Wings." };
  } else {
    source = await sourceFor(project);
    const command = listCommand(source);
    if (!command) {
      check = { at, ok: false, error: UNSUPPORTED };
    } else {
      try {
        tickets = parseTickets(source, await CLIS[command.program].json(command.args, { key: keyOf(source), cwd: project.path, empty: [] }));
        tickets = await markBranches(source, tickets, projects, project.id);
        const same: typeof projects = [];
        for (const p of projects) if (sameSource(await sourceFor(p), source)) same.push(p);
        tickets = await markIssues(tickets, same);
        check = { at, ok: true, count: tickets.length };
      } catch (e) {
        check = { at, ok: false, ...problemOf(e, source, command.program) };
      }
    }
  }
  await queue(async () => {
    const list = await readWatches();
    const current = list.find((x) => x.id === id);
    if (!current) return;
    let next: Watch = { ...current, source, lastCheck: check };
    let fresh: ReturnType<typeof applyCheck>["fresh"] = [];
    // Paused while the call ran: what it found is not looked at.
    if (tickets && current.enabled) ({ watch: next, fresh } = applyCheck(next, tickets, at));
    await writeWatches(list.map((x) => (x.id === id ? next : x)));
    for (const t of fresh) {
      wings
        .notify({
          title: `Assigned: ${t.title}`,
          body: `${project?.name ?? "A project"} · Start ${AGENTS[w.agent].label} on it in a new worktree?`,
          projectId: project?.id,
          actions: [
            { label: "Start", open: { view: "schedules", args: { start: { entry: id, ticket: t.id } } } },
            { label: "Review", open: { view: "schedules", args: { waiting: id } } },
          ],
        })
        .catch(console.error);
    }
  });
}

/** One call for each enabled entry. */
export const checkAll = singleFlight(async () => {
  for (const w of (await readWatches()).filter((x) => x.enabled)) await checkOne(w.id).catch(console.error);
});

const update = (id: string, change: (w: Watch) => Watch) =>
  queue(async () => {
    const list = await readWatches();
    if (list.some((w) => w.id === id)) await writeWatches(list.map((w) => (w.id === id ? change(w) : w)));
  });

const find = async (id: string) => {
  const w = (await readWatches()).find((x) => x.id === id);
  if (!w) throw new Error("That entry is gone.");
  return w;
};

/** Adds an entry that has checked nothing yet. `onlyOne` refuses a project that already has one. */
export const addWatch = (draft: WatchDraft, onlyOne = false) =>
  queue(async () => {
    const clean = cleanWatchDraft(draft);
    const list = await readWatches();
    if (onlyOne && list.some((w) => w.projectId === clean.projectId)) throw new Error("That project already has an assigned entry. Remove it first, or change it on the Schedules page.");
    sources.clear();
    const saved: Watch = { ...clean, id: `${Date.now().toString(36)}${Math.random().toString(36).slice(2, 8)}`, enabled: true, primed: false, seen: [], waiting: [] };
    await writeWatches([...list, saved]);
    return saved;
  });

export const removeWatch = (id: string) => queue(async () => writeWatches((await readWatches()).filter((w) => w.id !== id)));

const starting = new Set<string>();

/**
 * Makes the worktree for a ticket and opens the agent in it, in a tab behind the one on screen. The ticket is one that is
 * waiting, or one the last check listed in a state that is asked about, and not one already started (`force` skips both).
 * It starts in `projectId`, one of the entry's `targetsOf`, or in the entry's own project.
 */
export async function startTicket(id: string, ticketId: string, force = false, projectId?: string): Promise<{ title: string; target: string }> {
  const key = `${id}\n${ticketId}`;
  if (starting.has(key)) throw new Error("That one is already starting.");
  starting.add(key);
  try {
    const w = await find(id);
    if (w.started?.[ticketId] && !force) throw new Error("That ticket was already started. Pass force=true to start it again.");
    const waiting = w.waiting.find((t) => t.id === ticketId);
    const listed = w.tickets?.find((t) => t.id === ticketId);
    const ticket = waiting ?? listed;
    if (!ticket) throw new Error("That ticket isn't waiting or assigned to you any more.");
    if (listed && startedElsewhere(listed) && !force) throw new Error(`It was already started: ${startedElsewhere(listed)}. Pass force=true to start it anyway.`);
    if (!waiting && listed && !force && !asks(w, listed)) {
      throw new Error(`This entry doesn't ask about tickets in ${listed.state}${w.prefixes?.length ? ` or titles not starting with ${w.prefixes.join(", ")}` : ""}. Pass force=true to start it anyway.`);
    }
    const at = Date.now();
    let run: Run;
    let madeWorktree = false;
    let target = "";
    let madeIn: string | undefined;
    let issueProblem: string | undefined;
    try {
      const projects = await wings.projects();
      const want = projectId ?? w.projectId;
      if (!(await targetsOf(w, projects)).some((p) => p.id === want)) throw new Error("That project isn't one this entry's tickets can start in.");
      const project = projects.find((p) => p.id === want);
      if (!project) throw new Error("The project is no longer in Wings.");
      const problem = diskProblem(await wings.disks(), project.path, w.minFreeGb);
      if (problem) throw new Error(problem);
      const branch = branchName(ticket);
      // The base is a branch of the entry's own repo: another project starts from its default.
      const made = await wings.createWorktree({ projectId: project.id, branch, ...(w.base && project.id === w.projectId ? { base: w.base } : {}) });
      madeWorktree = true;
      madeIn = made.projectId;
      target = branch;
      const issue = await trackIssue(project.id, ticket, branch);
      issueProblem = issue.problem;
      const filled = fillPrompt(w.prompt, ticket);
      const prompt = issue.key ? withIssueKey(filled, issue.key) : filled;
      const { paneId } = await wings.openPane({ command: AGENTS[w.agent].command, args: [prompt], cwd: made.path, focus: false });
      run = { at, paneId, ok: true };
    } catch (e) {
      run = { at, ok: false, error: madeWorktree ? `Made the worktree, but couldn't start the agent: ${errorMessage(e)}` : errorMessage(e) };
    }
    // A ticket whose worktree exists isn't offered again: Start would only fail on the branch.
    const done = run.ok || madeWorktree;
    await update(id, (x) => ({ ...x, lastRun: run, waiting: done ? x.waiting.filter((t) => t.id !== ticketId) : x.waiting, ...(done ? { started: withStarted(x.started, ticketId, at) } : {}) }));
    if (!run.ok) throw new Error(run.error);
    wings
      .notify({ title: `Started ${ticket.title}`, body: `${AGENTS[w.agent].label} on ${target}${issueProblem ? `. ${issueProblem}.` : ""}`, projectId: madeIn, actions: [{ label: "Open Schedules", open: { view: "schedules" } }] })
      .catch(console.error);
    return { title: ticket.title, target };
  } finally {
    starting.delete(key);
  }
}

/** Changes an entry. One moved to another project starts over there, like a new one. */
export const saveWatch = (id: string, draft: WatchDraft) =>
  queue(async () => {
    const clean = cleanWatchDraft(draft);
    const list = await readWatches();
    sources.clear();
    const old = list.find((w) => w.id === id);
    if (!old) throw new Error("That entry is gone.");
    // Another project has other tickets, so it starts over: nothing it already has is asked about.
    const moved = old.projectId !== clean.projectId;
    const saved: Watch = { ...old, ...clean, ...(moved ? { primed: false, seen: [], waiting: [], states: clean.states, tickets: undefined, source: undefined, lastCheck: undefined } : {}) };
    await writeWatches(list.map((w) => (w.id === id ? saved : w)));
    if (saved.enabled) void checkOne(saved.id).catch(console.error);
    return saved;
  });

let signInCode: string | null = null;
export const currentSignInCode = () => signInCode;

const cliOf = async (id: string) => {
  const { source } = await find(id);
  const command = source && listCommand(source);
  if (!source || !command) throw new Error("There is no CLI to set up for this entry.");
  return { cli: CLIS[command.program], key: keyOf(source), github: source.kind === "github" ? source.host : null };
};

export function serveAssigned() {
  ch.answer("worktreeSetup", ({ projectId }) => wings.worktreeSetup(projectId));

  ch.answer("saveWatch", async ({ id, draft, worktree }) => {
    if (worktree) await wings.setWorktreeSetup(worktree.projectId, worktree.change);
    if (id === null) {
      const saved = await addWatch(draft);
      void checkOne(saved.id).catch(console.error);
      return;
    }
    await saveWatch(id, draft);
  });

  ch.answer("branches", async ({ projectId }) => {
    const project = (await wings.projects()).find((p) => p.id === projectId);
    const refs = project ? ((await gitOk(project.path, ["for-each-ref", "--format=%(refname:short)", "refs/heads", "refs/remotes/origin"])) ?? "") : "";
    return refs.split("\n").filter((b) => b && b !== "origin" && b !== "origin/HEAD");
  });

  ch.answer("toggleWatch", ({ id }) =>
    queue(async () => {
      const list = await readWatches();
      const old = list.find((w) => w.id === id);
      if (!old) return;
      // What was assigned while it was paused is not asked about: it starts over from what is assigned on resume.
      await writeWatches(list.map((w) => (w.id === id ? { ...w, enabled: !w.enabled, ...(w.enabled ? {} : { primed: false }) } : w)));
      if (!old.enabled) void checkOne(id).catch(console.error);
    }),
  );

  ch.answer("removeWatch", ({ id }) => removeWatch(id));
  ch.answer("checkWatch", async ({ id }) => {
    sources.clear();
    await find(id);
    await checkOne(id);
  });
  ch.answer("startTicket", async ({ id, ticketId, force, projectId }) => void (await startTicket(id, ticketId, force, projectId)));
  ch.answer("dismissWaiting", ({ id, ticketId }) => update(id, (w) => ({ ...w, waiting: w.waiting.filter((t) => t.id !== ticketId) })));

  ch.answer("install", async ({ id }) => void (await (await cliOf(id)).cli.install()));
  ch.answer("signIn", async ({ id }) => {
    const { cli: c, key, github } = await cliOf(id);
    try {
      await c.signIn(key, {
        // gh doesn't open the browser without a terminal: it prints a one-time code and a URL.
        onOutput(line) {
          if (!github) return;
          const code = line.match(/code(?::| \()\s*([A-Z0-9]{4}-[A-Z0-9]{4})/)?.[1];
          if (code) (signInCode = code), ch.refresh("state");
          const url = line.match(new RegExp(`https://${github.replace(/\./g, "\\.")}/\\S+`))?.[0];
          if (url) void wings.openUrl(url).catch(console.error);
        },
      });
    } finally {
      signInCode = null;
      ch.refresh("state");
    }
  });

  // After Try again or a sign-in the remotes are read again and the entries checked.
  for (const c of Object.values(CLIS)) {
    c.onRetry(() => {
      sources.clear();
      return checkAll();
    });
  }
}

export function pollAssigned() {
  void checkAll().catch(console.error);
  setInterval(() => void checkAll().catch(console.error), CHECK_MS);
}
