/** The PR tab's conversation: comments grouped into threads, and what each thread is (a person's, resolved, a bot's). */
import type { Comment } from "./types";

export interface Thread {
  root: Comment;
  replies: Comment[];
}

/** Written by a bot or by the provider itself: an app's `[bot]` login, a build service, a policy or system note. */
export const isAutomated = (c: Comment) => Boolean(c.system) || /\[bot\]$|\bbot$|build service|^github-actions|^azure pipelines/i.test(c.author.trim());

/** When a thread last had a comment: its newest reply, else its first comment. */
const lastActivity = (t: Thread) => Date.parse((t.replies.at(-1) ?? t.root).createdAt);

/** Comments grouped under their thread's first comment. The thread with the newest comment comes first; replies read oldest first. */
export function threadsOf(comments: Comment[]): Thread[] {
  const replies = new Map<number, Comment[]>();
  for (const c of comments) if (c.replyTo !== null) replies.set(c.replyTo, [...(replies.get(c.replyTo) ?? []), c]);
  return comments
    .filter((c) => c.replyTo === null)
    .map((root) => ({ root, replies: (replies.get(root.id) ?? []).sort((a, b) => Date.parse(a.createdAt) - Date.parse(b.createdAt)) }))
    .sort((a, b) => lastActivity(b) - lastActivity(a));
}

/** Where the diff viewer should look for a thread: its file, and its line on the side it was written on. */
export function focusOf(root: Comment): WingsDiffFocus | undefined {
  if (!root.path) return undefined;
  return root.line ? { path: root.path, line: root.line, side: root.side === "deletions" ? "old" : "new" } : { path: root.path };
}

export type Kind = "open" | "resolved" | "automated";

/** How a thread shows: a thread a person wrote and that is still open reads in full, the rest start collapsed. */
export const kindOf = ({ root }: Thread): Kind => (root.thread?.state === "resolved" ? "resolved" : isAutomated(root) ? "automated" : "open");

/** A comment's text on one line, without Markdown marks, for a collapsed thread's summary. */
export function oneLine(body: string, max = 140): string {
  const text = body
    .replace(/```[\s\S]*?```/g, " code ")
    .replace(/^[\s|:-]+$/gm, " ")
    .replace(/!?\[([^\]]*)\]\([^)]*\)/g, "$1")
    .replace(/[`*_~>#|]/g, "")
    .replace(/\s+/g, " ")
    .trim();
  return text.length > max ? `${text.slice(0, max - 1)}…` : text;
}

/** "just now", "5 min ago", "3 h ago", "2 d ago", then the date. */
export function ago(iso: string, now = Date.now()): string {
  const t = Date.parse(iso);
  if (Number.isNaN(t)) return "";
  const minutes = Math.max(0, Math.round((now - t) / 60_000));
  if (minutes < 1) return "just now";
  if (minutes < 60) return `${minutes} min ago`;
  if (minutes < 1440) return `${Math.round(minutes / 60)} h ago`;
  if (minutes < 43_200) return `${Math.round(minutes / 1440)} d ago`;
  return new Date(t).toLocaleDateString(undefined, { year: "numeric", month: "short", day: "numeric" });
}
