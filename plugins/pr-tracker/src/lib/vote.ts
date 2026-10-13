/** Azure DevOps votes through `az repos pr set-vote`. No `wings`, no I/O, so node tests drive it. */

/** The votes `az repos pr set-vote --vote` takes, as Azure's own UI words them. */
export const AZURE_VOTES = {
  approve: "Approve",
  "approve-with-suggestions": "Approve with suggestions",
  "wait-for-author": "Wait for author",
  reject: "Reject",
  reset: "Reset vote",
} as const;

export type AzureVote = keyof typeof AZURE_VOTES;

/** The organization URLs the tracker parses a remote into. */
const ORG = /^https:\/\/(dev\.azure\.com\/[A-Za-z0-9._-]+|[A-Za-z0-9-]+\.visualstudio\.com)$/;

/** The arguments of one vote. The manifest can't pin `--vote`'s value, so the vote, the PR id and the organization are checked here before `az` runs. */
export function voteArgs(number: number, org: string, vote: string): string[] {
  if (!Number.isSafeInteger(number) || number < 1) throw new Error(`Not a pull request id: ${number}`);
  if (!ORG.test(org)) throw new Error("Not an Azure DevOps organization URL.");
  if (!Object.hasOwn(AZURE_VOTES, vote)) throw new Error(`Not an Azure DevOps vote: ${vote}`);
  return ["repos", "pr", "set-vote", "--id", String(number), "--vote", vote, "--organization", org, "-o", "json"];
}
