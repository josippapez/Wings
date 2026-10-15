// The commit graph: every branch's commits with their lanes, like `git log --graph --all`. It runs git itself,
// since the history is far larger than one answer, lays the lanes out once, and only draws the rows on screen.

import { errorMessage, latestOnly } from "wings-plugin-sdk/async";
import { git } from "wings-plugin-sdk/git";
import { mountShell } from "wings-plugin-sdk/shell";

import { h } from "../ui/graph-dom";
import { draw, graphWidth, ROW } from "../ui/graph-draw";
import { fill, makeRow } from "../ui/graph-rows";
import { layout, type Placed } from "../lib/graph-layout";
import { LOG, parseLog } from "../lib/log";
import { ch } from "../lib/messages";
import "./graph.css";

/** Rows drawn above and below the screen, so a fast scroll doesn't show gaps. */
const OVERSCAN = 12;
const LIMIT = 10_000;

// Wings gives the page its frame; the graph lays itself out edge to edge inside it.
const head = h("header", { id: "head", class: "w-row", "aria-live": "polite" });
const canvas = h("canvas", { id: "lanes", "aria-hidden": "true" }) as HTMLCanvasElement;
const list = h("ul", { id: "rows", "aria-label": "Commits" });
const view = h("div", { id: "window" }, canvas, list);
const spacer = h("div", { id: "spacer" }, view);
const scroller = h("div", { id: "scroller", tabindex: "-1" }, spacer);
mountShell("view", { fill: true }).replaceChildren(head, scroller);
const ctx = canvas.getContext("2d")!;

let state: { root: string | null; name?: string; loading: boolean; error?: string; commits: Placed[]; lanes: number } = { root: null, loading: true, commits: [], lanes: 1 };
let selected = -1;

// ---------- drawing ----------

/** The row buttons on screen, reused as you scroll, and the first row they show. */
const pool: HTMLButtonElement[] = [];
let first = -1;
let frame = 0;

/** Moves the drawn rows to where the list is scrolled. Cheap when they're already there. */
function update(force = false) {
  frame = 0;
  const n = state.commits.length;
  const size = Math.min(n, Math.ceil(scroller.clientHeight / ROW) + OVERSCAN * 2);
  const top = Math.max(0, Math.min(n - size, Math.floor(scroller.scrollTop / ROW) - OVERSCAN));
  if (!force && top === first && size === pool.length) return;
  first = top;
  while (pool.length < size) {
    const { b, li } = makeRow();
    pool.push(b);
    list.append(li);
  }
  while (pool.length > size) pool.pop()?.parentElement?.remove();
  view.style.transform = `translateY(${top * ROW}px)`;
  for (let k = 0; k < size; k++) fill(pool[k], top + k, state.commits[top + k], selected);
  draw(canvas, ctx, state.commits, state.lanes, selected, top, size);
}

scroller.addEventListener("scroll", () => (frame ||= requestAnimationFrame(() => update())), { passive: true });
new ResizeObserver(() => update()).observe(scroller);

function openCommit(i: number) {
  const c = state.commits[i];
  if (!c || !state.root) return;
  selected = i;
  update(true);
  void ch.ask("commit", { root: state.root, hash: c.hash, subject: c.subject, author: c.author, at: c.at });
}

list.addEventListener("click", (e) => {
  const b = (e.target as HTMLElement).closest("button");
  if (b?.dataset.i) openCommit(Number(b.dataset.i));
});
// Arrow keys move between rows, scrolling the next one into view first, since it may not be drawn yet.
list.addEventListener("keydown", (e) => {
  if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
  const at = Number((e.target as HTMLElement).closest("button")?.dataset.i);
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
  const status =
    state.error ??
    (!state.root
      ? state.loading
        ? "Loading…"
        : "No git repo in your panes."
      : n === 0
        ? state.loading
          ? "Loading…"
          : "No commits yet."
        : `${n.toLocaleString()} commits on every branch${n >= LIMIT ? `, the newest ${LIMIT.toLocaleString()}` : ""}`);
  head.replaceChildren(
    h(
      "div",
      { class: "w-stack", "data-gap": "none", "data-grow": true },
      h("div", { class: "w-text", "data-weight": "strong", "data-truncate": true }, state.name ?? "Commit graph"),
      h("div", { class: "w-text", "data-tone": state.error ? "warn" : "muted", "data-truncate": true }, status),
    ),
    state.loading ? h("span", { class: "w-spinner", role: "img", "aria-label": "Loading" }) : "",
    h("button", { onclick: () => void load(), disabled: !state.root || state.loading }, "Refresh"),
  );
}

const nextLoad = latestOnly();

async function load() {
  const root = state.root;
  const stale = nextLoad();
  state = { ...state, loading: Boolean(root), error: undefined };
  renderHead();
  if (!root) return;
  try {
    const out = await git(root, ["log", "--all", "--topo-order", ...LOG, `--max-count=${LIMIT}`], { raw: true });
    if (stale()) return;
    const { rows, lanes } = layout(parseLog(out));
    state = { ...state, loading: false, commits: rows, lanes };
    selected = Math.min(selected, rows.length - 1);
    spacer.style.height = `${rows.length * ROW}px`;
    list.style.setProperty("--graph", `${graphWidth(lanes)}px`);
    for (const b of pool) delete b.dataset.i;
    update(true);
  } catch (error) {
    if (stale()) return;
    state = { ...state, loading: false, error: errorMessage(error) };
  }
  renderHead();
}

/** Asks main which repo to show, and loads it. */
async function show() {
  let repo;
  try {
    repo = await ch.ask("graph-repo");
  } catch (error) {
    state = { ...state, loading: false, error: errorMessage(error) };
    return renderHead();
  }
  // A second Graph click while it loads would only load it again.
  if (state.root && repo.root === state.root && state.loading) return;
  if (repo.root !== state.root) {
    state = { root: repo.root, name: repo.name ?? undefined, loading: false, commits: [], lanes: 1 };
    selected = -1;
    scroller.scrollTop = 0;
    spacer.style.height = "0px";
    update(true);
  }
  void load();
}

// Main tells when the Graph button was clicked again, maybe for another repo.
ch.on("graph-repo-changed", () => void show());
renderHead();
void show();
