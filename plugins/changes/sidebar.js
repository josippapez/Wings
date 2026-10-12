// @ts-check
/// <reference path="../../templates/wings-plugin/wings.d.ts" />

// The Changes sidebar: the changed files of the focused pane's repo as a folder tree. The plugin's main script
// runs git and broadcasts the file list here; a click asks it to open the diff viewer.

const POLL_MS = 15_000;
const app = /** @type {HTMLElement} */ (document.getElementById("app"));

/**
 * What main sent last. `files` rows are `[status, path, added, removed, oldPath]`.
 * @type {{ mode: "uncommitted" | "branch", loading?: boolean, repo: { root: string, name: string, branch?: string } | null, against?: string, files?: [string, string, number | null, number | null, string | null][], total?: number, added?: number, removed?: number, partial?: boolean, error?: string } | null}
 */
let state = null;
/** Folders you collapsed, by path. */
const collapsed = new Set();

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

const STATUS = { M: "Modified", A: "Added", D: "Deleted", R: "Renamed", C: "Copied", T: "Type changed", U: "Conflict", "?": "New, not tracked yet" };
const plural = (/** @type {number} */ n, /** @type {string} */ word) => `${n} ${word}${n === 1 ? "" : "s"}`;

/** @param {string | null} path */
const openDiff = (path) => state?.repo && void wings.broadcast({ type: "open", root: state.repo.root, mode: state.mode, path });
const refresh = () => void wings.broadcast({ type: "refresh", byUser: true });
/** @param {"uncommitted" | "branch"} mode */
const setMode = (mode) => {
  if (state && state.mode === mode) return;
  state = state && { mode, repo: state.repo, loading: true };
  render();
  void wings.broadcast({ type: "mode", mode });
};

/** @typedef {{ name: string, path: string, folders: Map<string, Folder>, files: NonNullable<typeof state>["files"] & {} }} Folder */

/** @param {NonNullable<NonNullable<typeof state>["files"]>} files */
function tree(files) {
  /** @type {Folder} */
  const root = { name: "", path: "", folders: new Map(), files: [] };
  for (const row of files) {
    const parts = row[1].split("/");
    let at = root;
    for (const part of parts.slice(0, -1)) {
      const path = at.path ? `${at.path}/${part}` : part;
      if (!at.folders.has(part)) at.folders.set(part, { name: part, path, folders: new Map(), files: [] });
      at = /** @type {Folder} */ (at.folders.get(part));
    }
    at.files.push(row);
  }
  return root;
}

/** Folder and file rows, a folder with only one folder in it shown as `a/b`. @param {Folder} folder @param {number} depth @returns {HTMLElement[]} */
function rows(folder, depth) {
  const out = [];
  for (const sub of folder.folders.values()) {
    let f = sub;
    let label = f.name;
    while (f.files.length === 0 && f.folders.size === 1) {
      f = /** @type {Folder} */ ([...f.folders.values()][0]);
      label += `/${f.name}`;
    }
    const isOpen = !collapsed.has(f.path);
    const toggle = () => {
      if (isOpen) collapsed.add(f.path);
      else collapsed.delete(f.path);
      render();
    };
    out.push(
      h(
        "li",
        {},
        h(
          "button",
          { id: `d:${f.path}`, class: "row folder", style: `padding-left: ${6 + depth * 12}px`, "aria-expanded": String(isOpen), onclick: toggle },
          h("span", { class: `chevron${isOpen ? " open" : ""}`, "aria-hidden": "true" }),
          h("span", { class: "grow ellipsis" }, label),
        ),
      ),
    );
    if (isOpen) out.push(...rows(f, depth + 1));
  }
  for (const [status, path, added, removed, oldPath] of folder.files) {
    const name = path.split("/").at(-1) ?? path;
    const what = STATUS[/** @type {keyof typeof STATUS} */ (status)] ?? status;
    out.push(
      h(
        "li",
        {},
        h(
          "button",
          { id: `f:${path}`, class: "row file", style: `padding-left: ${18 + depth * 12}px`, title: oldPath ? `${oldPath} → ${path}` : path, onclick: () => openDiff(path) },
          h("span", { class: `status s-${status === "?" ? "new" : status}`, title: what, "aria-label": what }, status),
          h("span", { class: "grow ellipsis" }, name),
          added == null ? h("span", { class: "muted" }, removed == null ? "binary" : "new") : null,
          added ? h("span", { class: "add" }, `+${added}`) : null,
          removed ? h("span", { class: "del" }, `−${removed}`) : null,
        ),
      ),
    );
  }
  return out;
}

function render() {
  const focused = document.activeElement?.id;
  if (!state) return app.replaceChildren(h("p", { class: "muted" }, "Loading…"));
  if (!state.repo) {
    return app.replaceChildren(h("div", { class: "empty" }, h("p", {}, "No git repo in your panes."), h("p", { class: "muted" }, "Open a terminal in a repo and its changes show here.")));
  }
  const mode = state.mode;
  const chip = (/** @type {"uncommitted" | "branch"} */ m, /** @type {string} */ label) =>
    h("button", { class: `chip${mode === m ? " on" : ""}`, "aria-pressed": String(mode === m), onclick: () => setMode(m) }, label);
  const parts = [
    h(
      "div",
      { class: "row" },
      h("div", { class: "grow" }, h("div", { class: "repo ellipsis" }, state.repo.name), h("div", { class: "muted ellipsis" }, state.repo.branch ?? "")),
      state.loading ? h("span", { class: "spinner", "aria-label": "Refreshing" }) : null,
      h("button", { onclick: refresh, title: "Refresh" }, "Refresh"),
    ),
    h("div", { class: "row filters", role: "group", "aria-label": "Compare" }, chip("uncommitted", "Uncommitted"), chip("branch", "This branch")),
  ];
  if (state.error) parts.push(h("p", { class: "warn" }, state.error));
  else if (state.files) {
    const files = state.files;
    const total = state.total ?? files.length;
    if (total === 0) parts.push(h("div", { class: "empty" }, h("p", {}, mode === "branch" ? "Nothing changed on this branch." : "No uncommitted changes."), h("p", { class: "muted" }, state.against ?? "")));
    else {
      parts.push(
        h(
          "div",
          { class: "row summary" },
          h(
            "div",
            { class: "grow" },
            h("div", {}, plural(total, "file"), " ", h("span", { class: "add" }, `+${state.added}`), " ", h("span", { class: "del" }, `−${state.removed}`), state.partial ? h("span", { class: "muted" }, " (some new files not counted)") : null),
            h("div", { class: "muted ellipsis" }, state.against ?? ""),
          ),
          h("button", { onclick: () => openDiff(null) }, "View all"),
        ),
        h("ul", { class: "list" }, ...rows(tree(files), 0)),
      );
      if (files.length < total) parts.push(h("p", { class: "muted" }, `Too large to list: showing ${files.length} of ${total} files.`));
    }
  }
  app.replaceChildren(...parts);
  if (focused) document.getElementById(focused)?.focus({ preventScroll: true });
}

wings.onBroadcast((message) => {
  const m = /** @type {any} */ (message);
  if (m?.type !== "state") return;
  // A refresh of the same repo and mode keeps the list on screen until the new one arrives.
  const same = state && m.loading && state.mode === m.mode && state.repo?.root === m.repo?.root;
  state = same ? { ...state, loading: true } : m;
  render();
});

render();
void wings.broadcast({ type: "hello" });
setInterval(() => void wings.broadcast({ type: "refresh" }), POLL_MS);
