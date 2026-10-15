// The Issue page: a long log, a short one, an epic, a bug with links, each way it can load or fail, and as the page runs it.
import IssuePage, { page } from "../src/pages/issue";
import { IssueView, type IssueViewProps } from "../src/ui/issue-view";
import { bugIssue, emptyIssue, epicIssue, issues, longIssue, project, shortIssue, storage } from "./_data";

export { page };

const noop = () => {};
/** The Issue page with every action doing nothing: a state says only which issue it is about. */
const view = (props: Pick<IssueViewProps, "issue" | "issueKey"> & Partial<IssueViewProps>) => (
  <IssueView project={project} issues={issues} error={null} send={() => Promise.resolve()} onBack={noop} onResume={noop} onStart={noop} onOpenUrl={noop} onOpenIssue={noop} {...props} />
);

export const states = {
  // A long log, three sessions and links.
  long: { element: view({ issue: longIssue, issueKey: "WEB-4" }) },
  // A new issue: no links, no sessions, one log entry.
  short: { element: view({ issue: shortIssue, issueKey: "WEB-1" }) },
  // An issue with nothing filled in: no description, links or sessions, one log entry.
  empty: { element: view({ issue: emptyIssue, issueKey: "WEB-11" }) },
  // An epic: child issues with progress.
  epic: { element: view({ issue: epicIssue, issueKey: "WEB-10" }) },
  // A bug under an epic, blocked by another issue and related to a third.
  bug: { element: view({ issue: bugIssue, issueKey: "WEB-2" }) },
  // Opened from its epic and then its feature: the breadcrumb leads back through both.
  trail: { element: view({ issue: issues.find((i) => i.key === "WEB-12")!, issueKey: "WEB-12", trail: ["WEB-10", "WEB-4"], onBackTo: noop }) },
  // An error from the last change.
  error: { element: view({ issue: bugIssue, issueKey: "WEB-2", error: "A project holds up to 300 issues. Remove one first." }) },
  // While it loads.
  loading: { element: view({ issue: undefined, issueKey: "WEB-4" }) },
  // A key the project doesn't have.
  missing: { element: view({ issue: null, issueKey: "WEB-99" }) },
  // As the page runs it: the issue read from storage, its key and the trail behind it from the tab's args.
  page: { element: <IssuePage />, project, args: { key: "WEB-12", projectId: "p1", trail: ["WEB-10", "WEB-4"] }, storage },
};
