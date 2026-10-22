// The disks overview: the disks to scan, the folders to scan, and the scans Wings is keeping. A view with no data of its own:
// the tab page feeds it from main, a fixture from made-up data.

import { useState, type ReactNode } from "react";
import { basename } from "wings-plugin-sdk/format";
import { Badge, Button, Card, Field, Gauge, Heading, List, ListItem, Loading, Notice, Page, Row, Stack, Text, TextInput, Time, Title, Toolbar } from "wings-plugin-ui";
import { bytes } from "../lib/format";
import type { DiskRow, ScanSummary } from "../lib/messages";
import { Dismissible } from "./parts";
import { rootName } from "./model";

export interface OverviewProps {
  /** `null` while main reads them. */
  disks: DiskRow[] | null;
  /** Why main couldn't, like Wings being too old or the permission being off. */
  disksError: string | null;
  projects: WingsProject[];
  /** The starred folders' paths. */
  starred: string[];
  /** The scans main keeps, or `null` while it reads them. */
  scans: ScanSummary[] | null;
  /** What went wrong when a scan wouldn't start. */
  error: string | null;
  onDismissError: () => void;
  onScan: (root: string) => void;
  onStar: (path: string, on: boolean) => void;
  onOpen: (root: string) => void;
  onForget: (root: string) => void;
}

function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <Stack as="section" gap="m" aria-label={title}>
      <Heading>{title}</Heading>
      {children}
    </Stack>
  );
}

/** Green while at least half the disk is free, red when it's nearly full. */
const fullness = (percent: number) => (percent < 50 ? "success" : percent >= 90 ? "danger" : percent >= 75 ? "warn" : undefined);

function DiskCard({ disk, startup, onScan }: { disk: DiskRow; startup: boolean; onScan: (root: string) => void }) {
  const percent = disk.total > 0 ? Math.round((disk.used / disk.total) * 100) : 0;
  const purgeable = disk.purgeable > 0 ? `, includes ${bytes(disk.purgeable)} purgeable` : "";
  return (
    <Card as="section" aria-label={disk.name} className="dk-disk">
      <Row gap="m">
        <Row gap="s" grow>
          <Title as="h3" truncate title={disk.mount}>{disk.name}</Title>
          {disk.kind !== "unknown" ? <Badge variant="secondary" size="sm">{disk.kind === "ssd" ? "SSD" : "HDD"}</Badge> : null}
          {disk.removable ? <Badge variant="outline" size="sm">Removable</Badge> : null}
        </Row>
        <Button variant={startup ? "default" : "secondary"} size="sm" aria-label={`Scan ${disk.name}`} onClick={() => onScan(disk.mount)}>Scan</Button>
      </Row>
      <Gauge label={`${bytes(disk.used)} used`} percent={percent} tone={fullness(percent)} note={`${bytes(disk.free)} free of ${bytes(disk.total)}${purgeable}`} />
    </Card>
  );
}

interface Place {
  path: string;
  name: string;
}

function placesOf(projects: WingsProject[], starred: string[]): Place[] {
  const places: Place[] = [{ path: "~", name: "Home folder" }];
  const add = (path: string, name: string) => places.some((p) => p.path === path) || places.push({ path, name });
  for (const project of projects) add(project.path, project.name);
  for (const path of starred) add(path, basename(path));
  return places;
}

function PlaceRow({ place, starred, onScan, onStar }: { place: Place; starred: boolean; onScan: (root: string) => void; onStar: (path: string, on: boolean) => void }) {
  return (
    <ListItem className="dk-place">
      <div className="dk-place-text">
        <Text weight="medium" truncate>{place.name}</Text>
        <Text tone="muted" size="small" truncate title={place.path}>{place.path}</Text>
      </div>
      <Button
        variant="ghost"
        size="icon-sm"
        className="dk-star"
        aria-pressed={starred}
        aria-label={`Star ${place.name}`}
        title={starred ? "Remove the star" : "Star this folder"}
        onClick={() => onStar(place.path, !starred)}
      >
        {starred ? "★" : "☆"}
      </Button>
      <Button variant="secondary" size="sm" aria-label={`Scan ${place.name}`} onClick={() => onScan(place.path)}>Scan</Button>
    </ListItem>
  );
}

/** Scan a folder by its path. A path has to be full or start with `~`, which Wings says before it asks main. */
function ScanFolder({ onScan }: { onScan: (root: string) => void }) {
  const [path, setPath] = useState("");
  const [error, setError] = useState<string | null>(null);
  const submit = () => {
    const root = path.trim();
    if (!root.startsWith("/") && !root.startsWith("~")) return setError("Use a full path, like ~/Downloads or /Users/you/Downloads.");
    setError(null);
    setPath("");
    onScan(root);
  };
  return (
    <Row
      as="form"
      gap="m"
      align="start"
      className="dk-scan-form"
      onSubmit={(e) => {
        e.preventDefault();
        submit();
      }}
    >
      <Field inline label="Scan a folder" error={error} className="dk-scan-field">
        <TextInput placeholder="~/Downloads" value={path} spellCheck={false} autoCapitalize="off" autoCorrect="off" onChange={(e) => setPath(e.target.value)} />
      </Field>
      <Button type="submit" variant="secondary" disabled={!path.trim()}>Scan</Button>
    </Row>
  );
}

function ScanRow({ scan, disks, onOpen, onForget }: { scan: ScanSummary; disks: DiskRow[] | null; onOpen: (root: string) => void; onForget: (root: string) => void }) {
  const name = rootName(scan.root, disks);
  return (
    <ListItem className="dk-scan-item">
      <div className="dk-place-text">
        <Text weight="medium" truncate>{name}</Text>
        <Text tone="muted" size="small" truncate title={scan.root}>{scan.root}</Text>
      </div>
      <Text numeric tone={scan.state === "error" ? "danger" : "muted"} className="dk-scan-size">{scan.size !== null ? bytes(scan.size) : scan.state === "scanning" ? "Scanning…" : "Failed"}</Text>
      <Text tone="muted" size="small" className="dk-scan-when"><Time iso={new Date(scan.at).toISOString()} /></Text>
      <Button variant="secondary" size="sm" aria-label={`Open the scan of ${name}`} onClick={() => onOpen(scan.root)}>Open</Button>
      <Button variant="ghost" size="sm" aria-label={`Forget the scan of ${name}`} title="Frees the memory the scan holds" onClick={() => onForget(scan.root)}>Forget</Button>
    </ListItem>
  );
}

export function Overview({ disks, disksError, projects, starred, scans, error, onDismissError, onScan, onStar, onOpen, onForget }: OverviewProps) {
  return (
    <Page toolbar={<Toolbar title="Disk usage" />}>
      <Stack gap="l" className="dk-wrap">
        {error ? <Dismissible tone="danger" onDismiss={onDismissError}>{error}</Dismissible> : null}

        <Section title="Disks">
          {disksError ? (
            <Notice tone="danger">{disksError}</Notice>
          ) : !disks ? (
            <Loading>Looking for disks</Loading>
          ) : disks.length === 0 ? (
            <Text tone="muted">Wings found no disks.</Text>
          ) : (
            disks.map((disk, i) => <DiskCard key={disk.mount} disk={disk} startup={i === 0} onScan={onScan} />)
          )}
        </Section>

        <Section title="Folders">
          <Card>
            <List aria-label="Folders to scan">
              {placesOf(projects, starred).map((place) => (
                <PlaceRow key={place.path} place={place} starred={starred.includes(place.path)} onScan={onScan} onStar={onStar} />
              ))}
            </List>
            <ScanFolder onScan={onScan} />
          </Card>
        </Section>

        {scans && scans.length > 0 ? (
          <Section title="Scans">
            <Card>
              <List aria-label="Scans Wings keeps">
                {scans.map((scan) => (
                  <ScanRow key={scan.root} scan={scan} disks={disks} onOpen={onOpen} onForget={onForget} />
                ))}
              </List>
            </Card>
          </Section>
        ) : null}
      </Stack>
    </Page>
  );
}
