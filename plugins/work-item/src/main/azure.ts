// What the badge, the sections and the Sprint block each need to know about a folder's Azure DevOps project.

import { cached } from "wings-plugin-sdk/async";
import { gitOk, parseRemote, type Remote } from "wings-plugin-sdk/git";
import { az, invoke } from "../lib/az";
import { currentIterations, type IterationNode } from "../lib/sections";

export type AzureRemote = Extract<Remote, { kind: "azure" }>;
/** What names an Azure DevOps project: an `AzureRemote` has it, and more. */
export type Project = Pick<AzureRemote, "org" | "project">;

const SPRINT_TTL_MS = 3_600_000;

const remotes = cached(async (cwd: string): Promise<AzureRemote | null> => {
  const remote = parseRemote((await gitOk(cwd, ["remote", "get-url", "origin"])) ?? "");
  return remote.kind === "azure" ? remote : null;
});

/** The Azure DevOps remote of the folder's origin, or null when it isn't one. A folder is read once. */
export const remoteFor = (cwd: string) => remotes.get(cwd);

const sprints = cached(
  async ({ org, project }: Project) => currentIterations(await az<IterationNode>(org, invoke("wit", "classificationNodes", { project, structureGroup: "iterations" }, { query: ["$depth=4"] }), { quiet: true, empty: {} }), Date.now()),
  { ttlMs: SPRINT_TTL_MS, key: JSON.stringify },
);

/** The paths of the sprints running now in an Azure project, read at most once an hour. */
export const sprintPaths = ({ org, project }: Project) => sprints.get({ org, project });
