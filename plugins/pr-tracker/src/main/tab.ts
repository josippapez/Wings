/**
 * The PR tab's data. Main loads a tab's PR, its conversation and its pushes as queries, so every tab on one PR shares
 * one copy, and a pane on it has its badge kept fresh by the same load. The tab asks main for the rest.
 */
import { errorMessage } from "wings-plugin-sdk/async";
import { prKey } from "../lib/args";
import { forgeOf, isKnown, linkOf } from "../lib/forges/registry";
import { ch, type PrKey, type PrValue } from "../lib/messages";
import type { KnownRemote, Link, PrArgs } from "../lib/types";
import { render } from "./badges";
import { openDiffFor } from "./diff";
import { answered, explains, shownProblem } from "./problems";
import { markSeen } from "./projects";
import { remoteFor } from "./remote";
import { failures, known, linkFor, links, state, status } from "./state";
import { prefetch } from "./status";

/** A tab refreshes what it shows this often when no pane keeps its PR fresh. */
const REFETCH_MS = 120_000;

/** PR url → the project folder a tab opened it from, for the forges that diff or read files there. */
const folders = new Map<string, string>();

/** The PR's link: a pane's, else one a list or a tab found before, else asked of its forge. */
async function linkOfKey(key: PrKey): Promise<Link> {
  const found = linkFor(key.url);
  if (found) return found;
  const link = await linkOf({ ...key, title: "", cwd: folders.get(key.url) ?? "" });
  known.set(key.url, link);
  return link;
}

/** The link a tab's request is about. With no pane on the PR, in the folder of the project the tab came from, where the diff and the snippets are read. */
async function linkOfArgs(args: PrArgs): Promise<Link> {
  folders.set(args.url, args.cwd);
  const pane = [...links.values()].find((l) => l.url === args.url);
  if (pane) return pane;
  let link = known.get(args.url) ?? (await linkOf(args));
  if ("cwd" in link) link = { ...link, cwd: args.cwd };
  known.set(args.url, link);
  return link;
}

/** The host a tab's PR is on, for its problems: the args say for most forges, the project's folder for GitHub. */
async function remoteOfKey(key: PrKey): Promise<KnownRemote | null> {
  if (key.remote && isKnown(key.remote)) return key.remote;
  const folder = folders.get(key.url);
  const remote = folder ? await remoteFor(folder) : null;
  return remote && isKnown(remote) ? remote : null;
}

/** Runs a tab's load. A host that needs signing in, a token or a CLI is noted for the badges and comes back as `problem`. */
async function guarded<T>(key: PrKey, load: (link: Link) => Promise<T>): Promise<T | { problem: NonNullable<PrValue["problem"]> }> {
  const remote = await remoteOfKey(key);
  try {
    const value = await load(await linkOfKey(key));
    if (remote) answered(remote);
    return value;
  } catch (error) {
    if (remote && explains(remote, error)) render();
    const problem = shownProblem(remote ?? undefined);
    if (problem) return { problem };
    throw new Error(errorMessage(error));
  }
}

export function serveTab() {
  ch.serve(
    "pr",
    async (key): Promise<PrValue> => {
      const forge = forgeOf(key);
      const can = { approve: Boolean(forge.approve), vote: Boolean(forge.vote), snippet: Boolean(forge.snippet) };
      const loaded = await guarded(key, (link) => forge.overview(link));
      if ("problem" in loaded) return { can, problem: loaded.problem };
      // The overview carries the status, so a pane on this PR shows it, and the diff and failed checks are fetched ahead.
      status.set(key.url, loaded.status);
      render();
      prefetch(key.url);
      return { can, overview: loaded };
    },
    { refetchMs: REFETCH_MS },
  );
  ch.serve(
    "pr-comments",
    async (key) => {
      const loaded = await guarded(key, (link) => forgeOf(link).comments(link));
      return "problem" in loaded ? [] : loaded;
    },
    { refetchMs: REFETCH_MS },
  );
  ch.serve(
    "pr-pushes",
    async (key) => {
      const loaded = await guarded(key, async (link) => (await forgeOf(link).pushes?.(link)) ?? []);
      return "problem" in loaded ? [] : loaded;
    },
    { refetchMs: REFETCH_MS },
  );

  /** What only main can do: the diff viewer that takes replies, the agent on the branch, marking the PR read, and approving or voting. */
  ch.answer("pr", async ({ op, args, focus, sha, vote }) => {
    if (op === "seen") {
      folders.set(args.url, args.cwd);
      return markSeen(args.url);
    }
    const link = await linkOfArgs(args);
    if (op === "approve" || op === "vote") {
      const forge = forgeOf(link);
      if (op === "approve") await forge.approve!(link, sha!);
      else await forge.vote!(link, vote!);
      return ch.refresh("pr", prKey(args));
    }
    if (op === "diff") return openDiffFor(link, { title: args.title, focus });
    // The agent working on this PR's branch: Send to agent in the viewer offers the one in this folder first.
    const agent = state.panes.find((p) => p.session && links.get(p.paneId)?.url === args.url);
    if (!agent) throw new Error("No agent is working on this branch. Start Claude in a terminal on it, then try again.");
    return openDiffFor(link, { title: args.title, cwd: agent.cwd, focus });
  });

  /** A reply goes into the conversation every tab on the PR shows. */
  ch.answer("pr-reply", async ({ args, root, body }) => {
    const link = await linkOfArgs(args);
    const reply = await forgeOf(link).reply(link, root, body);
    const key = prKey(args);
    ch.put("pr-comments", key, [...(ch.peek("pr-comments", key)?.data ?? []), reply]);
    return reply;
  });

  ch.answer("pr-snippet", async ({ args, root, head, now }) => {
    const link = await linkOfArgs(args);
    const pushes = ch.peek("pr-pushes", prKey(args))?.data ?? [];
    return (await forgeOf(link).snippet?.(link, root, pushes, head, now)) ?? null;
  });

  ch.answer("pr-run", async ({ args, run }) => {
    const link = await linkOfArgs(args);
    const forge = forgeOf(link);
    if (!forge.run) throw new Error("This provider's CI has no run to show here.");
    return forge.run(link, run);
  });

  ch.answer("pr-run-log", async ({ args, run, step }) => {
    const link = await linkOfArgs(args);
    return forgeOf(link).runLog!(link, run, step);
  });

  /** A failed check's details: the ones the badge fetched ahead, else its own load. */
  ch.answer("pr-failure", async ({ args, failing }) => {
    const ahead = failures.get(args.url)?.byId.get(failing.id);
    if (ahead) return ahead;
    const link = await linkOfArgs(args);
    return forgeOf(link).failure(link, failing);
  });
}
