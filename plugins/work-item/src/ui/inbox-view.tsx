// The Updates tab as a notifications inbox, drawn from the events it's given: filter chips, days, and a row per
// update with who did it, what it's about and when. sidebar.tsx owns the storage and the actions.

import { useState } from "react";
import { Avatar, Badge, Button, Chips, Dot, Empty, Heading, Page, Time, Toolbar } from "wings-plugin-ui";

import { TypeIcon } from "../lib/TypeIcon";
import { counts, FILTERS, groupByDay, matches, type Filter } from "../lib/inbox";
import type { StoredEvent } from "../lib/updates";

export interface InboxProps {
  events: StoredEvent[];
  onOpen: (e: StoredEvent) => void;
  onBrowser: (e: StoredEvent) => void;
  onMarkRead: (key?: string) => void;
  onClear: () => void;
}

const KIND_LABEL: Record<string, string> = { state: "State", mention: "Mention", "pr-mention": "PR mention", "pr-comment": "PR comment", "pr-vote": "PR review", "pr-status": "PR", "pr-reviewer": "PR review" };

function Row({ e, onOpen, onBrowser }: { e: StoredEvent } & Pick<InboxProps, "onOpen" | "onBrowser">) {
  return (
    <li className="wi-inbox-row" data-unread={e.read ? undefined : ""} data-testid="wi-inbox-row">
      <span className="wi-inbox-dot">{e.read ? null : <Dot tone="info" label="Unread" />}</span>
      <span className="wi-inbox-who">{e.by ? <Avatar name={e.by} size="large" /> : e.itemType ? <TypeIcon type={e.itemType} size={28} /> : <span className="wi-inbox-kind" aria-hidden="true">{e.kind.startsWith("pr-") ? "PR" : "WI"}</span>}</span>
      <button type="button" className="wi-inbox-main" onClick={() => onOpen(e)} aria-label={`${e.item ? "Open work item" : "Open"}: ${e.title}`}>
        <span className="wi-inbox-title">{e.title}</span>
        {e.body ? <span className="wi-inbox-body">{e.body}</span> : null}
      </button>
      <span className="wi-inbox-side">
        <Badge variant="outline">{KIND_LABEL[e.kind] ?? e.kind}</Badge>
        <Time iso={e.at} className="wi-inbox-time" />
        {e.item ? <Button variant="ghost" size="xs" onClick={() => onBrowser(e)} aria-label={`Open ${e.title} in the browser`}>In browser</Button> : null}
      </span>
    </li>
  );
}

export function InboxView({ events, onOpen, onBrowser, onMarkRead, onClear }: InboxProps) {
  const [filter, setFilter] = useState<Filter>("all");
  const unread = events.filter((e) => !e.read).length;
  const n = counts(events);
  const days = groupByDay(events.filter((e) => matches(e, filter)));

  return (
    <Page
      className="wi-page"
      toolbar={
        <Toolbar
          title={unread ? `Updates (${unread} new)` : "Updates"}
          actions={
            <>
              {unread ? <Button variant="ghost" size="sm" onClick={() => onMarkRead()}>Mark all read</Button> : null}
              {events.length ? <Button variant="ghost" size="sm" onClick={onClear}>Clear</Button> : null}
            </>
          }
        />
      }
    >
      <div className="wi-wrap">
        {events.length === 0 ? (
          <Empty title="Nothing new" description="State changes on your work items, comments that mention you and activity on your pull requests show up here." />
        ) : (
          <>
            <Chips label="Filter updates" value={filter} onChange={setFilter} options={FILTERS.map((f) => ({ value: f.value, label: `${f.label}${f.value === "all" ? "" : ` ${n[f.value]}`}` }))} data-testid="wi-inbox-filters" />
            {days.length === 0 ? (
              <Empty title="Nothing here" description="No updates match this filter." />
            ) : (
              days.map((d) => (
                <section key={d.key} className="wi-day" aria-label={d.label}>
                  <Heading level={3} className="wi-day-title">{d.label}</Heading>
                  <ul className="wi-inbox">{d.events.map((e) => <Row key={e.key} e={e} onOpen={onOpen} onBrowser={onBrowser} />)}</ul>
                </section>
              ))
            )}
          </>
        )}
      </div>
    </Page>
  );
}
