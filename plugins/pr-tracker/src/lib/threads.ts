/** The PR tab's conversation: comments grouped into threads, and what each thread is (a person's, resolved, a bot's). */
import { truncate } from "wings-plugin-sdk/format";
import type { Comment } from "./types";

export interface Thread {
  root: Comment;
  replies: Comment[];
}

/** Written by a bot or by the provider itself: an app's `[bot]` login, a build service, a policy or system note. */
export const isAutomated = (c: Comment) => Boolean(c.system) || /\[bot\]$|\bbot$|build service|^github-actions|^azure pipelines/i.test(c.author.trim());

/** When a thread last had a comment: its newest reply, else its first comment. */
export const lastActivity = (t: Thread) => Date.parse((t.replies.at(-1) ?? t.root).createdAt);

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

/** How a thread shows: resolved ones start collapsed; every other thread, a bot's too, reads in full. `automated` only marks it. */
export const kindOf = ({ root }: Thread): Kind => (root.thread?.state === "resolved" ? "resolved" : isAutomated(root) ? "automated" : "open");

/** A comment's text on one line, without Markdown marks, for a collapsed thread's summary. */
export function oneLine(body: string, max = 140): string {
  const text = body
    .replace(/```[\s\S]*?```/g, " code ")
    // Hidden in the full comment too: link reference definitions and HTML tags like <details>.
    .replace(/^\s{0,3}\[[^\]\n]{1,200}\]:.*$/gm, " ")
    .replace(/<\/?[a-z][^>\n]{0,200}>/gi, " ")
    .replace(/^[\s|:-]+$/gm, " ")
    .replace(/!?\[([^\]]*)\]\([^)]*\)/g, "$1")
    .replace(/[`*_~>#|]/g, "")
    .replace(/\s+/g, " ")
    .trim();
  return truncate(text, max);
}
