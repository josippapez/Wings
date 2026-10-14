// The Issue page for a new issue: no links, no sessions, one log entry.
import { IssueView } from "../src/lib/issue-view";
import "../src/tasks.css";
import { issues, longIssue, project } from "./_data";

void issues;
void longIssue;
const none = () => Promise.resolve();
export const fill = true;
export default <IssueView project={project} issues={issues} issue={issues[0]} issueKey="WEB-1" error={null} send={none} onBack={() => {}} onResume={() => {}} onStart={() => {}} onOpenUrl={() => {}} onOpenIssue={() => {}} />;
