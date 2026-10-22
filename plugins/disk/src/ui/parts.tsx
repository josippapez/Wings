// Small pieces the tab's views share.

import type { ReactNode } from "react";
import { Button, Notice, Row } from "wings-plugin-ui";

/** A message the Collector bar shows under its head, until it is dismissed. */
export interface Message {
  id: number;
  tone: "success" | "warn" | "danger";
  text: string;
}

/** A notice with a button that takes it away, for a message that stays until you've read it. */
export function Dismissible({ tone, onDismiss, children }: { tone: "info" | "success" | "warn" | "danger"; onDismiss: () => void; children: ReactNode }) {
  return (
    <Row gap="s" align="start" className="dk-notice">
      <Notice tone={tone}>{children}</Notice>
      <Button variant="ghost" size="icon-xs" aria-label="Dismiss" title="Dismiss" onClick={onDismiss}>×</Button>
    </Row>
  );
}
