// The This branch tab.
import { Pressed } from "./_sidebar";
import { changes, files } from "./_data";
export const place = "sidebar";
export const answers = {
  changes: ({ mode }: { mode: string }) => (mode === "branch" ? changes({ against: "against main (merge base c3d4e5f)", files: files.slice(0, 12), total: 12, added: 410, removed: 160 }) : changes()),
};
export default () => <Pressed button="Branch" />;
