// The PR page as it is when opened straight on a pull request: no breadcrumb.
import { PrView } from "../src/ui/pr-view";
import "../src/pages/pr.css";
import { props } from "./_data";

export const fill = true;
export default <PrView {...props} />;
