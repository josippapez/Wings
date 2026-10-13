// Azure DevOps web links, and the shape of a work item's update entries. No `wings`, no I/O, so node tests drive it.

interface Identity {
  id?: string;
  displayName?: string;
}

/** The URL prefix the manifest's `openUrl` allows: a visualstudio.com org also answers at dev.azure.com. */
export function webBase(org: string) {
  return org.replace(/^https:\/\/([^.]+)\.visualstudio\.com$/, "https://dev.azure.com/$1");
}

export const workItemUrl = (org: string, project: string, id: number) => `${webBase(org)}/${encodeURIComponent(project)}/_workitems/edit/${id}`;

export const prUrl = (org: string, project: string, repo: string, id: number, thread?: number) =>
  `${webBase(org)}/${encodeURIComponent(project)}/_git/${encodeURIComponent(repo)}/pullrequest/${id}${thread ? `?_a=overview&discussionId=${thread}` : ""}`;

/** One entry of `GET wit/workItems/{id}/updates`. The newest one's `revisedDate` is 9999-01-01, so the time is its ChangedDate field. */
export interface WorkItemUpdate {
  id: number;
  revisedBy?: Identity;
  revisedDate?: string;
  fields?: Record<string, { oldValue?: unknown; newValue?: unknown } | undefined>;
}

export function updateTime(u: WorkItemUpdate): string {
  const changed = u.fields?.["System.ChangedDate"]?.newValue;
  return typeof changed === "string" ? changed : (u.revisedDate ?? "");
}
