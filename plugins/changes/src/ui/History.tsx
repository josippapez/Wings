import { useCallback } from "react";
import { Badge, Button, Checkbox, Empty, List, ListButton, ListItem, Row, SearchField, Stack, Text } from "wings-plugin-ui";

import { ago, dateTimeOf } from "../lib/dates";
import { describeRef } from "../lib/log";
import type { CommitRow } from "../lib/types";
import "./ref-pill.css";

/** What the History tab shows. */
interface HistoryView {
  rows: CommitRow[];
  more: boolean;
  /** The first page for this search and scope is on its way, and `rows` may be of another. */
  pending: boolean;
  loadingMore: boolean;
}

/** Refs shown under a commit; the rest are a count, with their names in its tooltip. */
const MAX_REFS = 3;
/** A small tag for a branch or tag. `value` is `kind:name`. */
function RefPill({ value }: { value: string }) {
  const { head, label, title, tone } = describeRef(value);
  return (
    <Badge variant="outline" tone={tone} className={head ? "ref-head" : undefined} title={title}>
      {label}
    </Badge>
  );
}

function Commit({ row, onOpen }: { row: CommitRow; onOpen: (row: CommitRow) => void }) {
  const [hash, subject, author, at, refs] = row;
  return (
    <ListItem>
      <ListButton className="commit" title={`${subject}\n${hash}\n${author}, ${dateTimeOf(at)}`} onClick={() => onOpen(row)}>
        <Text truncate className="subject">
          {subject}
        </Text>
        <Row as="span">
          <Text tone="muted" size="tiny" mono>
            {hash.slice(0, 7)}
          </Text>
          <Text tone="muted" size="tiny" grow truncate>
            {author}
          </Text>
          <Text tone="muted" size="tiny">
            {ago(at)}
          </Text>
        </Row>
        {refs.length ? (
          <Row as="span" gap="xs" wrap>
            {refs.slice(0, MAX_REFS).map((r, i) => (
              <RefPill key={i} value={r} />
            ))}
            {refs.length > MAX_REFS ? (
              <Badge variant="outline" title={refs.slice(MAX_REFS).map((r) => r.slice(2)).join("\n")}>{`+${refs.length - MAX_REFS}`}</Badge>
            ) : null}
          </Row>
        ) : null}
      </ListButton>
    </ListItem>
  );
}

interface Props {
  view: HistoryView | null;
  query: string;
  all: boolean;
  /** The search box's text, kept while the tab is hidden, so switching back shows what you typed. */
  searchText: string;
  onSearch: (text: string) => void;
  onAll: (all: boolean) => void;
  onGraph: () => void;
  onOpen: (row: CommitRow) => void;
  onMore: () => void;
}

/** Keys for commit rows: the hash, made unique if a page repeats one. */
function keys(rows: CommitRow[]) {
  const seen = new Map<string, number>();
  return rows.map(([hash]) => {
    const n = seen.get(hash) ?? 0;
    seen.set(hash, n + 1);
    return n ? `${hash}:${n}` : hash;
  });
}

/** The History tab under the shared header. */
export function History({ view, query, all, searchText, onSearch, onAll, onGraph, onOpen, onMore }: Props) {
  const fillSearch = useCallback((el: HTMLInputElement | null) => {
    if (el) el.value = searchText;
    // Only when it mounts: after that the box holds what you type.
  }, []);
  let body = null;
  if (view && view.rows.length === 0 && !view.pending) {
    body = <Empty title={query ? "No commits match." : "No commits yet."} description={query ? `Searched messages, authors and hashes for “${query}”.` : undefined} />;
  } else if (view) {
    const k = keys(view.rows);
    body = (
      <>
        <List>
          {view.rows.map((row, i) => (
            <Commit key={k[i]} row={row} onOpen={onOpen} />
          ))}
        </List>
        {view.more ? (
          <Button id="more" variant="ghost" disabled={view.loadingMore} onClick={onMore}>
            {view.loadingMore ? "Loading…" : "Load more"}
          </Button>
        ) : null}
      </>
    );
  }
  return (
    <>
      <Row>
        <Stack grow>
          {/* Uncontrolled, so typing in it never loses the caret. Filled once when it mounts, without a value attribute. */}
          <SearchField id="search" placeholder="Search commits" label="Search commits" ref={fillSearch} onInput={(e) => onSearch(e.currentTarget.value)} />
        </Stack>
        <Button variant="outline" onClick={onGraph} title="Every branch as a commit graph">
          Graph
        </Button>
      </Row>
      <Checkbox id="all" checked={all} onChange={onAll}>
        <Text tone="muted">All branches</Text>
      </Checkbox>
      {body}
    </>
  );
}
