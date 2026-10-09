// @ts-check
/// <reference path="../../templates/wings-plugin/wings.d.ts" />

// What a terminal status line like claude-hud shows, in Wings instead: your usage limits and memory next to the
// HUD title bar button, and each Claude session's context, MCP servers and skills on its pane.
//
// The usage limits, and the context and cache Claude Code itself reports, come from `wings.statusline()`, once
// `wings statusline` is Claude Code's status line. Without it the context is worked out from the transcript the
// same way Claude Code does. MCP servers come from the tool lists Claude Code records as it connects to them.

/** Claude's replies hold the skills and MCP tools it used. */
const REPLIES = 500;
/** Tool results are most of a transcript, and only the newest show whether an MCP server's calls fail. */
const RESULTS = 100;
/** Context and limit colours, as claude-hud has them. */
const CONTEXT_WARN = 70;
const CONTEXT_CRITICAL = 85;
/** Usage percentages where the title bar pill turns amber then red and Wings notifies once, as the sidebar gauges do. */
const LIMIT_WARN = 75;
const LIMIT_CRITICAL = 90;
const MINUTE = 60_000;
/** Tools a claude.ai connector offers until you sign in to it. */
const SIGN_IN_TOOLS = new Set(["authenticate", "complete_authentication"]);

/** By session id. */
const sessions = new Map();
/** The sessions panes run now, by pane id. */
let panes = new Map();
/** @type {WingsStatusline | null} */
let reported = null;
/** @type {{ used: number, total: number } | null} */
let memory = null;
/** @type {number | null} */
let totalMemory = null;

function session(id) {
  let s = sessions.get(id);
  if (!s) {
    s = { id, paneId: null, project: "", name: null, startedAt: 0, loaded: false };
    resetRun(s);
    sessions.set(id, s);
  }
  return s;
}

/** A new `claude` process reconnects to every MCP server, so what it knows about them starts over. */
function resetRun(s) {
  Object.assign(s, {
    tokens: null,
    window: null,
    /** @type {Map<string, Set<string>>} connected servers and their tools */
    tools: new Map(),
    /** @type {Map<string, string>} servers that failed to connect, with the error */
    failed: new Map(),
    /** @type {Set<string>} */
    pending: new Set(),
    /** @type {Set<string>} servers whose tools Claude called */
    used: new Set(),
    /** @type {Map<string, boolean>} whether each server's last call failed */
    lastFailed: new Map(),
    /** @type {Map<string, string>} tool call id to server */
    calls: new Map(),
    /** @type {Set<string>} */
    skills: new Set(),
  });
}

/** `mcp__plugin_dev-core_chrome-devtools__click` is server `plugin_dev-core_chrome-devtools`, tool `click`. */
function mcpTool(name) {
  const m = /^mcp__(.+?)__(.+)$/.exec(String(name));
  return m ? { server: m[1], tool: m[2] } : null;
}

const text = (v, max = 120) => String(v ?? "").replace(/\s+/g, " ").trim().slice(0, max);

function ingest(s, entry) {
  if (entry.isSidechain) return;
  const at = Date.parse(entry.timestamp) || Date.now();
  // A couple of seconds of slack, since the process start time is only to the second.
  const thisRun = !s.startedAt || at >= s.startedAt - 2000;
  if (entry.type === "attachment") {
    const a = entry.attachment ?? {};
    if (a.type === "model") {
      // `claude-opus-5-5[1m]` is the 1M-token window; otherwise Claude Code's default of 200k.
      s.window = /\[1m\]$/i.test(String(a.identity?.modelId ?? "")) ? 1_000_000 : 200_000;
    } else if (a.type === "deferred_tools_delta" && thisRun) {
      for (const name of a.addedNames ?? []) {
        const t = mcpTool(name);
        if (t) s.tools.set(t.server, (s.tools.get(t.server) ?? new Set()).add(t.tool));
      }
      for (const name of a.removedNames ?? []) {
        const t = mcpTool(name);
        if (!t) continue;
        s.tools.get(t.server)?.delete(t.tool);
        if (s.tools.get(t.server)?.size === 0) s.tools.delete(t.server);
      }
      // These two list every server in that state now, not what changed.
      if (Array.isArray(a.failedMcpServers)) s.failed = new Map(a.failedMcpServers.map((f) => [text(f?.name, 64), text(f?.errorCode || f?.error, 80)]));
      if (Array.isArray(a.pendingMcpServers)) s.pending = new Set(a.pendingMcpServers.map((n) => text(n, 64)));
    } else if (a.type === "invoked_skills") {
      for (const skill of a.skills ?? []) if (skill?.name) s.skills.add(text(skill.name, 64));
    }
    return;
  }
  const message = entry.message ?? {};
  const blocks = Array.isArray(message.content) ? message.content : [];
  if (entry.type === "assistant") {
    const u = message.usage;
    // What Claude Code counts as context: everything sent in, cached or not.
    if (u) s.tokens = (u.input_tokens ?? 0) + (u.cache_creation_input_tokens ?? 0) + (u.cache_read_input_tokens ?? 0);
    for (const block of blocks) {
      if (block?.type !== "tool_use") continue;
      if (block.name === "Skill" && block.input?.skill) s.skills.add(text(block.input.skill, 64));
      const t = mcpTool(block.name);
      if (t) {
        s.used.add(t.server);
        s.calls.set(block.id, t.server);
      }
    }
  } else if (entry.type === "user") {
    for (const block of blocks) {
      const server = block?.type === "tool_result" ? s.calls.get(block.tool_use_id) : undefined;
      if (!server) continue;
      s.lastFailed.set(server, block.is_error === true);
      s.calls.delete(block.tool_use_id);
    }
  }
}

/** Claude Code's own numbers once it has reported this run, else the same sum from the transcript. */
function context(s) {
  const live = reported?.sessions?.[s.id];
  if (live && live.at >= s.startedAt && live.usedPercentage != null && live.contextWindowSize) {
    const u = live.currentUsage;
    const tokens = u ? u.inputTokens + u.cacheCreationInputTokens + u.cacheReadInputTokens : Math.round((live.usedPercentage / 100) * live.contextWindowSize);
    return { percent: Math.round(live.usedPercentage), tokens, window: live.contextWindowSize, cache: live.promptCache };
  }
  if (s.tokens == null) return null;
  const window = s.window ?? 200_000;
  return { percent: Math.min(100, Math.round((s.tokens / window) * 100)), tokens: s.tokens, window, cache: null };
}

/** Every server this run knows about, failed ones first. */
function servers(s) {
  const list = [];
  for (const [name, tools] of s.tools) {
    const signIn = [...tools].every((t) => SIGN_IN_TOOLS.has(t));
    list.push({ name, state: signIn ? "sign-in" : "connected", tools: tools.size });
  }
  for (const [name, error] of s.failed) if (!s.tools.has(name)) list.push({ name, state: "failed", error });
  for (const name of s.pending) if (!s.tools.has(name) && !s.failed.has(name)) list.push({ name, state: "connecting" });
  // Without tool search Claude Code records no tool lists, so a server Claude used is the only sign of it.
  for (const name of s.used) if (!list.some((x) => x.name === name)) list.push({ name, state: "connected", tools: null });
  const order = { failed: 0, connecting: 1, connected: 2, "sign-in": 3 };
  return list
    .map((x) => ({ ...x, used: s.used.has(x.name), lastFailed: s.lastFailed.get(x.name) === true }))
    .sort((a, b) => order[a.state] - order[b.state] || Number(b.used) - Number(a.used) || a.name.localeCompare(b.name));
}

const tokens = (n) => (n >= 1e6 ? `${+(n / 1e6).toFixed(1)}M` : n >= 1e3 ? `${Math.round(n / 1e3)}k` : String(n));
const clock = (ms) => new Date(ms).toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
/** `6.6 GB`, `16 GB`, rounded as claude-hud does. */
const gb = (bytes) => `${(bytes / 2 ** 30).toFixed(bytes >= 10 * 2 ** 30 ? 0 : 1)} GB`;
const names = (all, max = 4) => (all.length > max ? `${all.slice(0, max).join(", ")}, +${all.length - max} more` : all.join(", "));

/** `2h 54m`, `2d 11h`, `5m`. */
function until(epochSeconds, now = Date.now()) {
  let minutes = Math.max(0, Math.round((epochSeconds * 1000 - now) / MINUTE));
  const days = Math.floor(minutes / 1440);
  const hours = Math.floor((minutes % 1440) / 60);
  minutes %= 60;
  return days ? `${days}d ${hours}h` : hours ? `${hours}h ${minutes}m` : `${minutes}m`;
}

/** A limit window, unless it has already reset: Claude Code reports the new one with the next reply. */
function limit(w, now = Date.now()) {
  if (!w || (w.resetsAt && w.resetsAt * 1000 <= now)) return null;
  return { percent: Math.round(w.usedPercentage), resets: w.resetsAt ? until(w.resetsAt, now) : null, resetsAt: w.resetsAt };
}

function cacheText(cache, now = Date.now()) {
  if (!cache) return null;
  return cache.warm && cache.expiresAt && cache.expiresAt * 1000 > now ? `Warm until ${clock(cache.expiresAt * 1000)}` : "Expired";
}

/** @returns {WingsBadge} */
function badge(s) {
  const ctx = context(s);
  const all = servers(s);
  const failed = all.filter((x) => x.state === "failed");
  const lastFailed = all.filter((x) => x.lastFailed);
  const on = all.filter((x) => x.state === "connected");
  const label = [ctx ? `Context ${ctx.percent}%` : "Context", failed.length ? `${failed.length} MCP failed` : null].filter(Boolean).join(" · ");
  /** @type {Tone | undefined} */
  const contextTone = !ctx ? undefined : ctx.percent >= CONTEXT_CRITICAL ? "danger" : ctx.percent >= CONTEXT_WARN ? "warning" : undefined;
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
      ...failed.slice(0, 3).map((x) => ({ label: "Failed", value: x.error ? `${x.name}: ${x.error}` : x.name, tone: /** @type {const} */ ("danger") })),
      ...(lastFailed.length ? [{ label: "Last call failed", value: names(lastFailed.map((x) => x.name), 3), tone: /** @type {const} */ ("warning") }] : []),
      ...(on.some((x) => x.used) ? [{ label: "MCP used", value: names(on.filter((x) => x.used).map((x) => x.name)) }] : []),
      { label: `Skills (${skills.length})`, value: skills.length ? names(skills) : "None used yet" },
    ],
  };
}

/** The account-wide part: usage limits and memory, for the title bar and the sidebar. */
function overview(now = Date.now()) {
  const limits = reported?.rateLimits;
  return {
    fiveHour: limit(limits?.fiveHour, now),
    sevenDay: limit(limits?.sevenDay, now),
    // Without the status line Wings can't get the limits, so the sidebar explains how to turn it on.
    statusline: reported !== null && (limits != null || Object.keys(reported.sessions).length > 0),
    memory: memory && { used: memory.used, total: memory.total, percent: Math.round((memory.used / memory.total) * 100) },
  };
}

function titleLabel() {
  const o = overview();
  const parts = [o.fiveHour && `5h ${o.fiveHour.percent}%`, o.sevenDay && `wk ${o.sevenDay.percent}%`].filter(Boolean);
  const rows = [
    ...(o.fiveHour ? [{ label: "5-hour limit", value: `${o.fiveHour.percent}% used${o.fiveHour.resets ? `, resets in ${o.fiveHour.resets}` : ""}` }] : []),
    ...(o.sevenDay ? [{ label: "Weekly limit", value: `${o.sevenDay.percent}% used${o.sevenDay.resets ? `, resets in ${o.sevenDay.resets}` : ""}` }] : []),
    ...(!o.statusline ? [{ label: "Usage limits", value: "After Claude Code's next reply" }] : []),
    ...(o.memory ? [{ label: "Memory", value: `${gb(o.memory.used)} of ${gb(o.memory.total)} (${o.memory.percent}%)` }] : []),
  ];
  const label = parts.length ? parts.join(" · ") : o.memory ? `RAM ${o.memory.percent}%` : null;
  const highest = Math.max(o.fiveHour?.percent ?? 0, o.sevenDay?.percent ?? 0);
  /** @type {"warning" | "danger" | undefined} */
  const tone = highest >= LIMIT_CRITICAL ? "danger" : highest >= LIMIT_WARN ? "warning" : undefined;
  return { label, rows, tone };
}

/** What the sidebar shows, kept well under the 64 KB a broadcast may carry. */
function snapshot() {
  const shown = [...sessions.values()].filter((s) => s.paneId).sort((a, b) => a.project.localeCompare(b.project));
  return {
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

// Changes come in bursts while Claude works, so the badges and sidebar update at most every 300 ms.
let pending = new Set();
let timer = null;
function changed(s) {
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
  void wings.broadcast({ type: "hud", ...snapshot() }).catch(() => {});
}
/** Every badge, for numbers that changed for all of them, like the status line's. */
function changedAll() {
  for (const s of panes.values()) pending.add(s.id);
  changed(null);
}

/** The highest level each limit window has alerted at, so a refresh doesn't repeat it. A new resetsAt is a new window. */
const alerted = { fiveHour: { resetsAt: null, level: 0 }, sevenDay: { resetsAt: null, level: 0 } };

function alertLimits() {
  const o = overview();
  for (const [key, name] of /** @type {const} */ ([["fiveHour", "5-hour"], ["sevenDay", "weekly"]])) {
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
    reported = await wings.statusline();
  } catch (error) {
    reported = null;
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
    memory = totalMemory && page && active && wired ? { used: Math.min(totalMemory, (active + wired) * page), total: totalMemory } : null;
  } catch (error) {
    // Not macOS, or vm_stat is missing: the HUD just leaves memory out.
    memory = null;
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
  const current = new Map();
  for (const [paneId, pane] of next) {
    const s = session(pane.session.sessionId);
    const isNew = !s.loaded || s.startedAt !== pane.session.startedAt;
    Object.assign(s, { paneId, project: pane.project, name: pane.session.name });
    current.set(paneId, s);
    if (isNew) {
      resetRun(s);
      Object.assign(s, { loaded: true, startedAt: pane.session.startedAt });
      try {
        // These attachment kinds are about 340 KB of a 44 MB transcript and tell what this run connected to,
        // so all of them; the conversation only from its end.
        const [attachments, replies, results] = await Promise.all([
          wings.transcript(s.id, ["attachment:model", "attachment:deferred_tools_delta", "attachment:invoked_skills"]),
          wings.transcript(s.id, ["assistant"], { last: REPLIES }),
          wings.transcript(s.id, ["user"], { last: RESULTS }),
        ]);
        // Replies before results, so each tool result finds the call it answers.
        for (const entry of [...attachments, ...replies, ...results]) ingest(s, entry);
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
let statusTimer = null;
wings.onTranscript(({ sessionId, entry }) => {
  const s = sessions.get(sessionId);
  if (!s?.loaded) return;
  ingest(s, entry);
  changed(s);
  if (entry.type === "assistant") {
    clearTimeout(statusTimer);
    statusTimer = setTimeout(refreshStatus, 1500);
  }
});

// A sidebar that just opened asks for the current state.
wings.onBroadcast((message) => {
  if (/** @type {any} */ (message)?.type === "hello") void wings.broadcast({ type: "hud", ...snapshot() });
});

// Limits reset on their own and memory moves, so both are checked once a minute. Each check is a few ms.
void refreshStatus();
void refreshMemory();
setInterval(() => void Promise.all([refreshStatus(), refreshMemory()]), MINUTE);
