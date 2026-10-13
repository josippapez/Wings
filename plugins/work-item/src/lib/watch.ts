// Polls Azure DevOps for what changed since the last check: states of your work items, comments that mention you
// (work items and pull requests), and activity on pull requests you created or review. Read-only: every call is a
// GET through `az devops invoke` or a WIQL query through `az boards query`. The I/O is passed in, so a node harness
// can run it against recorded responses.

import {
  after, backoffMs, dayBefore, diffPrs, fresh, notes, rememberProject, remember, snapshot, statusEvent, threadEvents, watermark, workItemEvents,
  type Candidate, type Note, type Pr, type PrSnaps, type Seen, type StoredEvent, type Thread, type UpdateEvent, type WorkItemUpdate,
} from "./updates.ts";

/** Work items looked into per project and check: the newest changed since the last one. */
export const ITEM_CAP = 20;
/** Pull requests whose threads are read per project and check. */
export const PR_CAP = 20;
/** The newest events kept for the sidebar. */
export const EVENTS_CAP = 50;
const UPDATE_PAGES = 3;
const CONCURRENCY = 3;

export interface Settings {
  enabled: boolean;
  intervalMin: number;
}

export const DEFAULT_SETTINGS: Settings = { enabled: true, intervalMin: 2 };

export interface Deps {
  /** `az <args> --organization <org> -o json`, parsed. Rejects when az fails. */
  az(org: string, args: string[]): Promise<unknown>;
  get(key: string): Promise<unknown>;
  set(key: string, value: unknown): Promise<void>;
  notify(note: Note): Promise<void>;
  now(): number;
}

export interface Target {
  org: string;
  project: string;
}

interface WiqlRow {
  id: number;
  fields: Record<string, unknown> & { "System.ChangedDate"?: string; "System.Title"?: string; "System.WorkItemType"?: string; "System.AssignedTo"?: { id?: string }; "System.CreatedBy"?: { id?: string } };
}

const KEYS = {
  projects: "watch:projects",
  events: "watch:events",
  seen: "watch:seen",
  me: (org: string) => `watch:me:${org}`,
  wm: (t: Target) => `watch:wm:${t.org}|${t.project}`,
  prs: (t: Target) => `watch:prs:${t.org}|${t.project}`,
};

const quote = (s: string) => s.replaceAll("'", "''");

function invoke(area: string, resource: string, route: Record<string, string | number>, query: Record<string, string | number> = {}) {
  const q = Object.entries(query).map(([k, v]) => `${k}=${v}`);
  return [
    "devops", "invoke", "--area", area, "--resource", resource, "--api-version", "7.1",
    "--route-parameters", ...Object.entries(route).map(([k, v]) => `${k}=${v}`),
    ...(q.length ? ["--query-parameters", ...q] : []),
  ];
}

async function mapLimit<T, R>(list: T[], fn: (item: T) => Promise<R>): Promise<R[]> {
  const out: R[] = new Array(list.length);
  let next = 0;
  await Promise.all(
    Array.from({ length: Math.min(CONCURRENCY, list.length) }, async () => {
      while (next < list.length) {
        const i = next++;
        out[i] = await fn(list[i]);
      }
    }),
  );
  return out;
}

export function createWatcher(deps: Deps) {
  /** `org|project` → consecutive failed checks, and when to try again. */
  const failing = new Map<string, { failures: number; until: number }>();
  /** Everything that reads and then writes the stored events goes through here, one at a time. */
  let queue: Promise<unknown> = Promise.resolve();
  const exclusive = <T>(fn: () => Promise<T>): Promise<T> => {
    const run = queue.then(fn, fn);
    queue = run.catch(() => {});
    return run;
  };

  const wiql = async (t: Target, query: string): Promise<WiqlRow[]> => {
    // `az boards query` prints nothing at all for an empty result.
    const rows = await deps.az(t.org, ["boards", "query", "--project", t.project, "--wiql", query]);
    return Array.isArray(rows) ? (rows as WiqlRow[]) : [];
  };

  /** Your identity id in the org, which is what both kinds of mention carry. Found once, as the id on an item you changed, are assigned or created. */
  async function identity(t: Target): Promise<string> {
    const cached = await deps.get(KEYS.me(t.org));
    if (typeof cached === "string") return cached;
    const project = `[System.TeamProject] = '${quote(t.project)}'`;
    for (const [field, extra] of [["ChangedBy", " AND [System.ChangedDate] >= @Today - 90"], ["AssignedTo", ""], ["CreatedBy", ""]] as const) {
      const rows = await wiql(t, `SELECT [System.Id],[System.${field}] FROM WorkItems WHERE ${project} AND [System.${field}] = @Me${extra}`);
      const id = (rows[0]?.fields[`System.${field}`] as { id?: string } | undefined)?.id;
      if (id) {
        await deps.set(KEYS.me(t.org), id);
        return id;
      }
    }
    throw new Error("Couldn't tell which Azure DevOps identity is you: no work item in this project is yours yet.");
  }

  async function updatesOf(t: Target, id: number): Promise<WorkItemUpdate[]> {
    const all: WorkItemUpdate[] = [];
    for (let page = 0; page < UPDATE_PAGES; page++) {
      const res = (await deps.az(t.org, invoke("wit", "updates", { project: t.project, id }, page ? { $skip: page * 200 } : {}))) as { value?: WorkItemUpdate[] };
      const value = res.value ?? [];
      all.push(...value);
      if (value.length < 200) break;
    }
    return all;
  }

  async function listPrs(t: Target, criteria: Record<string, string>): Promise<Pr[]> {
    const res = (await deps.az(t.org, invoke("git", "pullRequests", { project: t.project }, { ...criteria, "searchCriteria.status": "active", $top: 100 }))) as { value?: Pr[] };
    return res.value ?? [];
  }

  async function activePrs(t: Target, me: string): Promise<Pr[]> {
    const [created, reviewing] = await Promise.all([listPrs(t, { "searchCriteria.creatorId": me }), listPrs(t, { "searchCriteria.reviewerId": me })]);
    const byId = new Map<number, Pr>();
    for (const pr of [...reviewing, ...created]) byId.set(pr.pullRequestId, pr);
    return [...byId.values()].sort((a, b) => b.pullRequestId - a.pullRequestId);
  }

  async function workItems(t: Target, me: string, since: string): Promise<UpdateEvent[]> {
    const rows = await wiql(
      t,
      `SELECT [System.Id],[System.Title],[System.WorkItemType],[System.ChangedDate],[System.AssignedTo],[System.CreatedBy] FROM WorkItems WHERE [System.TeamProject] = '${quote(t.project)}' AND [System.ChangedDate] >= '${dayBefore(since)}' ORDER BY [System.ChangedDate] DESC`,
    );
    const changed = rows.filter((r) => after(r.fields["System.ChangedDate"], since)).slice(0, ITEM_CAP);
    const lower = me.toLowerCase();
    const found = await mapLimit(changed, async (row) => {
      const item: Candidate = {
        id: row.id,
        title: row.fields["System.Title"] ?? "",
        type: row.fields["System.WorkItemType"],
        mine: [row.fields["System.AssignedTo"]?.id, row.fields["System.CreatedBy"]?.id].some((id) => id?.toLowerCase() === lower),
      };
      return workItemEvents(t.org, t.project, item, await updatesOf(t, row.id), me, since);
    });
    return found.flat();
  }

  async function pullRequests(t: Target, me: string, since: string, at: string): Promise<UpdateEvent[]> {
    const prs = await activePrs(t, me);
    const prev = (await deps.get(KEYS.prs(t))) as PrSnaps | null;
    const diff = diffPrs(t.org, t.project, prev ?? {}, prs, me, at);
    const events = [...diff.events];
    const next = snapshot(prs, me);
    for (const id of diff.gone) {
      const pr = (await deps.az(t.org, invoke("git", "pullRequests", { project: t.project, pullRequestId: id }))) as Pr;
      const event = statusEvent(t.org, t.project, pr, at);
      if (event) events.push(event);
      // Still active (the list is capped): keep it, so it's looked up again.
      else if (pr.status === "active" && prev?.[id]) next[id] = prev[id];
    }
    const threads = await mapLimit(prs.slice(0, PR_CAP), async (pr) => {
      const res = (await deps.az(t.org, invoke("git", "pullRequestThreads", { project: t.project, repositoryId: pr.repository.id ?? pr.repository.name, pullRequestId: pr.pullRequestId }))) as { value?: Thread[] };
      return threadEvents(t.org, t.project, pr, res.value ?? [], me, since);
    });
    events.push(...threads.flat());
    await deps.set(KEYS.prs(t), next);
    return events;
  }

  /** One project: nothing but a watermark and a snapshot on its first check, so old activity doesn't flood in. */
  async function check(t: Target, startedMs: number): Promise<UpdateEvent[]> {
    const me = await identity(t);
    const stored = await deps.get(KEYS.wm(t));
    const at = new Date(startedMs).toISOString();
    if (typeof stored !== "string") {
      const prs = await activePrs(t, me);
      await deps.set(KEYS.prs(t), snapshot(prs, me));
      await deps.set(KEYS.wm(t), watermark(startedMs));
      return [];
    }
    const events = [...(await workItems(t, me, stored)), ...(await pullRequests(t, me, stored, at))];
    await deps.set(KEYS.wm(t), watermark(startedMs));
    return events;
  }

  return {
    /** Remembers a project to watch: the ones your panes are in, and the ones a work item came from. */
    async addProject(org: string, project: string) {
      const known = ((await deps.get(KEYS.projects)) ?? {}) as Record<string, string[]>;
      if (known[org]?.[0] === project) return;
      await deps.set(KEYS.projects, rememberProject(known, org, project));
    },

    async targets(): Promise<Target[]> {
      const known = ((await deps.get(KEYS.projects)) ?? {}) as Record<string, string[]>;
      return Object.entries(known).flatMap(([org, projects]) => projects.map((project) => ({ org, project })));
    },

    async stored(): Promise<StoredEvent[]> {
      return (((await deps.get(KEYS.events)) as StoredEvent[] | null) ?? []).filter((e) => e && typeof e.key === "string");
    },

    /** Marks one event read, or all of them. The Updates pages ask main to do it, so main is the only writer. */
    markRead: (key?: string) =>
      exclusive(async () => {
        const kept = (await deps.get(KEYS.events)) as StoredEvent[] | null;
        await deps.set(KEYS.events, (kept ?? []).map((e) => (!key || e.key === key ? { ...e, read: true } : e)));
      }),

    clear: () => exclusive(() => deps.set(KEYS.events, [])),

    /** One pass over `targets`. Resolves the new events, already stored and notified. */
    async cycle(targets: Target[], intervalMs: number): Promise<UpdateEvent[]> {
      const startedMs = deps.now();
      const found: UpdateEvent[] = [];
      for (const t of targets) {
        const id = `${t.org}|${t.project}`;
        const hold = failing.get(id);
        if (hold && hold.until > startedMs) continue;
        try {
          found.push(...(await check(t, startedMs)));
          failing.delete(id);
        } catch (error) {
          const failures = (hold?.failures ?? 0) + 1;
          failing.set(id, { failures, until: startedMs + backoffMs(failures, intervalMs) });
          console.warn(`[work-item] update check failed (${failures} in a row):`, error instanceof Error ? error.message : error);
        }
      }
      return exclusive(() => report(found));
    },
  };

  async function report(found: UpdateEvent[]): Promise<UpdateEvent[]> {
    const seen = ((await deps.get(KEYS.seen)) as Seen | null) ?? {};
    const events = fresh(found, seen).sort((a, b) => Date.parse(a.at) - Date.parse(b.at));
    if (events.length === 0) return [];
    await deps.set(KEYS.seen, remember(seen, events));
    const kept = (await deps.get(KEYS.events)) as StoredEvent[] | null;
    await deps.set(KEYS.events, [...events.map((e) => ({ ...e, read: false })).reverse(), ...(kept ?? [])].slice(0, EVENTS_CAP));
    for (const note of notes(events)) {
      // Wings refuses past 3 a minute; the event is in the sidebar either way.
      await deps.notify(note).catch(() => {});
    }
    return events;
  }
}
