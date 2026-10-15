/** The Pull requests section under each project in the sidebar: its open PRs, yours first, then the ones waiting for your review. */
import { cached, singleFlight } from "wings-plugin-sdk/async";
import { plural } from "wings-plugin-sdk/format";
import { argsOf } from "../lib/args";
import { forgeFor, isKnown } from "../lib/forges/registry";
import { refOf } from "../lib/providers";
import { BLOCK_ROWS, fitPulls, orderPulls, type PullRow, type PullsResponse } from "../lib/pulls";
import type { KnownRemote, ListedPr, Status } from "../lib/types";
import { newPulls, notesFor } from "../lib/watch";
import { answered, explains, problemOf } from "./problems";
import { remoteFor } from "./remote";
import { known, links, prune, state, status } from "./state";

const SECTION = "pulls";
/** The sidebar shows only yours and the ones asked of you, so a short list. */
const MAX_ROWS = 50;
/** A little under the 60 s poll, so each poll fetches fresh while a burst of pane changes shares one list. */
const LIST_TTL_MS = 55_000;

/** repo → its open PRs. A failed list is kept for as long, so a signed-out CLI isn't asked on every pane change. */
const lists = cached(
  async ({ cwd, remote }: { cwd: string; remote: KnownRemote }) => {
    try {
      const items = await forgeFor(remote).list(cwd, remote);
      answered(remote);
      return items;
    } catch (error) {
      explains(remote, error);
      throw error;
    }
  },
  { ttlMs: LIST_TTL_MS, failTtlMs: LIST_TTL_MS, key: ({ remote }) => forgeFor(remote).repoKey(remote) },
);
/** PR url → comments and reviews counted when you last looked, to tell what's new. */
const seen = new Map<string, number>();
/** PR url → comments and reviews counted in the latest list. */
const activity = new Map<string, number>();
/** repo → the urls of its open PRs on the last check, to tell which are new. */
const opened = new Map<string, Set<string>>();
/** A check interval is set in Settings, so new PRs get a notification. */
let watching = false;
/** `seen` as last saved. */
let persisted = "";
let loaded: Promise<void> | undefined;
/** What was seen before this run, read once. */
const loadSeen = () =>
  (loaded ??= wings.storage.get("seen").then((saved) => {
    for (const [url, count] of Object.entries((saved as Record<string, number> | null) ?? {})) seen.set(url, count);
    persisted = JSON.stringify(Object.fromEntries(seen));
  }));

/** A repo never listed is fetched. A listed one is served at any age when `anyAge` (a redraw from pane changes), else only while younger than the TTL. */
function listFor(cwd: string, remote: KnownRemote, anyAge: boolean) {
  const list = { cwd, remote };
  return (anyAge ? lists.peek(list) : undefined) ?? lists.get(list);
}

/** Keeps every listed PR's ref for the tab and the diff, and the full status of a list that carries it for the rows. */
function remember(items: ListedPr[]) {
  for (const { link, status: st, loaded: full, activity: count } of items) {
    if (known.size > 300) known.delete(known.keys().next().value!);
    known.set(link.url, link);
    if (full) status.set(link.url, st);
    if (count !== null) activity.set(link.url, count);
  }
}

function row(item: ListedPr, cwd: string) {
  const { link } = item;
  // The full status, once a pane or the tab has loaded it, knows about checks that the list can't see.
  const entry = status.get(link.url);
  const st = (entry && !entry.error ? entry : item.status) as Status;
  const { failed, running } = st.checks;
  const count = activity.get(link.url);
  const unread = count !== undefined && seen.has(link.url) ? Math.max(0, count - seen.get(link.url)!) : 0;
  const agent = state.panes.find((p) => p.session && links.get(p.paneId)?.url === link.url);
  const detail = [
    refOf(link),
    !item.mine && "needs your review",
    failed ? `${plural(failed, "check")} failing` : running ? "checks running" : null,
    unread ? `${unread} unread` : null,
    agent && (agent.session!.state === "working" ? "agent working" : "agent here"),
  ].filter(Boolean).join(" · ");
  // The host caps an id at 64 characters, and a URL can be longer.
  return { id: `${link.provider}:${link.number}`, label: item.title.slice(0, 120), detail: detail.slice(0, 120), badge: { text: st.label, tone: st.tone }, icon: st.icon, open: { view: "pr", args: argsOf(link, item.title, cwd) } };
}

/** The `fresh` of the next run: set by every call that asks for it, read by the run that starts. */
let wantFresh = false;

/** One run at a time: a run asked for during another one goes again after it. */
const runProjects = singleFlight(async () => {
  const fresh = wantFresh;
  wantFresh = false;
  await loadSeen();
  const projects = await wings.projects();
  const live = new Set<string>();
  let complete = true;
  await Promise.all(
    projects.map(async (project) => {
      try {
        const remote = await remoteFor(project.path);
        const items = isKnown(remote) ? await listFor(project.path, remote, !fresh) : [];
        remember(items);
        if (isKnown(remote)) announce(project, forgeFor(remote).repoKey(remote), items);
        items.forEach((i) => live.add(i.link.url));
        // A PR seen for the first time starts from what it has, so only what comes after counts as unread.
        for (const i of items) if (i.activity !== null && !seen.has(i.link.url)) seen.set(i.link.url, i.activity);
        // The sidebar is yours and the ones asked of you; the tab and the block list the rest.
        const mine = [...items.filter((i) => i.mine), ...items.filter((i) => !i.mine && i.reviewing)];
        const rows = mine.slice(0, MAX_ROWS).map((i) => row(i, project.path));
        await wings.setProjectSection(project.id, SECTION, rows.length ? rows : null);
      } catch (error) {
        complete = false;
        console.error(error);
      }
    }),
  );
  if (complete) {
    // Forget what you've seen of PRs that are no longer open, but only when every list came back.
    for (const url of [...seen.keys()]) if (!live.has(url)) seen.delete(url);
    for (const url of [...activity.keys()]) if (!live.has(url)) activity.delete(url);
    prune(live);
  }
  const json = JSON.stringify(Object.fromEntries(seen));
  if (json !== persisted) void wings.storage.set("seen", JSON.parse(json));
  persisted = json;
});

/**
 * Sets each project's section from its repo's list (one call per repo). Only `fresh`, the focused poll, goes back to the
 * network for a repo it already listed; pane and project changes redraw the rows, with the agent marks, from the cache.
 */
export function refreshProjects(fresh = false): Promise<void> {
  wantFresh ||= fresh;
  return runProjects();
}

export function setWatching(on: boolean) {
  watching = on;
}

/** Notifies about the PRs that weren't open on the last check. Tracked with notifications off too, so turning them on doesn't announce every open PR. */
function announce(project: WingsProject, repo: string, items: ListedPr[]) {
  const fresh = newPulls(opened.get(repo), items).map((i) => ({ title: i.title, author: i.author, url: i.link.url, args: argsOf(i.link, i.title, project.path) }));
  opened.set(repo, new Set(items.map((i) => i.link.url)));
  if (watching) for (const note of notesFor(project.name, fresh)) void wings.notify({ ...note, projectId: project.id }).catch(console.error);
}

/** Sign-in or Try again: the lists that failed, or came back empty for lack of it, are fetched again. */
export function forgetLists() {
  lists.clear();
}

/** The tab for a PR is open: everything up to its latest comment counts as read. */
export function markSeen(url: string) {
  const count = activity.get(url);
  if (count === undefined) return;
  seen.set(url, count);
  void refreshProjects();
}

/**
 * The open pull requests of one project's repo for a Pull requests block. It shares the cache of the sidebar section,
 * so a block and a section cost one list per repo. A block asks only while visible and focused, so it never serves a list older than the TTL.
 */
export async function pullsFor(path: string): Promise<Omit<PullsResponse, "type" | "rid">> {
  await loadSeen();
  const remote = await remoteFor(path);
  if (!isKnown(remote)) return { none: true, host: remote.host };
  let items: ListedPr[];
  try {
    items = await listFor(path, remote, false);
  } catch (error) {
    // A host that needs signing in, a token or a CLI gets a short message and a button, not the error.
    const problem = problemOf(remote);
    if (problem?.kind === "missing") return { problem: { message: problem.message, action: "Install" } };
    if (problem?.kind === "signed-out") return { problem: { message: problem.message, action: "Sign in" } };
    if (problem?.kind === "token") return { problem: { message: problem.message, action: "Add token" } };
    throw error;
  }
  remember(items);
  const rows: PullRow[] = items.map((item) => {
    const { link } = item;
    const entry = status.get(link.url);
    const st = (entry && !entry.error ? entry : item.status) as Status;
    const count = activity.get(link.url);
    const agent = state.panes.find((p) => p.session && links.get(p.paneId)?.url === link.url);
    return {
      id: `${link.provider}:${link.number}`,
      title: item.title.slice(0, 160),
      number: refOf(link),
      author: item.author,
      branch: st.branch,
      scope: item.mine ? "mine" : item.reviewing ? "review" : "other",
      draft: st.label === "Draft",
      badge: { text: st.label, tone: st.tone },
      review: st.review,
      checks: st.checks,
      updated: st.updated,
      unread: count !== undefined && seen.has(link.url) ? Math.max(0, count - seen.get(link.url)!) : 0,
      agent: agent ? (agent.session!.state === "working" ? "working" : "here") : null,
      args: argsOf(link, item.title, path),
    };
  });
  return { rows: fitPulls(orderPulls(rows).slice(0, BLOCK_ROWS)) };
}
