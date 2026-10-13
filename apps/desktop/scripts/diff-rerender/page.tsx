// The diff viewer as App shows it: opened loading, its patch arriving a moment later, and App rendering it again
// with new callbacks whenever anything else in Wings changes. run.mjs drives it through window.__check.
import { useState } from "react";
import ReactDOM from "react-dom/client";

import { DiffViewer } from "@/components/diff-viewer";
import type { DiffView } from "@/lib/plugins";
import "@/index.css";

/** One file with `lines` changed lines among unchanged ones. */
function patchOf(lines: number) {
  let out = "diff --git a/src/big.ts b/src/big.ts\n--- a/src/big.ts\n+++ b/src/big.ts\n";
  out += `@@ -1,${lines * 2} +1,${lines * 2} @@\n`;
  for (let i = 0; i < lines; i++) out += `-export const old${i} = { id: ${i}, name: "row ${i}" };\n+export const row${i} = { id: ${i}, name: "row ${i}", seen: true };\n const kept${i} = ${i};\n`;
  return out;
}

const opened: DiffView = { id: "d1", pluginId: "changes", title: "Uncommitted changes" };
const check = { rerender: () => {}, setPatch: () => {} };
(window as unknown as { __check: typeof check }).__check = check;

function App() {
  const [diff, setDiff] = useState(opened);
  const [, setTick] = useState(0);
  check.rerender = () => setTick((n) => n + 1);
  check.setPatch = () => setDiff({ ...opened, patch: patchOf(3000) });
  return <DiffViewer diff={diff} onClose={() => {}} onReply={async () => {}} reviewTargets={() => ({ targets: [], preferred: null })} onSendReview={async () => {}} />;
}

ReactDOM.createRoot(document.getElementById("root")!).render(<App />);
