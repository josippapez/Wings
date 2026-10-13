// The Notes tab: the same editor with room to write.

import { Page, showTab, Toolbar } from "wings-plugin-ui";
import { Editor } from "./lib/Editor";

showTab(
  <Page toolbar={<Toolbar title="Notes" />}>
    <Editor rows={32} />
  </Page>,
  { fill: true },
);
