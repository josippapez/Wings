// Azure DevOps reads that one `az` call answers for many ids, so plugins that look at several work items start one
// process, not one each. The plugin lists `az rest [--resource --url -o]` in `permissions.exec`.
import { batched } from "./async.js";
import { azureApiOrg } from "./git.js";

/** The resource `az rest` gets a token for when it calls Azure DevOps. */
const DEVOPS_RESOURCE = "499b84ac-1321-427f-aa17-267ca6975798";
/** "Maximum 200 ids allowed" (learn.microsoft.com/en-us/rest/api/azure/devops/wit/work-items/list). */
export const WORK_ITEMS_MAX = 200;

/**
 * Work items of an Azure DevOps organization, with their relations, loaded through `az rest`: one call for every id
 * asked in the same microtask turn, 200 ids at most. `az` is the plugin's `cli("az", ...)`; `org` is the `org` of a
 * parsed Azure remote. A call goes out under `org` as the CLI's sign-in key, unless `quiet`. `get(id)` resolves the
 * item, or null for an id that doesn't exist or that you can't see (`errorPolicy=omit`).
 */
export function workItems(az, org, { quiet = false } = {}) {
  return batched(
    async (ids) => {
      // `$expand=relations` can't go with `fields`, so an item comes whole.
      const url = `${azureApiOrg(org)}/_apis/wit/workitems?ids=${ids.join(",")}&$expand=relations&errorPolicy=omit&api-version=7.1`;
      const { value } = await az.json(["rest", "--resource", DEVOPS_RESOURCE, "--url", url, "-o", "json"], { key: org, quiet });
      const byId = new Map(value.filter(Boolean).map((w) => [w.id, w]));
      return ids.map((id) => byId.get(Number(id)) ?? null);
    },
    { max: WORK_ITEMS_MAX },
  );
}
