import { useCallback } from "react";
import { Badge, Button, Checkbox, Empty, List, ListButton, ListItem, Row, SearchField, Stack, Text } from "wings-plugin-ui";

import type { CommitRow, Repo } from "../lib/types";

/** What the History tab shows. */
export interface HistoryView {
  loading?: boolean;
  loadingMore?: boolean;
  repo?: Repo | null;
  rows: CommitRow[];
  more?: boolean;
  error?: string;
  /** The repo, search and scope the rows are of, so a first page for the same list keeps the rows after it. */
  source?: string;
}

const relative = new Intl.RelativeTimeFormat(undefined, { numeric: "auto" });
const UNITS: [Intl.RelativeTimeFormatUnit, number][] = [
  ["year", 31_557_600],
  ["month", 2_629_800],
  ["week", 604_800],
  ["day", 86_400],
  ["hour", 3_600],
  ["minute", 60],
];
/** Like "3 hours ago". @param at Unix seconds. */
function ago(at: number) {
  const s = at - Date.now() / 1000;
  const unit = UNITS.find(([, size]) => Math.abs(s) >= size);
  return unit ? relative.format(Math.round(s / unit[1]), unit[0]) : "just now";
}

const REF_KIND = { H: "HEAD", b: "Branch", r: "Remote branch", t: "Tag", s: "Stash" };
const REF_TONE = { H: "info", b: "success", t: "warn" } as const;
/** A small tag for a branch or tag. `value` is `kind:name`. */
function RefPill({ value }: { value: string }) {
  const kind = value[0];
  const name = value.slice(2);
  const what = REF_KIND[kind as keyof typeof REF_KIND] ?? "Ref";
  return (
    <Badge variant="outline" tone={REF_TONE[kind as keyof typeof REF_TONE]} className={kind === "H" ? "head" : undefined} title={kind === "H" && name !== "HEAD" ? `HEAD, on ${name}` : `${what} ${name}`}>
      {kind === "t" ? `◆ ${name}` : name}
    </Badge>
  );
}

function Commit({ row, onOpen }: { row: CommitRow; onOpen: (row: CommitRow) => void }) {
  const [hash, subject, author, at, refs] = row;
  const date = new Date(at * 1000).toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });
  return (
    <ListItem>
      <ListButton id={`c:${hash}`} className="commit" title={`${subject}\n${hash}\n${author}, ${date}`} onClick={() => onOpen(row)}>
        <Row as="span">
          {refs.map((r, i) => (
            <RefPill key={i} value={r} />
          ))}
          <Text truncate className="subject">
            {subject}
          </Text>
        </Row>
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
  if (view?.error) {
    body = (
      <Text as="p" tone="warn">
        {view.error}
      </Text>
    );
  } else if (view && view.rows.length === 0 && !view.loading) {
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
          <Button id="more" variant="ghost" disabled={Boolean(view.loadingMore)} onClick={onMore}>
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
          <SearchField id="search" placeholder="Message, author or hash" label="Search commits" ref={fillSearch} onInput={(e) => onSearch(e.currentTarget.value)} />
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
