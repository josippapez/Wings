// The PR page when the pull request could not be loaded.
import { PrView } from "../src/ui/pr-view";
import "../src/pr.css";
import { props } from "./_data";

export const fill = true;
export default <PrView {...props} overview={null} comments={null} error="gh pr view failed: could not resolve to a pull request with the number 482 in acme/web. Run gh auth status to check you are signed in." onBack={() => {}} />;
