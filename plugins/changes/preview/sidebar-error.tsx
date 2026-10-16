// A git error (an index.lock during a commit) after the list arrived: the list stays, the error is a notice above it.
import { Pressed } from "./_sidebar";
import { changes } from "./_data";

let asked = 0;
export const place = "sidebar";
export const queries = {
  changes: () => {
    if (++asked === 1) return changes();
    throw new Error("fatal: Unable to create '/work/acme/web/.git/index.lock': File exists.");
  },
};
export default () => <Pressed button="Refresh" after={300} />;
