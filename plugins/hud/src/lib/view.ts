import { clock, tokens } from "wings-plugin-sdk/format";

import { names, text, until } from "./format";
import { CONTEXT_CRITICAL, CONTEXT_WARN, LIMIT_CRITICAL, LIMIT_WARN } from "./levels";
import { servers, type Session } from "./session";
import type { AccountLimits, Limit, Overview, Snapshot } from "./types";

type Limits = WingsStatusline["rateLimits"];
type Cache = WingsStatusline["sessions"][string]["promptCache"];

export const state = {
  reported: null as WingsStatusline | null,
  /** The last limits Claude Code reported, kept in storage so the title bar shows them before this run's first reply. */
  savedLimits: [] as Limits,
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

/** Wings cuts nothing from a status, it rejects one over its limits, so row text is cut here. */
const ROW_VALUE = 200;

/**
 * The strip under a Claude session's terminal, always shown: both usage limits (percent used, then time to reset), the
 * session's context, and a failed MCP server when there is one. The card on hover has the details, a click opens the sidebar.
 */
export function paneStatus(s: Session, now = Date.now()): WingsPaneStatus {
  const ctx = context(s);
  const o = sessionLimits(s, now);
  const all = servers(s);
  const failed = all.filter((x) => x.state === "failed");
  const lastFailed = all.filter((x) => x.lastFailed);
  const on = all.filter((x) => x.state === "connected");
  const signIn = all.filter((x) => x.state === "sign-in").length;
  const skills = [...s.skills];
  const cache = ctx && cacheText(ctx.cache, now);
  // Green while there's room, then amber and red, so the line reads at a glance.
  const contextTone: Tone | undefined = !ctx ? undefined : ctx.percent >= CONTEXT_CRITICAL ? "danger" : ctx.percent >= CONTEXT_WARN ? "warning" : "success";
  const limitTone = (l: Limit | null): Tone | undefined => (!l ? undefined : l.percent >= LIMIT_CRITICAL ? "danger" : l.percent >= LIMIT_WARN ? "warning" : "success");
  // `8% · 4h19m`: usage, then the time until that limit resets. A dash until Claude Code has reported the limit.
  const limitItem = (label: string, l: Limit | null): WingsStatusItem =>
    l ? { label, value: `${l.percent}%${l.left ? ` · ${l.left}` : ""}`, percent: Math.min(100, l.percent), tone: limitTone(l) } : { label, value: "–" };
  const limitRow = (label: string, l: Limit | null) => (l ? [{ label, value: `${l.percent}% used${l.resets ? `, resets in ${l.resets}` : ""}`, tone: limitTone(l) }] : []);
  const rows = [
    ...limitRow("5-hour limit", o.fiveHour),
    ...limitRow("Weekly limit", o.sevenDay),
    ...(o.account ? [{ label: "Account", value: o.account }] : []),
    ...(!overview(now).statusline ? [{ label: "Usage limits", value: "After Claude Code's next reply" }] : []),
    { label: "Context", value: ctx ? `${tokens(ctx.tokens)} of ${tokens(ctx.window)} tokens (${ctx.percent}%)` : "No reply yet", tone: contextTone },
    ...(cache ? [{ label: "Prompt cache", value: cache }] : []),
    { label: "MCP servers", value: [`${on.length} connected`, failed.length && `${failed.length} failed`, signIn && `${signIn} need sign-in`].filter(Boolean).join(", ") },
    ...failed.slice(0, 3).map((x) => ({ label: "Failed", value: x.error ? `${x.name}: ${x.error}` : x.name, tone: "danger" as const })),
    ...(lastFailed.length ? [{ label: "Last call failed", value: names(lastFailed.map((x) => x.name), 3), tone: "warning" as const }] : []),
    ...(on.some((x) => x.used) ? [{ label: "MCP used", value: names(on.filter((x) => x.used).map((x) => x.name)) }] : []),
    { label: `Skills (${skills.length})`, value: skills.length ? names(skills) : "None used yet" },
  ];
  return {
    items: [
      limitItem("5h", o.fiveHour),
      limitItem("wk", o.sevenDay),
      ctx ? { label: "ctx", value: `${ctx.percent}%`, percent: Math.min(100, ctx.percent), tone: contextTone } : { label: "ctx", value: "–" },
      ...(failed.length ? [{ label: "MCP", value: `${failed.length} failed`, tone: "danger" as const }] : []),
    ],
    rows: rows.slice(0, 12).map((r) => ({ ...r, value: text(r.value, ROW_VALUE) })),
    open: "hud",
  };
}

/** Each account's limits, newest first: the last report, else the ones saved from an earlier run. */
function allLimits(): Limits {
  const reported = state.reported?.rateLimits ?? [];
  return reported.length ? reported : state.savedLimits;
}

function accountLimits(l: Limits[number], now: number): AccountLimits {
  return { account: l.account, fiveHour: limit(l.fiveHour, now), sevenDay: limit(l.sevenDay, now) };
}

/**
 * The limits of the account a session runs as, named when there is more than one. Before Claude Code has reported the
 * session, the newest account's.
 */
function sessionLimits(s: Session, now: number): AccountLimits {
  const all = allLimits();
  const account = state.reported?.sessions?.[s.id]?.account;
  const l = (account !== undefined && all.find((x) => x.account === account)) || all[0];
  if (!l) return { account: null, fiveHour: null, sevenDay: null };
  return { ...accountLimits(l, now), account: all.length > 1 ? (l.account ?? "default") : null };
}

/** The account-wide part: usage limits and memory, for the title bar and the sidebar. */
export function overview(now = Date.now()): Overview {
  const { reported, memory } = state;
  const limits = allLimits();
  return {
    limits: limits.map((l) => accountLimits(l, now)),
    // Without the status line Wings can't get the limits, so the sidebar explains how to turn it on.
    statusline: reported !== null && (limits.length > 0 || Object.keys(reported.sessions).length > 0),
    memory: memory && { used: memory.used, total: memory.total, percent: Math.round((memory.used / memory.total) * 100) },
  };
}

/**
 * The session the sidebar shows: the one in the selected pane, which it follows from pane to pane. `followed` is the
 * last such session, kept while the selected pane runs no Claude. A session picked from the list shows in its place
 * until another pane is selected: `picked.over` is what was followed when it was picked.
 */
export function shown<S extends { id: string }>(sessions: S[], followed: string | null, picked: { id: string; over: string | null } | null): S | undefined {
  const choice = picked && picked.over === followed ? picked.id : followed;
  return sessions.find((s) => s.id === choice) ?? sessions[0];
}

/**
 * The accounts' limits with the shown session's account first, and whether that account has any: before a session's
 * account has reported its limits, another account's must not pass for them.
 */
export function limitsFor(limits: AccountLimits[], account: string | null | undefined): { limits: AccountLimits[]; missing: boolean } {
  if (account === undefined) return { limits, missing: false };
  const own = limits.filter((l) => l.account === account);
  return { limits: [...own, ...limits.filter((l) => l.account !== account)], missing: own.length === 0 && limits.length > 0 };
}

/** What the sidebar shows for the sessions that run in panes, kept well under the 64 KB a broadcast may carry. */
export function snapshot(sessions: Session[]): Snapshot {
  const shown = [...sessions].sort((a, b) => a.project.localeCompare(b.project));
  return {
    overview: overview(),
    sessions: shown.slice(0, 8).map((s) => {
      const ctx = context(s);
      return {
        id: s.id,
        project: s.project,
        name: s.name,
        focused: s.focused,
        account: state.reported?.sessions?.[s.id]?.account,
        context: ctx && { percent: ctx.percent, tokens: ctx.tokens, window: ctx.window, cache: cacheText(ctx.cache) },
        servers: servers(s).slice(0, 80),
        skills: [...s.skills].slice(0, 60),
      };
    }),
  };
}
