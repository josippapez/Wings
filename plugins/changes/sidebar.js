// @ts-check
/// <reference path="../../templates/wings-plugin/wings.d.ts" />

// The Changes sidebar: the changed files of the focused pane's repo as a folder tree, and its commit historyTab. The
// plugin's main script runs git and broadcasts the files and commits here; a click asks it to open the diff viewer.

const POLL_MS = 15_000;
/** How long the search waits after the last key before it asks git. */
const SEARCH_MS = 250;
const app = /** @type {HTMLElement} */ (document.getElementById("app"));

/**
 * What main sent last. `files` rows are `[status, path, added, removed, oldPath]`.
 * @type {{ mode: "uncommitted" | "branch", loading?: boolean, repo: { root: string, name: string, branch?: string } | null, against?: string, files?: [string, string, number | null, number | null, string | null][], total?: number, added?: number, removed?: number, partial?: boolean, error?: string } | null}
 */
let state = null;
/** Folders you collapsed, by path. */
const collapsed = new Set();
/** @type {"changes" | "history"} */
let tab = "changes";
/**
 * The History tab's commits. `rows` are `[hash, subject, author, at, refs]`, refs as `kind:name`.
 * @typedef {[string, string, string, number, string[]]} CommitRow
 * @type {{ loading?: boolean, loadingMore?: boolean, repo?: { root: string, name: string, branch?: string } | null, rows: CommitRow[], more?: boolean, error?: string } | null}
 */
let historyTab = null;
/** The search and scope the History tab asked for; replies for others are stale. */
let query = "";
let all = false;

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
const refresh = () => {
  if (tab === "history" && historyTab) historyTab = { ...historyTab, loading: true };
  render();
  void wings.broadcast({ type: "refresh", byUser: true, tab });
};
/** @param {"uncommitted" | "branch"} mode */
const setMode = (mode) => {
  if (tab === "changes" && state && state.mode === mode) return;
  tab = "changes";
  state = state && { mode, repo: state.repo, loading: true };
  render();
  void wings.broadcast({ type: "mode", mode });
};
/** Asks main for a page of commits: the first one for a new search, or the next. @param {number} skip */
const loadHistory = (skip) => {
  historyTab = skip && historyTab ? { ...historyTab, loadingMore: true } : { rows: [], ...historyTab, loading: true, error: undefined };
  render();
  void wings.broadcast({ type: "history", query, all, skip });
};
const showHistory = () => {
  if (tab === "history") return;
  tab = "history";
  loadHistory(0);
};

/** Kept across renders, so typing in it never loses the caret. */
let searchTimer = 0;
const search = /** @type {HTMLInputElement} */ (
  h("input", {
    id: "search",
    type: "search",
    placeholder: "Message, author or hash",
    "aria-label": "Search commits",
    oninput: () => {
      clearTimeout(searchTimer);
      searchTimer = setTimeout(() => {
        if (search.value.trim() === query) return;
        query = search.value.trim();
        loadHistory(0);
      }, SEARCH_MS);
    },
  })
);

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

// ---------- historyTab ----------

const relative = new Intl.RelativeTimeFormat(undefined, { numeric: "auto" });
/** @type {[Intl.RelativeTimeFormatUnit, number][]} */
const UNITS = [
  ["year", 31_557_600],
  ["month", 2_629_800],
  ["week", 604_800],
  ["day", 86_400],
  ["hour", 3_600],
  ["minute", 60],
];
/** Like "3 hours ago". @param {number} at Unix seconds. */
function ago(at) {
  const s = at - Date.now() / 1000;
  const unit = UNITS.find(([, size]) => Math.abs(s) >= size);
  return unit ? relative.format(Math.round(s / unit[1]), unit[0]) : "just now";
}

const REF_KIND = { H: "HEAD", b: "Branch", r: "Remote branch", t: "Tag", s: "Stash" };
/** A small pill for a branch or tag. @param {string} ref `kind:name` */
function refPill(ref) {
  const kind = ref[0];
  const name = ref.slice(2);
  const what = REF_KIND[/** @type {keyof typeof REF_KIND} */ (kind)] ?? "Ref";
  return h("span", { class: `ref ref-${kind}`, title: kind === "H" && name !== "HEAD" ? `HEAD, on ${name}` : `${what} ${name}` }, kind === "t" ? `◆ ${name}` : name);
}

/** @param {CommitRow} row */
function commitRow([hash, subject, author, at, refs]) {
  const open = () => historyTab?.repo && void wings.broadcast({ type: "commit", root: historyTab.repo.root, hash, subject, author, at });
  const date = new Date(at * 1000).toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });
  return h(
    "li",
    {},
    h(
      "button",
      { id: `c:${hash}`, class: "commit", title: `${subject}\n${hash}\n${author}, ${date}`, onclick: open },
      h("span", { class: "row" }, ...refs.map(refPill), h("span", { class: "subject ellipsis" }, subject)),
      h("span", { class: "row muted meta" }, h("span", { class: "hash" }, hash.slice(0, 7)), h("span", { class: "grow ellipsis" }, author), h("span", { class: "when" }, ago(at))),
    ),
  );
}

/** The History tab under the shared header. @returns {HTMLElement[]} */
function historyParts() {
  const parts = [
    h(
      "div",
      { class: "row" },
      h("div", { class: "grow" }, search),
      h("button", { onclick: () => historyTab?.repo && void wings.broadcast({ type: "graph", root: historyTab.repo.root }), title: "Every branch as a commit graph" }, "Graph"),
    ),
    h(
      "label",
      { class: "row check muted" },
      h("input", {
        type: "checkbox",
        id: "all",
        checked: all,
        onchange: (/** @type {Event} */ e) => {
          all = /** @type {HTMLInputElement} */ (e.target).checked;
          loadHistory(0);
        },
      }),
      "All branches",
    ),
  ];
  if (!historyTab) return parts;
  if (historyTab.error) parts.push(h("p", { class: "warn" }, historyTab.error));
  else if (historyTab.rows.length === 0 && !historyTab.loading) {
    parts.push(h("div", { class: "empty" }, h("p", {}, query ? "No commits match." : "No commits yet."), query ? h("p", { class: "muted" }, `Searched messages, authors and hashes for “${query}”.`) : null));
  } else {
    parts.push(h("ul", { class: "list commits" }, ...historyTab.rows.map(commitRow)));
    if (historyTab.more) {
      parts.push(h("button", { id: "more", class: "ghost", disabled: Boolean(historyTab.loadingMore), onclick: () => loadHistory(historyTab?.rows.length ?? 0) }, historyTab.loadingMore ? "Loading…" : "Load more"));
    }
  }
  return parts;
}

function render() {
  const focused = document.activeElement?.id;
  const view = tab === "history" ? historyTab : state;
  const repo = view?.repo ?? (tab === "history" ? state?.repo : undefined);
  if (!view && !repo) return app.replaceChildren(h("p", { class: "muted" }, "Loading…"));
  if (!repo) {
    return app.replaceChildren(h("div", { class: "empty" }, h("p", {}, "No git repo in your panes."), h("p", { class: "muted" }, "Open a terminal in a repo and its changes show here.")));
  }
  const mode = state?.mode ?? "uncommitted";
  const chip = (/** @type {boolean} */ on, /** @type {string} */ label, /** @type {() => void} */ onclick) =>
    h("button", { class: `chip${on ? " on" : ""}`, "aria-pressed": String(on), onclick }, label);
  const parts = [
    h(
      "div",
      { class: "row" },
      h("div", { class: "grow" }, h("div", { class: "repo ellipsis" }, repo.name), h("div", { class: "muted ellipsis" }, repo.branch ?? "")),
      view?.loading ? h("span", { class: "spinner", "aria-label": "Refreshing" }) : null,
      h("button", { onclick: refresh, title: "Refresh" }, "Refresh"),
    ),
    h(
      "div",
      { class: "row filters", role: "group", "aria-label": "Show" },
      chip(tab === "changes" && mode === "uncommitted", "Uncommitted", () => setMode("uncommitted")),
      chip(tab === "changes" && mode === "branch", "This branch", () => setMode("branch")),
      chip(tab === "history", "History", showHistory),
    ),
  ];
  if (tab === "history") parts.push(...historyParts());
  else if (state?.error) parts.push(h("p", { class: "warn" }, state.error));
  else if (state?.files) {
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
  if (m?.type === "history") {
    if (m.query !== query || m.all !== all) return;
    const rowsOf = /** @type {CommitRow[]} */ (m.rows ?? []);
    // A next page appends, unless the repo changed under it; a first page replaces the list.
    if (m.skip > 0 && historyTab && historyTab.repo?.root === m.repo?.root && m.skip === historyTab.rows.length) historyTab = { ...historyTab, rows: [...historyTab.rows, ...rowsOf], more: m.more, loadingMore: false };
    else if (m.skip === 0) historyTab = { repo: m.repo, rows: rowsOf, more: m.more, error: m.error };
    else return;
    if (tab === "history") render();
    return;
  }
  if (m?.type !== "state") return;
  // A refresh of the same repo and mode keeps the list on screen until the new one arrives.
  const same = state && m.loading && state.mode === m.mode && state.repo?.root === m.repo?.root;
  state = same ? { ...state, loading: true } : m;
  if (tab === "changes") render();
});

render();
void wings.broadcast({ type: "hello" });
setInterval(() => void wings.broadcast({ type: "refresh", tab }), POLL_MS);
