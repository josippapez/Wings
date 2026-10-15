// The Session sidebar: sessions working and idle, none at all, rows opened for their detail, and what goes wrong there.
import Sidebar, { page } from "../src/pages/sidebar";
import { detail, idle, working } from "./_data";
import { PressWhenDrawn } from "./_press";

export { page };
export default Sidebar;
export const shared = { sessions: [working, idle] };
export const answers = { detail };

export const states = {
  // A working session: running tools, to-dos, edited files, activity.
  working: {},
  // An idle session with a short history and nothing running.
  idle: { shared: { sessions: [idle] } },
  // No Claude session in the panes.
  none: { shared: { sessions: [] } },
  // Rows opened for their detail: your message in full, an edit, a failed test and a call that is still running.
  opened: {
    element: (
      <PressWhenDrawn selectors={["#item-p1", "#item-t3", "#item-t7", "#item-t9"]}>
        <Sidebar />
      </PressWhenDrawn>
    ),
  },
  // What goes wrong: a row that main doesn't keep any more, and a file whose diff can't open.
  errors: {
    answers: {
      detail,
      diff: () => {
        throw new Error("cart.tsx is outside web, so there is no diff to show.");
      },
    },
    element: (
      <PressWhenDrawn selectors={["#item-t1", '[title="Diff of src/routes/cart.tsx"]']}>
        <Sidebar />
      </PressWhenDrawn>
    ),
  },
};
