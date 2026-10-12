// What a terminal status line like claude-hud shows, in Wings instead: your usage limits and memory next to the
// HUD title bar button, and each Claude session's context, MCP servers and skills on its pane.
//
// The usage limits, and the context and cache Claude Code itself reports, come from `wings.statusline()`, once
// `wings statusline` is Claude Code's status line. Without it the context is worked out from the transcript the
// same way Claude Code does. MCP servers come from the tool lists Claude Code records as it connects to them.

import { pollWhileFocused } from "wings-plugin-sdk/poll";
import { ingest, resetRun, session, sessions, type Entry, type Session } from "./lib/session";
import { badge, LIMIT_CRITICAL, LIMIT_WARN, overview, snapshot, state, titleLabel } from "./lib/view";

/** Claude's replies hold the skills and MCP tools it used. */
const REPLIES = 500;
/** Tool results are most of a transcript, and only the newest show whether an MCP server's calls fail. */
const RESULTS = 100;
const MINUTE = 60_000;

/** The sessions panes run now, by pane id. */
let panes = new Map<string, Session>();
let totalMemory: number | null = null;

// Changes come in bursts while Claude works, so the badges and sidebar update at most every 300 ms.
let pending = new Set<string>();
let timer: ReturnType<typeof setTimeout> | null = null;
function changed(s: Session | null) {
  if (s) pending.add(s.id);
  timer ??= setTimeout(flush, 300);
}
function flush() {
  timer = null;
  for (const id of pending) {
    const s = sessions.get(id);
    if (s?.paneId) void wings.setBadge(s.paneId, badge(s)).catch(() => {});
  }
  pending = new Set();
  const { label, rows, tone } = titleLabel();
  void wings.setSidebarLabel("hud", label, { rows, tone }).catch(() => {});
  void wings.broadcast(snapshot()).catch(() => {});
}
/** Every badge, for numbers that changed for all of them, like the status line's. */
function changedAll() {
  for (const s of panes.values()) pending.add(s.id);
  changed(null);
}

/** The highest level each limit window has alerted at, so a refresh doesn't repeat it. A new resetsAt is a new window. */
const alerted = { fiveHour: { resetsAt: null as number | null, level: 0 }, sevenDay: { resetsAt: null as number | null, level: 0 } };

function alertLimits() {
  const o = overview();
  for (const [key, name] of [["fiveHour", "5-hour"], ["sevenDay", "weekly"]] as const) {
    const l = o[key];
    if (!l) continue;
    const seen = alerted[key];
    if (seen.resetsAt !== (l.resetsAt ?? null)) Object.assign(seen, { resetsAt: l.resetsAt ?? null, level: 0 });
    const level = l.percent >= LIMIT_CRITICAL ? LIMIT_CRITICAL : l.percent >= LIMIT_WARN ? LIMIT_WARN : 0;
    if (level <= seen.level) continue;
    seen.level = level;
    void wings.notify({ title: `${l.percent}% of your ${name} Claude limit used`, body: l.resets ? `Resets in ${l.resets}` : undefined }).catch(() => {});
  }
}

async function refreshStatus() {
  try {
    state.reported = await wings.statusline();
    const limits = state.reported?.rateLimits;
    if (limits && JSON.stringify(limits) !== JSON.stringify(state.savedLimits)) {
      state.savedLimits = limits;
      void wings.storage.set("limits", limits).catch(() => {});
    }
  } catch (error) {
    state.reported = null;
    console.error(error);
  }
  alertLimits();
  changedAll();
}

/** Active and wired pages, as claude-hud counts them: what macOS can't hand to another app right away. */
async function refreshMemory() {
  try {
    totalMemory ??= Number((await wings.exec("sysctl", ["-n", "hw.memsize"])).stdout.trim()) || null;
    const out = (await wings.exec("vm_stat")).stdout;
    const page = Number(/page size of (\d+) bytes/.exec(out)?.[1]);
    const active = Number(/Pages active:\s+(\d+)/.exec(out)?.[1]);
    const wired = Number(/Pages wired down:\s+(\d+)/.exec(out)?.[1]);
    state.memory = totalMemory && page && active && wired ? { used: Math.min(totalMemory, (active + wired) * page), total: totalMemory } : null;
  } catch (error) {
    // Not macOS, or vm_stat is missing: the HUD just leaves memory out.
    state.memory = null;
    console.error(error);
  }
  changed(null);
}

wings.onPanes(async (list) => {
  const open = new Set(list.map((p) => p.paneId));
  const next = new Map(list.filter((p) => p.session).map((p) => [p.paneId, p]));
  for (const [paneId, s] of panes) {
    if (next.get(paneId)?.session?.sessionId !== s.id) {
      s.paneId = null;
      if (open.has(paneId)) void wings.setBadge(paneId, null).catch(() => {});
    }
  }
  const current = new Map<string, Session>();
  for (const [paneId, pane] of next) {
    const run = pane.session!;
    const s = session(run.sessionId);
    const isNew = !s.loaded || s.startedAt !== run.startedAt;
    Object.assign(s, { paneId, project: pane.project, name: run.name });
    current.set(paneId, s);
    if (isNew) {
      resetRun(s);
      Object.assign(s, { loaded: true, startedAt: run.startedAt });
      try {
        // These attachment kinds are about 340 KB of a 44 MB transcript and tell what this run connected to,
        // so all of them; the conversation only from its end.
        const [attachments, replies, results] = await Promise.all([
          wings.transcript(s.id, ["attachment:model", "attachment:deferred_tools_delta", "attachment:invoked_skills"]),
          wings.transcript(s.id, ["assistant"], { last: REPLIES }),
          wings.transcript(s.id, ["user"], { last: RESULTS }),
        ]);
        // Replies before results, so each tool result finds the call it answers.
        for (const entry of [...attachments, ...replies, ...results]) ingest(s, entry as unknown as Entry);
      } catch (error) {
        console.error(error);
      }
    }
    changed(s);
  }
  panes = current;
  changed(null);
});

// The status line runs right after each reply, so look for its numbers just after the reply lands.
let statusTimer: ReturnType<typeof setTimeout> | undefined;
wings.onTranscript(({ sessionId, entry }) => {
  const s = sessions.get(sessionId);
  if (!s?.loaded) return;
  ingest(s, entry as Entry);
  changed(s);
  if (entry.type === "assistant") {
    clearTimeout(statusTimer);
    statusTimer = setTimeout(refreshStatus, 1500);
  }
});

// A sidebar that just opened asks for the current state.
wings.onBroadcast((message) => {
  if ((message as { type?: string } | null)?.type === "hello") void wings.broadcast(snapshot());
});

// Limits reset on their own and memory moves, so both are checked once a minute. Each check is a few ms.
void wings.storage
  .get("limits")
  .then(
    (limits) => {
      state.savedLimits ??= limits as typeof state.savedLimits;
      changedAll();
    },
    () => {},
  )
  .finally(() => void refreshStatus());
void refreshMemory();
// The status check also raises limit alerts, which matter most when you're in another app, so it keeps running.
// Memory is only drawn for someone looking, and costs a `vm_stat` run, so it pauses while Wings is in the background.
setInterval(() => void refreshStatus(), MINUTE);
pollWhileFocused(() => void refreshMemory(), MINUTE);
