// What a terminal status line like claude-hud shows, in Wings instead: a strip under each Claude session's terminal with
// your 5-hour and weekly limits, its context and any failed MCP server, and the HUD sidebar with the rest.
//
// The usage limits, and the context and cache Claude Code itself reports, come from `wings.statusline()`, once
// `wings statusline` is Claude Code's status line. Without it the context is worked out from the transcript the
// same way Claude Code does. MCP servers come from the tool lists Claude Code records as it connects to them.

import { pollWhileFocused } from "wings-plugin-sdk/poll";
import { followSessions } from "wings-plugin-sdk/sessions";
import { MINUTE } from "./lib/format";
import { LIMIT_CRITICAL, LIMIT_WARN } from "./lib/levels";
import { ch } from "./lib/messages";
import { ingest, session, startRun } from "./lib/session";
import { overview, paneStatus, snapshot, state } from "./lib/view";

/** Claude's replies hold the skills and MCP tools it used. */
const REPLIES = 500;
/** Tool results are most of a transcript, and only the newest show whether an MCP server's calls fail. */
const RESULTS = 100;

let totalMemory: number | null = null;

ch.serve("hud", () => snapshot(follow.sessions()), { throttleMs: 300 });

// Changes come in bursts while Claude works, so the strips are drawn at most every 300 ms. Wings skips a strip that is
// unchanged, so each pass can go through every session. A run whose backlog isn't in has nothing to show: its strip
// stays as it was, and `changed` runs again when the backlog lands.
let strips: ReturnType<typeof setTimeout> | undefined;
function changed() {
  ch.refresh("hud");
  strips ??= setTimeout(drawStrips, 300);
}
function drawStrips() {
  strips = undefined;
  for (const s of follow.sessions()) if (follow.loaded(s.id)) void wings.setPaneStatus(s.paneId, paneStatus(s)).catch(console.error);
}

// The status line runs right after each reply, so look for its numbers just after the reply lands.
let statusTimer: ReturnType<typeof setTimeout> | undefined;

const follow = followSessions({
  create: session,
  pane: (s, pane) => void Object.assign(s, { paneId: pane.paneId, project: pane.project, name: pane.session.name }),
  startRun: (s, run) => startRun(s, run.startedAt),
  async backlog(s) {
    // These attachment kinds are about 340 KB of a 44 MB transcript and tell what this run connected to,
    // so all of them; the conversation only from its end.
    const [attachments, replies, results] = await Promise.all([
      wings.transcript(s.id, ["attachment:model", "attachment:deferred_tools_delta", "attachment:invoked_skills"]),
      wings.transcript(s.id, ["assistant"], { last: REPLIES }),
      wings.transcript(s.id, ["user"], { last: RESULTS }),
    ]);
    // Replies before results, so each tool result finds the call it answers.
    return [...attachments, ...replies, ...results];
  },
  entry(s, entry, { live, thisRun }) {
    ingest(s, entry, thisRun);
    if (live && entry.type === "assistant") {
      clearTimeout(statusTimer);
      statusTimer = setTimeout(refreshStatus, 1500);
    }
  },
  gone(_s, paneId, stillOpen) {
    // A closed pane's strip goes with it; one that runs something else now is cleared here.
    if (stillOpen) void wings.setPaneStatus(paneId, null).catch(() => {});
  },
  changed,
});

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
  changed();
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
  ch.refresh("hud");
}

// Limits reset on their own and memory moves, so both are checked once a minute. Each check is a few ms.
void wings.storage
  .get("limits")
  .then(
    (limits) => {
      state.savedLimits ??= limits as typeof state.savedLimits;
      changed();
    },
    () => {},
  )
  .finally(() => void refreshStatus());
void refreshMemory();
// The status check also raises limit alerts, which matter most when you're in another app, so it keeps running.
// Memory is only drawn for someone looking, and costs a `vm_stat` run, so it pauses while Wings is in the background.
setInterval(() => void refreshStatus(), MINUTE);
pollWhileFocused(() => void refreshMemory(), MINUTE);
