// A git error (an index.lock during a commit) after the list arrived: the list stays, the error is a notice above it.
import { useEffect } from "react";
import Sidebar from "../src/pages/sidebar";
import { repo, state } from "./_data";

export const place = "sidebar";
export const respond = (m: unknown) => {
  const { type } = m as { type: string };
  if (type === "hello") return state();
  if (type === "refresh") return { type: "state", mode: "uncommitted", repo: { root: repo.root, name: repo.name }, error: "fatal: Unable to create '/work/acme/web/.git/index.lock': File exists." };
  return undefined;
};
function Fixture() {
  useEffect(() => {
    const t = setTimeout(() => void wings.broadcast({ type: "refresh" }), 300);
    return () => clearTimeout(t);
  }, []);
  return <Sidebar />;
}
export default Fixture;
