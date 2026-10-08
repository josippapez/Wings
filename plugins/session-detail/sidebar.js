// @ts-check
/// <reference path="../../templates/wings-plugin/wings.d.ts" />

// The Session sidebar: what one Claude session is doing now, the files it edited and its recent tool calls.
// The plugin's main script keeps the state and broadcasts it here.

const app = /** @type {HTMLElement} */ (document.getElementById("app"));
/** @type {any[] | null} */
let sessions = null;
/** The session you picked; otherwise the one that changed last. */
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

const tokens = (/** @type {number | null} */ n) => (n == null ? "—" : n >= 1e6 ? `${(n / 1e6).toFixed(1)}M` : n >= 1e3 ? `${Math.round(n / 1e3)}k` : String(n));
const took = (/** @type {number | null} */ ms) => (ms == null ? "" : ms < 1000 ? `${ms} ms` : ms < 60_000 ? `${(ms / 1000).toFixed(1)} s` : `${Math.round(ms / 60_000)} min`);
const clock = (/** @type {number} */ at) => new Date(at).toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
const status = { working: "Working", blocked: "Waiting for you", done: "Finished", idle: "Idle" };
const basename = (/** @type {string} */ path) => path.split("/").filter(Boolean).at(-1) ?? path;

function render() {
  if (sessions === null) return app.replaceChildren(h("p", { class: "muted" }, "Loading…"));
  if (sessions.length === 0) {
    return app.replaceChildren(h("div", { class: "empty" }, h("p", {}, "No Claude session in your panes."), h("p", { class: "muted" }, "Run claude in a pane and its activity shows here.")));
  }
  const s = sessions.find((x) => x.id === picked) ?? sessions[0];
  const parts = [];
  if (sessions.length > 1) {
    parts.push(
      h(
        "select",
        { "aria-label": "Session", onchange: (/** @type {Event} */ e) => ((picked = /** @type {HTMLSelectElement} */ (e.target).value), render()) },
        ...sessions.map((x) => h("option", { value: x.id, selected: x.id === s.id }, `${x.project} · ${x.name ?? x.id.slice(0, 8)}`)),
      ),
    );
  }
  parts.push(
    h(
      "dl",
      { class: "stats" },
      h("div", {}, h("dt", {}, "Status"), h("dd", { class: s.state === "blocked" ? "warn" : "" }, status[/** @type {keyof typeof status} */ (s.state)] ?? s.state)),
      h("div", {}, h("dt", {}, "Model"), h("dd", {}, s.model || "—")),
      h("div", {}, h("dt", {}, "Context"), h("dd", {}, tokens(s.context))),
    ),
  );
  if (s.state === "working" && s.running.length) {
    parts.push(
      h("h2", {}, "Now"),
      h("ul", { class: "list" }, ...s.running.map((/** @type {any} */ t) => h("li", { class: "row" }, h("span", { class: "spinner", "aria-hidden": true }), h("strong", {}, t.name), h("span", { class: "grow muted ellipsis" }, t.summary)))),
    );
  }
  if (s.files.length) {
    parts.push(
      h("h2", {}, `Files edited (${s.files.length})`),
      h("ul", { class: "list" }, ...s.files.map(([/** @type {string} */ path, /** @type {number} */ n]) => h("li", { class: "row", title: path }, h("span", { class: "grow ellipsis" }, basename(path)), h("span", { class: "muted" }, n > 1 ? `${n} edits` : "1 edit")))),
    );
  }
  parts.push(h("h2", {}, "Recent activity"));
  parts.push(
    s.items.length
      ? h(
          "ol",
          { class: "list timeline" },
          ...s.items.map((/** @type {any} */ item) =>
            item.kind === "prompt"
              ? h("li", { class: "prompt" }, h("span", { class: "who" }, "You"), h("span", { class: "grow" }, item.summary), h("time", { class: "muted" }, clock(item.at)))
              : h(
                  "li",
                  { class: `row${item.error ? " error" : ""}` },
                  item.done ? h("span", { class: item.error ? "dot err" : "dot", "aria-hidden": true }) : h("span", { class: "spinner", "aria-hidden": true }),
                  h("strong", {}, item.name),
                  h("span", { class: "grow muted ellipsis", title: item.summary }, item.summary),
                  h("span", { class: "muted" }, item.error ? "failed" : took(item.ms)),
                ),
          ),
        )
      : h("p", { class: "muted" }, "Nothing yet."),
  );
  app.replaceChildren(...parts);
}

wings.onBroadcast((message) => {
  const m = /** @type {any} */ (message);
  if (m?.type !== "sessions") return;
  sessions = m.sessions;
  render();
});

render();
void wings.broadcast({ type: "hello" });
