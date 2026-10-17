// A check's run in the Checks tab: an Azure DevOps build with two stages, a failed step's log with colours, groups and
// the CI's error lines; the same run still going; while it loads; and when it can't be read.
import { Page } from "wings-plugin-ui";
import type { CheckItem, Run, RunStep } from "../src/lib/types";
import { RunView } from "../src/ui/run-view";
import "../src/pages/pr.css";

const item: CheckItem = { id: "build", name: "PR validation", state: "failed", required: true, kind: "Build", at: new Date(Date.now() - 38 * 60_000).toISOString(), seconds: 371, url: "https://dev.azure.com/acme/Web/_build/results?buildId=812", note: "The build failed", run: { build: 812 } };

const step = (id: string, name: string, state: RunStep["state"], seconds: number | null): RunStep => ({ id, name, state, seconds, log: state === "skipped" || state === "queued" ? null : id });

const run: Run = {
  url: "https://dev.azure.com/acme/Web/_build/results?buildId=812&view=logs",
  jobs: [
    { id: "skip", name: "Detect skip-ci flag", stage: "Skip CI check", state: "passed", seconds: 4, steps: [step("s1", "Initialize job", "passed", 1), step("s2", "Checkout web@refs/pull/482/merge", "passed", 2), step("s3", "Check commit message", "passed", 0), step("s4", "Finalize Job", "passed", 0)] },
    { id: "lint", name: "Lint and Prettier", stage: "Validation", state: "passed", seconds: 348, steps: [step("l1", "Initialize job", "passed", 2), step("l2", "Install dependencies", "passed", 38), step("l3", "Lint affected projects", "passed", 275), step("l4", "Prettier check", "passed", 24)] },
    { id: "test", name: "Tests", stage: "Validation", state: "failed", seconds: 268, steps: [step("t1", "Initialize job", "passed", 2), step("t2", "Install dependencies", "passed", 41), step("t3", "Unit tests", "failed", 182), step("t4", "Publish coverage", "skipped", null), step("t5", "Finalize Job", "passed", 0)] },
    { id: "types", name: "Typecheck", stage: "Validation", state: "passed", seconds: 346, steps: [step("y1", "Initialize job", "passed", 2), step("y2", "Typecheck affected projects", "passed", 213)] },
  ],
};

const t = (s: number) => `2026-10-16T17:0${3 + Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}.4518137Z `;
const log = [
  `${t(0)}##[section]Starting: Unit tests`,
  `${t(0)}==============================================================================`,
  `${t(0)}Task         : Bash`,
  `${t(0)}Version      : 3.279.0`,
  `${t(0)}==============================================================================`,
  `${t(0)}Generating script.`,
  `${t(1)}[command]/usr/bin/bash /home/vsts/work/_temp/run-tests.sh`,
  `${t(1)}##[group]Install summary`,
  `${t(1)}Scope: all 61 workspace projects`,
  `${t(2)}Lockfile is up to date, resolution step is skipped`,
  `${t(3)}Done in 4.1s`,
  `${t(3)}##[endgroup]`,
  `${t(4)}\x1b[1m\x1b[36m>\x1b[39m  NX   Running target test for 12 projects\x1b[22m`,
  ...Array.from({ length: 24 }, (_, i) => `${t(5 + i)} \x1b[32m✓\x1b[39m src/sync/module-${i + 1}.test.ts \x1b[2m(${8 + i} tests)\x1b[22m \x1b[33m${120 + i * 7}ms\x1b[39m`),
  `${t(30)} \x1b[31m✗\x1b[39m src/sync/client.test.ts \x1b[2m(14 tests | 3 failed)\x1b[22m`,
  `${t(30)}   \x1b[31m✗\x1b[39m retries with backoff > waits for Retry-After`,
  `${t(30)}     \x1b[31mAssertionError: expected 1200 to be 30000\x1b[39m`,
  `${t(30)}       at src/sync/client.test.ts:88:24`,
  `${t(31)}##[warning]Coverage is below the 80% threshold for src/sync`,
  `${t(31)} Test Files  \x1b[1m\x1b[31m1 failed\x1b[39m\x1b[22m | \x1b[1m\x1b[32m24 passed\x1b[39m\x1b[22m (25)`,
  `${t(31)}##[error]Bash exited with code '1'.`,
  `${t(31)}##[section]Finishing: Unit tests`,
];

const props = { item, version: 1, onLoadRun: async () => run, onLoadLog: async () => log, onBack: () => {} };

const frame = (el: React.ReactElement) => (
  <Page className="pr-page">
    <div className="pr-card">{el}</div>
  </Page>
);

const never = () => new Promise<never>(() => {});

export const states = {
  failed: { element: frame(<RunView {...props} />) },
  running: { element: frame(<RunView {...props} item={{ ...item, state: "running", note: null }} onLoadRun={async () => ({ ...run, jobs: run.jobs.map((j) => (j.id === "test" ? { ...j, state: "running" as const, steps: j.steps.map((s) => (s.id === "t3" ? { ...s, state: "running" as const } : s.id === "t4" || s.id === "t5" ? { ...s, state: "queued" as const, log: null } : s)) } : j)) })} onLoadLog={async () => log.slice(0, 20)} />) },
  loading: { element: frame(<RunView {...props} onLoadRun={never} />) },
  error: { element: frame(<RunView {...props} onLoadRun={() => Promise.reject(new Error("az devops invoke failed: TF400813: The user is not authorized to access this resource."))} />) },
};
