// What the tab shows in place of the map: a scan that runs, one that failed, and one main no longer has. Each is a card in the
// middle of the page, under the same toolbar: the way back to the disks, and the scan's root.

import type { ReactNode } from "react";
import { Button, Card, Empty, Gauge, Notice, Page, Row, Spinner, Stats, Text, Title, Toolbar } from "wings-plugin-ui";
import { bytes } from "../lib/format";
import { Elapsed } from "./elapsed";
import type { RunningScan } from "./model";

function Frame({ name, onDisks, actions, children }: { name: string; onDisks: () => void; actions?: ReactNode; children: ReactNode }) {
  return (
    <Page
      toolbar={
        <Toolbar className="dk-toolbar" actions={actions}>
          <Button variant="ghost" size="sm" className="dk-disks" onClick={onDisks}>← Disks</Button>
          <Title as="h1" truncate>{name}</Title>
        </Toolbar>
      }
    >
      <div className="dk-center">{children}</div>
    </Page>
  );
}

export interface ScanningViewProps {
  name: string;
  /** Null until main has the scan going. */
  scan: RunningScan | null;
  onDisks: () => void;
  onStop: () => void;
}

export function ScanningView({ name, scan, onDisks, onStop }: ScanningViewProps) {
  const found = scan?.progress.bytes ?? 0;
  const expected = scan?.expected || null;
  return (
    <Frame name={name} onDisks={onDisks} actions={<Button variant="outline" size="sm" onClick={onStop}>Stop</Button>}>
      <Card className="dk-scan-card">
        <Row gap="m">
          <Title as="h2" truncate>Scanning {name}</Title>
          {expected === null ? <Spinner label="Scanning" /> : null}
        </Row>
        {scan ? (
          <>
            {expected !== null ? <Gauge label="Progress" percent={Math.min(100, Math.round((found / expected) * 100))} note={`Expecting about ${bytes(expected)}`} /> : null}
            <Stats
              items={[
                { label: "Files scanned", value: scan.progress.entries.toLocaleString() },
                { label: "Found so far", value: bytes(found) },
                { label: "Elapsed", value: <Elapsed since={scan.startedAt} /> },
              ]}
            />
            <Text tone="muted" size="small" mono truncate title={scan.progress.path}>{scan.progress.path || " "}</Text>
          </>
        ) : (
          <Text tone="muted">Starting the scan</Text>
        )}
      </Card>
    </Frame>
  );
}

export interface ScanErrorViewProps {
  name: string;
  message: string;
  onDisks: () => void;
  onRetry: () => void;
}

export function ScanErrorView({ name, message, onDisks, onRetry }: ScanErrorViewProps) {
  return (
    <Frame name={name} onDisks={onDisks}>
      <Card className="dk-scan-card">
        <Title as="h2" truncate>Couldn't scan {name}</Title>
        <Notice tone="danger">{message}</Notice>
        <Row gap="s">
          <Button onClick={onRetry}>Try again</Button>
        </Row>
      </Card>
    </Frame>
  );
}

export interface NoScanViewProps {
  name: string;
  onDisks: () => void;
  onScan: () => void;
}

/** The tab was left on a scan and main has lost it, like after Wings restarted. */
export function NoScanView({ name, onDisks, onScan }: NoScanViewProps) {
  return (
    <Frame name={name} onDisks={onDisks}>
      <Empty title={`No results for ${name}`} description="Wings doesn't have this scan any more. Scan it again to see what takes the space.">
        <Button onClick={onScan}>Scan again</Button>
      </Empty>
    </Frame>
  );
}
