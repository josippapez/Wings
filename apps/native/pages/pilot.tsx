// The pilot page: React that the native app draws with its own controls. Build with `node apps/native/pages/build.mjs`.
import { useEffect, useState } from "react";
import { Badge, Button, Page, Row, Stack, Text, Toolbar, mount } from "wings-plugin-ui/native";

function Pilot() {
  const [count, setCount] = useState(0);
  const [ready, setReady] = useState(false);
  useEffect(() => setReady(true), []);

  return (
    <Page toolbar={<Toolbar title="Pilot" actions={<Badge tone={ready ? "success" : "info"}>{ready ? "Ready" : "Starting"}</Badge>} />}>
      <Stack gap="l">
        <Row gap="m">
          <Button onClick={() => setCount((n) => n + 1)}>Increment</Button>
          <Text weight="medium">{`Count: ${count}`}</Text>
        </Row>
        <Text size="small" tone="muted">
          This page is React running in the app's own JS engine. The app draws what it renders and sends clicks back.
        </Text>
      </Stack>
    </Page>
  );
}

// A tab page, which fills its tab as `page = { place: "tab", fill: true }` does.
mount(<Pilot />, { fill: true });
