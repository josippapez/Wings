// The Issue page with a long log, three sessions and links.
import { IssueView } from "../src/ui/issue-view";
import "../src/pages/tasks.css";
import { issues, longIssue, project } from "./_data";

void issues;
void longIssue;
const none = () => Promise.resolve();
export const fill = true;
export default <IssueView project={project} issues={issues} issue={longIssue} issueKey="WEB-4" error={null} send={none} onBack={() => {}} onResume={() => {}} onStart={() => {}} onOpenUrl={() => {}} onOpenIssue={() => {}} />;
