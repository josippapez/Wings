// The Issue page with an error from the last change.
import { IssueView } from "../src/ui/issue-view";
import "../src/pages/tasks.css";
import { issues, longIssue, project } from "./_data";

void issues;
void longIssue;
const none = () => Promise.resolve();
export const fill = true;
export default <IssueView project={project} issues={issues} issue={issues[1]} issueKey="WEB-2" error={"A project holds up to 300 issues. Remove one first."} send={none} onBack={() => {}} onResume={() => {}} onStart={() => {}} onOpenUrl={() => {}} onOpenIssue={() => {}} />;
