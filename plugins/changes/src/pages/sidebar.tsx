// The Changes sidebar page: the changed files of the focused pane's repo as a folder tree, and its commit history. The
// plugin's main script runs git and keeps what this page shows fresh; a click asks it to open the diff viewer.

import { useRef, useState } from "react";
import { plural } from "wings-plugin-sdk/format";
import { useQuery } from "wings-plugin-sdk/react";
import { Button, Card, Chips, Empty, Loading, Row, Spinner, Stack, Text } from "wings-plugin-ui";

import { FileTree } from "../ui/FileTree";
import { History } from "../ui/History";
import { useHistory } from "../ui/useHistory";
import { ch } from "../lib/messages";
import type { CommitRow, Mode, Repo } from "../lib/types";
import "./sidebar.css";

export const page = { place: "sidebar" };

/** How long the search waits after the last key before it asks git. */
const SEARCH_MS = 250;

/** What the chips choose between: the two lists of changes, and the commit history. */
type View = Mode | "history";

export default function Sidebar() {
  const [view, setView] = useState<View>("uncommitted");
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());
  const [query, setQuery] = useState("");
  const [all, setAll] = useState(false);
  /** What the search box holds, which becomes the query after a pause. */
  const typed = useRef("");
  const searchTimer = useRef<ReturnType<typeof setTimeout>>(undefined);
  /** The repo the header showed last, for the moment after a switch when the new answer isn't here yet. */
  const known = useRef<Repo | null>(null);

  const onHistory = view === "history";
  const mode: Mode = view === "branch" ? "branch" : "uncommitted";
  const changes = useQuery(ch, "changes", { mode }, { skip: onHistory });
  const history = useHistory({ query, all, skip: !onHistory });

  /** What the tab on screen last answered: null before its first answer, and its `repo` is null when no pane is in one. */
  const answered = onHistory ? history.shown : changes.data;
  const loading = onHistory ? history.loading : changes.loading;
  const error = onHistory ? history.error : changes.error?.message;
  if (answered) known.current = answered.repo;
  const repo = answered ? answered.repo : known.current;
  if (!answered && !repo && !error) return <Loading />;
  if (answered && !repo) return <Empty title="No git repo in your panes." description="Open a terminal in a repo and its changes show here." />;

  const openDiff = (path: string | null) => repo && void ch.ask("open", { root: repo.root, mode, path });
  const refresh = () => {
    // Sent first, so main forgets its kept repos before it hears the asks.
    void ch.tell("refresh");
    changes.refresh();
    history.refresh();
  };
  const onSearch = (text: string) => {
    typed.current = text;
    clearTimeout(searchTimer.current);
    searchTimer.current = setTimeout(() => setQuery(typed.current.trim()), SEARCH_MS);
  };

  let body = null;
  if (onHistory) {
    const shown = history.shown;
    body = (
      <History
        view={shown?.repo ? { rows: shown.rows, more: shown.more, pending: history.pending, loadingMore: history.loadingMore } : null}
        query={query}
        all={all}
        searchText={typed.current}
        onSearch={onSearch}
        onAll={setAll}
        onGraph={() => repo && void ch.ask("graph", { root: repo.root })}
        onOpen={([hash, subject, author, at]: CommitRow) => repo && void ch.ask("commit", { root: repo.root, hash, subject, author, at })}
        onMore={history.loadMore}
      />
    );
  } else if (changes.data?.repo) {
    const { files, total, added, removed, partial, against } = changes.data;
    if (total === 0) {
      body = <Empty title={mode === "branch" ? "Nothing changed on this branch." : "No uncommitted changes."} description={against} />;
    } else {
      body = (
        <>
          <Card compact>
            <Row>
              <Stack gap="none" grow>
                <div>
                  {plural(total, "file")}
                  {" "}
                  <Text tone="success" numeric>{`+${added}`}</Text>
                  {" "}
                  <Text tone="danger" numeric>{`−${removed}`}</Text>
                  {partial ? <Text tone="muted">{" (some new files not counted)"}</Text> : null}
                </div>
                <Text as="div" tone="muted" truncate>
                  {against}
                </Text>
              </Stack>
              <Button variant="outline" onClick={() => openDiff(null)}>View all</Button>
            </Row>
          </Card>
          <FileTree
            files={files}
            collapsed={collapsed}
            onToggle={(path, isOpen) =>
              setCollapsed((prev) => {
                const next = new Set(prev);
                if (isOpen) next.add(path);
                else next.delete(path);
                return next;
              })
            }
            onOpen={openDiff}
          />
          {files.length < total ? <Text as="p" tone="muted">{`Too large to list: showing ${files.length} of ${total} files.`}</Text> : null}
        </>
      );
    }
  }

  return (
    <>
      {repo ? (
        <Row>
          <Stack gap="none" grow>
            <Text as="div" weight="strong" truncate>
              {repo.name}
            </Text>
            <Text as="div" tone="muted" truncate>
              {repo.branch}
            </Text>
          </Stack>
          {loading ? <Spinner label="Refreshing" /> : null}
          <Button variant="outline" onClick={refresh} title="Refresh">
            Refresh
          </Button>
        </Row>
      ) : null}
      <Chips
        label="Show"
        options={[
          { value: "uncommitted", label: "Changes" },
          { value: "branch", label: "Branch" },
          { value: "history", label: "History" },
        ]}
        value={view}
        onChange={setView}
      />
      {error ? (
        <Text as="p" tone="warn" className="notice" role="alert">
          {error}
        </Text>
      ) : null}
      {body}
    </>
  );
}
