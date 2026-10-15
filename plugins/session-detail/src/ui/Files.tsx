import { Chip, Heading, List, ListButton, ListItem, Text } from "wings-plugin-ui";

import { basename } from "./format";

/** The files the session edited, each opening its diff, and a button for every change in the working tree. */
export function Files({ files, openDiff }: { files: [string, number][]; openDiff: (path: string | null) => void }) {
  const all = (
    <Chip title="Everything changed in the working tree, against HEAD" onClick={() => openDiff(null)}>
      View all changes
    </Chip>
  );
  return (
    <>
      <Heading actions={all}>{`Files edited (${files.length})`}</Heading>
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
