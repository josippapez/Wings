// The project's open pull requests as a list: every open one, with chips to narrow it to yours, the ones waiting for
// your review or drafts. The Pull requests block of a Project page shows it, and so does a "pr" tab opened with no pull
// request. The main script owns the lists and their cache; this asks it, only while Wings is in front and the list is on screen.

import { useCallback, useEffect, useRef, useState } from "react";
import { pollWhileFocused } from "wings-plugin-sdk/poll";
import { Avatar, Button, Chips, Empty, Row, Stack, Text } from "wings-plugin-ui";
import { countPulls, filterOf, filterPulls, splitTitle, type Filter, type PullRow, type PullsAction, type PullsRequest, type PullsResponse } from "../lib/pulls";
import { ago } from "../lib/threads";
import "./pulls-list.css";
import type { PrArgs } from "../lib/types";

const POLL_MS = 60_000;

type Answer = Omit<PullsResponse, "type" | "rid">;

/** The project's list: once at the start, then on each poll while Wings is focused. A hidden block asks for nothing. `ask` is Refresh. */
function usePulls(path: string): { answer: Answer | null; ask: () => void; busy: boolean } {
  const [answer, setAnswer] = useState<Answer | null>(null);
  const [busy, setBusy] = useState(false);
  const asker = useRef<() => void>(() => {});
  useEffect(() => {
    let pending = "";
    const ask = () => {
      if (document.visibilityState === "hidden") return;
      pending = crypto.randomUUID();
      setBusy(true);
      wings.broadcast({ type: "pulls-request", rid: pending, path } satisfies PullsRequest).catch((e: Error) => {
        setBusy(false);
        setAnswer({ error: e.message });
      });
    };
    asker.current = ask;
    const off = wings.onBroadcast((message) => {
      const m = message as PullsResponse | null;
      if (m?.type === "pulls-response" && m.rid === pending) {
        setBusy(false);
        setAnswer({ rows: m.rows, none: m.none, host: m.host, problem: m.problem, error: m.error });
      }
      // A sign-in, token or install finished in main.
      if ((message as { type?: string } | null)?.type === "pulls-changed") ask();
    });
    const shown = () => ask();
    document.addEventListener("visibilitychange", shown);
    ask();
    const stop = pollWhileFocused(ask, POLL_MS);
    return () => {
      off();
      stop();
      document.removeEventListener("visibilitychange", shown);
    };
  }, [path]);
  return { answer, ask: useCallback(() => asker.current(), []), busy };
}

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

/** The chosen filter, kept with the tab or the block so it is there when it opens again. */
function useFilter(store: "tab" | "block"): [Filter, (f: Filter) => void] {
  const [filter, setFilter] = useState<Filter>("all");
  useEffect(() => {
    void (store === "tab" ? wings.tabState() : wings.blockState()).then((saved) => setFilter(filterOf(saved))).catch(() => {});
  }, [store]);
  const pick = (f: Filter) => {
    setFilter(f);
    void (store === "tab" ? wings.setTabState({ filter: f }) : wings.setBlockState({ filter: f })).catch(() => {});
  };
  return [filter, pick];
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
  const { answer, ask, busy } = usePulls(path);
  const [filter, setFilter] = useFilter(store);
  const rows = answer?.rows ?? [];
  const counts = countPulls(rows);
  const shown = filterPulls(rows, filter);
  const action = () => void wings.broadcast({ type: "pulls-action", path } satisfies PullsAction);

  let body;
  if (!answer) body = <ListSkeleton />;
  else if (answer.none) {
    body = answer.host ? (
      <Stack gap="s">
        <Text tone="muted">No pull request provider found for {answer.host}. For GitHub Enterprise or self-managed GitLab, sign in to it with gh or glab in a terminal, then check again.</Text>
        <Row><Button size="sm" onClick={action}>Check again</Button></Row>
      </Stack>
    ) : (
      <Text tone="muted">No GitHub, GitLab, Azure DevOps, Bitbucket or Codeberg remote for this project.</Text>
    );
  } else if (answer.problem) {
    body = (
      <Stack gap="s">
        <Text tone="muted">{answer.problem.message}</Text>
        <Row><Button size="sm" onClick={action}>{answer.problem.action}</Button></Row>
      </Stack>
    );
  } else if (answer.error) {
    body = (
      <div className="pl-error" role="alert" data-testid="pulls-error">
        <Text tone="danger">{answer.error}</Text>
        <Button size="sm" variant="outline" onClick={ask}>Try again</Button>
      </div>
    );
  } else if (!shown.length) body = <Empty title={EMPTY[filter]} description={filter !== "all" && rows.length ? `${rows.length} open in all.` : undefined} />;
  else {
    body = (
      <ul className="pl-list" aria-label="Open pull requests" data-testid="pulls-list">
        {shown.map((r) => <PullItem key={r.id} row={r} showScope={filter === "all"} onOpen={onOpen} />)}
      </ul>
    );
  }

  const ready = Boolean(answer?.rows);
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
          <Button size="sm" variant="outline" busy={busy} onClick={ask} data-testid="pulls-refresh">Refresh</Button>
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
    <div className="pl-skeleton" role="status" aria-busy="true" aria-label="Loading pull requests">
      {[0, 1, 2].map((i) => <div key={i} className="pl-skel" />)}
    </div>
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
  const when = ago(row.updated);
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
            {when ? <time dateTime={row.updated} title={new Date(row.updated).toLocaleString()}>{when}</time> : null}
          </span>
        </span>
      </button>
    </li>
  );
}
