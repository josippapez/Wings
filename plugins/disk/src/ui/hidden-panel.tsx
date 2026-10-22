// The hidden space of a disk scan, opened in place of the list: what the scan didn't find, and what can be done about each part.

import type { ReactNode } from "react";
import { Button, Confirm, Dot, Heading, List, ListItem, Stack, Text, Title } from "wings-plugin-ui";
import { bytes } from "../lib/format";
import type { Hidden } from "../lib/messages";
import { snapshotLabel } from "./model";

/** What is asked before it is done: freeing the purgeable space, or deleting one snapshot. */
export type HiddenConfirm = { kind: "purge" } | { kind: "snapshot"; date: string };

export interface HiddenPanelProps {
  hidden: Hidden;
  /** The disk's mount point, which `purge` takes. */
  mount: string;
  confirm: HiddenConfirm | null;
  onConfirm: (confirm: HiddenConfirm | null) => void;
  onBack: () => void;
  onPurge: (mount: string) => Promise<unknown>;
  onDeleteSnapshot: (date: string) => Promise<unknown>;
}

function Part({ name, note, size, action }: { name: string; note?: string; size: number | null; action?: ReactNode }) {
  return (
    <ListItem className="dk-part">
      <Dot tone="muted" />
      <Text truncate>{name}</Text>
      {size !== null ? <Text numeric tone="muted" className="dk-size">{bytes(size)}</Text> : <span />}
      {action ?? <span />}
      {note ? <Text tone="muted" size="tiny" className="dk-part-note">{note}</Text> : null}
    </ListItem>
  );
}

export function HiddenPanel({ hidden, mount, confirm, onConfirm, onBack, onPurge, onDeleteSnapshot }: HiddenPanelProps) {
  return (
    <div className="dk-list">
      <header className="dk-list-head">
        <Button variant="ghost" size="xs" className="dk-list-back" onClick={onBack}>← Back</Button>
        <Title as="h1">Hidden space</Title>
        <Text tone="muted" size="small">{bytes(hidden.size)}</Text>
      </header>
      <div className="dk-list-body">
        <Stack gap="m">
          {hidden.purgeable > 0 ? (
            <Stack gap="s">
              <List aria-label="Purgeable space">
                <Part
                  name="Purgeable"
                  note="Caches and local snapshots that macOS can remove on its own"
                  size={hidden.purgeable}
                  action={<Button variant="outline" size="xs" aria-label="Free the purgeable space" onClick={() => onConfirm({ kind: "purge" })}>Free it</Button>}
                />
              </List>
              {confirm?.kind === "purge" ? (
                <Confirm
                  title="Free the purgeable space?"
                  onCancel={() => onConfirm(null)}
                  actions={[
                    {
                      id: "purge",
                      label: "Free it",
                      onConfirm: async () => {
                        await onPurge(mount);
                        onConfirm(null);
                      },
                    },
                  ]}
                >
                  <Text tone="muted" size="small">macOS decides what goes: caches and local snapshots it can make again. You may get back less than {bytes(hidden.purgeable)}.</Text>
                </Confirm>
              ) : null}
            </Stack>
          ) : null}

          {hidden.others.length > 0 ? (
            <Stack gap="xs">
              <Heading>Other volumes</Heading>
              <List aria-label="Other volumes on this disk">
                {hidden.others.map((volume) => (
                  <Part key={volume.mount} name={volume.name} note={volume.mount} size={volume.used} />
                ))}
              </List>
            </Stack>
          ) : null}

          {hidden.snapshots.length > 0 ? (
            <Stack gap="xs">
              <Heading>Time Machine snapshots</Heading>
              <List aria-label="Time Machine snapshots">
                {hidden.snapshots.map((date) => (
                  <Part
                    key={date}
                    name={snapshotLabel(date)}
                    size={null}
                    action={<Button variant="outline" size="xs" aria-label={`Delete the snapshot of ${snapshotLabel(date)}`} onClick={() => onConfirm({ kind: "snapshot", date })}>Delete</Button>}
                  />
                ))}
              </List>
              <Text tone="muted" size="tiny">Their space is part of the purgeable space.</Text>
              {confirm?.kind === "snapshot" ? (
                <Confirm
                  title={`Delete the snapshot of ${snapshotLabel(confirm.date)}?`}
                  onCancel={() => onConfirm(null)}
                  actions={[
                    {
                      id: "snapshot",
                      label: "Delete snapshot",
                      onConfirm: async () => {
                        await onDeleteSnapshot(confirm.date);
                        onConfirm(null);
                      },
                    },
                  ]}
                >
                  <Text tone="muted" size="small">It is deleted for good. Files that only it kept can't be restored.</Text>
                </Confirm>
              ) : null}
            </Stack>
          ) : null}

          {hidden.rest > 0 ? (
            <List aria-label="What is still hidden">
              <Part name="Still hidden" note="Folders Wings can't open, other users' files and system data" size={hidden.rest} />
            </List>
          ) : null}
        </Stack>
      </div>
    </div>
  );
}
