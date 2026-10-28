// Issues left in progress or in review after their pull request merged: every 10 minutes while Wings is focused, each
// live issue's linked PR is looked up, and a merged one finishes the issue. An issue with a branch and no PR link gets
// the PR for that branch looked up and linked. The PR tracker also says right away when a PR it knew merged or closed
// (`pr-state`). Background reads: a missing or signed-out CLI only skips the issue.

import { gitOk, parseRemote, type Remote } from "wings-plugin-sdk/git";
import { pollWhileFocused } from "wings-plugin-sdk/poll";
import { isLive } from "../lib/journal";
import { branchPrCommand, branchPrOf, foundOps, isPrOf, prOps, prStateCommand, prStateEventOf, prStateOf, type FoundPr, type PrState } from "../lib/pr-state";
import { readStore } from "../lib/storage";
import { write } from "./write";

const EVERY_MS = 10 * 60 * 1000;

async function stateOf(url: string): Promise<PrState | null> {
  const command = prStateCommand(url);
  if (!command) return null;
  const result = await wings.exec(command.program, command.args).catch(() => null);
  return result?.code === 0 ? prStateOf(result.stdout) : null;
}

/** The PR for a branch, looked up in the project's folder: the worktrees of a project share its remote. */
async function prOfBranch(path: string, branch: string): Promise<FoundPr | null> {
  const remote: Remote = parseRemote((await gitOk(path, ["remote", "get-url", "origin"])) ?? "");
  const command = branchPrCommand(remote, branch);
  if (!command) return null;
  const result = await wings.exec(command.program, command.args, { cwd: path }).catch(() => null);
  return result?.code === 0 ? branchPrOf(remote, result.stdout) : null;
}

export async function checkPrs(): Promise<void> {
  const states = new Map<string, Promise<PrState | null>>();
  for (const { id, name, path, parentId } of await wings.projects()) {
    if (parentId) continue;
    const { issues } = await readStore(id, name);
    const branches = new Map<string, Promise<FoundPr | null>>();
    for (const issue of issues.filter((i) => isLive(i) && !i.links?.pr && i.links?.branch)) {
      const branch = issue.links!.branch!;
      if (!branches.has(branch)) branches.set(branch, prOfBranch(path, branch));
      const found = await branches.get(branch)!;
      for (const op of found ? foundOps(issue, found, issues) : []) await write(id, name, op, "git").catch(console.error);
    }
    for (const issue of issues.filter((i) => isLive(i) && i.links?.pr)) {
      const url = issue.links!.pr!;
      if (!states.has(url)) states.set(url, stateOf(url));
      const state = await states.get(url)!;
      // Found by the plugin itself, like a commit with the issue's key.
      for (const op of state ? prOps(issue, state, issues) : []) await write(id, name, op, "git").catch(console.error);
    }
  }
}

export function watchPrs(): void {
  pollWhileFocused(() => void checkPrs().catch(console.error), EVERY_MS, { immediate: true });
}

/** The PR tracker saw a PR merge or close: the live issues it belongs to, by link or branch, are finished or noted at once. */
export function listenPrState(): void {
  wings.onEvent(({ from, name, data }) => {
    const event = from === "pr-tracker" && name === "pr-state" ? prStateEventOf(data) : null;
    if (event) void applyPrState(event).catch(console.error);
  });
}

async function applyPrState(event: NonNullable<ReturnType<typeof prStateEventOf>>): Promise<void> {
  const projects = await wings.projects();
  const source = projects.find((p) => p.id === event.projectId);
  // A worktree's issues are kept in the project it was made from.
  const home = projects.find((p) => p.id === source?.parentId) ?? source;
  if (!home) return;
  const { id, name } = home;
  const { issues } = await readStore(id, name);
  for (const issue of issues.filter((i) => isPrOf(i, event)))
    for (const op of foundOps(issue, { url: event.url, state: event.state }, issues)) await write(id, name, op, "git").catch(console.error);
}
