// The Changes sidebar page: the changed files of the focused pane's repo as a folder tree, and its commit history. The
// plugin's main script runs git and broadcasts the files and commits here; a click asks it to open the diff viewer.

import { useEffect, useLayoutEffect, useReducer, useRef } from "react";
import { pollWhileFocused } from "wings-plugin-sdk/poll";
import { Button, Card, Chips, Empty, Loading, Row, Spinner, Stack, Text } from "wings-plugin-ui";

import { FileTree } from "./FileTree";
import { History, type HistoryView } from "./History";
import { plural } from "../lib/format";
import { withFirstPage } from "../lib/history-rows";
import { type ChangesView, mergeState } from "../lib/state-merge";
import type { CommitRow, HistoryMessage, Mode, StateMessage, Tab } from "../lib/types";

const POLL_MS = 15_000;
/** How long the search waits after the last key before it asks git. */
const SEARCH_MS = 250;

/**
 * Everything the page shows, kept across renders and changed in place. It renders when something on screen
 * changed, like the old page did, so a broadcast for the tab you don't see doesn't render.
 */
interface Ui {
  state: ChangesView | null;
  /** Folders you collapsed, by path. */
  collapsed: Set<string>;
  tab: Tab;
  /** The History tab's commits. */
  historyTab: HistoryView | null;
  /** The search and scope the History tab asked for; replies for others are stale. */
  query: string;
  all: boolean;
  /** What the search box holds, and its timer. */
  searchText: string;
  searchTimer: ReturnType<typeof setTimeout> | undefined;
}

export function Sidebar({ tab: firstTab = "changes" }: { tab?: Tab }) {
  const ui = useRef<Ui>({ state: null, collapsed: new Set(), tab: firstTab, historyTab: null, query: "", all: false, searchText: "", searchTimer: undefined }).current;
  const [, render] = useReducer((n: number) => n + 1, 0);

  // A render puts focus back on the element that had it, by id, like the old page that rebuilt every row.
  const focused = document.activeElement?.id;
  useLayoutEffect(() => {
    if (focused) document.getElementById(focused)?.focus({ preventScroll: true });
  });

  useEffect(() => {
    const off = wings.onBroadcast((message) => {
      const m = message as HistoryMessage | StateMessage | null;
      if (m?.type === "history") {
        if (m.query !== ui.query || m.all !== ui.all) return;
        const rowsOf = m.rows ?? [];
        const historyTab = ui.historyTab;
        const source = JSON.stringify([m.repo?.root, m.query, m.all]);
        // A next page appends, unless the repo changed under it. A first page replaces the list, or for the list on
        // screen (a poll, a refresh) only its top, so the pages Load more added stay.
        if (m.skip > 0 && historyTab && historyTab.repo?.root === m.repo?.root && m.skip === historyTab.rows.length) ui.historyTab = { ...historyTab, rows: [...historyTab.rows, ...rowsOf], more: m.more, loadingMore: false };
        else if (m.skip === 0) {
          const rows = historyTab?.source === source && !m.error ? withFirstPage(historyTab.rows, rowsOf) : rowsOf;
          ui.historyTab = { repo: m.repo, rows, more: rows.length > rowsOf.length ? historyTab?.more : m.more, error: m.error, source };
        } else return;
        if (ui.tab === "history") render();
        return;
      }
      if (m?.type !== "state") return;
      ui.state = mergeState(ui.state, m);
      if (ui.tab === "changes") render();
    });
    void wings.broadcast({ type: "hello" });
    const stopPolling = pollWhileFocused(() => void wings.broadcast({ type: "refresh", tab: ui.tab }), POLL_MS);
    return () => {
      off();
      stopPolling();
    };
  }, [ui]);

  const { state, historyTab, tab } = ui;
  const openDiff = (path: string | null) => state?.repo && void wings.broadcast({ type: "open", root: state.repo.root, mode: state.mode, path });
  const refresh = () => {
    if (ui.tab === "history" && ui.historyTab) ui.historyTab = { ...ui.historyTab, loading: true };
    render();
    void wings.broadcast({ type: "refresh", byUser: true, tab: ui.tab });
  };
  const setMode = (mode: Mode) => {
    if (ui.tab === "changes" && ui.state && ui.state.mode === mode) return;
    // A search still waiting would ask for history the Changes tab doesn't show; it runs when you come back.
    clearTimeout(ui.searchTimer);
    ui.tab = "changes";
    ui.state = ui.state && { mode, repo: ui.state.repo, loading: true };
    render();
    void wings.broadcast({ type: "mode", mode });
  };
  /** Asks main for a page of commits: the first one for a new search, or the next. */
  const loadHistory = (skip: number) => {
    ui.historyTab = skip && ui.historyTab ? { ...ui.historyTab, loadingMore: true } : { rows: [], ...ui.historyTab, loading: true, error: undefined };
    render();
    void wings.broadcast({ type: "history", query: ui.query, all: ui.all, skip });
  };
  const showHistory = () => {
    if (ui.tab === "history") return;
    ui.tab = "history";
    ui.query = ui.searchText.trim();
    loadHistory(0);
  };
  const onSearch = (text: string) => {
    ui.searchText = text;
    clearTimeout(ui.searchTimer);
    ui.searchTimer = setTimeout(() => {
      if (ui.searchText.trim() === ui.query) return;
      ui.query = ui.searchText.trim();
      loadHistory(0);
    }, SEARCH_MS);
  };

  const view = tab === "history" ? historyTab : state;
  const repo = view?.repo ?? (tab === "history" ? state?.repo : undefined);
  if (!view && !repo) return <Loading />;
  if (!repo) return <Empty title="No git repo in your panes." description="Open a terminal in a repo and its changes show here." />;
  const mode = state?.mode ?? "uncommitted";

  let body = null;
  if (tab === "history") {
    body = (
      <History
        view={historyTab}
        query={ui.query}
        all={ui.all}
        searchText={ui.searchText}
        onSearch={onSearch}
        onAll={(all) => {
          ui.all = all;
          loadHistory(0);
        }}
        onGraph={() => historyTab?.repo && void wings.broadcast({ type: "graph", root: historyTab.repo.root })}
        onOpen={([hash, subject, author, at]: CommitRow) => historyTab?.repo && void wings.broadcast({ type: "commit", root: historyTab.repo.root, hash, subject, author, at })}
        onMore={() => loadHistory(historyTab?.rows.length ?? 0)}
      />
    );
  } else if (state?.error && !state.files) {
    body = (
      <Text as="p" tone="warn">
        {state.error}
      </Text>
    );
  } else if (state?.files) {
    const files = state.files;
    const total = state.total ?? files.length;
    if (total === 0) {
      body = <Empty title={mode === "branch" ? "Nothing changed on this branch." : "No uncommitted changes."} description={state.against ?? ""} />;
    } else {
      body = (
        <>
          <Card compact>
            <Row>
              <Stack gap="none" grow>
                <div>
                  {plural(total, "file")}
                  {" "}
                  <Text tone="success" numeric>{`+${state.added}`}</Text>
                  {" "}
                  <Text tone="danger" numeric>{`−${state.removed}`}</Text>
                  {state.partial ? <Text tone="muted">{" (some new files not counted)"}</Text> : null}
                </div>
                <Text as="div" tone="muted" truncate>
                  {state.against ?? ""}
                </Text>
              </Stack>
              <Button variant="outline" onClick={() => openDiff(null)}>View all</Button>
            </Row>
          </Card>
          <FileTree
            files={files}
            collapsed={ui.collapsed}
            onToggle={(path, isOpen) => {
              if (isOpen) ui.collapsed.add(path);
              else ui.collapsed.delete(path);
              render();
            }}
            onOpen={openDiff}
          />
          {files.length < total ? <Text as="p" tone="muted">{`Too large to list: showing ${files.length} of ${total} files.`}</Text> : null}
        </>
      );
    }
  }

  // An error beside a list: the list stays, and this says why it may be out of date.
  const notice =
    tab === "changes" && state?.files && state.error ? (
      <Text as="p" tone="warn" className="notice" role="alert">
        {state.error}
      </Text>
    ) : null;

  return (
    <>
      <Row>
        <Stack gap="none" grow>
          <Text as="div" weight="strong" truncate>
            {repo.name}
          </Text>
          <Text as="div" tone="muted" truncate>
            {repo.branch ?? ""}
          </Text>
        </Stack>
        {view?.loading ? <Spinner label="Refreshing" /> : null}
        <Button variant="outline" onClick={refresh} title="Refresh">
          Refresh
        </Button>
      </Row>
      <Chips
        label="Show"
        options={[
          { value: "uncommitted", label: "Changes" },
          { value: "branch", label: "Branch" },
          { value: "history", label: "History" },
        ]}
        value={tab === "history" ? "history" : mode}
        onChange={(v) => (v === "history" ? showHistory() : setMode(v))}
      />
      {notice}
      {body}
    </>
  );
}
