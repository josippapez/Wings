/**
 * Tells the agent on a PR's branch when its PR gets news (see lib/news.ts). It polls only the PRs that have an agent
 * pane, every 2 minutes, in the background too: the forge can't call a local app, so the news has to be asked for.
 */
import { mapLimit } from "wings-plugin-sdk/async";
import { forgeFor, forgeOf, isKnown } from "../lib/forges/registry";
import { commentsDue, diffNews, FORGET_MS, hasNews, failuresOf, messageFor, settled, tellAgents, underCap, type PrNews } from "../lib/news";
import type { Link, Status } from "../lib/types";
import { remoteFor } from "./remote";
import { links, state, status } from "./state";
import { loadFailures, refreshStatus } from "./status";

const KEY = "agent-news";
export const NEWS_MS = 120_000;
/** Watched PRs read at once: a forge CLI call takes a second or two. */
const PARALLEL = 3;

let memory: Record<string, PrNews> = {};
let persisted = "";
let loaded: Promise<void> | undefined;
const load = () =>
  (loaded ??= wings.storage.get(KEY).then((saved) => {
    memory = saved && typeof saved === "object" && !Array.isArray(saved) ? (saved as Record<string, PrNews>) : {};
    persisted = JSON.stringify(memory);
  }));

function save() {
  const json = JSON.stringify(memory);
  if (json !== persisted) void wings.storage.set(KEY, JSON.parse(json));
  persisted = json;
}

/** The PRs with an agent in a pane on their branch, each with those panes. */
function watched() {
  const byUrl = new Map<string, { link: Link; panes: WingsPane[] }>();
  for (const pane of state.panes) {
    const link = links.get(pane.paneId);
    if (!pane.session || !link) continue;
    const entry = byUrl.get(link.url) ?? { link, panes: [] };
    entry.panes.push(pane);
    byUrl.set(link.url, entry);
  }
  return byUrl;
}

/** Who you are on the PR's forge, which tells your comments from the others. */
async function meOf(link: Link, cwd: string) {
  const remote = "remote" in link ? link.remote : await remoteFor(cwd);
  return isKnown(remote) ? forgeFor(remote).me(remote) : null;
}

async function look(url: string, { link, panes }: { link: Link; panes: WingsPane[] }) {
  // Focused, the pane poll keeps the status fresh; in the background nothing does, so it is read here.
  if (!wings.focused) await refreshStatus(url);
  const entry = status.get(url);
  if (!entry || entry.error !== undefined) return;
  const st = entry as Status;
  if (st.icon === "pr-merged" || st.icon === "pr-closed") return;
  const now = Date.now();
  const prev = memory[url];
  const known = prev && now - prev.watched <= FORGET_MS ? prev : undefined;
  const stamp = `${st.key}|${st.updated}`;
  let comments = null;
  let me = null;
  if (commentsDue(known, link, stamp)) {
    try {
      [me, comments] = await Promise.all([meOf(link, panes[0].cwd), forgeOf(link).comments(link)]);
    } catch (error) {
      console.error(error);
    }
  }
  // Without its comments a first look would count every old one as new on the next.
  if (!known && !(comments && me)) return;
  const { next, found } = diffNews(known, { failing: st.failing, conflict: st.label === "Conflicts", comments, stamp }, me, now);
  if (!hasNews(found)) {
    memory[url] = next;
    return;
  }
  if (!underCap(next)) {
    memory[url] = settled(next, "dropped", now);
    return;
  }
  const details = found.checks.length ? await loadFailures(url).catch(() => []) : [];
  const text = messageFor({ link, title: st.title, base: st.base ?? st.branch.split(" → ")[1] }, found, failuresOf(found.checks, details));
  for (const pane of panes) {
    try {
      await wings.sendToAgent(pane.paneId, text);
      memory[url] = settled(next, "sent", now);
      return;
    } catch {
      // No agent there any more, on a permission prompt, or text typed and unsent: the next look tries again, and the news is still new then.
    }
  }
}

let running = false;
async function tick() {
  if (running || !(await tellAgents.get())) return;
  running = true;
  try {
    await load();
    const now = Date.now();
    const open = watched();
    await mapLimit([...open], PARALLEL, ([url, w]) => look(url, w).catch(console.error));
    for (const [url, news] of Object.entries(memory)) if (!open.has(url) && now - news.watched > FORGET_MS) delete memory[url];
    save();
  } finally {
    running = false;
  }
}

let timer: ReturnType<typeof setInterval> | undefined;
function arm(on: boolean) {
  clearInterval(timer);
  timer = on ? setInterval(() => void tick(), NEWS_MS) : undefined;
  if (on) void tick();
}

/** Starts and stops the checks with the setting. */
export function startNews() {
  void tellAgents.get().then(arm);
  tellAgents.onChange(arm);
}
