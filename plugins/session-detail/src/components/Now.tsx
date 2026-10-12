import { Heading, List, ListItem, Spinner, Text } from "wings-plugin-ui";

import type { SessionView } from "../lib/types";

/** The tool calls still running. */
export function Now({ running }: { running: SessionView["running"] }) {
  return (
    <>
      <Heading>Now</Heading>
      <List compact>
        {running.map((t, i) => (
          <ListItem key={i}>
            <Spinner />
            <Text as="strong">{t.name}</Text>
            <Text grow truncate tone="muted">
              {t.summary}
            </Text>
          </ListItem>
        ))}
      </List>
    </>
  );
}
