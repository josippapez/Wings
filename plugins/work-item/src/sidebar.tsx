// The Updates list: what Azure DevOps told you since you last looked (state changes, mentions, pull request
// activity), newest first, read or unread. It shows in the sidebar, and in a tab as an inbox with more of each
// update. A desktop notification can't be clicked, so this is where an update is opened: a work item opens in the
// item tab, a pull request in the browser.

import { useCallback, useEffect, useState } from "react";
import { useBroadcast } from "wings-plugin-sdk/react";
import { Button, Dot, Empty, Heading, List, ListItem, Loading, Page, Row, showSidebar, showTab, Stack, Text, Toolbar } from "wings-plugin-ui";

import type { StoredEvent } from "./lib/updates";
import { InboxView } from "./ui/inbox-view";
import "./item.css";

const KEY = "watch:events";
const inTab = wings.placement() === "tab";

const ago = (iso: string) => {
  const minutes = Math.max(0, Math.round((Date.now() - Date.parse(iso)) / 60_000));
  if (minutes < 1) return "now";
  if (minutes < 60) return `${minutes} min ago`;
  if (minutes < 1440) return `${Math.round(minutes / 60)} h ago`;
  return `${Math.round(minutes / 1440)} d ago`;
};

/** Main is the only writer of the events, so a change is a message to it, and it broadcasts `updates` when done. */
const ask = (op: "read" | "clear", key?: string) => wings.broadcast({ type: "events-op", rid: crypto.randomUUID(), op, key });

function Updates() {
  const [events, setEvents] = useState<StoredEvent[] | null>(null);

  const load = useCallback(async () => {
    setEvents(((await wings.storage.get(KEY)) as StoredEvent[] | null) ?? []);
  }, []);

  useEffect(() => void load(), [load]);
  useBroadcast((message) => {
    if ((message as { type?: string } | null)?.type === "updates") void load();
  });

  if (!events) return inTab ? <Page toolbar={<Toolbar title="Updates" />}><Loading /></Page> : <Loading />;

  const unread = events.filter((e) => !e.read).length;
  const markRead = async (key?: string) => {
    setEvents(events.map((e) => (!key || e.key === key ? { ...e, read: true } : e)));
    await ask("read", key);
  };
  const open = async (e: StoredEvent) => {
    void markRead(e.key);
    if (e.item) await wings.openView("item", { as: "tab", args: e.item });
    else await wings.openUrl(e.url);
  };
  const clear = () => {
    setEvents([]);
    void ask("clear");
  };
  if (inTab) {
    return <InboxView events={events} onOpen={(e) => void open(e)} onBrowser={(e) => { void markRead(e.key); void wings.openUrl(e.url); }} onMarkRead={(key) => void markRead(key)} onClear={clear} />;
  }
  const actions = (
    <>
      {unread ? <Button variant="ghost" size="sm" onClick={() => void markRead()}>Mark all read</Button> : null}
      {events.length ? <Button variant="ghost" size="sm" onClick={clear}>Clear</Button> : null}
    </>
  );

  const list =
    events.length === 0 ? (
      <Empty title="Nothing new" description="State changes on your work items, comments that mention you and activity on your pull requests show up here." />
    ) : (
      <List>
        {events.map((e) => (
          <ListItem key={e.key}>
            <Row gap="s" align="start" justify="between">
              <Dot tone={e.read ? undefined : "info"} style={e.read ? { visibility: "hidden" } : undefined} label={e.read ? undefined : "Unread"} />
              <Stack gap="xs" grow>
                <Text weight={e.read ? undefined : "strong"}>{e.title}</Text>
                <Text tone="muted" size="small" clamp>{e.body}</Text>
                <Text tone="muted" size="tiny">{ago(e.at)}</Text>
              </Stack>
              <Row gap="xs">
                <Button variant="ghost" size="sm" onClick={() => void open(e)}>{e.item ? "Open item" : "Open"}</Button>
              </Row>
            </Row>
          </ListItem>
        ))}
      </List>
    );

  return (
    <Stack gap="m">
      <Heading actions={actions}>Updates</Heading>
      {list}
    </Stack>
  );
}

if (inTab) showTab(<Updates />, { fill: true });
else showSidebar(<Updates />);
