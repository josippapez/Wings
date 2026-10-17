import { useState } from "react";
import { errorMessage } from "wings-plugin-sdk/async";
import { basename } from "wings-plugin-sdk/format";
import { Chip, Heading, List, ListButton, ListItem, Text } from "wings-plugin-ui";

import { ch } from "../lib/messages";

/** The files the session edited, each opening its diff, and a button for every change in the working tree. */
export function Files({ sessionId, files }: { sessionId: string; files: [string, number][] }) {
  /** Why the last diff didn't open. */
  const [error, setError] = useState<string | null>(null);
  const openDiff = (path: string | null) => {
    setError(null);
    ch.ask("diff", { sessionId, path }).catch((e) => setError(errorMessage(e)));
  };
  const all = (
    <Chip title="Everything changed in the working tree, against HEAD" onClick={() => openDiff(null)}>
      View all changes
    </Chip>
  );
  return (
    <>
      <Heading actions={all}>{`Files edited (${files.length})`}</Heading>
      {error ? (
        <Text as="p" tone="danger" role="alert">
          {error}
        </Text>
      ) : null}
      <List>
        {files.map(([path, n]) => (
          <ListItem key={path}>
            <ListButton title={`Diff of ${path}`} onClick={() => openDiff(path)}>
              <Text grow truncate>
                {basename(path)}
              </Text>
              <Text tone="muted">{n > 1 ? `${n} edits` : "1 edit"}</Text>
            </ListButton>
          </ListItem>
        ))}
      </List>
    </>
  );
}
