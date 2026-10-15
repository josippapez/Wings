// The project's open pull requests as a list: every open one, with chips to narrow it to yours, the ones waiting for
// your review or drafts. The Pull requests block of a Project page shows it, and so does a "pr" tab opened with no pull
// request. The main script owns the lists and their cache; this asks it, only while Wings is in front and the list is on screen.

import { useEffect } from "react";
import { useAsk } from "wings-plugin-sdk/react";
import { Avatar, Button, Chips, Empty, Row, Skeleton, Skeletons, Stack, Text, Time, useAction, useBlockState, useTabState } from "wings-plugin-ui";
import { ch } from "../lib/messages";
import { countPulls, filterOf, filterPulls, splitTitle, type Filter, type PullRow } from "../lib/pulls";
import "./pulls-list.css";
import type { PrArgs } from "../lib/types";

const POLL_MS = 60_000;
/** A sign-in waits for the browser for up to 5 minutes, and main answers when it is done. */
const ACTION_TIMEOUT_MS = 330_000;

const FILTERS: { value: Filter; label: string }[] = [
  { value: "all", label: "All open" },
  { value: "mine", label: "Mine" },
  { value: "review", label: "Review requested" },
  { value: "drafts", label: "Drafts" },
];

const EMPTY: Record<Filter, string> = {
  all: "No open pull requests",
  mine: "None of the open pull requests are yours",
  review: "Nobody has asked for your review",
  drafts: "No draft pull requests",
};

const ALL: { filter: Filter } = { filter: "all" };
const savedFilter = (saved: unknown) => ({ filter: filterOf(saved) });

/** The chosen filter, kept with the tab or the block so it is there when it opens again. Only the state of this place holds anything: the other reads nothing. */
function useFilter(store: "tab" | "block"): [Filter, (f: Filter) => void] {
  const tab = useTabState(ALL, savedFilter);
  const block = useBlockState(ALL, savedFilter);
  const [saved, save] = store === "tab" ? tab : block;
  return [saved?.filter ?? "all", (filter) => save({ filter })];
}

interface ListProps {
  path: string;
  /** The project's name, for the header. */
  name?: string;
  /** Where the chosen filter is kept: the tab's state or the block's. */
  store: "tab" | "block";
  onOpen: (args: PrArgs) => void;
}

export function ProjectPulls({ path, name, store, onOpen }: ListProps) {
  // The project's list: once at the start, then on each poll while Wings is focused. `refresh` is Refresh.
  const { data, error, loading, refresh } = useAsk(ch, "pulls", { path }, { pollMs: POLL_MS });
  // A sign-in, token or install finished in main.
  useEffect(() => ch.on("pulls-changed", refresh), [refresh]);
  const [filter, setFilter] = useFilter(store);
  const sign = useAction();
  const rows = data?.rows ?? [];
  const counts = countPulls(rows);
  const shown = filterPulls(rows, filter);
  const action = () => void sign.run(() => ch.ask("pulls-action", { path }, { timeoutMs: ACTION_TIMEOUT_MS }));
  const failed = sign.error ? <Text tone="danger" role="alert">{sign.error}</Text> : null;

  let body;
  if (error) {
    body = (
      <div className="pl-error" role="alert" data-testid="pulls-error">
        <Text tone="danger">{error.message}</Text>
        <Button size="sm" variant="outline" onClick={refresh}>Try again</Button>
      </div>
    );
  } else if (!data) body = <ListSkeleton />;
  else if (data.none) {
    body = data.host ? (
      <Stack gap="s">
        <Text tone="muted">No pull request provider found for {data.host}. For GitHub Enterprise or self-managed GitLab, sign in to it with gh or glab in a terminal, then check again.</Text>
        <Row><Button size="sm" busy={sign.busy} onClick={action}>Check again</Button></Row>
        {failed}
      </Stack>
    ) : (
      <Text tone="muted">No GitHub, GitLab, Azure DevOps, Bitbucket or Codeberg remote for this project.</Text>
    );
  } else if (data.problem) {
    body = (
      <Stack gap="s">
        <Text tone="muted">{data.problem.message}</Text>
        <Row><Button size="sm" busy={sign.busy} onClick={action}>{data.problem.action}</Button></Row>
        {failed}
      </Stack>
    );
  } else if (!shown.length) body = <Empty title={EMPTY[filter]} description={filter !== "all" && rows.length ? `${rows.length} open in all.` : undefined} />;
  else {
    body = (
      <ul className="pl-list" aria-label="Open pull requests" data-testid="pulls-list">
        {shown.map((r) => <PullItem key={r.id} row={r} showScope={filter === "all"} onOpen={onOpen} />)}
      </ul>
    );
  }

  const ready = !error && Boolean(data?.rows);
  return (
    <div className="pl-wrap" data-store={store} data-testid="pulls">
      <header className="pl-head">
        <Row justify="between" gap="s">
          {store === "tab" ? (
            <h1 className="pl-heading">
              {name ? `${name} · ` : ""}Pull requests{ready ? <span className="pl-count"> {counts.all}</span> : null}
            </h1>
          ) : (
            <Text tone="muted" size="small">{ready ? `${counts.all} open` : ""}</Text>
          )}
          <Button size="sm" variant="outline" busy={loading} onClick={refresh} data-testid="pulls-refresh">Refresh</Button>
        </Row>
        {ready ? (
          <Chips
            label="Show"
            value={filter}
            onChange={setFilter}
            options={FILTERS.filter((f) => f.value !== "drafts" || counts.drafts > 0 || filter === "drafts").map((f) => ({ value: f.value, label: `${f.label} ${counts[f.value]}`, "data-testid": `pulls-filter-${f.value}` }))}
          />
        ) : null}
      </header>
      {body}
    </div>
  );
}

function ListSkeleton() {
  return (
    <Skeletons className="pl-skeleton" label="Loading pull requests">
      {[0, 1, 2].map((i) => <Skeleton key={i} height={58} />)}
    </Skeletons>
  );
}

const SCOPE_WORD = { mine: "Yours", review: "Review requested", other: "" } as const;
const AGENT_WORD = { working: "Agent working", here: "Agent here" } as const;

function PullItem({ row, showScope, onOpen }: { row: PullRow; showScope: boolean; onOpen: (args: PrArgs) => void }) {
  const { tags, text } = splitTitle(row.title);
  const [source, target] = row.branch.split(" → ");
  const { failed, running, passed } = row.checks;
  const checks = failed ? `${failed} failing` : running ? `${running} running` : passed ? `${passed} passed` : null;
  const checksTone = failed ? "danger" : running ? "warn" : "success";
  return (
    <li className="pl-item" data-scope={row.scope}>
      <button type="button" className="pl-row" onClick={() => onOpen(row.args)} data-testid="pulls-row">
        <span className="pl-dot" data-tone={row.badge.tone} role="img" aria-label={row.badge.text} title={row.badge.text} />
        <span className="pl-main">
          <span className="pl-title-line">
            {tags.map((t, i) => <span key={i} className="pl-tag">{t}</span>)}
            <span className="pl-title" title={row.title}>{text}</span>
          </span>
          <span className="pl-meta">
            <span className="pl-number">{row.number}</span>
            {row.author ? <span className="pl-author"><Avatar name={row.author} size="small" /><span>{row.author}</span></span> : null}
            <span className="pl-branch" title={row.branch}>
              <code>{source}</code>
              {target ? <><span aria-hidden="true">→</span><code>{target}</code></> : null}
            </span>
          </span>
        </span>
        <span className="pl-side">
          <span className="pl-flags">
            {showScope && SCOPE_WORD[row.scope] ? <span className="pl-flag" data-kind="scope">{SCOPE_WORD[row.scope]}</span> : null}
            {row.agent ? <span className="pl-flag" data-kind="agent">{AGENT_WORD[row.agent]}</span> : null}
            {row.unread ? <span className="pl-flag" data-kind="unread">{row.unread} new</span> : null}
          </span>
          <span className="pl-facts">
            <span>{row.badge.text}</span>
            {row.review ? <span>{row.review}</span> : null}
            {checks ? <span className="pl-checks" data-tone={checksTone}>{checks}</span> : null}
            <Time iso={row.updated} />
          </span>
        </span>
      </button>
    </li>
  );
}
