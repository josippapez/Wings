import { useCallback } from "react";

import type { CommitRow, Repo } from "../lib/types";

/** What the History tab shows. */
export interface HistoryView {
  loading?: boolean;
  loadingMore?: boolean;
  repo?: Repo | null;
  rows: CommitRow[];
  more?: boolean;
  error?: string;
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
/** A small pill for a branch or tag. `value` is `kind:name`. */
function RefPill({ value }: { value: string }) {
  const kind = value[0];
  const name = value.slice(2);
  const what = REF_KIND[kind as keyof typeof REF_KIND] ?? "Ref";
  return (
    <span className={`ref ref-${kind}`} title={kind === "H" && name !== "HEAD" ? `HEAD, on ${name}` : `${what} ${name}`}>
      {kind === "t" ? `◆ ${name}` : name}
    </span>
  );
}

function Commit({ row, onOpen }: { row: CommitRow; onOpen: (row: CommitRow) => void }) {
  const [hash, subject, author, at, refs] = row;
  const date = new Date(at * 1000).toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });
  return (
    <li>
      <button id={`c:${hash}`} className="commit" title={`${subject}\n${hash}\n${author}, ${date}`} onClick={() => onOpen(row)}>
        <span className="row">
          {refs.map((r, i) => (
            <RefPill key={i} value={r} />
          ))}
          <span className="subject ellipsis">{subject}</span>
        </span>
        <span className="row muted meta">
          <span className="hash">{hash.slice(0, 7)}</span>
          <span className="grow ellipsis">{author}</span>
          <span className="when">{ago(at)}</span>
        </span>
      </button>
    </li>
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
  if (view?.error) body = <p className="warn">{view.error}</p>;
  else if (view && view.rows.length === 0 && !view.loading) {
    body = (
      <div className="empty">
        <p>{query ? "No commits match." : "No commits yet."}</p>
        {query ? <p className="muted">{`Searched messages, authors and hashes for “${query}”.`}</p> : null}
      </div>
    );
  } else if (view) {
    const k = keys(view.rows);
    body = (
      <>
        <ul className="list commits">
          {view.rows.map((row, i) => (
            <Commit key={k[i]} row={row} onOpen={onOpen} />
          ))}
        </ul>
        {view.more ? (
          <button id="more" className="ghost" disabled={Boolean(view.loadingMore)} onClick={onMore}>
            {view.loadingMore ? "Loading…" : "Load more"}
          </button>
        ) : null}
      </>
    );
  }
  return (
    <>
      <div className="row">
        <div className="grow">
          {/* Uncontrolled, so typing in it never loses the caret. Filled once when it mounts, without a value attribute. */}
          <input id="search" type="search" placeholder="Message, author or hash" aria-label="Search commits" ref={fillSearch} onInput={(e) => onSearch(e.currentTarget.value)} />
        </div>
        <button onClick={onGraph} title="Every branch as a commit graph">
          Graph
        </button>
      </div>
      <label className="row check muted">
        <input type="checkbox" id="all" checked={all} onChange={(e) => onAll(e.currentTarget.checked)} />
        All branches
      </label>
      {body}
    </>
  );
}
