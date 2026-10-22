// The Disk usage main script: it runs every scan and keeps the results for the tabs and for Claude's tools, keeps the
// Collector, and does what the tab asks on the disk (the Trash, Finder, Quick Look, local snapshots).

import { registerTools } from "wings-plugin-sdk/tools";

import { ch } from "./lib/messages";
import { deleteSnapshot, preview, purge, reveal, trashCollected } from "./main/actions";
import { collect, collectorNow, loadCollector, uncollect } from "./main/collector";
import { loadDisks } from "./main/disks";
import { forgetScan, listAll, refine, scans, startScan, stopScan, summaries } from "./main/scans";
import { tools } from "./tools";

registerTools(tools);

// The disks change on their own as files come and go, so they are read again while a tab shows them.
ch.serve("disks", loadDisks, { refetchMs: 15_000, staleMs: 10_000 });
// A scan, the list of them and the Collector are kept here and put on every change, so a tab that starts to show one gets
// what is kept, and is never sent to read it again.
ch.serve("scan", ({ root }) => scans.get(root) ?? null, { staleMs: Infinity });
ch.serve("scans", summaries, { staleMs: Infinity });
ch.serve("collector", collectorNow, { staleMs: Infinity });

// A scan answers once it shows as scanning: its progress and result come through the `scan` query.
ch.answer("scan", ({ root }) => startScan(root).started);
ch.answer("stop", ({ root }) => stopScan(root));
ch.answer("forget", ({ root }) => forgetScan(root));
ch.answer("refine", ({ root, path }) => refine(root, path));
ch.answer("listAll", ({ path }) => listAll(path));

ch.answer("collect", async ({ items }) => {
  const { refused } = await collect(items);
  if (refused.length) throw new Error(refused.map((r) => r.reason).join(" "));
});
ch.answer("uncollect", ({ paths }) => uncollect(paths));
ch.answer("trash", trashCollected);

ch.answer("reveal", ({ path }) => reveal(path));
ch.answer("preview", ({ path }) => preview(path));
ch.answer("deleteSnapshot", ({ date }) => deleteSnapshot(date));
ch.answer("purge", ({ mount }) => purge(mount));

loadCollector().catch(console.error);
