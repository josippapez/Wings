// The Collector: what you picked to move to the Trash. A bar along the bottom of the map that folders and files are dropped on.
// Move to Trash waits five seconds for a Cancel before it asks main to do it.

import { useEffect, useRef, useState, type DragEvent } from "react";
import { basename } from "wings-plugin-sdk/format";
import { Button, Icon, List, ListItem, Row, Spinner, Text, Title } from "wings-plugin-ui";
import { bytes } from "../lib/format";
import type { Collected } from "../lib/messages";
import { parentPath } from "../lib/tree";
import { count, parseDrop } from "./model";
import { Dismissible, type Message } from "./parts";
import { DRAG_TYPE } from "./sunburst";

export interface CollectorBarProps {
  items: Collected[];
  total: number;
  /** Starts with the items shown. */
  open?: boolean;
  /** How many seconds Move to Trash waits for a Cancel. 5 unless a fixture says otherwise. */
  countdown?: number;
  messages: Message[];
  onDismiss: (id: number) => void;
  /** Folders and files dropped on the bar. */
  onDrop: (items: Collected[]) => void;
  onRemove: (path: string) => void;
  onReveal: (path: string) => void;
  /** Moves everything in the Collector to the Trash, and says what came of it in `messages`. Never rejects. */
  onTrash: () => Promise<void>;
}

type Phase = { kind: "idle" } | { kind: "counting"; left: number } | { kind: "moving" };

const accepts = (e: DragEvent) => e.dataTransfer.types.includes(DRAG_TYPE);

export function CollectorBar({ items, total, open: startOpen = false, countdown = 5, messages, onDismiss, onDrop, onRemove, onReveal, onTrash }: CollectorBarProps) {
  const [open, setOpen] = useState(startOpen);
  const [over, setOver] = useState(false);
  // Entering a child of the bar leaves the bar for a moment: count what is entered, so the hover doesn't flicker.
  const inside = useRef(0);
  const [phase, setPhase] = useState<Phase>({ kind: "idle" });

  // The countdown names what it will move, so a change to the Collector ends it.
  const what = `${total}\n${items.map((i) => i.path).join("\n")}`;
  useEffect(() => setPhase((p) => (p.kind === "counting" ? { kind: "idle" } : p)), [what]);

  useEffect(() => {
    if (phase.kind === "counting") {
      const timer = setTimeout(() => setPhase(phase.left > 1 ? { kind: "counting", left: phase.left - 1 } : { kind: "moving" }), 1000);
      return () => clearTimeout(timer);
    }
    if (phase.kind === "moving") void onTrash().finally(() => setPhase({ kind: "idle" }));
    // Only the phase starts a move: `onTrash` changes with every render and must not start another.
  }, [phase]);

  const empty = items.length === 0;
  const start = () => setPhase(countdown > 0 ? { kind: "counting", left: countdown } : { kind: "moving" });

  return (
    <section
      className="dk-collector"
      aria-label="Collector"
      data-over={over ? "" : undefined}
      data-open={open ? "" : undefined}
      onDragEnter={(e) => {
        if (!accepts(e)) return;
        e.preventDefault();
        inside.current++;
        setOver(true);
      }}
      onDragOver={(e) => accepts(e) && e.preventDefault()}
      onDragLeave={(e) => {
        if (!accepts(e)) return;
        inside.current = Math.max(0, inside.current - 1);
        if (inside.current === 0) setOver(false);
      }}
      onDrop={(e) => {
        if (!accepts(e)) return;
        e.preventDefault();
        inside.current = 0;
        setOver(false);
        const dropped = parseDrop(e.dataTransfer.getData(DRAG_TYPE));
        if (dropped.length) onDrop(dropped);
      }}
    >
      <div className="dk-collector-head">
        <Button variant="ghost" size="icon-xs" className="dk-fold" aria-expanded={open} aria-label="Show what is in the Collector" onClick={() => setOpen(!open)}>
          <Icon name="chevron-down" />
        </Button>
        <Title as="h2">Collector</Title>
        {phase.kind === "counting" ? (
          <>
            <Text className="dk-collector-text" aria-hidden>Moving {count(items.length, "item")}, {bytes(total)}, to the Trash in {phase.left} s</Text>
            <span className="dk-sr-only" role="status">Moving {count(items.length, "item")} to the Trash in {countdown} seconds. Cancel to stop.</span>
          </>
        ) : phase.kind === "moving" ? (
          <Row gap="s" className="dk-collector-text">
            <Spinner />
            <Text>Moving to the Trash</Text>
          </Row>
        ) : (
          <Text tone="muted" className="dk-collector-text">{empty ? "Drag folders and files here" : `${count(items.length, "item")}, ${bytes(total)}`}</Text>
        )}
        {phase.kind === "counting" ? (
          <Button key="cancel" variant="outline" size="sm" autoFocus onClick={() => setPhase({ kind: "idle" })}>Cancel</Button>
        ) : (
          <Button key="trash" variant="destructive" size="sm" busy={phase.kind === "moving"} disabled={empty} onClick={start}>Move to Trash</Button>
        )}
      </div>

      {messages.length ? (
        <div className="dk-collector-messages">
          {messages.map((m) => (
            <Dismissible key={m.id} tone={m.tone} onDismiss={() => onDismiss(m.id)}>{m.text}</Dismissible>
          ))}
        </div>
      ) : null}

      {open ? (
        empty ? (
          <Text tone="muted" size="small" className="dk-collector-empty">Nothing yet. Drag folders and files here, or choose Add to Collector in a row's menu.</Text>
        ) : (
          <List compact className="dk-collector-list" aria-label="In the Collector">
            {items.map((item) => (
              <ListItem key={item.path} className="dk-collector-item">
                <div className="dk-place-text">
                  <Text truncate>{basename(item.path)}</Text>
                  <Text tone="muted" size="tiny" truncate title={item.path}>{parentPath(item.path)}</Text>
                </div>
                <Text numeric tone="muted" className="dk-size">{bytes(item.size)}</Text>
                <Button variant="ghost" size="xs" aria-label={`Reveal ${basename(item.path)} in Finder`} onClick={() => onReveal(item.path)}>Reveal</Button>
                <Button variant="ghost" size="icon-xs" aria-label={`Remove ${basename(item.path)} from the Collector`} title="Remove" onClick={() => onRemove(item.path)}>×</Button>
              </ListItem>
            ))}
          </List>
        )
      ) : null}
    </section>
  );
}
