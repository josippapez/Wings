// The Disk usage tab: the disks overview, a scan running, the map with its list and Collector, and each way a scan can go wrong.
import MapPage, { page } from "../src/pages/map";
import { MapView, type MapViewProps } from "../src/ui/map-view";
import { Overview, type OverviewProps } from "../src/ui/overview";
import { NoScanView, ScanErrorView, ScanningView } from "../src/ui/scan-states";
import { collector, disks, done, homePath, homeScan, listAll, projects, root, scanning, scanningFolder, scans, trashed } from "./_data";
import { Steps } from "./_steps";

export { page };

const noop = () => {};
const ok = () => Promise.resolve();

const overview: OverviewProps = {
  disks,
  disksError: null,
  projects,
  starred: ["/Users/ada/Downloads", "~/Code/notes"],
  scans,
  error: null,
  onDismissError: noop,
  onScan: noop,
  onStar: noop,
  onOpen: noop,
  onForget: noop,
};

/** The map at the root of the startup disk, with an empty Collector. */
const map: MapViewProps = {
  scan: done,
  name: "Macintosh HD",
  focus: root,
  canBack: false,
  canForward: false,
  split: 320,
  collector: { items: [], total: 0 },
  onSplit: noop,
  onDisks: noop,
  onBack: noop,
  onForward: noop,
  onFocus: noop,
  onRescan: noop,
  onReveal: ok,
  onPreview: ok,
  onCollect: ok,
  onUncollect: ok,
  onTrash: () => Promise.resolve({ trashed: [], failed: [], freed: 0 }),
  onListAll: (folder) => Promise.resolve(listAll(folder)),
  onPurge: ok,
  onDeleteSnapshot: ok,
};

// What main serves and answers when the page itself runs.
export const queries = {
  disks: () => disks,
  scans: () => scans,
  collector: () => collector,
  scan: ({ root: r }: { root: string }) => (r === root ? done : null),
};
export const answers = { scan: () => undefined, refine: () => undefined, listAll: ({ path }: { path: string }) => listAll(path) };
export const storage = { starred: ["/Users/ada/Downloads"] };

export const states = {
  // The disks to scan, the folders, and the scans Wings keeps.
  overview: { element: <Overview {...overview} /> },
  // Before main has read the disks and the scans.
  "overview-loading": { element: <Overview {...overview} disks={null} scans={null} /> },
  // Wings can't list the disks, and a scan that wouldn't start.
  "overview-error": { element: <Overview {...overview} disks={null} disksError="Wings 0.1.69 can't list disks. Update Wings to 0.1.70 or newer." error="Couldn't scan /Volumes/Backup: No such file or directory" scans={[]} /> },
  // A disk's scan, with its used space to measure against.
  scanning: { element: <ScanningView name="Macintosh HD" scan={scanning} onDisks={noop} onStop={noop} /> },
  // A folder's scan, which has nothing to measure against.
  "scanning-folder": { element: <ScanningView name="Code" scan={scanningFolder} onDisks={noop} onStop={noop} /> },
  // Asked for, and main hasn't begun.
  starting: { element: <ScanningView name="Macintosh HD" scan={null} onDisks={noop} onStop={noop} /> },
  // The root of the disk, with its hidden space.
  map: { element: <MapView {...map} /> },
  // Gone into the Library folder: restricted folders, and back.
  drilled: { element: <MapView {...map} focus="/Users/ada/Library" canBack /> },
  // Six folders down: the folders in the middle of the path fold into a menu.
  deep: { element: <MapView {...map} focus="/Users/ada/Library/Developer/Xcode/DerivedData" canBack canForward /> },
  // A scan of a folder: no disk, so no hidden space.
  folder: { element: <MapView {...map} scan={homeScan} name="Home folder" focus={homePath} /> },
  // The pointer on the Library wedge while the root is in the middle: the list shows what is in it.
  pointing: { element: <MapView {...map} initialPoint={{ kind: "entry", path: "/Users/ada/Library" }} /> },
  // A folder with smaller items, opened: the entries `listAll` found under it.
  smaller: { element: <Steps steps={[{ click: "800 smaller items" }]}><MapView {...map} focus="/Users/ada/Downloads" canBack /></Steps> },
  // A row picked, with what is in the Collector shown.
  collector: { element: <Steps steps={[{ click: "Xcode_27.xip" }, { click: "Show what is in the Collector" }]}><MapView {...map} focus="/Users/ada/Downloads" collector={collector} /></Steps> },
  // Move to Trash clicked: it waits five seconds for a Cancel.
  countdown: { element: <Steps steps={[{ click: "Move to Trash" }]}><MapView {...map} collector={collector} /></Steps> },
  // The same with no wait, to see what comes after it: two moved, one that didn't.
  trashed: { element: <Steps steps={[{ click: "Move to Trash" }]}><MapView {...map} collector={collector} countdown={0} onTrash={() => Promise.resolve(trashed)} /></Steps> },
  // Main refuses a path for the Collector.
  refusal: {
    element: (
      <Steps steps={[{ click: "More for ada" }, { click: "Add to Collector" }]}>
        <MapView {...map} focus="/Users" onCollect={() => Promise.reject(new Error("/Users/ada is a home folder."))} />
      </Steps>
    ),
  },
  // The hidden space of the disk, opened.
  hidden: { element: <Steps steps={[{ click: "Hidden space" }]}><MapView {...map} /></Steps> },
  // Deleting a snapshot asks first.
  "hidden-confirm": { element: <Steps steps={[{ click: "Hidden space" }, { click: "Delete the snapshot" }]}><MapView {...map} /></Steps> },
  // The scan failed.
  error: { element: <ScanErrorView name="Macintosh HD" message="Wings couldn't read /Volumes/Backup: the disk was removed." onDisks={noop} onRetry={noop} /> },
  // The tab was left on a scan that main no longer has.
  "no-scan": { element: <NoScanView name="Macintosh HD" onDisks={noop} onScan={noop} /> },
  // As the page runs it: the disks, scans and folders from main and the settings.
  page: { element: <MapPage />, project: { id: "p1", name: "web", path: "/work/acme/web", branch: "main" } },
  // Clicking Scan on the startup disk and a folder in its map, through the page's own state and queries.
  "page-map": { element: <Steps steps={[{ click: "Scan" }, { click: "Users" }]}><MapPage /></Steps> },
};
