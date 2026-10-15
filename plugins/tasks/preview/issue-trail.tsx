// The Issue page opened from its epic and then its feature: the breadcrumb leads back through both.
import { IssueView } from "../src/ui/issue-view";
import "../src/pages/tasks.css";
import { issues, project } from "./_data";

const none = () => Promise.resolve();
const issue = issues.find((i) => i.key === "WEB-12")!;
export const fill = true;
export default <IssueView project={project} issues={issues} issue={issue} issueKey="WEB-12" error={null} send={none} onBack={() => {}} trail={["WEB-10", "WEB-4"]} onBackTo={() => {}} onResume={() => {}} onStart={() => {}} onOpenUrl={() => {}} onOpenIssue={() => {}} />;
