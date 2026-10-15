// What goes wrong: a row that main doesn't keep any more, and a file whose diff can't open.
import Sidebar from "../src/pages/sidebar";
import { detail, idle, working } from "./_data";
import { PressWhenDrawn } from "./_press";

export const place = "sidebar";
export const shared = { sessions: [working, idle] };
export const answers = {
  detail,
  diff: () => {
    throw new Error("cart.tsx is outside web, so there is no diff to show.");
  },
};
export default (
  <PressWhenDrawn selectors={["#item-t1", '[title="Diff of src/routes/cart.tsx"]']}>
    <Sidebar />
  </PressWhenDrawn>
);
