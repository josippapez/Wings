/** The Pull requests section under each project in the sidebar: its open PRs, yours first, then the ones waiting for your review, then, when Settings says so, the rest. */
import { cached, singleFlight } from "wings-plugin-sdk/async";
import { plural } from "wings-plugin-sdk/format";
import { argsOf } from "../lib/args";
import { forgeFor, forgeOf, isKnown } from "../lib/forges/registry";
import { refOf } from "../lib/providers";
import type { PullsAnswer } from "../lib/messages";
import { BLOCK_ROWS, orderPulls, type PullRow } from "../lib/pulls";
import type { KnownRemote, ListedPr, Status } from "../lib/types";
import { eventFor, finalState, retry, settle, type Tracked } from "../lib/finished";
import { newPulls, newReviews, notesFor, type Notify, type Sidebar } from "../lib/watch";
import { checkMentions, markRead, pruneMentions } from "./inbox";
import { answered, explains, shownProblem } from "./problems";
import { remoteFor } from "./remote";
import { known, links, prune, state, status } from "./state";

const SECTION = "pulls";
/** The host's limit on a section's rows. */
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
/** repo → the urls of its open PRs that asked for your review on the last check, to tell which ask now. */
const asked = new Map<string, Set<string>>();
/** repo → its open PRs on the last check, as `settle` keeps them, to tell which merged or closed. */
const tracked = new Map<string, Map<string, Tracked>>();
/** What Settings says to notify about. */
let notify: Notify = "off";
/** What Settings says the sidebar lists. */
let sidebar: Sidebar = "mine";
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
    !item.mine && (item.reviewing ? "needs your review" : item.author),
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
        if (isKnown(remote)) {
          announce(project, forgeFor(remote).repoKey(remote), items);
          void announceFinished(project, forgeFor(remote).repoKey(remote), items);
        }
        items.forEach((i) => live.add(i.link.url));
        // A PR seen for the first time starts from what it has, so only what comes after counts as unread.
        for (const i of items) if (i.activity !== null && !seen.has(i.link.url)) seen.set(i.link.url, i.activity);
        // Yours, then the ones asked of you, then the rest when Settings says the sidebar lists every open PR.
        const shown = [...items.filter((i) => i.mine), ...items.filter((i) => !i.mine && i.reviewing), ...(sidebar === "all" ? items.filter((i) => !i.mine && !i.reviewing) : [])];
        const rows = shown.slice(0, MAX_ROWS).map((i) => row(i, project.path));
        await wings.setProjectSection(project.id, SECTION, rows.length ? rows : null);
        if (isKnown(remote)) await checkMentions(project, remote, items).catch(console.error);
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
    pruneMentions(live);
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

export function setNotify(to: Notify) {
  notify = to;
}

export function setSidebar(to: Sidebar) {
  sidebar = to;
  void refreshProjects();
}

/**
 * Notifies about the PRs that ask for your review and didn't on the last check, and with "all" also the PRs that weren't
 * open. Tracked with notifications off too, so turning them on doesn't announce every open PR.
 */
function announce(project: WingsProject, repo: string, items: ListedPr[]) {
  const reviews = newReviews(asked.get(repo), items);
  const fresh = newPulls(opened.get(repo), items).filter((i) => !reviews.includes(i));
  opened.set(repo, new Set(items.map((i) => i.link.url)));
  asked.set(repo, new Set(items.filter((i) => i.reviewing).map((i) => i.link.url)));
  if (notify === "off") return;
  const notes = (list: ListedPr[], review: boolean) =>
    notesFor(project.name, list.map((i) => ({ title: i.title, author: i.author, url: i.link.url, args: argsOf(i.link, i.title, project.path) })), review);
  for (const note of [...notes(reviews, true), ...(notify === "all" ? notes(fresh, false) : [])]) void wings.notify({ ...note, projectId: project.id }).catch(console.error);
}

/**
 * Tells the other built-in plugins (Issues) about a PR that left the repo's open list because it merged or closed. The
 * list can't say which, so each gone PR's own status is read once; one that can't be read is tried again on the next lists.
 */
async function announceFinished(project: WingsProject, repo: string, items: ListedPr[]) {
  const { next, gone } = settle(tracked.get(repo), items, { projectId: project.id, cwd: project.path });
  tracked.set(repo, next);
  for (const t of gone) {
    try {
      const forge = forgeOf(t.link);
      const link = await forge.linkOf(argsOf(t.link, t.title, t.cwd));
      const st = await forge.status(link);
      const final = finalState(st.icon);
      if (final) await wings.emit("pr-state", eventFor(t, final));
    } catch (error) {
      console.error(error);
      retry(next, t);
    }
  }
}

/** Sign-in or Try again: the lists that failed, or came back empty for lack of it, are fetched again. */
export function forgetLists() {
  lists.clear();
}

/** The tab for a PR is open: everything up to its latest comment counts as read. */
export function markSeen(url: string) {
  void loadSeen().then(() => markRead(url));
  const count = activity.get(url);
  if (count === undefined) return;
  seen.set(url, count);
  void refreshProjects();
}

/**
 * The open pull requests of one project's repo for a Pull requests block. It shares the cache of the sidebar section,
 * so a block and a section cost one list per repo. A block asks only while visible and focused, so it never serves a list older than the TTL.
 */
export async function pullsFor(path: string): Promise<PullsAnswer> {
  await loadSeen();
  const remote = await remoteFor(path);
  if (!isKnown(remote)) return { none: true, host: remote.host };
  let items: ListedPr[];
  try {
    items = await listFor(path, remote, false);
  } catch (error) {
    // A host that needs signing in, a token or a CLI gets a short message and a button, not the error.
    const problem = shownProblem(remote);
    if (problem) return { problem };
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
  return { rows: orderPulls(rows).slice(0, BLOCK_ROWS) };
}
