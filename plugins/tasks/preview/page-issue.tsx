// The Issue page as it runs: the issue read from storage, its key and the trail behind it from the tab's args.
import IssuePage from "../src/pages/issue";
import { seedStorage } from "./_data";

export { project } from "./_data";

export const fill = true;
export const args = { key: "WEB-12", projectId: "p1", trail: ["WEB-10", "WEB-4"] };
export default function Page() {
  seedStorage();
  return <IssuePage />;
}
