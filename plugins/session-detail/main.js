// Follows each Claude session in a pane: a badge with its model and context, and a sidebar with recent activity.
// It reads the newest transcript entries once per run of `claude`, then only what Claude appends. A resumed
// session's transcript also holds earlier runs, which only count for the model and context.

const BACKLOG = 300;
const KEEP = 150;
const EDITS = new Set(["Edit", "Write", "MultiEdit", "NotebookEdit"]);

/** By session id. */
const sessions = new Map();
/** The sessions panes run now, by pane id. */
let panes = new Map();

function session(id) {
  let s = sessions.get(id);
  if (!s) {
    s = { id, paneId: null, project: "", name: null, state: "idle", model: null, context: null, items: [], tools: new Map(), files: new Map(), lastPrompt: null, updated: 0 };
    sessions.set(id, s);
  }
  return s;
}

const basename = (path) => String(path ?? "").split("/").filter(Boolean).at(-1) ?? "";
const oneLine = (text, max = 90) => {
  const line = String(text ?? "").replace(/\s+/g, " ").trim();
  return line.length > max ? `${line.slice(0, max - 1)}…` : line;
};

/** A few words on what a tool call does, from its input. */
function summary(name, input = {}) {
  if (name === "Bash") return oneLine(input.command);
  if (input.file_path || input.notebook_path) return basename(input.file_path ?? input.notebook_path);
  if (name === "Grep") return oneLine(input.pattern);
  if (name === "Glob") return oneLine(input.pattern);
  if (name === "WebFetch") return oneLine(input.url);
  if (name === "WebSearch") return oneLine(input.query);
  if (input.description) return oneLine(input.description);
  return "";
}

/** `mcp__server__tool` reads as `server · tool`. */
const toolName = (name) => (name.startsWith("mcp__") ? name.slice(5).split("__").join(" · ") : name);

/** Claude's own prompt wrappers, interrupt markers and the like aren't things you typed. */
const typed = (text) => text && !text.startsWith("<") && !text.startsWith("[Request interrupted");

function ingest(s, entry) {
  if (entry.isSidechain) return;
  const at = Date.parse(entry.timestamp) || Date.now();
  const message = entry.message ?? {};
  // A couple of seconds of slack, since the process start time is only to the second.
  const earlierRun = s.startedAt && at < s.startedAt - 2000;
  if (entry.type === "assistant") {
    if (message.model && message.model !== "<synthetic>") s.model = message.model;
    const u = message.usage;
    if (u) s.context = (u.input_tokens ?? 0) + (u.cache_creation_input_tokens ?? 0) + (u.cache_read_input_tokens ?? 0);
    if (earlierRun) return;
    for (const block of Array.isArray(message.content) ? message.content : []) {
      if (block?.type !== "tool_use") continue;
      const item = { kind: "tool", id: block.id, name: toolName(String(block.name)), summary: summary(block.name, block.input), at, ms: null, error: false, done: false };
      s.tools.set(block.id, item);
      s.items.push(item);
      const path = EDITS.has(block.name) ? (block.input?.file_path ?? block.input?.notebook_path) : null;
      if (path) s.files.set(path, (s.files.get(path) ?? 0) + 1);
    }
  } else if (entry.type === "user" && !entry.isMeta && !earlierRun) {
    const content = message.content;
    if (typeof content === "string") {
      if (typed(content)) {
        s.lastPrompt = oneLine(content, 200);
        s.items.push({ kind: "prompt", summary: oneLine(content, 140), at });
      }
      return;
    }
    for (const block of Array.isArray(content) ? content : []) {
      if (block?.type !== "tool_result") continue;
      const item = s.tools.get(block.tool_use_id);
      if (!item) continue;
      item.done = true;
      item.error = block.is_error === true;
      item.ms = Math.max(0, at - item.at);
      s.tools.delete(block.tool_use_id);
    }
  }
  if (s.items.length > KEEP) s.items.splice(0, s.items.length - KEEP);
  s.updated = Math.max(s.updated, at);
}

const modelName = (model) => {
  const m = /^claude-([a-z]+)-(\d+)-(\d+)/.exec(model ?? "");
  return m ? `${m[1][0].toUpperCase()}${m[1].slice(1)} ${m[2]}.${m[3]}` : (model ?? "");
};
const tokens = (n) => (n == null ? "—" : n >= 1e6 ? `${(n / 1e6).toFixed(1)}M` : n >= 1e3 ? `${Math.round(n / 1e3)}k` : String(n));
const running = (s) => [...s.tools.values()].filter((t) => !t.done);

function badge(s) {
  const now = s.state === "working" ? running(s).at(-1) : null;
  const label = now ? now.name : [modelName(s.model), s.context != null ? tokens(s.context) : null].filter(Boolean).join(" · ") || "Claude";
  const status = { working: "Working", blocked: "Waiting for you", done: "Finished", idle: "Idle" }[s.state] ?? s.state;
  return {
    label,
    tone: s.state === "blocked" ? "warning" : "neutral",
    loading: Boolean(now),
    title: "Claude session",
    subtitle: s.name ?? undefined,
    rows: [
      { label: "Status", value: now ? `${now.name} ${now.summary}`.trim() : status, tone: s.state === "blocked" ? "warning" : undefined },
      { label: "Model", value: modelName(s.model) || "—" },
      { label: "Context", value: s.context != null ? `${tokens(s.context)} tokens` : "—" },
      { label: "Files edited", value: String(s.files.size) },
      ...(s.lastPrompt ? [{ label: "Last prompt", value: s.lastPrompt }] : []),
    ],
  };
}

/** What the sidebar shows, kept under the 64 KB a broadcast may carry. */
function snapshot() {
  const list = [...sessions.values()].filter((s) => s.paneId).sort((a, b) => b.updated - a.updated).slice(0, 6);
  const build = (n) =>
    list.map((s) => ({
      id: s.id,
      paneId: s.paneId,
      project: s.project,
      name: s.name,
      state: s.state,
      model: modelName(s.model),
      context: s.context,
      running: running(s).map(({ name, summary, at }) => ({ name, summary, at })),
      items: s.items.slice(-n).reverse().map(({ kind, name, summary, at, ms, error, done }) => ({ kind, name, summary, at, ms, error, done })),
      files: [...s.files].sort((a, b) => b[1] - a[1]).slice(0, 30),
    }));
  for (const n of [80, 40, 15, 0]) {
    const sessions = build(n);
    if (JSON.stringify(sessions).length < 60_000) return sessions;
  }
  return [];
}

// Changes come in bursts while Claude works, so the badge and sidebar update at most every 300 ms.
let pending = new Set();
let timer = null;
function changed(s) {
  pending.add(s.id);
  timer ??= setTimeout(flush, 300);
}
function flush() {
  timer = null;
  for (const id of pending) {
    const s = sessions.get(id);
    if (s?.paneId) void wings.setBadge(s.paneId, badge(s)).catch(() => {});
  }
  pending = new Set();
  void wings.broadcast({ type: "sessions", sessions: snapshot() });
}

wings.onPanes(async (list) => {
  const open = new Set(list.map((p) => p.paneId));
  const next = new Map(list.filter((p) => p.session).map((p) => [p.paneId, p]));
  // A pane that stopped running this session loses its badge. Wings drops the badges of closed panes itself.
  for (const [paneId, s] of panes) {
    if (next.get(paneId)?.session.sessionId !== s.id) {
      s.paneId = null;
      if (open.has(paneId)) void wings.setBadge(paneId, null).catch(() => {});
    }
  }
  const current = new Map();
  for (const [paneId, pane] of next) {
    const s = session(pane.session.sessionId);
    // A new `claude` process, even for the same session after a resume, starts the activity afresh.
    const isNew = !s.loaded || s.startedAt !== pane.session.startedAt;
    Object.assign(s, { paneId, project: pane.project, name: pane.session.name, state: pane.session.state });
    current.set(paneId, s);
    if (isNew) {
      Object.assign(s, { loaded: true, startedAt: pane.session.startedAt, items: [], tools: new Map(), files: new Map(), lastPrompt: null });
      try {
        for (const entry of await wings.transcript(s.id, ["assistant", "user"], { last: BACKLOG })) ingest(s, entry);
      } catch (error) {
        console.error(error);
      }
    }
    changed(s);
  }
  panes = current;
  if (current.size === 0) void wings.broadcast({ type: "sessions", sessions: [] });
});

wings.onTranscript(({ sessionId, entry }) => {
  const s = sessions.get(sessionId);
  if (!s?.loaded) return;
  ingest(s, entry);
  changed(s);
});

// A sidebar that just opened asks for the current state.
wings.onBroadcast((message) => {
  if (message?.type === "hello") void wings.broadcast({ type: "sessions", sessions: snapshot() });
});
