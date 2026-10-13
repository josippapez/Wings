// The PR page on its Checks tab: every check and policy, grouped by how it stands.
import { PrView } from "../src/ui/pr-view";
import "../src/pr.css";
import { withChecks } from "./_data";

export const fill = true;
export default <PrView {...withChecks} initialTab="checks" onBack={() => {}} />;
