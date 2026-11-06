// The fixture plugin's page: the count main keeps, and a button that asks main to add one. Build with `node apps/native/pages/build.mjs`.
import { useState } from "react";
import { useQuery } from "wings-plugin-sdk/react";
import { Badge, Button, Page, Row, Stack, Text, Toolbar, mount } from "wings-plugin-ui/native";
import { ch } from "./messages";

function Fixture() {
  const { data, error } = useQuery(ch, "count", undefined);
  const [failed, setFailed] = useState<string | null>(null);
  const problem = failed ?? error?.message ?? null;

  return (
    <Page toolbar={<Toolbar title="Fixture" actions={<Badge tone={problem ? "danger" : "success"}>{problem ? "Problem" : "Plugin"}</Badge>} />}>
      <Stack gap="l">
        <Row gap="m">
          <Button onClick={() => ch.ask("increment").then(() => setFailed(null), (e: Error) => setFailed(e.message))}>Increment</Button>
          <Text weight="medium">{data === null ? "Loading…" : `Count: ${data}`}</Text>
        </Row>
        {problem ? <Text tone="danger">{problem}</Text> : null}
      </Stack>
    </Page>
  );
}

mount(<Fixture />);
