// The PR page as it is when opened from the project's list: a breadcrumb back in the toolbar.
import { PrView } from "../src/ui/pr-view";
import "../src/pr.css";
import { props } from "./_data";

export const fill = true;
export default <PrView {...props} onBack={() => {}} />;
