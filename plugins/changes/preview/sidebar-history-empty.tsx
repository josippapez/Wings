// The History tab of a repo with no commits yet.
import { Pressed } from "./_sidebar";
import { changes, history } from "./_data";
export const place = "sidebar";
export const queries = { changes: () => changes(), history: () => history({ rows: [], more: false }) };
export default () => <Pressed button="History" />;
