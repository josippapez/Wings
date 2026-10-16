// Notifications about comments that mention you or reply in a thread you're in, on every PR the lists show. Which comments
// count and what is remembered are here. No `wings` is touched until the setting is read, so node tests drive it.
import { setting } from "wings-plugin-sdk/settings";
import type { Me } from "./forges/types";
import { same } from "./forges/shared";
import { oneLine } from "./threads";
import type { Comment } from "./types";

/** Set on the Settings page, next to the new pull request notifications. */
export const mentionsOn = setting<boolean>("notify-mentions", { default: false, check: (v) => (typeof v === "boolean" ? v : undefined) });

/** The comment keys kept, newest last. A PR's comments are only looked at once the PR changes, so this only has to span a few checks. */
export const MAX_SEEN = 400;
/** The PRs whose comments one check fetches. The ones checked longest ago go first, so a repo with more is covered over a few checks. */
export const MAX_FETCH = 15;
/** More comments than this at once are one notification, so a burst stays under the host's 20 a minute. */
export const MAX_NOTES = 3;

/** What is kept in `wings.storage`: when each PR was last looked at, and the comments already notified. */
export interface Inbox {
  /** PR url → what it was last checked at: its `updatedAt` from the list, or the time of the check where the list has none. Comments before this are old news. */
  marks: Record<string, string>;
  seen: string[];
}

export interface Hit {
  kind: "mention" | "reply";
  key: string;
  /** The PR it is on. */
  url: string;
  comment: Comment;
}

const escape = (s: string) => s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

/** Whether a comment's text writes one of your handles as `@handle`. GitHub, GitLab and Codeberg write `@login`, Bitbucket may wrap the id in `{}` and Azure DevOps in `<>`. */
export function mentions(body: string, handles: string[]): boolean {
  return handles.some((h) => new RegExp(`(?<![\\w@])@[{<]?${escape(h)}[}>]?(?![\\w-])`, "i").test(body));
}

/** The hits among `comments` that are newer than `after` (ms), skipping yours and the provider's own: a mention, or any other comment in a thread you wrote in. */
export function hitsIn(comments: Comment[], me: Me, after: number, url: string): Hit[] {
  const mine = (c: Comment) => c.by !== undefined && me.ids.some((id) => same(id, c.by!));
  const threads = new Set(comments.filter(mine).map((c) => c.replyTo ?? c.id));
  const hits: Hit[] = [];
  for (const c of comments) {
    if (c.system || mine(c) || !(Date.parse(c.createdAt) > after)) continue;
    const kind = mentions(c.body, me.handles) ? "mention" : threads.has(c.replyTo ?? c.id) ? "reply" : null;
    if (kind) hits.push({ kind, key: keyOf(url, c), url, comment: c });
  }
  return hits;
}

/** A short hash of the PR's url, so the keys stay small. */
function short(text: string) {
  let h = 5381;
  for (let i = 0; i < text.length; i++) h = (Math.imul(h, 33) ^ text.charCodeAt(i)) >>> 0;
  return h.toString(36);
}

/** What names a comment across checks. A GitHub conversation comment's id counts its place in the list and moves when one is added, so the id alone can't. */
export const keyOf = (url: string, c: Comment) => `${short(url)}:${c.createdAt}:${short(`${c.by ?? c.author}${c.body.length}`)}`;

export interface Item {
  url: string;
  updatedAt: string | null;
}

/**
 * What a check does with the PRs of one list. `track` only moves each PR's mark, for when notifications are off, so
 * turning them on doesn't announce what happened meanwhile. `hold` leaves a changed PR's mark alone, for while Wings is
 * focused: the next check in the background looks at what changed. `notify` fetches the comments of each changed PR.
 * A PR seen for the first time is only marked, in every mode: its first check is where counting starts.
 */
export async function scan(opts: {
  inbox: Inbox;
  items: Item[];
  mode: "track" | "hold" | "notify";
  now: string;
  me: () => Promise<Me>;
  load: (url: string) => Promise<Comment[]>;
}): Promise<Hit[]> {
  const { inbox, items, mode, now } = opts;
  const due: Item[] = [];
  for (const item of items) {
    const mark = inbox.marks[item.url];
    if (mark === undefined || mode === "track") inbox.marks[item.url] = item.updatedAt ?? now;
    // A list with no time for a PR can't say it is unchanged, so it is looked at on every check.
    else if (item.updatedAt === null || item.updatedAt !== mark) due.push(item);
  }
  if (mode !== "notify" || !due.length) return [];
  const me = await opts.me();
  const seen = new Set(inbox.seen);
  const hits: Hit[] = [];
  for (const item of due.sort((a, b) => Date.parse(inbox.marks[a.url]) - Date.parse(inbox.marks[b.url])).slice(0, MAX_FETCH)) {
    const mark = inbox.marks[item.url];
    try {
      for (const hit of hitsIn(await opts.load(item.url), me, Date.parse(mark), item.url)) {
        if (seen.has(hit.key)) continue;
        seen.add(hit.key);
        inbox.seen.push(hit.key);
        hits.push(hit);
      }
      inbox.marks[item.url] = item.updatedAt ?? now;
    } catch (error) {
      console.error(error);
    }
  }
  inbox.seen = inbox.seen.slice(-MAX_SEEN);
  return hits;
}

type Action = { label: string; open: { view: string; args?: unknown } } | { label: string; url: string };

/** A hit with what the notification says of its PR. */
export interface Note {
  hit: Hit;
  title: string;
  args: unknown;
}

const WORDS = { mention: "mentioned you", reply: "replied" } as const;

/** One notification per hit, with buttons to open the PR in a tab or the browser, or one for all of them when there are more than `MAX_NOTES`. */
export function notesFor(project: string, notes: Note[]): { title: string; body: string; actions: Action[] }[] {
  if (notes.length > MAX_NOTES) return [{ title: `${notes.length} new comments for you in ${project}`, body: notes.map((n) => `${n.hit.comment.author} ${WORDS[n.hit.kind]} on ${n.title}`).join("\n"), actions: [{ label: "All pull requests", open: { view: "pr" } }] }];
  return notes.map((n) => ({
    title: `${n.hit.comment.author} ${WORDS[n.hit.kind]} in ${project}`,
    body: `${n.title}\n${oneLine(n.hit.comment.body, 160)}`,
    actions: [{ label: "Open", open: { view: "pr", args: n.args } }, { label: "Open in browser", url: n.hit.url }],
  }));
}
