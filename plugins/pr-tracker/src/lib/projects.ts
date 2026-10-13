/** The Pull requests section under each project in the sidebar: its open PRs, yours first, then the ones waiting for your review. */
import { azureList } from "./azure";
import { bitbucketList } from "./bitbucket";
import { giteaList } from "./gitea";
import { githubList } from "./github";
import { gitlabList } from "./gitlab";
import { remoteFor } from "./remote";
import { argsOf } from "./refs";
import { known, links, problems, refOf, state, status } from "./state";
import { problemKey } from "./view";
import { BLOCK_ROWS, fitRows, orderPulls, serveCached, type PullRow, type PullsResponse } from "./pulls";
import type { AzureRemote, BitbucketRemote, GiteaRemote, GithubRemote, GitlabRemote, KnownRemote, ListedPr, Status } from "./types";

const SECTION = "pulls";
/** The sidebar shows only yours and the ones asked of you, so a short list. */
const MAX_ROWS = 50;
/** A little under the 60 s poll, so each poll fetches fresh while a burst of pane changes shares one list. */
const LIST_TTL_MS = 55_000;

/** `origin` → the repo's open PRs. A failed list is kept for the same time, so a signed-out CLI isn't asked on every pane change. */
const lists = new Map<string, { at: number; items: Promise<ListedPr[]> }>();
/** Project id → the rows last sent, so an unchanged section isn't sent again. */
const sent = new Map<string, string>();
/** PR url → comments and reviews counted when you last looked, to tell what's new. */
const seen = new Map<string, number>();
/** PR url → comments and reviews counted in the latest list. */
const activity = new Map<string, number>();
/** `seen` as last saved. */
let persisted = "";
const loaded = wings.storage.get("seen").then((saved) => {
  for (const [url, count] of Object.entries((saved as Record<string, number> | null) ?? {})) seen.set(url, count);
  persisted = JSON.stringify(Object.fromEntries(seen));
});

/** A repo never listed is fetched. A listed one is served at any age when `anyAge` (a redraw from pane changes), else only while younger than the TTL. */
function listFor(cwd: string, remote: KnownRemote, anyAge: boolean) {
  const hit = lists.get(remote.origin!);
  if (hit && serveCached(Date.now() - hit.at, LIST_TTL_MS, anyAge)) return hit.items;
  const items = {
    github: () => githubList(cwd, remote as GithubRemote),
    azure: () => azureList(cwd, remote as AzureRemote),
    gitlab: () => gitlabList(remote as GitlabRemote),
    bitbucket: () => bitbucketList(cwd, remote as BitbucketRemote),
    gitea: () => giteaList(remote as GiteaRemote),
    none: () => Promise.resolve([]),
  }[remote.kind]();
  lists.set(remote.origin!, { at: Date.now(), items });
  return items;
}

/** Keeps every listed PR's ref for the tab and the diff, and a GitHub list's full status for the rows. */
function remember(items: ListedPr[]) {
  for (const { link, status: st, activity: count } of items) {
    if (known.size > 300) known.delete(known.keys().next().value!);
    known.set(link.url, link);
    if (link.provider === "github") status.set(link.url, st);
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
    failed ? `${failed} ${failed === 1 ? "check" : "checks"} failing` : running ? "checks running" : null,
    unread ? `${unread} unread` : null,
    agent && (agent.session!.state === "working" ? "agent working" : "agent here"),
  ].filter(Boolean).join(" · ");
  // The host caps an id at 64 characters, and a URL can be longer.
  return { id: `${link.provider}:${link.number}`, label: item.title.slice(0, 120), detail: detail.slice(0, 120), badge: { text: st.label, tone: st.tone }, open: { view: "pr", args: argsOf(link, item.title, cwd) } };
}

let running = false;
let again: { fresh: boolean } | null = null;

/**
 * Sets each project's section from its repo's list (one call per repo). Only `fresh`, the focused poll, goes back to the
 * network for a repo it already listed; pane and project changes redraw the rows, with the agent marks, from the cache.
 */
export async function refreshProjects(fresh = false) {
  // One run at a time: a run asked for during another one goes again after it.
  if (running) return void (again = { fresh: fresh || Boolean(again?.fresh) });
  running = true;
  try {
    await loaded;
    const projects = await wings.projects();
    const live = new Set<string>();
    let complete = true;
    await Promise.all(
      projects.map(async (project) => {
        try {
          const remote = await remoteFor(project.path);
          const items = remote.kind === "none" ? [] : await listFor(project.path, remote, !fresh);
          remember(items);
          items.forEach((i) => live.add(i.link.url));
          // A PR seen for the first time starts from what it has, so only what comes after counts as unread.
          for (const i of items) if (i.activity !== null && !seen.has(i.link.url)) seen.set(i.link.url, i.activity);
          // The sidebar is yours and the ones asked of you; the tab and the block list the rest.
          const mine = [...items.filter((i) => i.mine), ...items.filter((i) => !i.mine && i.reviewing)];
          const rows = mine.slice(0, MAX_ROWS).map((i) => row(i, project.path));
          const json = JSON.stringify(rows);
          if (sent.get(project.id) === json) return;
          await wings.setProjectSection(project.id, SECTION, rows.length ? rows : null);
          sent.set(project.id, json);
        } catch (error) {
          complete = false;
          console.error(error);
        }
      }),
    );
    // Forget what you've seen of PRs that are no longer open, but only when every list came back.
    if (complete) for (const url of [...seen.keys()]) if (!live.has(url)) seen.delete(url);
    const json = JSON.stringify(Object.fromEntries(seen));
    if (json !== persisted) void wings.storage.set("seen", JSON.parse(json));
    persisted = json;
  } finally {
    running = false;
    if (again) {
      const next = again;
      again = null;
      void refreshProjects(next.fresh);
    }
  }
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
  await loaded;
  const remote = await remoteFor(path);
  if (remote.kind === "none") return { none: true, host: remote.host };
  let items: ListedPr[];
  try {
    items = await listFor(path, remote, false);
  } catch (error) {
    // A host that needs signing in, a token or a CLI gets a short message and a button, not the error.
    const problem = problems.get(problemKey(remote));
    if (problem?.missing) return { problem: { message: problem.message, action: "Install" } };
    if (problem?.login) return { problem: { message: problem.message, action: problem.token ? "Add token" : "Sign in" } };
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
      number: refOf(link).split(" ").at(-1)!,
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
  return { rows: fitRows(orderPulls(rows).slice(0, BLOCK_ROWS)) };
}
