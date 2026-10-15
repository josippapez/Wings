// The PR page for a pull request with no description, checks or comments yet.
import { PrView } from "../src/ui/pr-view";
import "../src/pages/pr.css";
import { props } from "./_data";

export const fill = true;
export default <PrView {...props} comments={[]} overview={{ ...props.overview!, body: "", reviewers: [], status: { ...props.overview!.status, review: "No reviews yet", checks: { passed: 0, failed: 0, running: 0 }, changes: null } }} />;
