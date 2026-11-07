// A page that uses every component of wings-plugin-ui/native, with data like the HUD sidebar's. The tests and the `shot`
// example draw it. Build with `node apps/native/pages/build.mjs`.
import { useState } from "react";
import { Badge, Button, Card, Disclosure, Dot, Gauge, Heading, List, ListItem, Page, Row, Select, Stack, Text, Toolbar, mount } from "wings-plugin-ui/native";

const sessions = [
  { value: "web", label: "web · Refactor the API client" },
  { value: "api", label: "api · Fix the login redirect" },
  { value: "docs", label: "docs · Archived", disabled: true },
];

type Tone = "muted" | "info" | "warn" | "danger" | undefined;

function Server({ name, tone, note }: { name: string; tone?: Tone; note?: string }) {
  return (
    <ListItem>
      <Dot tone={tone} />
      <Text grow truncate>
        {name}
      </Text>
      {note ? <Text tone={tone === "danger" ? "danger" : "muted"}>{note}</Text> : null}
    </ListItem>
  );
}

function Components() {
  const [refreshed, setRefreshed] = useState(0);
  const [retried, setRetried] = useState(0);
  const [session, setSession] = useState("web");

  return (
    <Page toolbar={<Toolbar title="Components" actions={<Badge tone="info">Preview</Badge>} />}>
      <Row gap="m">
        <Button onClick={() => setRefreshed((n) => n + 1)}>Refresh</Button>
        <Text size="small" tone="muted" truncate grow>{`Session ${session}, refreshed ${refreshed}, retried ${retried}`}</Text>
      </Row>
      <Select label="Session" value={session} onChange={setSession} options={sessions} />
      <Disclosure summary="2 need sign-in">
        <List>
          <Server name="notes" tone="muted" note="Needs sign-in" />
          <Server name="calendar" tone="muted" note="Needs sign-in" />
        </List>
        <Row>
          <Button variant="outline" onClick={() => setRetried((n) => n + 1)}>
            Retry
          </Button>
        </Row>
      </Disclosure>
      <Heading>Usage</Heading>
      <Card>
        <Gauge label="5-hour limit" percent={40} note="Resets in 2h 10m" levels={[70, 90]} />
        <Gauge label="Weekly limit" percent={85} note="Resets in 3d 4h" levels={[70, 90]} />
        <Gauge label="Memory" percent={96} note="15.4 GB of 16 GB in use" levels={[70, 90]} />
        <Gauge label="Disk" percent={12} />
      </Card>
      <Heading>MCP servers (5)</Heading>
      <List>
        <Server name="filesystem" />
        <Server name="browser" tone="info" note="Used" />
        <Server name="search" tone="warn" note="Last call failed" />
        <Server name="database" tone="danger" note="Failed" />
        <Server name="a server with a very long name that has to be cut short somewhere before the edge" tone="muted" note="Connecting" />
      </List>
      <Heading>Skills (3)</Heading>
      <List>
        {["review", "simplify", "run"].map((name) => (
          <ListItem key={name}>
            <Stack gap="none" grow>
              <Text truncate>{name}</Text>
            </Stack>
          </ListItem>
        ))}
      </List>
    </Page>
  );
}

// A tab page, which fills its tab as `page = { place: "tab", fill: true }` does.
mount(<Components />, { fill: true });
