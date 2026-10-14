// The Issue page with an error from the last change.
import { IssueView } from "../src/lib/issue-view";
import "../src/tasks.css";
import { issues, longIssue, project } from "./_data";

void issues;
void longIssue;
const none = () => Promise.resolve();
export const fill = true;
export default <IssueView project={project} issue={issues[1]} issueKey="WEB-2" error={"A project holds up to 300 issues. Remove one first."} send={none} onBack={() => {}} onResume={() => {}} onStart={() => {}} onOpenUrl={() => {}} />;
