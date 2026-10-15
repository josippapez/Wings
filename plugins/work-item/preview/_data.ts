// Made-up Azure Boards data for the fixtures in this folder, shaped like what item.tsx and sprint.tsx load.
import { commentsOf, discussionOf, revisionsOf, type ItemArgs, type LinkedItem } from "../src/lib/item";
import type { SprintResponse } from "../src/lib/sections";
import type { WorkItemUpdate } from "../src/lib/urls";
import type { ItemViewProps, Shown } from "../src/ui/item-view";

const ago = (minutes: number) => new Date(Date.now() - minutes * 60_000).toISOString();
const who = (displayName: string) => ({ displayName });

export const args: ItemArgs = { id: 1432, org: "https://dev.azure.com/acme" };

export const item: Shown = {
  id: 1432,
  fields: {
    "System.Title": "Sync client gives up after the first failed request",
    "System.TeamProject": "Web",
    "System.WorkItemType": "Bug",
    "System.State": "Active",
    "System.AreaPath": "Web\\Platform",
    "System.IterationPath": "Web\\Sprint 14",
    "System.Parent": 1400,
    "System.AssignedTo": who("Ada Lovelace"),
    "System.CreatedBy": who("Grace Hopper"),
    "System.CreatedDate": ago(60 * 24 * 6),
    "System.ChangedBy": who("Ada Lovelace"),
    "System.ChangedDate": ago(35),
    "System.Tags": "sync; reliability; customer-reported",
    "System.Description":
      "<p>When the server answers <code>503</code> the sync client stops and the user sees stale data until they reload.</p><ul><li>Reproduces on the staging server</li><li>Does not happen on a clean network</li></ul><p>Expected: the client retries with a growing delay.</p>",
    "Microsoft.VSTS.TCM.ReproSteps": "<ol><li>Start the web app</li><li>Stop the API for ten seconds</li><li>Start it again and watch the list</li></ol>",
    "Microsoft.VSTS.Common.AcceptanceCriteria": "<p>Five retries, doubling from 200 ms, and the user sees a notice while it retries.</p>",
    "Microsoft.VSTS.Common.Priority": 2,
    "Microsoft.VSTS.Common.Severity": "2 - High",
    "Microsoft.VSTS.Scheduling.RemainingWork": 6,
    "Microsoft.VSTS.Scheduling.CompletedWork": 10,
  },
  relations: [
    { rel: "System.LinkTypes.Hierarchy-Reverse", url: "https://dev.azure.com/acme/_apis/wit/workItems/1400" },
    { rel: "System.LinkTypes.Hierarchy-Forward", url: "https://dev.azure.com/acme/_apis/wit/workItems/1433" },
    { rel: "System.LinkTypes.Hierarchy-Forward", url: "https://dev.azure.com/acme/_apis/wit/workItems/1434" },
    { rel: "System.LinkTypes.Related", url: "https://dev.azure.com/acme/_apis/wit/workItems/1390" },
    { rel: "ArtifactLink", url: "vstfs:///Git/PullRequestId/p%2Fr1%2F482", attributes: { name: "Pull Request" } },
  ],
};

export const states = [
  { name: "New", category: "Proposed" },
  { name: "Active", category: "InProgress" },
  { name: "Resolved", category: "Resolved" },
  { name: "Closed", category: "Completed" },
];

export const linkedItems: Record<number, LinkedItem> = {
  1400: { id: 1400, title: "Make the sync client reliable", state: "Active", type: "Feature", assignee: "Grace Hopper" },
  1433: { id: 1433, title: "Add retry with backoff", state: "Active", type: "Task", assignee: "Ada Lovelace" },
  1434: { id: 1434, title: "Show a notice while retrying", state: "New", type: "Task" },
  1390: { id: 1390, title: "Offline banner flickers", state: "Closed", type: "Bug", assignee: "Alan Turing" },
};

const entries: WorkItemUpdate[] = [
  { id: 1, revisedBy: who("Grace Hopper"), fields: { "System.ChangedDate": { newValue: ago(60 * 24 * 6) }, "System.State": { newValue: "New" }, "System.Title": { newValue: item.fields["System.Title"] } } },
  { id: 2, revisedBy: who("Grace Hopper"), fields: { "System.ChangedDate": { newValue: ago(60 * 24 * 5) }, "System.AssignedTo": { newValue: who("Ada Lovelace") }, "System.IterationPath": { oldValue: "Web", newValue: "Web\\Sprint 14" } } },
  { id: 3, revisedBy: who("Ada Lovelace"), fields: { "System.ChangedDate": { newValue: ago(60 * 24 * 4) }, "System.State": { oldValue: "New", newValue: "Active" }, "System.History": { newValue: "<p>Reproduced it. The client drops the request queue on the first error.</p>" } } },
  { id: 4, revisedBy: who("Alan Turing"), fields: { "System.ChangedDate": { newValue: ago(60 * 20) }, "System.History": { newValue: "<p>Should the delay have jitter? A burst of clients retrying together is how we took the API down last time.</p>" } } },
  { id: 5, revisedBy: who("Ada Lovelace"), fields: { "System.ChangedDate": { newValue: ago(35) }, "Microsoft.VSTS.Scheduling.RemainingWork": { oldValue: 10, newValue: 6 }, "System.History": { newValue: "<p>Yes, equal jitter. Pull request is up.</p>" } } },
];
const comments = commentsOf(entries);
const revisions = revisionsOf(entries);

export const props: ItemViewProps = {
  args,
  item,
  states,
  linked: { data: linkedItems, error: null },
  updates: { data: { comments, revisions, feed: discussionOf(comments, revisions) }, error: null },
  prs: [{ id: 482, repoId: "r1", title: "Add retry with backoff to the sync client", status: "active", repo: "web" }],
  moving: null,
  moveError: null,
  onMove: () => {},
  onRefresh: () => {},
  onOpenItem: () => {},
  onOpenUrl: () => {},
  prUrl: () => "https://dev.azure.com/acme/Web/_git/web/pullrequest/482",
  workItemUrl: "https://dev.azure.com/acme/Web/_workitems/edit/1432",
};

export const sprint: SprintResponse = {
  type: "sprint-response",
  rid: "",
  org: "https://dev.azure.com/acme",
  project: "Web",
  sprints: ["Sprint 14"],
  total: 9,
  items: [
    { id: 1432, title: "Sync client gives up after the first failed request", type: "Bug", state: "Active", assignee: "Ada Lovelace" },
    { id: 1433, title: "Add retry with backoff", type: "Task", state: "Active", assignee: "Ada Lovelace" },
    { id: 1434, title: "Show a notice while retrying", type: "Task", state: "New", assignee: "Grace Hopper" },
    { id: 1440, title: "Settings page uses the new form components", type: "Product Backlog Item", state: "New" },
    { id: 1441, title: "Date picker overflows on small screens", type: "Bug", state: "Resolved", assignee: "Alan Turing" },
    { id: 1442, title: "Upgrade the build toolchain", type: "Task", state: "Closed", assignee: "Edsger Dijkstra" },
    { id: 1443, title: "Write the sync retry runbook", type: "Task", state: "Closed", assignee: "Ada Lovelace" },
  ],
};
