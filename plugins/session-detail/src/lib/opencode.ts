// OpenCode sessions: no transcript to follow, so Wings is asked what a session is doing (`wings.agentActivity`)
// when its pane changes and when Wings says it did something (`wings.onAgentActivity`).

import { singleFlight } from "wings-plugin-sdk/async";
import { basename, truncate } from "wings-plugin-sdk/format";

import { oneLine } from "./format";
import { highlights } from "./highlights";
import { session, type Session, type ToolItem } from "./session";
import type { Item } from "./types";

/** The OpenCode tool calls Wings reports: which input says what a call is about. */
function summary(name: string, input: Record<string, string>, files: string[]) {
  if (files.length) return oneLine(files.map((f) => basename(f)).join(", "));
  if (name === "shell") return oneLine(input.command);
  const file = input.path ?? input.filePath;
  if (file) return basename(file);
  return oneLine(input.pattern ?? input.url ?? input.query ?? input.description ?? input.name ?? input.code ?? "");
}

/** Replaces what the session shows with what Wings reported: the activity is read whole each time, not appended to. */
export function applyActivity(s: Session, a: WingsAgentActivity) {
  const items: Item[] = [];
  const running = new Map<string, ToolItem>();
  const files = new Map<string, number>();
  for (const i of a.items) {
    if (i.kind === "prompt") {
      items.push({ kind: "prompt", key: i.id, summary: oneLine(i.text, 140), text: truncate(i.text, 4000), at: i.at });
      continue;
    }
    const marks = i.name === "shell" ? highlights(i.input.command) : null;
    const done = i.status !== "running";
    const tool: ToolItem = {
      kind: "tool",
      key: i.id,
      name: i.name,
      summary: marks?.title ?? summary(i.name, i.input, i.files),
      tags: marks?.tags ?? null,
      outcome: null,
      input: Object.entries(i.input),
      result: i.result,
      at: i.at,
      ms: i.ms,
      error: i.status === "error",
      done,
    };
    items.push(tool);
    if (!done) running.set(i.id, tool);
    for (const path of i.files) files.set(path, (files.get(path) ?? 0) + 1);
  }
  s.items = items;
  s.tools = running;
  s.files = files;
  s.model = a.model;
  s.context = a.context;
  s.tokens = { input: a.tokens.input, output: a.tokens.output + a.tokens.reasoning, cacheRead: a.tokens.cacheRead, cacheWrite: a.tokens.cacheWrite };
  s.cost = a.cost;
  s.updated = Math.max(s.updated, ...items.map((i) => i.at));
}

/** Follows the OpenCode sessions that run in panes. `changed` is called whenever one of them did. */
export function followOpencode(changed: (s: Session) => void, w = wings) {
  const tracked = new Map<string, { s: Session; load: () => Promise<void> }>();

  w.onPanes((list) => {
    const running = new Map(list.filter((p) => p.session?.kind === "opencode").map((p) => [p.session!.sessionId, p]));
    for (const id of tracked.keys()) if (!running.has(id)) tracked.delete(id);
    for (const [id, pane] of running) {
      let t = tracked.get(id);
      const first = !t;
      if (!t) {
        const s = session(id, "opencode");
        const load = singleFlight(async () => {
          try {
            applyActivity(s, await w.agentActivity(id));
          } catch (error) {
            // OpenCode's service isn't running or forgot the session: the sidebar keeps what it had.
            console.error(error);
          }
          changed(s);
        });
        tracked.set(id, (t = { s, load }));
      }
      const before = t.s.state;
      Object.assign(t.s, { cwd: pane.cwd, project: pane.project, name: pane.session!.name, state: pane.session!.state });
      // A run starting or ending is the moment to read what it did, besides what Wings reports while it works.
      if (first || before !== t.s.state) void t.load();
      changed(t.s);
    }
  });
  w.onAgentActivity(({ sessionId }) => void tracked.get(sessionId)?.load());

  return { sessions: () => [...tracked.values()].map((t) => t.s), get: (id: string) => tracked.get(id)?.s };
}
