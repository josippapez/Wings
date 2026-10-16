/** The PR's pushes and how they sit among its threads. No `wings`, no I/O, so node tests drive it. */
import { lastActivity, type Thread } from "./threads";

interface PushCommit {
  sha: string;
  /** The first line of the commit message. */
  message: string;
  author: string;
  at: string;
}

/** One push to the PR's branch: Azure DevOps numbers them as the PR's iterations. */
export interface Push {
  id: number;
  author: string;
  at: string;
  commits: PushCommit[];
  /** The list was cut by the provider, so there are more commits than shown. */
  more: boolean;
  /** The source and the merge base at this iteration, which a thread's snippet is read from. */
  head: string | null;
  base: string | null;
  /** The push that opened the PR: Azure DevOps gives its iteration the reason `create`. */
  created: boolean;
}

/** An iteration from `pullRequests/{id}/iterations?includeCommits=true`, the parts read. */
export interface AzureIteration {
  id: number;
  author?: { displayName?: string };
  createdDate: string;
  hasMoreCommits?: boolean;
  reason?: string;
  sourceRefCommit?: { commitId?: string };
  commonRefCommit?: { commitId?: string };
  commits?: { commitId: string; comment?: string; author?: { name?: string; date?: string } }[];
}

/**
 * Pushes, oldest first. A commit shows in the first push that has it, so a list that repeats earlier commits (as a
 * cumulative one would) still reads as what each push added. A push with no new commits, like a merge of the target
 * branch's own update, has no card.
 */
export function pushesOf(iterations: AzureIteration[]): Push[] {
  const seen = new Set<string>();
  const pushes: Push[] = [];
  for (const it of [...iterations].sort((a, b) => a.id - b.id)) {
    const fresh = (it.commits ?? []).filter((c) => !seen.has(c.commitId));
    fresh.forEach((c) => seen.add(c.commitId));
    if (!fresh.length) continue;
    const commits = fresh
      .map((c): PushCommit => ({ sha: c.commitId, message: (c.comment ?? "").split("\n")[0].trim(), author: c.author?.name ?? "unknown", at: c.author?.date ?? it.createdDate }))
      .sort((a, b) => Date.parse(b.at) - Date.parse(a.at));
    pushes.push({ id: it.id, author: it.author?.displayName ?? commits[0].author, at: it.createdDate, commits, more: Boolean(it.hasMoreCommits), head: it.sourceRefCommit?.commitId ?? null, base: it.commonRefCommit?.commitId ?? null, created: it.reason === "create" });
  }
  return pushes;
}

type Entry = { kind: "push"; push: Push; at: number } | { kind: "thread"; thread: Thread; at: number };

/** Pushes and threads in one list, the newest activity first. */
export function timelineOf(threads: Thread[], pushes: Push[]): Entry[] {
  return [
    ...threads.map((thread): Entry => ({ kind: "thread", thread, at: lastActivity(thread) })),
    ...pushes.map((push): Entry => ({ kind: "push", push, at: Date.parse(push.at) })),
  ].sort((a, b) => b.at - a.at);
}

/** "Today at 10:50 AM", "Yesterday at 9:33 AM", else "12 Oct, 10:50 AM". */
export function dayTime(iso: string, now = new Date()): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return "";
  const time = d.toLocaleTimeString("en-US", { hour: "numeric", minute: "2-digit" });
  const days = Math.round((new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime() - new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime()) / 86_400_000);
  if (days === 0) return `Today at ${time}`;
  if (days === 1) return `Yesterday at ${time}`;
  return `${d.toLocaleDateString("en-GB", { day: "numeric", month: "short" })}, ${time}`;
}
