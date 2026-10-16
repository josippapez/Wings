// The History tab: merge, branch, tag and remote labels.
import { Pressed } from "./_sidebar";
import { changes, history } from "./_data";
export const place = "sidebar";
export const queries = { changes: () => changes(), history: () => history() };
export default () => <Pressed button="History" />;
