// The Azure CLI as this plugin calls it. A frame has one instance, so the main script keeps the sign-in problems its
// badges show, and the item tab has its own, which nothing reads.

import { workItems, type WorkItem } from "wings-plugin-sdk/azure";
import { cli } from "wings-plugin-sdk/cli";

export const azCli = cli("az", { signedOut: /login/i, signIn: ["login", "--allow-no-subscriptions", "--output", "none"] });

const forOrg = (org: string, args: string[]) => [...args, "--organization", org, "-o", "json"];

/**
 * Runs `az args` for the organization `org` and resolves what it printed, parsed. Nothing printed, which is what
 * `az boards query` does for no rows, gives `empty`, and is an error when there is none. A failure throws a `CliError`
 * with the last line of stderr. A call records "not installed" and "signed out" for the pane badges, unless it is
 * `quiet`, a background read that must not turn them into a sign-in prompt.
 */
export const az = <T = unknown>(org: string, args: string[], { quiet = false, ...rest }: { quiet?: boolean; empty?: T } = {}) => azCli.json<T>(forOrg(org, args), { key: org, quiet, ...rest });

/** `az` for a command whose output isn't read, like an update: nothing is parsed. */
export const azRun = (org: string, args: string[], { quiet = false } = {}) => azCli.run(forOrg(org, args), { key: org, quiet });

/** The arguments of `az devops invoke`, a call of Azure DevOps' REST API. `query` are the URL's query parameters, and `jmes` cuts the answer with a JMESPath. */
export const invoke = (area: string, resource: string, route: Record<string, string | number>, { query = [], jmes }: { query?: string[]; jmes?: string } = {}) => [
  "devops", "invoke", "--area", area, "--resource", resource,
  "--route-parameters", ...Object.entries(route).map(([name, value]) => `${name}=${value}`),
  ...(query.length ? ["--query-parameters", ...query] : []),
  "--api-version", "7.1",
  ...(jmes ? ["--query", jmes] : []),
];

/** `${org} ${quiet}` → the loader of that organization's work items. Each caller names the fields it reads, so the loaders keep any. */
const loaders = new Map<string, ReturnType<typeof workItems<any>>>();

/**
 * One work item with its relations, or null when there is none or you can't see it. Every item asked in the same
 * microtask turn is one `az rest` call, so a badge per pane, a parent title and the Work items section cost one process
 * between them, not one each. `quiet` is for a background read, as in `az`. This replaces `az boards work-item show` and
 * `az devops invoke --resource workItems`, which the installed azure-cli fails on (`KeyError: 'type'`).
 */
export function workItem<F>(org: string, id: number | string, { quiet = false } = {}): Promise<WorkItem<F> | null> {
  const key = `${org} ${quiet}`;
  let loader = loaders.get(key);
  if (!loader) loaders.set(key, (loader = workItems(azCli, org, { quiet })));
  return loader.get(Number(id));
}
