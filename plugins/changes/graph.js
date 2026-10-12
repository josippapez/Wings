// @ts-check
/// <reference path="../../templates/wings-plugin/wings.d.ts" />

// The commit graph: every branch's commits with their lanes, like `git log --graph --all`. It runs git itself,
// since the history is far larger than one broadcast, lays the lanes out once, and only draws the rows on screen.

const ROW = 26;
const LANE = 14;
/** Rows drawn above and below the screen, so a fast scroll doesn't show gaps. */
const OVERSCAN = 12;
/** Lanes past this are cut off; a repo rarely has this many branches side by side. */
const MAX_LANES = 24;
const LIMIT = 10_000;
/** Same fields as main's `LOG`: hash, parents, author, author time, committer time, refs, subject. */
const LOG = ["-z", "--no-color", "--decorate=full", "--format=%H%x00%P%x00%an%x00%at%x00%ct%x00%D%x00%s"];
const LOG_FIELDS = 7;

const head = /** @type {HTMLElement} */ (document.getElementById("head"));
const scroller = /** @type {HTMLElement} */ (document.getElementById("scroller"));
const spacer = /** @type {HTMLElement} */ (document.getElementById("spacer"));
const view = /** @type {HTMLElement} */ (document.getElementById("window"));
const canvas = /** @type {HTMLCanvasElement} */ (document.getElementById("lanes"));
const list = /** @type {HTMLElement} */ (document.getElementById("rows"));
const ctx = /** @type {CanvasRenderingContext2D} */ (canvas.getContext("2d"));

/**
 * A commit and where the graph draws it. `lines` holds four numbers a line: from lane, to lane, part and colour,
 * where part 0 runs from the row's top into the dot, 1 from the dot to the row's bottom, and 2 straight through.
 * @typedef {{ hash: string, parents: string[], author: string, at: number, refs: string[], subject: string, lane: number, color: number, lines: number[] }} Commit
 */

/** @type {{ root: string | null, name?: string, loading: boolean, error?: string, commits: Commit[], lanes: number, took?: number }} */
let state = { root: null, loading: true, commits: [], lanes: 1 };
let selected = -1;

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

// ---------- reading and laying out the history ----------

/** `%D` with `--decorate=full`, as short `kind:name` refs, like main's. @param {string} decoration */
function parseRefs(decoration) {
  const out = [];
  for (let ref of decoration ? decoration.split(", ") : []) {
    if (ref === "HEAD") {
      out.push("H:HEAD");
      continue;
    }
    const isHead = ref.startsWith("HEAD -> ");
    if (isHead) ref = ref.slice(8);
    if (ref.startsWith("tag: ")) ref = ref.slice(5);
    if (ref.startsWith("refs/heads/")) out.push(`${isHead ? "H" : "b"}:${ref.slice(11)}`);
    else if (ref.startsWith("refs/remotes/") && !ref.endsWith("/HEAD")) out.push(`r:${ref.slice(13)}`);
    else if (ref.startsWith("refs/tags/")) out.push(`t:${ref.slice(10)}`);
    else if (ref === "refs/stash") out.push("s:stash");
  }
  return out;
}

/** @param {string} out @returns {Commit[]} */
function parseLog(out) {
  const f = out.split("\0");
  const commits = [];
  for (let i = 0; i + LOG_FIELDS <= f.length && f[i]; i += LOG_FIELDS) {
    commits.push({ hash: f[i], parents: f[i + 1] ? f[i + 1].split(" ") : [], author: f[i + 2], at: Number(f[i + 3]), refs: parseRefs(f[i + 5]), subject: f[i + 6], lane: 0, color: 0, lines: [] });
  }
  return commits;
}

/**
 * Gives each commit a lane, top to bottom in topological order. Each lane waits for one hash: a commit takes the
 * lane waiting for it (the others waiting for it end there), and hands it to its first parent; other parents get
 * a free lane unless one already waits for them. Lanes keep their place, so lines only bend at merges and forks.
 * @param {Commit[]} commits
 */
function layout(commits) {
  /** @type {(string | null)[]} */
  const lanes = [];
  /** @type {number[]} */
  const colors = [];
  let next = 0;
  let widest = 1;
  const free = () => {
    const i = lanes.indexOf(null);
    return i < 0 ? lanes.length : i;
  };
  for (const c of commits) {
    let lane = lanes.indexOf(c.hash);
    if (lane < 0) {
      // A branch tip: nothing above leads to it, so it starts in a free lane with a colour of its own.
      lane = free();
      colors[lane] = next++;
    }
    const color = colors[lane];
    const lines = [];
    for (let j = 0; j < lanes.length; j++) {
      if (lanes[j] === c.hash) {
        lines.push(j, lane, 0, colors[j]);
        lanes[j] = null;
      } else if (lanes[j] !== null) lines.push(j, j, 2, colors[j]);
    }
    lanes[lane] = null;
    c.parents.forEach((p, k) => {
      let at = lanes.indexOf(p);
      if (at < 0) {
        at = k === 0 ? lane : free();
        lanes[at] = p;
        colors[at] = k === 0 ? color : next++;
      }
      lines.push(lane, at, 1, colors[at]);
    });
    while (lanes.length && lanes[lanes.length - 1] === null) lanes.pop();
    c.lane = lane;
    c.color = color;
    c.lines = lines;
    widest = Math.max(widest, lane + 1, lanes.length);
  }
  return Math.min(widest, MAX_LANES);
}

// ---------- drawing ----------

const css = getComputedStyle(document.documentElement);
/** Wings' own colours first, then four more that read on the dark background. */
const PALETTE = [...["--wings-info", "--wings-success", "--wings-warning", "--wings-danger"].map((v) => css.getPropertyValue(v).trim()), "#c4a7ff", "#6fd3d0", "#f0a3d0", "#b9cf6e"];
const day = new Intl.DateTimeFormat(undefined, { dateStyle: "medium" });
const full = new Intl.DateTimeFormat(undefined, { dateStyle: "medium", timeStyle: "short" });
const REF_KIND = { H: "HEAD", b: "Branch", r: "Remote branch", t: "Tag", s: "Stash" };

/** The row buttons on screen, reused as you scroll, and the first row they show. @type {HTMLButtonElement[]} */
const pool = [];
let first = -1;
let frame = 0;
const laneX = (/** @type {number} */ lane) => 10 + lane * LANE;
const graphWidth = () => laneX(state.lanes) + 4;

/** @param {string} ref `kind:name` */
function refPill(ref) {
  const kind = ref[0];
  const name = ref.slice(2);
  const what = REF_KIND[/** @type {keyof typeof REF_KIND} */ (kind)] ?? "Ref";
  return h("span", { class: `ref ref-${kind}`, title: kind === "H" && name !== "HEAD" ? `HEAD, on ${name}` : `${what} ${name}` }, kind === "t" ? `◆ ${name}` : name);
}

function makeRow() {
  const b = /** @type {HTMLButtonElement} */ (
    h("button", {}, h("span", { class: "lane-gap" }), h("span", { class: "refs" }), h("span", { class: "grow ellipsis" }), h("span", { class: "hash muted" }), h("span", { class: "author ellipsis muted" }), h("span", { class: "date muted" }))
  );
  pool.push(b);
  list.append(h("li", {}, b));
}

/** @param {HTMLButtonElement} b @param {number} i */
function fill(b, i) {
  b.classList.toggle("on", i === selected);
  if (b.dataset.i === String(i)) return;
  const c = state.commits[i];
  const [, refs, subject, hash, author, date] = /** @type {HTMLElement[]} */ ([...b.children]);
  b.dataset.i = String(i);
  b.title = `${c.subject}\n${c.hash}\n${c.author}, ${full.format(c.at * 1000)}`;
  refs.replaceChildren(...c.refs.map(refPill));
  subject.textContent = c.subject;
  hash.textContent = c.hash.slice(0, 7);
  author.textContent = c.author;
  date.textContent = day.format(c.at * 1000);
}

/** The lanes of rows `top` to `top + size`, onto a canvas as tall as those rows. @param {number} top @param {number} size */
function draw(top, size) {
  const dpr = window.devicePixelRatio || 1;
  const width = graphWidth();
  const height = size * ROW;
  if (canvas.width !== Math.round(width * dpr) || canvas.height !== Math.round(height * dpr)) {
    canvas.width = Math.round(width * dpr);
    canvas.height = Math.round(height * dpr);
    canvas.style.width = `${width}px`;
    canvas.style.height = `${height}px`;
  }
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.clearRect(0, 0, width, height);
  ctx.lineWidth = 1.75;
  ctx.lineCap = "round";
  for (let k = 0; k < size; k++) {
    const c = state.commits[top + k];
    const y0 = k * ROW;
    const ym = y0 + ROW / 2;
    const y1 = y0 + ROW;
    const l = c.lines;
    for (let n = 0; n < l.length; n += 4) {
      const [a, b, part] = [l[n], l[n + 1], l[n + 2]];
      if (a >= MAX_LANES && b >= MAX_LANES) continue;
      const [xa, xb] = [laneX(a), laneX(b)];
      const [ya, yb] = part === 0 ? [y0, ym] : part === 1 ? [ym, y1] : [y0, y1];
      ctx.strokeStyle = PALETTE[l[n + 3] % PALETTE.length];
      ctx.beginPath();
      ctx.moveTo(xa, ya);
      if (xa === xb) ctx.lineTo(xb, yb);
      else ctx.bezierCurveTo(xa, (ya + yb) / 2, xb, (ya + yb) / 2, xb, yb);
      ctx.stroke();
    }
  }
  for (let k = 0; k < size; k++) {
    const c = state.commits[top + k];
    if (c.lane >= MAX_LANES) continue;
    const x = laneX(c.lane);
    const y = k * ROW + ROW / 2;
    ctx.fillStyle = PALETTE[c.color % PALETTE.length];
    ctx.beginPath();
    ctx.arc(x, y, top + k === selected ? 5 : 4, 0, Math.PI * 2);
    ctx.fill();
    if (c.parents.length > 1) {
      // A merge is a ring, so it stands out from the commits on its lane.
      ctx.globalCompositeOperation = "destination-out";
      ctx.beginPath();
      ctx.arc(x, y, 2, 0, Math.PI * 2);
      ctx.fill();
      ctx.globalCompositeOperation = "source-over";
    }
  }
}

/** Moves the drawn rows to where the list is scrolled. Cheap when they're already there. @param {boolean} [force] */
function update(force = false) {
  frame = 0;
  const n = state.commits.length;
  const size = Math.min(n, Math.ceil(scroller.clientHeight / ROW) + OVERSCAN * 2);
  const top = Math.max(0, Math.min(n - size, Math.floor(scroller.scrollTop / ROW) - OVERSCAN));
  if (!force && top === first && size === pool.length) return;
  first = top;
  while (pool.length < size) makeRow();
  while (pool.length > size) pool.pop()?.parentElement?.remove();
  view.style.transform = `translateY(${top * ROW}px)`;
  for (let k = 0; k < size; k++) fill(pool[k], top + k);
  draw(top, size);
}

scroller.addEventListener("scroll", () => (frame ||= requestAnimationFrame(() => update())), { passive: true });
new ResizeObserver(() => update()).observe(scroller);

/** @param {number} i */
function openCommit(i) {
  const c = state.commits[i];
  if (!c || !state.root) return;
  selected = i;
  update(true);
  void wings.broadcast({ type: "commit", root: state.root, hash: c.hash, subject: c.subject, author: c.author, at: c.at });
}

list.addEventListener("click", (e) => {
  const b = /** @type {HTMLElement} */ (e.target).closest("button");
  if (b?.dataset.i) openCommit(Number(b.dataset.i));
});
// Arrow keys move between rows, scrolling the next one into view first, since it may not be drawn yet.
list.addEventListener("keydown", (e) => {
  if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
  const at = Number(/** @type {HTMLElement} */ (e.target).closest("button")?.dataset.i);
  if (Number.isNaN(at)) return;
  e.preventDefault();
  const i = Math.max(0, Math.min(state.commits.length - 1, at + (e.key === "ArrowDown" ? 1 : -1)));
  if (i * ROW < scroller.scrollTop) scroller.scrollTop = i * ROW;
  else if ((i + 1) * ROW > scroller.scrollTop + scroller.clientHeight) scroller.scrollTop = (i + 1) * ROW - scroller.clientHeight;
  update();
  pool.find((b) => b.dataset.i === String(i))?.focus();
});

// ---------- the header, and loading ----------

function renderHead() {
  const n = state.commits.length;
  const status = !state.root
    ? state.loading
      ? "Loading…"
      : "No git repo in your panes."
    : (state.error ?? (n === 0 ? (state.loading ? "Loading…" : "No commits yet.") : `${n.toLocaleString()} commits on every branch${n >= LIMIT ? `, the newest ${LIMIT.toLocaleString()}` : ""}`));
  head.replaceChildren(
    h("div", { class: "grow" }, h("div", { class: "repo ellipsis" }, state.name ?? "Commit graph"), h("div", { class: `ellipsis ${state.error ? "warn" : "muted"}` }, status)),
    state.loading ? h("span", { class: "spinner", "aria-label": "Loading" }) : "",
    h("button", { onclick: () => void load(), disabled: !state.root || state.loading }, "Refresh"),
  );
}

async function load() {
  const root = state.root;
  state = { ...state, loading: Boolean(root), error: undefined };
  renderHead();
  if (!root) return;
  try {
    const r = await wings.exec("git", ["log", "--all", "--topo-order", ...LOG, `--max-count=${LIMIT}`], { cwd: root });
    if (root !== state.root) return;
    if (r.code !== 0) throw new Error(r.stderr.trim().split("\n")[0] || "git log failed");
    const commits = parseLog(r.stdout);
    state = { ...state, loading: false, commits, lanes: layout(commits) };
    selected = Math.min(selected, commits.length - 1);
    spacer.style.height = `${commits.length * ROW}px`;
    list.style.setProperty("--graph", `${graphWidth()}px`);
    for (const b of pool) delete b.dataset.i;
    update(true);
  } catch (error) {
    if (root !== state.root) return;
    state = { ...state, loading: false, error: String(/** @type {Error} */ (error).message ?? error) };
  }
  renderHead();
}

wings.onBroadcast((message) => {
  const m = /** @type {any} */ (message);
  // A second Graph click while it loads would only load it again.
  if (m?.type !== "graph-repo" || (state.root && m.root === state.root && state.loading)) return;
  if (m.root !== state.root) {
    state = { root: m.root ?? null, name: m.name ?? undefined, loading: false, commits: [], lanes: 1 };
    selected = -1;
    scroller.scrollTop = 0;
    spacer.style.height = "0px";
    update(true);
  }
  void load();
});

renderHead();
void wings.broadcast({ type: "graph-hello" });
