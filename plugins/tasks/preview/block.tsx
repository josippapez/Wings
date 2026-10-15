// The Issues block of a Project page: with a mix of issues, none, while they load, and as the page runs it.
import BlockPage, { page } from "../src/pages/block";
import { BlockView } from "../src/ui/block-view";
import { issues, project, storage } from "./_data";

export { page };

const noop = () => {};
const view = { project, apply: () => Promise.resolve(), openBoard: noop, openIssue: noop };

export const states = {
  mixed: { element: <BlockView {...view} issues={issues} /> },
  empty: { element: <BlockView {...view} issues={[]} /> },
  // While the list loads.
  loading: { element: <BlockView {...view} issues={null} /> },
  // As the page runs it, with its issues read from storage.
  page: { element: <BlockPage />, project, storage },
};
