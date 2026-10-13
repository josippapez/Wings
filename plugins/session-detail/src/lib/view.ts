import { modelName, tokens } from "./format";
import { checklist, sessions, type Session } from "./session";
import type { SessionView } from "./types";

const tokenLine = (s: Session) => {
  const t = s.tokens;
  return `${tokens(t.input + t.output + t.cacheRead + t.cacheWrite)}${s.partial ? "+" : ""} (in ${tokens(t.input)}, out ${tokens(t.output)}, cache ${tokens(t.cacheRead + t.cacheWrite)})`;
};
const running = (s: Session) => [...s.tools.values()].filter((t) => !t.done);

/** What the sidebar shows, kept under the 64 KB a broadcast may carry. */
export function snapshot(): SessionView[] {
  const list = [...sessions.values()].filter((s) => s.paneId).sort((a, b) => b.updated - a.updated).slice(0, 6);
  const build = (n: number): SessionView[] =>
    list.map((s) => ({
      id: s.id,
      paneId: s.paneId,
      project: s.project,
      name: s.name,
      state: s.state,
      model: modelName(s.model),
      context: s.context,
      tokens: tokenLine(s),
      // The newest calls, at least 10 so a smaller snapshot always has room for them.
      running: running(s)
        .slice(-Math.max(n, 10))
        .map(({ name, summary, at }) => ({ name, summary, at })),
      items: (n ? s.items.slice(-n) : []).reverse().map(({ kind, key, name, summary, tags, outcome, at, ms, error, done }) => ({ kind, key, name, summary, tags, outcome, at, ms, error, done })),
      // Like running calls: the first max(n, 10), so a full list only shrinks in the smaller snapshots.
      todos: checklist(s)
        .slice(0, Math.max(n, 10))
        .map((t) => ({ text: t.text, active: t.active, status: t.status })),
      files: [...s.files].sort((a, b) => b[1] - a[1]).slice(0, 30),
    }));
  for (const n of [80, 40, 15, 0]) {
    const sessions = build(n);
    if (JSON.stringify(sessions).length < 60_000) return sessions;
  }
  return [];
}
