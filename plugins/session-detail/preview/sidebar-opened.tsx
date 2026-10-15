// Rows opened for their detail: your message in full, an edit, a failed test and a call that is still running.
import Sidebar from "../src/pages/sidebar";
import { detail, idle, working } from "./_data";
import { PressWhenDrawn } from "./_press";

export const place = "sidebar";
export const shared = { sessions: [working, idle] };
export const answers = { detail };
export default (
  <PressWhenDrawn selectors={["#item-p1", "#item-t3", "#item-t7", "#item-t9"]}>
    <Sidebar />
  </PressWhenDrawn>
);
