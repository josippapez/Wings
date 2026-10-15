import type { ReactNode } from "react";
import { Badge, Chips, Dot, Heading, List, ListButton, ListItem, Spinner, Text } from "wings-plugin-ui";

import type { Detail, ItemView } from "../lib/types";
import { Card } from "./Card";
import { clock, took } from "./format";

/** Badges Wings colours itself; git ones get the plugin's own colour from sidebar.css. */
const tones: Record<string, "info" | "success" | "danger"> = { push: "info", ok: "success", danger: "danger" };

/** One row of the timeline: a button that opens its card. */
function Row({ item, open, detail, toggle }: { item: ItemView; open: boolean; detail: Detail | undefined; toggle: () => void }) {
  const button = (align: "baseline" | undefined, children: ReactNode) => (
    <ListButton align={align} id={`item-${item.key}`} aria-expanded={open} onClick={toggle}>
      {children}
    </ListButton>
  );
  return item.kind === "prompt" ? (
    <ListItem className="prompt">
      {button(
        "baseline",
        <>
          <Text tone="info" weight="strong">
            You
          </Text>
          <Text grow clamp={!open}>
            {item.summary}
          </Text>
          <Text as="time" tone="muted">
            {clock(item.at)}
          </Text>
        </>,
      )}
      {open ? <Card item={item} detail={detail} /> : null}
    </ListItem>
  ) : (
    <ListItem>
      {button(
        undefined,
        <>
          {item.done ? <Dot tone={item.error ? "danger" : undefined} /> : <Spinner />}
          {(item.tags ?? []).map((t, i) => (
            <Badge variant="secondary" key={i} tone={tones[t.kind]} className={t.kind}>
              {t.label}
            </Badge>
          ))}
          {item.tags ? null : (
            <Text as="strong" tone={item.error ? "danger" : undefined}>
              {item.name}
            </Text>
          )}
          <Text grow truncate tone={item.tags ? undefined : "muted"}>
            {item.summary}
          </Text>
          {item.outcome ? <Badge variant="secondary" tone={item.outcome.endsWith("failed") ? "danger" : "success"}>{item.outcome}</Badge> : null}
          <Text tone="muted">{item.error ? "failed" : took(item.ms)}</Text>
        </>,
      )}
      {open ? <Card item={item} detail={detail} /> : null}
    </ListItem>
  );
}

/** Recent activity, all of it or only your messages, highlighted commands and failures. */
export function Activity({
  items,
  highlightsOnly,
  setHighlightsOnly,
  isOpen,
  detailOf,
  toggle,
}: {
  items: ItemView[];
  highlightsOnly: boolean;
  setHighlightsOnly: (on: boolean) => void;
  isOpen: (key: string) => boolean;
  detailOf: (key: string) => Detail | undefined;
  toggle: (key: string) => void;
}) {
  const shown = highlightsOnly ? items.filter((i) => i.kind === "prompt" || i.tags || i.error) : items;
  const filters = (
    <Chips
      label="Show"
      options={[
        { value: "all", label: "All" },
        { value: "highlights", label: "Highlights" },
      ]}
      value={highlightsOnly ? "highlights" : "all"}
      onChange={(v) => setHighlightsOnly(v === "highlights")}
    />
  );
  return (
    <>
      <Heading actions={filters}>Recent activity</Heading>
      {shown.length ? (
        <List ordered>
          {shown.map((item) => (
            <Row key={item.key} item={item} open={isOpen(item.key)} detail={detailOf(item.key)} toggle={() => toggle(item.key)} />
          ))}
        </List>
      ) : (
        <Text as="p" tone="muted">
          {highlightsOnly ? "No highlights in this run yet." : "Nothing yet."}
        </Text>
      )}
    </>
  );
}
