// @ts-check
/// <reference path="../../templates/wings-plugin/wings.d.ts" />

// The HUD sidebar: usage limits and memory, then one session's context, MCP servers and skills. The plugin's
// main script keeps the state and broadcasts it here.

const app = /** @type {HTMLElement} */ (document.getElementById("app"));
/** @type {any} */
let hud = null;
/** The session you picked; otherwise the first. */
let picked = /** @type {string | null} */ (null);

/** Strings become text, never HTML. @param {string} tag @param {Record<string, any>} [props] @param {...(Node | string | null | false | undefined)} children */
function h(tag, props = {}, ...children) {
  const el = document.createElement(tag);
  for (const [key, value] of Object.entries(props)) {
    if (key.startsWith("on")) el.addEventListener(key.slice(2), value);
    else if (key === "class") el.className = value;
    else if (value === true) el.setAttribute(key, "");
    else if (value !== false && value != null) el.setAttribute(key, String(value));
  }
  for (const child of children) if (child) el.append(child);
  return el;
}

const tokens = (/** @type {number} */ n) => (n >= 1e6 ? `${+(n / 1e6).toFixed(1)}M` : n >= 1e3 ? `${Math.round(n / 1e3)}k` : String(n));
const gb = (/** @type {number} */ bytes) => `${(bytes / 2 ** 30).toFixed(bytes >= 10 * 2 ** 30 ? 0 : 1)} GB`;

/**
 * A labelled bar. `warn` and `danger` are where it changes colour.
 * @param {string} label @param {number} percent @param {string | null} note @param {[number, number]} levels
 */
function gauge(label, percent, note, levels) {
  const level = percent >= levels[1] ? " danger" : percent >= levels[0] ? " warn" : "";
  return h(
    "div",
    { class: "gauge" },
    h("span", {}, label),
    h("span", { class: "value" }, `${percent}%`),
    h("div", { class: `bar${level}`, role: "meter", "aria-label": label, "aria-valuemin": 0, "aria-valuemax": 100, "aria-valuenow": percent }, h("span", { style: `width: ${Math.min(100, percent)}%` })),
    note ? h("span", { class: "note" }, note) : null,
  );
}

const states = { connected: "Connected", failed: "Failed", connecting: "Connecting", "sign-in": "Needs sign-in" };

/** @param {any} server */
function serverRow(server) {
  const tags = [];
  if (server.state === "failed") tags.push(h("span", { class: "tag danger", title: server.error || undefined }, server.error || "Failed"));
  else if (server.state === "connecting") tags.push(h("span", { class: "tag" }, "Connecting"));
  if (server.lastFailed) tags.push(h("span", { class: "tag warn" }, "Last call failed"));
  else if (server.used) tags.push(h("span", { class: "tag" }, "Used"));
  return h(
    "li",
    { title: `${server.name}: ${states[/** @type {keyof typeof states} */ (server.state)]}${server.tools ? `, ${server.tools} ${server.tools === 1 ? "tool" : "tools"}` : ""}` },
    h("span", { class: `dot ${server.state}`, "aria-hidden": true }),
    h("span", { class: "name" }, server.name),
    ...tags,
  );
}

function render() {
  if (hud === null) return app.replaceChildren(h("p", { class: "muted" }, "Loading…"));
  const { overview: o, sessions } = hud;
  const parts = [h("h2", {}, "Usage")];
  const usage = [];
  if (o.fiveHour) usage.push(gauge("5-hour limit", o.fiveHour.percent, o.fiveHour.resets && `Resets in ${o.fiveHour.resets}`, [75, 90]));
  if (o.sevenDay) usage.push(gauge("Weekly limit", o.sevenDay.percent, o.sevenDay.resets && `Resets in ${o.sevenDay.resets}`, [75, 90]));
  if (!o.statusline) {
    usage.push(
      h(
        "p",
        { class: "hint muted" },
        "Your 5-hour and weekly limits show after Claude Code's next reply, on Pro and Max plans. If they don't, connect Claude Code in Plugins.",
      ),
    );
  }
  if (o.memory) usage.push(gauge("Memory", o.memory.percent, `${gb(o.memory.used)} of ${gb(o.memory.total)} in use`, [75, 90]));
  parts.push(h("div", { class: "card" }, ...usage));

  if (sessions.length === 0) {
    parts.push(h("div", { class: "empty" }, h("p", {}, "No Claude session in your panes."), h("p", { class: "muted" }, "Run claude in a pane to see its context, MCP servers and skills.")));
    return app.replaceChildren(...parts);
  }
  const s = sessions.find((/** @type {any} */ x) => x.id === picked) ?? sessions[0];
  if (sessions.length > 1) {
    parts.push(
      h(
        "select",
        { "aria-label": "Session", onchange: (/** @type {Event} */ e) => ((picked = /** @type {HTMLSelectElement} */ (e.target).value), render()) },
        ...sessions.map((/** @type {any} */ x) => h("option", { value: x.id, selected: x.id === s.id }, `${x.project} · ${x.name ?? x.id.slice(0, 8)}`)),
      ),
    );
  }

  parts.push(h("h2", {}, "Context"));
  parts.push(
    h(
      "div",
      { class: "card" },
      s.context
        ? gauge("Context window", s.context.percent, [`${tokens(s.context.tokens)} of ${tokens(s.context.window)} tokens`, s.context.cache && `prompt cache ${s.context.cache.toLowerCase()}`].filter(Boolean).join(" · "), [70, 85])
        : h("p", { class: "muted" }, "Claude hasn't replied yet."),
    ),
  );

  const servers = s.servers.filter((/** @type {any} */ x) => x.state !== "sign-in");
  const signIn = s.servers.filter((/** @type {any} */ x) => x.state === "sign-in");
  parts.push(h("h2", {}, `MCP servers (${servers.length})`));
  parts.push(servers.length ? h("ul", { class: "list" }, ...servers.map(serverRow)) : h("p", { class: "muted" }, "None connected yet."));
  if (signIn.length) {
    parts.push(h("details", {}, h("summary", {}, `${signIn.length} need sign-in`), h("ul", { class: "list" }, ...signIn.map(serverRow))));
  }

  parts.push(h("h2", {}, `Skills (${s.skills.length})`));
  parts.push(
    s.skills.length
      ? h("ul", { class: "list" }, ...s.skills.map((/** @type {string} */ name) => h("li", {}, h("span", { class: "name", title: name }, name))))
      : h("p", { class: "muted" }, "None used yet."),
  );
  app.replaceChildren(...parts);
}

wings.onBroadcast((message) => {
  const m = /** @type {any} */ (message);
  if (m?.type !== "hud") return;
  hud = m;
  render();
});

render();
void wings.broadcast({ type: "hello" });
