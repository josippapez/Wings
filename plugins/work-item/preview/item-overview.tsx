// The work item page on its Overview tab.
import { ItemView } from "../src/ui/item-view";
import "../src/pages/item.css";
import { props } from "./_data";

export const fill = true;
export default <ItemView {...props} initialTab="overview" />;
