// The work item page: on each of its tabs, while the work item loads, and when it could not be read.
import { ItemError, ItemSkeleton, ItemView } from "../src/ui/item-view";
import { props } from "./_data";

export { page } from "../src/pages/item";

export const states = {
  overview: { element: <ItemView {...props} initialTab="overview" /> },
  history: { element: <ItemView {...props} initialTab="history" /> },
  links: { element: <ItemView {...props} initialTab="links" /> },
  // Its comments and linked items are still loading, and its pull request call failed.
  parts: { element: <ItemView {...props} states={[]} linked={{ data: null, error: "az boards query failed: network unreachable" }} updates={{ data: null, error: null }} prs={[]} /> },
  // More pull requests than the list shows at first: the rest are behind "Show all".
  manyPrs: { element: <ItemView {...props} initialTab="links" prs={Array.from({ length: 12 }, (_, i) => ({ id: 480 + i, repoId: "r1", title: `Sync client change ${i + 1}`, status: i % 3 ? "completed" : "active", repo: "web" }))} /> },
  loading: { element: <ItemSkeleton id={1432} /> },
  error: { element: <ItemError id={1432} message="TF401232: Work item 1432 does not exist, or you do not have permissions to read it." onRetry={() => {}} /> },
};
