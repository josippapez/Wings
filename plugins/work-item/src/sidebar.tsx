// The Updates sidebar: what Azure DevOps told you since you last looked (state changes, mentions, pull request
// activity), newest first, each with a link. A desktop notification can't be clicked, so the links live here.

import { useCallback, useEffect, useState } from "react";
import { useBroadcast } from "wings-plugin-sdk/react";
import { Button, Empty, Heading, List, ListItem, Loading, Row, showSidebar, Stack, Text } from "wings-plugin-ui";

import type { StoredEvent } from "./lib/updates";

const ago = (iso: string) => {
  const minutes = Math.max(0, Math.round((Date.now() - Date.parse(iso)) / 60_000));
  if (minutes < 1) return "now";
  if (minutes < 60) return `${minutes} min ago`;
  if (minutes < 1440) return `${Math.round(minutes / 60)} h ago`;
  return `${Math.round(minutes / 1440)} d ago`;
};

function Updates() {
  const [events, setEvents] = useState<StoredEvent[] | null>(null);

  const load = useCallback(async () => {
    const stored = ((await wings.storage.get("watch:events")) as StoredEvent[] | null) ?? [];
    setEvents(stored);
    // Seeing the list is reading it: the label next to the title bar button clears.
    if (stored.some((e) => !e.read)) {
      await wings.storage.set("watch:events", stored.map((e) => ({ ...e, read: true })));
      wings.broadcast({ type: "read" });
    }
  }, []);

  useEffect(() => void load(), [load]);
  useBroadcast((message) => {
    if ((message as { type?: string } | null)?.type === "updates") void load();
  });

  if (!events) return <Loading />;
  const clear = async () => {
    await wings.storage.set("watch:events", []);
    setEvents([]);
  };
  return (
    <Stack gap="m">
      <Heading actions={events.length ? <Button variant="ghost" size="sm" onClick={clear}>Clear</Button> : undefined}>Updates</Heading>
      {events.length === 0 ? (
        <Empty title="Nothing new" description="State changes on your work items, comments that mention you and activity on your pull requests show up here." />
      ) : (
        <List>
          {events.map((e) => (
            <ListItem key={e.key}>
              <Row gap="s" align="start" justify="between">
                <Stack gap="xs" grow>
                  <Text weight={e.read ? undefined : "strong"}>{e.title}</Text>
                  <Text tone="muted" size="small" clamp>{e.body}</Text>
                  <Text tone="muted" size="tiny">{ago(e.at)}</Text>
                </Stack>
                <Button variant="ghost" size="sm" onClick={() => wings.openUrl(e.url)}>Open</Button>
              </Row>
            </ListItem>
          ))}
        </List>
      )}
    </Stack>
  );
}

showSidebar(<Updates />);
