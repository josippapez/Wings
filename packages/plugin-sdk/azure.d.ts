/// <reference path="./wings.d.ts" />
import type { Batched } from "./async.js";
import type { Cli } from "./cli.js";

/** The most ids one `az rest` call asks for. */
export const WORK_ITEMS_MAX: 200;

/** A work item as Azure DevOps' REST API gives it, with its relations. `F` is the shape of the fields a plugin reads, like `{ "System.Title": string }`. */
export interface WorkItem<F = Record<string, any>> {
  id: number;
  rev?: number;
  fields: F;
  relations?: { rel?: string; url?: string; attributes?: Record<string, unknown> }[] | null;
  url?: string;
}

/**
 * Work items of an Azure DevOps organization, with their relations, loaded through `az rest`: one call for every id
 * asked in the same microtask turn, 200 ids at most. `az` is the plugin's `cli("az", ...)`; `org` is the `org` of a
 * parsed Azure remote. A call goes out under `org` as the CLI's sign-in key, unless `quiet`. `get(id)` resolves the
 * item, or null for an id that doesn't exist or that you can't see (`errorPolicy=omit`).
 */
export function workItems<F = Record<string, any>>(az: Cli, org: string, options?: { quiet?: boolean }): Batched<number, WorkItem<F> | null>;
