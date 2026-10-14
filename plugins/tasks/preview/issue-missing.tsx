// The Issue page for a key the project doesn't have.
import { IssueView } from "../src/lib/issue-view";
import "../src/tasks.css";
import { issues, longIssue, project } from "./_data";

void issues;
void longIssue;
const none = () => Promise.resolve();
export const fill = true;
export default <IssueView project={project} issue={null} issueKey="WEB-99" error={null} send={none} onBack={() => {}} onResume={() => {}} onStart={() => {}} onOpenUrl={() => {}} />;
