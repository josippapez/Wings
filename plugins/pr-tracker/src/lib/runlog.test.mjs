import assert from "node:assert/strict";
import test from "node:test";
import { azureRun } from "./forges/azure.ts";
import { parseLog, stepLines } from "./runlog.ts";

const text = (line) => line.spans.map((s) => s.text).join("");

test("parseLog lifts timestamps, reads the CI's markers and folds its groups", () => {
  const out = parseLog([
    "2026-10-16T17:03:38.4518137Z ##[section]Starting: Lint",
    "2026-10-16T17:03:38.5688826Z [command]/usr/bin/bash run.sh",
    "2026-10-16T17:03:39.0000000Z ##[group]Install",
    "2026-10-16T17:03:39.1000000Z Done in 4s",
    "2026-10-16T17:03:39.2000000Z ##[endgroup]",
    "2026-10-16T17:03:40.0000000Z ##[error]Bash exited with code '1'.",
  ]);
  assert.deepEqual(out.map((e) => ("title" in e ? `group ${text(e.title)}: ${e.lines.map(text)}` : `${e.tone} ${text(e)}`)), [
    "section Starting: Lint",
    "command /usr/bin/bash run.sh",
    "group Install: Done in 4s",
    "error Bash exited with code '1'.",
  ]);
  assert.equal(out[0].time, "17:03:38");
});

test("parseLog keeps ANSI colours as styles, carried over to the next line until reset", () => {
  const [a, b] = parseLog(["\x1b[1m\x1b[31mfailed\x1b[39m ok", "\x1b[22mplain"]);
  assert.deepEqual(a.spans, [{ text: "failed", bold: true, fg: 1 }, { text: " ok", bold: true }]);
  assert.deepEqual(b.spans, [{ text: "plain" }]);
});

test("parseLog folds GitLab's sections, which can end and start on one line, and keeps what a progress redraw left", () => {
  const out = parseLog([
    "section_start:1:prepare\r\x1b[0K\x1b[36;1mPreparing\x1b[0;m",
    "Using docker",
    "section_end:1:prepare\r\x1b[0Ksection_start:2:script[collapsed=true]\r\x1b[0KRunning script",
    "10%\r50%\r100%",
    "section_end:2:script\r\x1b[0K",
    "Job succeeded",
  ]);
  assert.deepEqual(out.map((e) => ("title" in e ? `${text(e.title)}: ${e.lines.map(text)}` : text(e))), ["Preparing: Using docker", "Running script: 100%", "Job succeeded"]);
});

test("stepLines cuts an Actions job's log into steps that start in the same second", () => {
  const at = (s, rest) => `2026-10-17T06:55:0${s}.1000000Z ${rest}`;
  const parts = stepLines(
    [at(8, "Current runner version"), at(9, "##[group] Inputs"), at(9, "##[endgroup]"), at(9, "##[group]Run actions/checkout@v4"), at(9, "with:"), at(9, "Cleaning up orphan processes")],
    [
      { start: "2026-10-17T06:55:08Z", end: "2026-10-17T06:55:09Z" },
      { start: "2026-10-17T06:55:09Z", end: "2026-10-17T06:55:09Z" },
      { start: "2026-10-17T06:55:09Z", end: "2026-10-17T06:55:10Z" },
    ],
  );
  assert.deepEqual(parts.map((p) => p.length), [3, 2, 1]);
});

test("azureRun lists jobs by stage and phase order, each with its tasks, and names stages only when there are several", () => {
  const r = (id, type, name, parentId, order, extra = {}) => ({ id, type, name, parentId, order, state: "completed", result: "succeeded", ...extra });
  const records = [
    r("s1", "Stage", "Validate", null, 1),
    r("p2", "Phase", "Tests", "s1", 2),
    r("p1", "Phase", "Lint", "s1", 1),
    r("j2", "Job", "Tests", "p2", 1, { result: "failed" }),
    r("j1", "Job", "Lint", "p1", 1),
    r("t2", "Task", "Run tests", "j2", 2, { result: "failed", log: { id: 9 } }),
    r("t1", "Task", "Checkout", "j2", 1, { log: { id: 8 } }),
    r("t3", "Task", "Lint", "j1", 1, { state: "inProgress", result: null, log: { id: 7 } }),
  ];
  const run = azureRun(records, "https://x");
  assert.deepEqual(run.jobs.map((j) => [j.name, j.stage, j.state, j.steps.map((s) => `${s.name}:${s.state}:${s.log}`)]), [
    ["Lint", null, "passed", ["Lint:running:7"]],
    ["Tests", null, "failed", ["Checkout:passed:8", "Run tests:failed:9"]],
  ]);
});
