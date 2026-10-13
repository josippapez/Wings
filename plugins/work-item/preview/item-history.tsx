// The work item page on its History tab.
import { ItemView } from "../src/ui/item-view";
import "../src/item.css";
import { props } from "./_data";

export const fill = true;
export default <ItemView {...props} initialTab="history" />;
