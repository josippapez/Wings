// Issues left in progress or in review after their pull request merged: every 10 minutes while Wings is focused, each
// live issue's linked PR is looked up, and a merged one finishes the issue. Background reads: a missing or signed-out CLI
// only skips the issue.

import { pollWhileFocused } from "wings-plugin-sdk/poll";
import { isLive } from "../lib/journal";
import { prOps, prStateCommand, prStateOf, type PrState } from "../lib/pr-state";
import { readStore } from "../lib/storage";
import { write } from "./write";

const EVERY_MS = 10 * 60 * 1000;

async function stateOf(url: string): Promise<PrState | null> {
  const command = prStateCommand(url);
  if (!command) return null;
  const result = await wings.exec(command.program, command.args).catch(() => null);
  return result?.code === 0 ? prStateOf(result.stdout) : null;
}

export async function checkPrs(): Promise<void> {
  const states = new Map<string, Promise<PrState | null>>();
  for (const { id, name } of await wings.projects()) {
    const { issues } = await readStore(id, name);
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
