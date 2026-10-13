// The work item page when its comments and linked items are still loading, and its pull request call failed.
import { ItemView } from "../src/ui/item-view";
import "../src/item.css";
import { props } from "./_data";

export const fill = true;
export default <ItemView {...props} states={[]} linked={{ data: null, error: "az boards query failed: network unreachable" }} updates={{ data: null, error: null }} prs={[]} />;
