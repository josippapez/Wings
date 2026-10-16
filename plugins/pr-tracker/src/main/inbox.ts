/** Tells you when a comment on a listed PR mentions you or replies in a thread you wrote in. It rides on the lists the sidebar already loads: only a PR whose list entry changed has its comments fetched. */
import { argsOf } from "../lib/args";
import { forgeFor, forgeOf } from "../lib/forges/registry";
import { mentionsOn, notesFor, scan, type Inbox } from "../lib/inbox";
import type { KnownRemote, ListedPr } from "../lib/types";

const KEY = "inbox";

let inbox: Inbox = { marks: {}, seen: [] };
let persisted = "";
let loaded: Promise<void> | undefined;
const load = () =>
  (loaded ??= wings.storage.get(KEY).then((saved) => {
    const s = saved as Partial<Inbox> | null;
    inbox = { marks: s?.marks && typeof s.marks === "object" ? s.marks : {}, seen: Array.isArray(s?.seen) ? s.seen : [] };
    persisted = JSON.stringify(inbox);
  }));

/** PR url → its `updatedAt` in the latest list, which opening the PR's tab marks as read. */
const latest = new Map<string, string>();

function save() {
  const json = JSON.stringify(inbox);
  if (json !== persisted) void wings.storage.set(KEY, JSON.parse(json));
  persisted = json;
}

/** Looks at a project's listed PRs. While Wings is focused nothing is fetched or sent: a PR that changed is looked at by the next check in the background. */
export async function checkMentions(project: WingsProject, remote: KnownRemote, items: ListedPr[]) {
  await load();
  const on = await mentionsOn.get();
  for (const i of items) if (i.updatedAt) latest.set(i.link.url, i.updatedAt);
  const forge = forgeFor(remote);
  const byUrl = new Map(items.map((i) => [i.link.url, i]));
  const hits = await scan({
    inbox,
    items: items.map((i) => ({ url: i.link.url, updatedAt: i.updatedAt })),
    mode: !on ? "track" : wings.focused ? "hold" : "notify",
    now: new Date().toISOString(),
    me: () => forge.me(remote),
    load: (url) => forgeOf(byUrl.get(url)!.link).comments(byUrl.get(url)!.link),
  });
  save();
  const notes = hits.map((hit) => {
    const pr = byUrl.get(hit.url)!;
    return { hit, title: pr.title, args: argsOf(pr.link, pr.title, project.path) };
  });
  for (const note of notesFor(project.name, notes)) void wings.notify({ ...note, projectId: project.id }).catch(console.error);
}

/** Forgets what is kept of PRs that are no longer open, once every list came back. */
export function pruneMentions(live: Set<string>) {
  for (const url of Object.keys(inbox.marks)) if (!live.has(url)) delete inbox.marks[url];
  for (const url of [...latest.keys()]) if (!live.has(url)) latest.delete(url);
  save();
}

/** The PR's tab is open: what it has up to now counts as read. */
export function markRead(url: string) {
  inbox.marks[url] = latest.get(url) ?? new Date().toISOString();
  save();
}
