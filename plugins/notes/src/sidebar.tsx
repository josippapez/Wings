// Points to the Project page, which replaced this plugin.

import { Card, Heading, List, ListItem, showSidebar, Stack, Text } from "wings-plugin-ui";

function Sidebar() {
  return (
    <Card>
      <Stack gap="m">
        <Heading level={3}>Project info moved to the Project page</Heading>
        <Text tone="muted">Your facts and sections came along. Open the page from:</Text>
        <List compact aria-label="Where to open the Project page">
          <ListItem>A project in the sidebar</ListItem>
          <ListItem>The + menu</ListItem>
          <ListItem>The command palette</ListItem>
        </List>
        <Text tone="muted" size="small">You can turn this plugin off.</Text>
      </Stack>
    </Card>
  );
}

showSidebar(<Sidebar />);
