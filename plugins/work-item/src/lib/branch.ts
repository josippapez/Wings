/** Dates and versions in a branch name (2026-10-13, 2026.10, v1.2.3): their numbers aren't work item ids. A date starts with a 19xx or 20xx year, so `feature/1234-2-login` keeps its id. */
export const DATE_OR_VERSION = /(?:19|20)\d{2}[-./]\d{1,2}(?:[-./]\d{1,2})?|v?\d+(?:\.\d+)+/gi;

/** The first run of 3+ digits that is a whole path segment or followed by `-`: `feature/1234-login` → 1234. */
export function workItemId(branch: string) {
  return branch.replace(DATE_OR_VERSION, " ").match(/(?:^|[/_-])(\d{3,})(?=-|\/|$)/)?.[1] ?? null;
}
