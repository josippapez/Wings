// The Issue page for an issue with nothing filled in: no description, links or sessions, one log entry.
import { IssueView } from "../src/ui/issue-view";
import "../src/pages/tasks.css";
import { emptyIssue, issues, project } from "./_data";

const none = () => Promise.resolve();
export const fill = true;
export default <IssueView project={project} issues={issues} issue={emptyIssue} issueKey="WEB-11" error={null} send={none} onBack={() => {}} onResume={() => {}} onStart={() => {}} onOpenUrl={() => {}} onOpenIssue={() => {}} />;
