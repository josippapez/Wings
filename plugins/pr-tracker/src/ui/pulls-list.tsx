// The project's open pull requests as a list, yours first and then the ones waiting for your review. The Pull requests
// block of a Project page shows it, and so does a "pr" tab opened with no pull request. The main script owns the lists
// and their cache; this asks it, only while Wings is in front and the list is on screen.

import { useEffect, useState } from "react";
import { pollWhileFocused } from "wings-plugin-sdk/poll";
import { Badge, Button, Empty, List, ListButton, ListItem, Loading, Row, Stack, Text } from "wings-plugin-ui";
import type { PullRow, PullsAction, PullsRequest, PullsResponse } from "../lib/pulls";
import type { PrArgs } from "../lib/types";

const POLL_MS = 60_000;
const BADGE_TONE: Record<string, "info" | "success" | "warn" | "danger" | undefined> = { info: "info", success: "success", warning: "warn", danger: "danger", merged: "info" };

type Answer = Omit<PullsResponse, "type" | "rid">;

/** The project's list: once at the start, then on each poll while Wings is focused. A hidden block asks for nothing. */
function usePulls(path: string): Answer | null {
  const [answer, setAnswer] = useState<Answer | null>(null);
  useEffect(() => {
    let pending = "";
    const ask = () => {
      if (document.visibilityState === "hidden") return;
      pending = crypto.randomUUID();
      wings.broadcast({ type: "pulls-request", rid: pending, path } satisfies PullsRequest).catch((e: Error) => setAnswer({ error: e.message }));
    };
    const off = wings.onBroadcast((message) => {
      const m = message as PullsResponse | null;
      if (m?.type === "pulls-response" && m.rid === pending) setAnswer({ rows: m.rows, none: m.none, host: m.host, problem: m.problem, error: m.error });
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
  return answer;
}

export function ProjectPulls({ path, onOpen }: { path: string; onOpen: (args: PrArgs) => void }) {
  const answer = usePulls(path);
  if (!answer) return <Loading>Loading pull requests</Loading>;
  if (answer.none) {
    return answer.host ? (
      <Stack gap="s">
        <Text tone="muted">No pull request provider found for {answer.host}. For GitHub Enterprise or self-managed GitLab, sign in to it with gh or glab in a terminal, then check again.</Text>
        <Row><Button size="sm" onClick={() => void wings.broadcast({ type: "pulls-action", path } satisfies PullsAction)}>Check again</Button></Row>
      </Stack>
    ) : (
      <Text tone="muted">No GitHub, GitLab, Azure DevOps, Bitbucket or Codeberg remote for this project.</Text>
    );
  }
  if (answer.problem) {
    return (
      <Stack gap="s">
        <Text tone="muted">{answer.problem.message}</Text>
        <Row><Button size="sm" onClick={() => void wings.broadcast({ type: "pulls-action", path } satisfies PullsAction)}>{answer.problem.action}</Button></Row>
      </Stack>
    );
  }
  if (answer.error) return <Text tone="danger" role="alert">{answer.error}</Text>;
  const rows = answer.rows ?? [];
  if (!rows.length) return <Empty title="No open pull requests" />;
  return (
    <List compact aria-label="Open pull requests">
      {rows.map((r) => <PullItem key={r.id} row={r} onOpen={onOpen} />)}
    </List>
  );
}

function PullItem({ row, onOpen }: { row: PullRow; onOpen: (args: PrArgs) => void }) {
  return (
    <ListItem>
      <ListButton onClick={() => onOpen(row.args)}>
        <Row gap="s" justify="between">
          <Stack gap="xs" grow>
            <Text weight={row.mine ? "medium" : undefined} truncate>{row.title}</Text>
            <Text tone="muted" size="tiny" truncate>{row.detail}</Text>
          </Stack>
          <Badge tone={BADGE_TONE[row.badge.tone]}>{row.badge.text}</Badge>
        </Row>
      </ListButton>
    </ListItem>
  );
}
