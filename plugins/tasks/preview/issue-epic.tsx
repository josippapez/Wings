// An epic: child issues with progress.
import { IssueView } from "../src/ui/issue-view";
import "../src/pages/tasks.css";
import { epicIssue, issues, project } from "./_data";

const none = () => Promise.resolve();
export const fill = true;
export default <IssueView project={project} issues={issues} issue={epicIssue} issueKey="WEB-10" error={null} send={none} onBack={() => {}} onResume={() => {}} onStart={() => {}} onOpenUrl={() => {}} onOpenIssue={() => {}} />;
