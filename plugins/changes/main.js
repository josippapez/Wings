// @ts-check
/// <reference path="../../templates/wings-plugin/wings.d.ts" />

// Shows what changed in each pane's git repo: a badge with the counts, a sidebar with the files as a tree, and
// the diff viewer for one file or all of them. Everything is local git, about 25 ms a call, cached per repo.

const POLL_MS = 15_000;
/** A result this fresh is reused, so the badge poll and the sidebar's refresh don't run git twice. */
const FRESH_MS = 3_000;
/** About 2 MB of patch. Past this the viewer would take seconds to draw, so "View all" says so instead. */
const MAX_PATCH_LINES = 50_000;
const MAX_PATCH_BYTES = 2 * 1024 * 1024;
const MAX_PATCH_FILES = 1_500;
/** Untracked files read through `git grep`, which takes their paths as arguments. */
const MAX_UNTRACKED_READ = 200;
/** What one broadcast to the sidebar may spend on files, under Wings' 64 KB limit. */
const MAX_LIST_BYTES = 56_000;
/** Git's empty tree, the base of a repo with no commits yet. */
const EMPTY_TREE = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";
const COLOR = ["--no-color", "--no-ext-diff", "--no-textconv", "--no-relative"];

/** @typedef {"uncommitted" | "branch"} Mode */
/**
 * @typedef {{ path: string, oldPath?: string, status: string, added: number | null, removed: number | null, untracked?: boolean }} FileChange
 * `added` and `removed` are null for binary files, and `added` for untracked files git couldn't read as text.
 */
/**
 * @typedef {{ root: string, name: string, branch: string, mode: Mode, base: string, baseLabel: string, files: FileChange[], added: number, removed: number, partial: boolean, at: number }} Changes
 */

/** @type {WingsPane[]} */
let panes = [];
/** Repo root by pane cwd; null outside a repo. */
const roots = new Map();
/** Changes by `${root}\0${mode}`, and the lookups still running. */
const cache = new Map();
const running = new Map();
/** The default branch each repo's "This branch" compares against. */
const defaults = new Map();
/** What the sidebar shows: its mode, the repo it follows, and when it last asked. */
let sidebar = { mode: /** @type {Mode} */ ("uncommitted"), root: /** @type {string | null} */ (null), seen: 0 };

/** @param {string} cwd @param {string[]} args */
async function git(cwd, args) {
  const r = await wings.exec("git", args, { cwd });
  if (r.code !== 0) throw new Error(r.stderr.trim().split("\n")[0] || `git ${args[0]} failed`);
  return r.stdout;
}
/** @param {string} cwd @param {string[]} args */
const gitOk = (cwd, args) => wings.exec("git", args, { cwd }).then((r) => (r.code === 0 ? r.stdout.trim() : null));

/** Pathspecs that match these exact paths, not as globs. @param {string[]} paths */
const literal = (paths) => paths.map((p) => `:(literal)${p}`);
const basename = (/** @type {string} */ path) => path.split("/").filter(Boolean).at(-1) ?? path;

/** @param {string} cwd @returns {Promise<string | null>} */
async function rootOf(cwd) {
  if (!cwd) return null;
  if (!roots.has(cwd)) roots.set(cwd, gitOk(cwd, ["rev-parse", "--show-toplevel"]).then((r) => r || null));
  return roots.get(cwd);
}

/** `origin/HEAD` as the remote set it, else the usual names. @param {string} root @returns {Promise<string | null>} */
async function defaultBranch(root) {
  if (defaults.has(root)) return defaults.get(root);
  let ref = await gitOk(root, ["rev-parse", "--abbrev-ref", "origin/HEAD"]);
  if (!ref || ref === "origin/HEAD") ref = null;
  for (const name of ["origin/main", "origin/master", "main", "master"]) {
    if (ref) break;
    if (await gitOk(root, ["rev-parse", "--verify", "--quiet", `${name}^{commit}`])) ref = name;
  }
  defaults.set(root, ref);
  return ref;
}

/** Splits `git diff --numstat -z` and `--name-status -z` into one entry per file. @param {string} numstat @param {string} names */
function parseDiff(numstat, names) {
  /** @type {Map<string, FileChange>} */
  const files = new Map();
  const n = names.split("\0");
  for (let i = 0; i < n.length && n[i]; ) {
    const status = n[i];
    const two = status[0] === "R" || status[0] === "C";
    const file = two ? { path: n[i + 2], oldPath: n[i + 1], status: status[0] } : { path: n[i + 1], status: status[0] };
    files.set(file.path, { ...file, added: 0, removed: 0 });
    i += two ? 3 : 2;
  }
  const s = numstat.split("\0");
  for (let i = 0; i < s.length && s[i]; ) {
    const [added, removed, ...rest] = s[i].split("\t");
    const path = rest.join("\t") || s[i + 2];
    i += rest.join("\t") ? 1 : 3;
    const file = files.get(path);
    if (!file) continue;
    file.added = added === "-" ? null : Number(added);
    file.removed = removed === "-" ? null : Number(removed);
  }
  return [...files.values()];
}

/** Line counts of untracked text files, from `git grep --count`; binary and empty files are left out. @param {string} root @param {string[]} paths */
async function lineCounts(root, paths) {
  /** @type {Map<string, number>} */
  const counts = new Map();
  if (paths.length === 0) return counts;
  const r = await wings.exec("git", ["grep", "--untracked", "-I", "-z", "--count", "--no-color", "-e", "", "--", ...literal(paths)], { cwd: root });
  // 1 is "no match": every file was empty or binary.
  if (r.code > 1) throw new Error(r.stderr.trim() || "git grep failed");
  for (const line of r.stdout.split("\n")) {
    const at = line.lastIndexOf("\0");
    if (at > 0) counts.set(line.slice(0, at), Number(line.slice(at + 1)));
  }
  return counts;
}

/** The changed files of a repo: against HEAD, or against where this branch left the default branch. @param {string} root @param {Mode} mode @returns {Promise<Changes>} */
async function load(root, mode) {
  const [branch, head] = await Promise.all([gitOk(root, ["branch", "--show-current"]), gitOk(root, ["rev-parse", "--verify", "--quiet", "HEAD"])]);
  let base = head ?? EMPTY_TREE;
  let baseLabel = head ? "HEAD" : "an empty repo";
  if (mode === "branch") {
    const ref = await defaultBranch(root);
    if (!ref) throw new Error("No default branch found (origin/HEAD, main or master).");
    if (!head) throw new Error("This repo has no commits yet.");
    base = (await git(root, ["merge-base", "HEAD", ref])).trim();
    baseLabel = `${ref} at ${base.slice(0, 7)}`;
  }
  const [numstat, names, others] = await Promise.all([
    git(root, ["diff", base, "--numstat", "-z", "-M", ...COLOR]),
    git(root, ["diff", base, "--name-status", "-z", "-M", ...COLOR]),
    git(root, ["ls-files", "--others", "--exclude-standard", "-z"]),
  ]);
  const files = parseDiff(numstat, names);
  const untracked = others.split("\0").filter(Boolean);
  const counts = await lineCounts(root, untracked.slice(0, MAX_UNTRACKED_READ));
  for (const path of untracked) files.push({ path, status: "?", added: counts.get(path) ?? null, removed: 0, untracked: true });
  files.sort((a, b) => (a.path < b.path ? -1 : a.path > b.path ? 1 : 0));
  return {
    root,
    name: basename(root),
    branch: branch || (head ? `detached at ${head.slice(0, 7)}` : "no commits"),
    mode,
    base,
    baseLabel,
    files,
    added: files.reduce((n, f) => n + (f.added ?? 0), 0),
    removed: files.reduce((n, f) => n + (f.removed ?? 0), 0),
    partial: untracked.length > MAX_UNTRACKED_READ,
    at: Date.now(),
  };
}

/** Cached for a moment, and one lookup at a time per repo and mode. @param {string} root @param {Mode} mode @param {{ fresh?: boolean }} [options] */
function changes(root, mode, { fresh = false } = {}) {
  const key = `${root}\0${mode}`;
  const hit = cache.get(key);
  if (!fresh && hit && Date.now() - hit.at < FRESH_MS) return Promise.resolve(hit);
  if (running.has(key)) return running.get(key);
  const p = load(root, mode).then(
    (c) => (cache.set(key, c), c),
    (error) => {
      cache.delete(key);
      throw error;
    },
  );
  running.set(key, p);
  void p.finally(() => running.delete(key)).catch(() => {});
  return p;
}

// ---------- badges ----------

const plural = (/** @type {number} */ n, /** @type {string} */ word) => `${n} ${word}${n === 1 ? "" : "s"}`;
const lines = (/** @type {Changes} */ c) => `+${c.added} −${c.removed}`;

/** @param {Changes} c */
function badgeFor(c) {
  const kinds = { M: "modified", A: "added", D: "deleted", R: "renamed", "?": "new" };
  /** @type {Record<string, number>} */
  const by = {};
  for (const f of c.files) by[f.status] = (by[f.status] ?? 0) + 1;
  const summary = Object.entries(by).map(([s, n]) => `${n} ${kinds[/** @type {keyof typeof kinds} */ (s)] ?? s}`).join(", ");
  return {
    label: c.added || c.removed ? lines(c) : "changed",
    tone: /** @type {const} */ ("neutral"),
    counts: [{ icon: /** @type {const} */ ("changed"), value: c.files.length }],
    title: plural(c.files.length, "changed file"),
    subtitle: `${c.name} · ${c.branch}`,
    rows: [
      { label: "Files", value: summary },
      { label: "Lines", value: c.partial ? `${lines(c)}, not counting every new file` : lines(c) },
    ],
    actions: [
      { id: "open", label: "Open changes", primary: true },
      { id: "branch", label: "Changes on this branch" },
      { id: "sidebar", label: "Show in sidebar" },
      { id: "refresh", label: "Refresh" },
    ],
  };
}

/** Looks up each repo once, then badges every pane in it. @param {{ fresh?: boolean }} [options] */
async function refreshBadges(options) {
  const list = panes;
  const byPane = await Promise.all(list.map(async (p) => /** @type {const} */ ([p, await rootOf(p.cwd).catch(() => null)])));
  const results = new Map();
  for (const root of new Set(byPane.map(([, r]) => r).filter(Boolean))) {
    results.set(root, await changes(/** @type {string} */ (root), "uncommitted", options).catch((error) => (console.error(error), null)));
  }
  for (const [pane, root] of byPane) {
    const c = root ? results.get(root) : null;
    void wings.setBadge(pane.paneId, c && c.files.length ? badgeFor(c) : null).catch(() => {});
  }
}

// ---------- the diff viewer ----------

/** A patch for files git doesn't track yet, read with `git grep`, since Wings refuses `git diff --no-index`. @param {string} root @param {FileChange[]} files */
async function untrackedPatch(root, files) {
  const read = files.slice(0, MAX_UNTRACKED_READ);
  if (read.length === 0) return "";
  const paths = read.map((f) => f.path);
  const r = await wings.exec("git", ["grep", "--untracked", "-I", "-z", "--no-color", "--no-line-number", "--no-column", "-e", "", "--", ...literal(paths)], { cwd: root });
  if (r.code > 1) throw new Error(r.stderr.trim() || "git grep failed");
  /** @type {Map<string, string[]>} */
  const text = new Map();
  for (const line of r.stdout.split("\n")) {
    const at = line.indexOf("\0");
    if (at < 0) continue;
    const path = line.slice(0, at);
    if (!text.has(path)) text.set(path, []);
    text.get(path)?.push(line.slice(at + 1));
  }
  // Files with no text lines are empty or binary; counting without -I tells them apart.
  const rest = paths.filter((p) => !text.has(p));
  const binary = new Set();
  if (rest.length) {
    const all = await wings.exec("git", ["grep", "--untracked", "-z", "--count", "--no-color", "-e", "", "--", ...literal(rest)], { cwd: root });
    for (const line of all.stdout.split("\n")) if (line.includes("\0")) binary.add(line.slice(0, line.lastIndexOf("\0")));
  }
  return paths
    .map((path) => {
      const head = `diff --git a/${path} b/${path}\nnew file mode 100644\n`;
      if (binary.has(path)) return `${head}Binary files /dev/null and b/${path} differ\n`;
      const body = text.get(path);
      if (!body) return head;
      return `${head}--- /dev/null\n+++ b/${path}\n@@ -0,0 +1,${body.length} @@\n${body.map((l) => `+${l}\n`).join("")}`;
    })
    .join("");
}

/** The patch for one file, or all of them. @param {Changes} c @param {string | null} path */
async function patchOf(c, path) {
  const files = path == null ? c.files : c.files.filter((f) => f.path === path);
  if (path != null && files.length === 0) throw new Error("This file has no changes any more.");
  const size = files.reduce((n, f) => n + (f.added ?? 0) + (f.removed ?? 0), 0);
  if (files.length > MAX_PATCH_FILES || size > MAX_PATCH_LINES) {
    throw new Error(`Too large to show: ${plural(files.length, "file")}, ${plural(size, "changed line")}. Open files one at a time from the Changes sidebar.`);
  }
  const tracked = files.filter((f) => !f.untracked);
  const untracked = files.filter((f) => f.untracked);
  const spec = path == null ? [] : ["--", ...literal(tracked.flatMap((f) => (f.oldPath ? [f.oldPath, f.path] : [f.path])))];
  const [diff, added] = await Promise.all([
    tracked.length ? git(c.root, ["diff", c.base, "-M", ...COLOR, ...spec]) : "",
    untrackedPatch(c.root, untracked),
  ]);
  const patch = diff + added;
  if (patch.length > MAX_PATCH_BYTES) throw new Error(`Too large to show: ${Math.round(patch.length / 1024 / 1024)} MB of changes in ${plural(files.length, "file")}.`);
  const skipped = untracked.length - Math.min(untracked.length, MAX_UNTRACKED_READ);
  return { patch, note: skipped ? ` · ${plural(skipped, "new file")} not shown` : "" };
}

const against = (/** @type {Mode} */ mode, /** @type {string} */ label) => (mode === "branch" ? `Since ${label}` : "Working tree against HEAD");

/** Opens the viewer at once in its loading state and fills it when git answers. @param {string} root @param {Mode} mode @param {string | null} path */
async function openChanges(root, mode, path) {
  const title = path ?? (mode === "branch" ? "Changes on this branch" : "Uncommitted changes");
  const { id } = await wings.openDiff({ title, subtitle: `${basename(root)} · ${mode === "branch" ? "This branch" : "Working tree against HEAD"}` });
  try {
    const c = await changes(root, mode);
    if (c.files.length === 0) return void (await wings.updateDiff(id, { error: mode === "branch" ? "Nothing changed on this branch." : "No uncommitted changes." }));
    const { patch, note } = await patchOf(c, path);
    if (!patch) return void (await wings.updateDiff(id, { error: `No diff to show${note}.` }));
    await wings.updateDiff(id, { patch });
  } catch (error) {
    await wings.updateDiff(id, { error: String(/** @type {Error} */ (error).message ?? error) });
  }
}

// ---------- the sidebar ----------

/** The focused pane's repo, else the one the sidebar showed, else the first pane in a repo. */
async function sidebarRoot() {
  const focused = panes.find((p) => p.focused);
  const root = focused && (await rootOf(focused.cwd).catch(() => null));
  if (root) return root;
  const all = await Promise.all(panes.map((p) => rootOf(p.cwd).catch(() => null)));
  return all.includes(sidebar.root) ? sidebar.root : (all.find(Boolean) ?? null);
}

/** Files as `[status, path, added, removed, oldPath]`, as many as fit in one broadcast. @param {FileChange[]} files */
function compact(files) {
  const out = [];
  let bytes = 0;
  for (const f of files) {
    const row = [f.status, f.path, f.added, f.removed, f.oldPath ?? null];
    bytes += JSON.stringify(row).length + 1;
    if (bytes > MAX_LIST_BYTES) break;
    out.push(row);
  }
  return out;
}

/** Sends the sidebar what it shows. @param {{ fresh?: boolean }} [options] */
async function sendSidebar(options) {
  const root = await sidebarRoot();
  sidebar.root = root;
  const mode = sidebar.mode;
  if (!root) return void (await wings.broadcast({ type: "state", mode, repo: null }));
  void wings.broadcast({ type: "state", mode, loading: true, repo: { root, name: basename(root) } }).catch(() => {});
  try {
    const c = await changes(root, mode, options);
    if (sidebar.root !== root || sidebar.mode !== mode) return;
    const files = compact(c.files);
    await wings.broadcast({
      type: "state",
      mode,
      repo: { root, name: c.name, branch: c.branch },
      against: against(mode, c.baseLabel),
      files,
      total: c.files.length,
      added: c.added,
      removed: c.removed,
      partial: c.partial,
      at: c.at,
    });
  } catch (error) {
    if (sidebar.root !== root || sidebar.mode !== mode) return;
    await wings.broadcast({ type: "state", mode, repo: { root, name: basename(root) }, error: String(/** @type {Error} */ (error).message ?? error) });
  }
}

const sidebarOpen = () => Date.now() - sidebar.seen < POLL_MS * 2;

wings.onBroadcast((message) => {
  if (!message || typeof message !== "object") return;
  const m = /** @type {Record<string, unknown>} */ (message);
  if (m.type === "hello" || m.type === "refresh" || m.type === "mode") {
    sidebar.seen = Date.now();
    if (m.type === "mode" && (m.mode === "uncommitted" || m.mode === "branch")) sidebar.mode = m.mode;
    if (m.type === "refresh" && m.byUser) defaults.clear();
    void sendSidebar({ fresh: m.type === "refresh" }).catch(console.error);
  }
  if (m.type === "open" && typeof m.root === "string" && (m.mode === "uncommitted" || m.mode === "branch")) {
    void openChanges(m.root, m.mode, typeof m.path === "string" ? m.path : null).catch(console.error);
  }
});

// ---------- panes, actions and the tool ----------

wings.onPanes((list) => {
  const before = new Set(panes.map((p) => `${p.paneId}\0${p.cwd}`));
  panes = list;
  if (list.some((p) => !before.has(`${p.paneId}\0${p.cwd}`)) || list.length !== before.size) void refreshBadges().catch(console.error);
  if (!sidebarOpen()) return;
  void sidebarRoot().then((root) => (root === sidebar.root ? undefined : sendSidebar()));
});

setInterval(() => {
  // A repo becomes one after `git init`, and stops after it's removed.
  for (const [cwd, root] of roots) void root.then((r) => r === null && roots.delete(cwd));
  void refreshBadges().catch(console.error);
}, POLL_MS);

wings.onAction(async ({ paneId, actionId }) => {
  const pane = panes.find((p) => p.paneId === paneId);
  const root = pane && (await rootOf(pane.cwd));
  if (!root) return;
  if (actionId === "open") return openChanges(root, "uncommitted", null);
  if (actionId === "branch") return openChanges(root, "branch", null);
  if (actionId === "refresh") return refreshBadges({ fresh: true });
  if (actionId === "sidebar") return wings.openView("changes");
});

wings.onTool("changed_files", async (input) => {
  const cwd = typeof input?.cwd === "string" ? input.cwd : null;
  if (!cwd) throw new Error("cwd is required: a folder inside a git repo.");
  const root = await rootOf(cwd);
  if (!root) throw new Error(`${cwd} is not inside a git repo.`);
  const mode = input?.against === "branch" ? "branch" : "uncommitted";
  const c = await changes(root, mode, { fresh: true });
  return {
    repo: c.root,
    branch: c.branch,
    against: against(mode, c.baseLabel),
    added: c.added,
    removed: c.removed,
    total: c.files.length,
    files: c.files.slice(0, 500).map(({ path, oldPath, status, added, removed }) => ({ path, ...(oldPath ? { oldPath } : {}), status, added, removed })),
  };
});
