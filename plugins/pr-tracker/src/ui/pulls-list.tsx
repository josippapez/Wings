// The project's open pull requests as a list: every open one, with chips to narrow it to yours, the ones waiting for
// your review or drafts. The Pull requests block of a Project page shows it, and so does a "pr" tab opened with no pull
// request. The main script owns the lists and their cache, and keeps this one fresh while it is on screen and Wings is in front.

import { useQuery } from "wings-plugin-sdk/react";
import { Avatar, Badge, Button, Card, Chips, Dot, Empty, List, ListButton, ListItem, Notice, Row, Skeleton, Skeletons, Stack, Text, Time, Title, useAction, useBlockState, useTabState } from "wings-plugin-ui";
import { ch } from "../lib/messages";
import { countPulls, filterOf, filterPulls, splitTitle, type Filter, type PullRow } from "../lib/pulls";
import "./pulls-list.css";
import { LargeText, Tag } from "./parts";
import type { PrArgs } from "../lib/types";

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

/** A row's status colour: the dot is green unless the tone says more. */
const DOT_TONE = { neutral: "muted", info: "info", merged: "info", warning: "warn", danger: "danger" } as const;
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
  // The project's list, which main keeps fresh while it is on screen and Wings is focused, and sends again after a sign-in. `refresh` is Refresh.
  const { data, error, loading, refresh } = useQuery(ch, "pulls", { path });
  const [filter, setFilter] = useFilter(store);
  const sign = useAction();
  const rows = data?.rows ?? [];
  const counts = countPulls(rows);
  const shown = filterPulls(rows, filter);
  const action = () => void sign.run(() => ch.ask("pulls-action", { path }, { timeoutMs: ACTION_TIMEOUT_MS }));
  const failed = sign.error ? <Notice tone="danger">{sign.error}</Notice> : null;

  let body;
  if (error) {
    body = (
      <Stack gap="m" className="pl-error" data-testid="pulls-error">
        <Notice tone="danger">{error.message}</Notice>
        <Row><Button size="sm" variant="outline" onClick={refresh}>Try again</Button></Row>
      </Stack>
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
    const items = shown.map((r) => <PullItem key={r.id} row={r} showScope={filter === "all"} onOpen={onOpen} />);
    // pulls-list.css's rows are cards 6px apart; a native List holds rows edge to edge.
    body = WINGS_NATIVE ? (
      <Stack gap="s">{items}</Stack>
    ) : (
      <List className="pl-list" aria-label="Open pull requests" data-testid="pulls-list">
        {items}
      </List>
    );
  }

  const ready = !error && Boolean(data?.rows);
  return (
    <Stack gap="m" maxWidth={store === "block" ? undefined : 860} className="pl-wrap" data-store={store} data-testid="pulls">
      <Stack as="header" gap="m" className="pl-head">
        <Row justify="between" gap="s">
          {store === "tab" && WINGS_NATIVE ? (
            // pulls-list.css's 20px heading, its count muted and not bold.
            <Row gap="m" align="baseline">
              <LargeText size="large" weight="strong" truncate>{name ? `${name} · ` : ""}Pull requests</LargeText>
              {ready ? <LargeText size="large" tone="muted">{counts.all}</LargeText> : null}
            </Row>
          ) : store === "tab" ? (
            <Title className="pl-heading">
              {name ? `${name} · ` : ""}Pull requests{ready ? <Text tone="muted" className="pl-count"> {counts.all}</Text> : null}
            </Title>
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
      </Stack>
      {body}
    </Stack>
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
  // The native app has no pulls-list.css: a row's muted parts and its flags' look are told here.
  const muted = WINGS_NATIVE ? "muted" : undefined;
  const button = (
    <ListButton className="pl-row" onClick={() => onOpen(row.args)} data-testid="pulls-row">
      <Dot className="pl-dot" data-tone={row.badge.tone} tone={row.badge.tone === "success" ? undefined : DOT_TONE[row.badge.tone as keyof typeof DOT_TONE]} label={row.badge.text} />
      <Stack gap="xs" grow className="pl-main">
        <Row gap="s" className="pl-title-line">
          {tags.map((t, i) => (WINGS_NATIVE ? <Tag key={i}>{t}</Tag> : <Badge key={i} size="sm" variant="outline" className="pl-tag">{t}</Badge>))}
          <Text weight="medium" truncate grow className="pl-title" title={row.title}>{text}</Text>
        </Row>
        <Row gap="m" wrap className="pl-meta">
          <Text size="tiny" tone="muted" className="pl-number">{row.number}</Text>
          {row.author ? <Row gap="s" className="pl-author"><Avatar name={row.author} size="small" /><Text size="small" tone={muted}>{row.author}</Text></Row> : null}
          <Text mono size="tiny" tone="muted" truncate className="pl-branch" title={row.branch}>{target ? `${source} → ${target}` : source}</Text>
        </Row>
      </Stack>
      <Stack gap="xs" className="pl-side">
        <Row gap="s" wrap justify="end" className="pl-flags">
          {showScope && SCOPE_WORD[row.scope] ? <Badge size="sm" variant={WINGS_NATIVE ? "secondary" : "outline"} className="pl-flag" data-kind="scope">{SCOPE_WORD[row.scope]}</Badge> : null}
          {row.agent ? <Badge size="sm" variant="outline" tone="info" className="pl-flag" data-kind="agent">{AGENT_WORD[row.agent]}</Badge> : null}
          {/* pulls-list.css paints it in the info colour on the flags' grey. */}
          {row.unread ? (
            WINGS_NATIVE ? <Badge size="sm" variant="secondary"><Text tone="info">{row.unread} new</Text></Badge> : <Badge size="sm" tone="warn" className="pl-flag" data-kind="unread">{row.unread} new</Badge>
          ) : null}
        </Row>
        <Row gap="s" wrap justify="end" className="pl-facts">
          <Text size="small" tone={muted}>{row.badge.text}</Text>
          {row.review ? <Text size="small" tone="muted">{row.review}</Text> : null}
          {checks ? <Text size="small" tone={checksTone} className="pl-checks" data-tone={checksTone}>{checks}</Text> : null}
          <Text size="small" tone="muted"><Time iso={row.updated} /></Text>
        </Row>
      </Stack>
    </ListButton>
  );
  // pulls-list.css draws a row as a card; the app's list item is a plain row.
  if (WINGS_NATIVE) return <Card>{button}</Card>;
  return (
    <ListItem className="pl-item" data-scope={row.scope}>
      {button}
    </ListItem>
  );
}
