// "Allow this schedule?": an agent asks to add or change a timed schedule, which then starts agents by itself. Main opens
// this modal for each ask and closes it once none is left.

import { useEffect, useState } from "react";
import { useQuery } from "wings-plugin-sdk/react";
import { Button, Card, Empty, Loading, Notice, Page, Properties, Row, Stack, Text, useAction } from "wings-plugin-ui";
import type { Approval } from "../lib/approval";
import { ch } from "../lib/messages";
import { AGENTS, describeWhen, type Draft } from "../lib/schedule";

export const page = { place: "view", fill: true };

/** Allow waits this long after an ask shows, so a click meant for something else can't land on it. */
const ARM_MS = 1000;

/** The value, and under it what it was, when a change makes it different. */
function Changed({ now, was }: { now: string; was?: string }) {
  if (was === undefined || was === now) return <Text>{now}</Text>;
  return (
    <Stack gap="xs">
      <Text>{now}</Text>
      <Text tone="muted" size="small">
        Was: {was}
      </Text>
    </Stack>
  );
}

function Details({ ask }: { ask: Approval }) {
  const p = ask.proposed;
  const c: Draft | null = ask.current;
  return (
    <Properties
      items={[
        { label: "Project", value: <Text>{ask.projectName}</Text> },
        { label: "Runs", value: <Changed now={describeWhen(p.when)} was={c ? describeWhen(c.when) : undefined} /> },
        { label: "Agent", value: <Changed now={AGENTS[p.agent].label} was={c ? AGENTS[c.agent].label : undefined} /> },
        ...(p.name ? [{ label: "Name", value: <Changed now={p.name} was={c?.name || undefined} /> }] : []),
        { label: "Prompt", value: <Changed now={p.prompt} was={c?.prompt} /> },
      ]}
    />
  );
}

function ApprovalView({ approvals, error, onAnswer }: { approvals: Approval[] | null; error: string | null; onAnswer: (id: string, allow: boolean) => Promise<void> }) {
  const ask = approvals?.[0];
  const [armed, setArmed] = useState(false);
  useEffect(() => {
    setArmed(false);
    if (!ask) return;
    const timer = setTimeout(() => setArmed(true), ARM_MS);
    return () => clearTimeout(timer);
  }, [ask?.id]);
  const { run, busy, error: answerError } = useAction();

  if (error) return <Empty title="Couldn't load what's waiting" description={error} />;
  if (!approvals) return <Loading />;
  if (!ask) return <Empty title="Nothing is waiting" description="The agent stopped waiting, or the request was answered." />;
  const more = approvals.length - 1;
  return (
    <Page>
      <Stack gap="m">
        <Text>
          {ask.asker ? `"${ask.asker}"` : "An agent"} asks to {ask.current ? "change a schedule" : "add a schedule"} in {ask.projectName}.{more > 0 ? ` ${more} more waiting.` : ""}
        </Text>
        <Card compact>
          <Details ask={ask} />
        </Card>
        <Text tone="muted" size="small">
          It starts the agent with this prompt by itself, without asking again, while Wings runs.
        </Text>
        {answerError && <Notice tone="danger">{answerError}</Notice>}
        <Row gap="s" justify="end">
          <Button variant="ghost" disabled={busy} onClick={() => run(() => onAnswer(ask.id, false))}>
            Deny
          </Button>
          <Button disabled={!armed || busy} onClick={() => run(() => onAnswer(ask.id, true))}>
            Allow
          </Button>
        </Row>
      </Stack>
    </Page>
  );
}

export default function ApprovePage() {
  const { data, error } = useQuery(ch, "approvals", undefined);
  return <ApprovalView approvals={data} error={error?.message ?? null} onAnswer={(id, allow) => ch.ask("answerApproval", { id, allow })} />;
}
