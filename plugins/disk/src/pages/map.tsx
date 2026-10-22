// The Disk usage tab: the disks and folders to scan, a scan as a map you click into, and the Collector. Main runs every scan and
// keeps its result, so this page reads them as queries and asks main for everything that touches the disk. Where the tab is, the
// scan it shows and the folder in the middle, is kept in the tab's state, so a reload returns to the same place.

import { useEffect, useRef, useState } from "react";
import { errorMessage } from "wings-plugin-sdk/async";
import { useQuery, useSetting } from "wings-plugin-sdk/react";
import { Loading, Page, Toolbar, useTabState } from "wings-plugin-ui";
import { ch, type Collector } from "../lib/messages";
import { starred } from "../lib/settings";
import { find } from "../lib/tree";
import { MapView } from "../ui/map-view";
import { existingFocus, isCoarse, rootName, type DoneScan } from "../ui/model";
import { Overview } from "../ui/overview";
import { NoScanView, ScanErrorView, ScanningView } from "../ui/scan-states";
import "./map.css";

export const page = { place: "tab", fill: true };

/** Where the tab is: the scan it shows (null for the disks), the folder in the middle of its map (null for the root) and the list's width. */
interface Tab {
  root: string | null;
  focus: string | null;
  split: number;
}

const FIRST: Tab = { root: null, focus: null, split: 320 };

const checkTab = (raw: unknown): Tab | null => {
  const t = raw as Partial<Tab> | null;
  if (!t || typeof t !== "object") return null;
  return {
    root: typeof t.root === "string" ? t.root : null,
    focus: typeof t.focus === "string" ? t.focus : null,
    split: typeof t.split === "number" && t.split >= 200 ? t.split : FIRST.split,
  };
};

/** Main answers `refine` and `listAll` when the folder has been scanned again, which can take minutes for a big one. */
const LONG = 60 * 60 * 1000;
/** `purge` can take macOS up to five minutes, and `deleteSnapshot` a minute: the default wait for an answer is 30 seconds. */
const FIVE_MINUTES = 300_000;

const NOTHING: Collector = { items: [], total: 0 };

export default function MapPage() {
  const [tab, save] = useTabState(FIRST, checkTab);
  return tab ? (
    <Disks saved={tab} save={save} />
  ) : (
    <Page toolbar={<Toolbar title="Disk usage" />}>
      <Loading />
    </Page>
  );
}

/** The folders of Wings, in sidebar order, kept current. */
function useProjects(): WingsProject[] {
  const [projects, setProjects] = useState<WingsProject[]>([]);
  useEffect(() => wings.onProjects(setProjects), []);
  return projects;
}

function Disks({ saved, save }: { saved: Tab; save: (tab: Tab) => void }) {
  // Two changes in one event, like a new scan and its folder, each build on the one before.
  const latest = useRef(saved);
  latest.current = saved;
  const patch = (change: Partial<Tab>) => {
    latest.current = { ...latest.current, ...change };
    save(latest.current);
  };
  const root = saved.root;

  const { data: disks, error: disksError } = useQuery(ch, "disks", undefined);
  const { data: scans } = useQuery(ch, "scans", undefined);
  const { data: scan, error: scanError, at, refresh } = useQuery(ch, "scan", { root: root ?? "" }, { skip: root === null });
  const projects = useProjects();
  const [stars, setStars] = useSetting(starred);

  // Asked for by this page and not in the query yet: a scan just begun is not "no scan".
  const [starting, setStarting] = useState<string | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  useEffect(() => {
    if (scan) setStarting(null);
  }, [scan]);

  const leave = () => patch({ root: null, focus: null });
  const start = (target: string) => {
    setProblem(null);
    setStarting(target);
    patch({ root: target, focus: target === latest.current.root ? latest.current.focus : null });
    ch.ask("scan", { root: target }).catch((error) => {
      setProblem(`Couldn't scan ${target}: ${errorMessage(error)}`);
      setStarting(null);
      if (latest.current.root === target) leave();
    });
  };
  const tell = (promise: Promise<unknown>) => promise.catch((error) => setProblem(errorMessage(error)));

  if (root === null) {
    return (
      <Overview
        disks={disks}
        disksError={disksError?.message ?? null}
        projects={projects}
        starred={stars ?? []}
        scans={scans}
        error={problem}
        onDismissError={() => setProblem(null)}
        onScan={start}
        onStar={(path, on) => void setStars(on ? [...(stars ?? []).filter((p) => p !== path), path] : (stars ?? []).filter((p) => p !== path))}
        onOpen={(target) => patch({ root: target, focus: null })}
        onForget={(target) => void tell(ch.ask("forget", { root: target }))}
      />
    );
  }

  const name = rootName(root, disks);
  const stop = () => {
    void tell(ch.ask("stop", { root }));
    leave();
  };
  if (!scan) {
    if (scanError) return <ScanErrorView name={name} message={scanError.message} onDisks={leave} onRetry={refresh} />;
    // Main says there is none once it has answered the query; before that, or just after a scan was asked for, one is on its way.
    if (at > 0 && starting !== root) return <NoScanView name={name} onDisks={leave} onScan={() => start(root)} />;
    return <ScanningView name={name} scan={null} onDisks={leave} onStop={stop} />;
  }
  if (scan.state === "scanning") return <ScanningView name={name} scan={scan} onDisks={leave} onStop={stop} />;
  if (scan.state === "error") return <ScanErrorView name={name} message={scan.message} onDisks={leave} onRetry={() => start(root)} />;

  return <Mapped scan={scan} name={name} tab={saved} patch={patch} onDisks={leave} onRescan={() => start(scan.root)} />;
}

/** Where the folders you went into are, to go back and forward through. Starts again for another scan. */
interface Trail {
  root: string | null;
  stack: (string | null)[];
  at: number;
}

function Mapped({ scan, name, tab, patch, onDisks, onRescan }: { scan: DoneScan; name: string; tab: Tab; patch: (change: Partial<Tab>) => void; onDisks: () => void; onRescan: () => void }) {
  const { data: collector } = useQuery(ch, "collector", undefined);

  const focus = existingFocus(scan.tree, tab.focus);
  const fresh = (): Trail => ({ root: tab.root, stack: [tab.focus], at: 0 });
  const [saved, setTrail] = useState<Trail>(fresh);
  const trail = saved.root === tab.root ? saved : fresh();
  const go = (path: string) => {
    if (path === focus) return;
    const stack = [...trail.stack.slice(0, trail.at + 1), path];
    setTrail({ root: tab.root, stack, at: stack.length - 1 });
    patch({ focus: path });
  };
  const walk = (at: number) => {
    setTrail({ ...trail, at });
    patch({ focus: trail.stack[at] });
  };

  // Going into a folder that was scanned coarsely has main scan it again in more detail. A refinement that fails leaves the folder
  // coarse and publishes the scan again, so each folder is asked for once.
  const asked = useRef(new Set<string>());
  useEffect(() => {
    const entries = find(scan.tree, focus);
    if (!entries || !isCoarse(scan, entries) || scan.refining.includes(focus) || asked.current.has(focus)) return;
    asked.current.add(focus);
    ch.ask("refine", { root: scan.root, path: focus }, { timeoutMs: LONG }).catch((error) => console.error(`Wings couldn't add detail to ${focus}:`, errorMessage(error)));
  }, [scan, focus]);

  return (
    <MapView
      scan={scan}
      name={name}
      focus={focus}
      canBack={trail.at > 0}
      canForward={trail.at < trail.stack.length - 1}
      split={tab.split}
      collector={collector ?? NOTHING}
      onSplit={(split) => patch({ split })}
      onDisks={onDisks}
      onBack={() => walk(trail.at - 1)}
      onForward={() => walk(trail.at + 1)}
      onFocus={go}
      onRescan={onRescan}
      onReveal={(path) => ch.ask("reveal", { path })}
      onPreview={(path) => ch.ask("preview", { path })}
      onCollect={(items) => ch.ask("collect", { items })}
      onUncollect={(paths) => ch.ask("uncollect", { paths })}
      onTrash={() => ch.ask("trash", undefined, { timeoutMs: FIVE_MINUTES })}
      onListAll={(path) => ch.ask("listAll", { path }, { timeoutMs: LONG })}
      onPurge={(mount) => ch.ask("purge", { mount }, { timeoutMs: FIVE_MINUTES })}
      onDeleteSnapshot={(date) => ch.ask("deleteSnapshot", { date }, { timeoutMs: FIVE_MINUTES })}
    />
  );
}
