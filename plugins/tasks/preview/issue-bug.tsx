// A bug under an epic, blocked by another issue and related to a third.
import { IssueView } from "../src/ui/issue-view";
import "../src/pages/tasks.css";
import { bugIssue, issues, project } from "./_data";

const none = () => Promise.resolve();
export const fill = true;
export default <IssueView project={project} issues={issues} issue={bugIssue} issueKey="WEB-2" error={null} send={none} onBack={() => {}} onResume={() => {}} onStart={() => {}} onOpenUrl={() => {}} onOpenIssue={() => {}} />;
