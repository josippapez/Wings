// The "Elsewhere on this Mac" section: the button that scans the disks for repos with worktrees, what a scan is doing or
// found, and the repos it found, which the page draws as `children`.

import type { ReactNode } from "react";
import { ago, plural } from "wings-plugin-sdk/format";
import { Button, Heading, Notice, Row, Text } from "wings-plugin-ui";

/** What a running scan is doing: looking at the files and folders on the disks, then asking git about the repos it found. */
export type Scanning = { step: "finding" | "checking"; count: number };

export function Elsewhere(p: {
  /** When the last scan finished, in ms since 1970; 0 before the first. */
  at: number;
  scanning: Scanning | null;
  /** Why the last scan failed. */
  error: string | null;
  /** How many repos the children list. */
  found: number;
  onScan: () => void;
  onStop: () => void;
  /** The sections of the repos found, and the notices about loading them. */
  children: ReactNode;
}) {
  const { at, scanning, found } = p;
  const note = scanning
    ? scanning.step === "finding"
      ? `${scanning.count.toLocaleString()} files looked at`
      : `Checking ${plural(scanning.count, "repo")} for worktrees…`
    : at === 0
      ? "Finds repos with worktrees anywhere on this Mac, not just your projects. It reads folder names only, and macOS may ask for access to some folders."
      : found > 0
        ? `Scanned ${ago(at)} · ${plural(found, "repo")} with worktrees`
        : `Scanned ${ago(at)}. No repo outside your projects has more than one worktree.`;
  return (
    <section aria-label="Elsewhere on this Mac" className="elsewhere">
      <Heading
        level={2}
        actions={
          <Row gap="xs">
            <Button variant="outline" size="xs" busy={scanning !== null} onClick={p.onScan}>
              {at === 0 ? "Scan this Mac" : "Scan again"}
            </Button>
            {scanning ? (
              <Button variant="outline" size="xs" onClick={p.onStop}>
                Stop
              </Button>
            ) : null}
          </Row>
        }
      >
        Elsewhere on this Mac
      </Heading>
      <Text as="div" tone="muted" size="small" numeric={scanning !== null} className="elsewhere-note">
        {note}
      </Text>
      {p.error ? (
        <Notice tone="danger" className="note">
          {p.error}
        </Notice>
      ) : null}
      {p.children}
    </section>
  );
}
