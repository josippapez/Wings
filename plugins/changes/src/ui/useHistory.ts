import { useLayoutEffect, useState } from "react";
import { errorMessage } from "wings-plugin-sdk/async";
import { useQuery } from "wings-plugin-sdk/react";

import { withFirstPage } from "../lib/history-rows";
import { ch } from "../lib/messages";
import type { CommitRow, Repo } from "../lib/types";

interface Shown {
  /** Null when no pane is in a repo. */
  repo: Repo | null;
  rows: CommitRow[];
  more: boolean;
  /** The repo, search and scope the rows are of. */
  source: string;
}

const sourceOf = (repo: Repo, query: string, all: boolean) => JSON.stringify([repo.root, query, all]);

/**
 * The History tab's commits. The first page is a query that main keeps fresh and replaces itself. The rest is explicit
 * because the newest answer doesn't win there: Load more asks for the next page and appends it, and a first page that
 * arrives later goes over the top of the rows already shown, so a refetch after a new commit keeps the pages Load more added.
 * `skip` leaves the tab alone while it is hidden; its rows stay for when it comes back.
 */
export function useHistory({ query, all, skip }: { query: string; all: boolean; skip: boolean }) {
  const first = useQuery(ch, "history", { query, all }, { skip });
  const [shown, setShown] = useState<Shown | null>(null);
  const [loadingMore, setLoadingMore] = useState(false);
  const [moreError, setMoreError] = useState<string | null>(null);

  // `first.data` is the first page of the key on screen: `useQuery` drops it when the key changes. A layout effect, so the
  // rows on screen never lag the answer by a painted frame.
  useLayoutEffect(() => {
    const page = first.data;
    if (!page) return;
    setMoreError(null);
    if (!page.repo) {
      setShown({ repo: null, rows: [], more: false, source: "" });
      return;
    }
    const { repo, rows: top, more } = page;
    const source = sourceOf(repo, query, all);
    setShown((prev) => {
      if (prev?.source !== source) return { repo, rows: top, more, source };
      const rows = withFirstPage(prev.rows, top);
      return { repo, rows, more: rows.length > top.length ? prev.more : more, source };
    });
  }, [first.data]);

  const loadMore = () => {
    if (!shown?.repo) return;
    const { source, rows } = shown;
    setLoadingMore(true);
    setMoreError(null);
    ch.ask("history", { query, all, skip: rows.length })
      .then((page) =>
        // Dropped when the list moved on while this was on its way: another repo or search, or a poll that changed the rows.
        setShown((prev) => (prev?.source === source && prev.rows.length === rows.length && page.repo && sourceOf(page.repo, query, all) === source ? { ...prev, rows: [...prev.rows, ...page.rows], more: page.more } : prev)),
      )
      .catch((error) => setMoreError(errorMessage(error)))
      .finally(() => setLoadingMore(false));
  };

  return {
    shown,
    /** The first page isn't here yet, or Refresh waits for it. */
    loading: first.loading,
    /** The first page of this search and scope isn't here yet, so `shown` may be of another. A refetch isn't pending. */
    pending: first.data === null,
    loadingMore,
    error: first.error?.message ?? moreError,
    refresh: first.refresh,
    loadMore,
  };
}
