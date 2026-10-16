// The sidebar: each pane's branch, as main shares it.

import { useShared } from "wings-plugin-sdk/react";
import { Empty, List, ListItem, Loading, Stack, Text } from "wings-plugin-ui";

import { ch } from "../lib/messages";

export const page = { place: "sidebar" };

export default function Sidebar() {
  const branches = useShared(ch, "branches");
  if (branches === null) return <Loading />;
  if (branches.length === 0) return <Empty title="No branches" description="Open a terminal in a git repo." />;
  return (
    <List>
      {branches.map(({ paneId, project, branch }) => (
        <ListItem key={paneId} title={`${project}: ${branch}`}>
          <Stack gap="none" grow>
            <Text truncate>{branch}</Text>
            <Text tone="muted" size="small" truncate>
              {project}
            </Text>
          </Stack>
        </ListItem>
      ))}
    </List>
  );
}
