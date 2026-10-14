// The Issue page for an issue with nothing filled in: no description, links or sessions, one log entry.
import { IssueView } from "../src/lib/issue-view";
import "../src/tasks.css";
import { emptyIssue, project } from "./_data";

const none = () => Promise.resolve();
export const fill = true;
export default <IssueView project={project} issue={emptyIssue} issueKey="WEB-10" error={null} send={none} onBack={() => {}} onResume={() => {}} onStart={() => {}} onOpenUrl={() => {}} />;
