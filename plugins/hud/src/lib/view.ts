import { clock, gb, names, tokens, until } from "./format";
import { servers, sessions, type Session } from "./session";
import type { Limit, Overview, Snapshot } from "./types";

/** Context and limit colours, as claude-hud has them. */
const CONTEXT_WARN = 70;
const CONTEXT_CRITICAL = 85;
/** Usage percentages where the title bar pill turns amber then red and Wings notifies once, as the sidebar gauges do. */
export const LIMIT_WARN = 75;
export const LIMIT_CRITICAL = 90;

type Limits = NonNullable<WingsStatusline["rateLimits"]>;
type Cache = WingsStatusline["sessions"][string]["promptCache"];

export const state = {
  reported: null as WingsStatusline | null,
  /** The last limits Claude Code reported, kept in storage so the title bar shows them before this run's first reply. */
  savedLimits: null as Limits | null,
  memory: null as { used: number; total: number } | null,
};

/** Claude Code's own numbers once it has reported this run, else the same sum from the transcript. */
function context(s: Session): { percent: number; tokens: number; window: number; cache: Cache } | null {
  const live = state.reported?.sessions?.[s.id];
  if (live && live.at >= s.startedAt && live.usedPercentage != null && live.contextWindowSize) {
    const u = live.currentUsage;
    const tokens = u ? u.inputTokens + u.cacheCreationInputTokens + u.cacheReadInputTokens : Math.round((live.usedPercentage / 100) * live.contextWindowSize);
    return { percent: Math.round(live.usedPercentage), tokens, window: live.contextWindowSize, cache: live.promptCache };
  }
  if (s.tokens == null) return null;
  const window = s.window ?? 200_000;
  return { percent: Math.min(100, Math.round((s.tokens / window) * 100)), tokens: s.tokens, window, cache: null };
}

/** A limit window, unless it has already reset: Claude Code reports the new one with the next reply. */
function limit(w: WingsLimitWindow | null | undefined, now = Date.now()): Limit | null {
  if (!w) return null;
  // Past its reset the window is empty until the next message starts a new one.
  if (w.resetsAt && w.resetsAt * 1000 <= now) return { percent: 0, resets: null, left: null, resetsAt: null };
  return { percent: Math.round(w.usedPercentage), resets: w.resetsAt ? until(w.resetsAt, now) : null, left: w.resetsAt ? until(w.resetsAt, now, true) : null, resetsAt: w.resetsAt };
}

function cacheText(cache: Cache, now = Date.now()) {
  if (!cache) return null;
  return cache.warm && cache.expiresAt && cache.expiresAt * 1000 > now ? `Warm until ${clock(cache.expiresAt * 1000)}` : "Expired";
}

export function badge(s: Session): WingsBadge {
  const ctx = context(s);
  const all = servers(s);
  const failed = all.filter((x) => x.state === "failed");
  const lastFailed = all.filter((x) => x.lastFailed);
  const on = all.filter((x) => x.state === "connected");
  const label = [ctx ? `Context ${ctx.percent}%` : "Context", failed.length ? `${failed.length} MCP failed` : null].filter(Boolean).join(" · ");
  const contextTone: Tone | undefined = !ctx ? undefined : ctx.percent >= CONTEXT_CRITICAL ? "danger" : ctx.percent >= CONTEXT_WARN ? "warning" : undefined;
  const signIn = all.filter((x) => x.state === "sign-in").length;
  const skills = [...s.skills];
  const cache = ctx && cacheText(ctx.cache);
  return {
    label,
    tone: contextTone ?? (failed.length ? "warning" : "neutral"),
    title: "Context and tools",
    subtitle: s.name ?? undefined,
    rows: [
      { label: "Context", value: ctx ? `${tokens(ctx.tokens)} of ${tokens(ctx.window)} tokens (${ctx.percent}%)` : "No reply yet", tone: contextTone },
      ...(cache ? [{ label: "Prompt cache", value: cache }] : []),
      { label: "MCP servers", value: [`${on.length} connected`, failed.length && `${failed.length} failed`, signIn && `${signIn} need sign-in`].filter(Boolean).join(", ") },
      ...failed.slice(0, 3).map((x) => ({ label: "Failed", value: x.error ? `${x.name}: ${x.error}` : x.name, tone: "danger" as const })),
      ...(lastFailed.length ? [{ label: "Last call failed", value: names(lastFailed.map((x) => x.name), 3), tone: "warning" as const }] : []),
      ...(on.some((x) => x.used) ? [{ label: "MCP used", value: names(on.filter((x) => x.used).map((x) => x.name)) }] : []),
      { label: `Skills (${skills.length})`, value: skills.length ? names(skills) : "None used yet" },
    ],
  };
}

/** The account-wide part: usage limits and memory, for the title bar and the sidebar. */
export function overview(now = Date.now()): Overview {
  const { reported, memory } = state;
  const limits = reported?.rateLimits ?? state.savedLimits;
  return {
    fiveHour: limit(limits?.fiveHour, now),
    sevenDay: limit(limits?.sevenDay, now),
    // Without the status line Wings can't get the limits, so the sidebar explains how to turn it on.
    statusline: reported !== null && (limits != null || Object.keys(reported.sessions).length > 0),
    memory: memory && { used: memory.used, total: memory.total, percent: Math.round((memory.used / memory.total) * 100) },
  };
}

export function titleLabel() {
  const o = overview();
  // `5h 3% (2h14m) · wk 83% (3d5h)`: usage, then the time until that limit resets. The once-a-minute refresh keeps it current.
  // Always both, so the state shows at a glance: "–" until Claude Code has reported a limit.
  const pill = (name: string, l: Limit | null) => (l ? `${name} ${l.percent}%${l.left ? ` (${l.left})` : ""}` : `${name} –`);
  const parts = [pill("5h", o.fiveHour), pill("wk", o.sevenDay)];
  const rows = [
    ...(o.fiveHour ? [{ label: "5-hour limit", value: `${o.fiveHour.percent}% used${o.fiveHour.resets ? `, resets in ${o.fiveHour.resets}` : ""}` }] : []),
    ...(o.sevenDay ? [{ label: "Weekly limit", value: `${o.sevenDay.percent}% used${o.sevenDay.resets ? `, resets in ${o.sevenDay.resets}` : ""}` }] : []),
    ...(!o.statusline ? [{ label: "Usage limits", value: "After Claude Code's next reply" }] : []),
    ...(o.memory ? [{ label: "Memory", value: `${gb(o.memory.used)} of ${gb(o.memory.total)} (${o.memory.percent}%)` }] : []),
  ];
  const label = parts.join(" · ");
  const highest = Math.max(o.fiveHour?.percent ?? 0, o.sevenDay?.percent ?? 0);
  const tone = highest >= LIMIT_CRITICAL ? ("danger" as const) : highest >= LIMIT_WARN ? ("warning" as const) : undefined;
  return { label, rows, tone };
}

/** What the sidebar shows, kept well under the 64 KB a broadcast may carry. */
export function snapshot(): Snapshot {
  const shown = [...sessions.values()].filter((s) => s.paneId).sort((a, b) => a.project.localeCompare(b.project));
  return {
    type: "hud",
    overview: overview(),
    sessions: shown.slice(0, 8).map((s) => {
      const ctx = context(s);
      return {
        id: s.id,
        project: s.project,
        name: s.name,
        context: ctx && { percent: ctx.percent, tokens: ctx.tokens, window: ctx.window, cache: cacheText(ctx.cache) },
        servers: servers(s).slice(0, 80),
        skills: [...s.skills].slice(0, 60),
      };
    }),
  };
}
