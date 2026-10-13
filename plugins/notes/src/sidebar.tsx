// The Notes sidebar: the focused project's notes, for a quick look and edit. The notes tab has more room.

import { Button, Heading, showSidebar, Stack } from "wings-plugin-ui";
import { Editor } from "./lib/Editor";

function Sidebar() {
  return (
    <Stack gap="m">
      <Heading actions={<Button size="xs" variant="outline" onClick={() => void wings.openView("editor", { as: "tab" })}>Open in tab</Button>}>Notes</Heading>
      <Editor rows={14} />
    </Stack>
  );
}

showSidebar(<Sidebar />);
