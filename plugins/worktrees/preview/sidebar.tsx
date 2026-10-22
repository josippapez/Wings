// The Worktrees sidebar with two repos: the first at rest, and the second asking to remove its merged worktrees. A second
// state asks to remove one worktree, and the removal fails. The "Elsewhere on this Mac" section is shown before a scan,
// during one, after one that found nothing, and after one that found two repos. Those aren't your projects, so the page
// doesn't run git status in them: their worktrees say their changes aren't checked, have no "Open in new pane" and no
// "Remove all merged", and removing one warns first.
// The page loads its repos with git, which a fixture can't run, so this shows the sections it draws from them.
import type { ReactNode } from "react";
import { rowsOf, type Repo } from "../src/lib/git";
import { parseWorktrees } from "../src/lib/worktrees";
import { Elsewhere, type Scanning } from "../src/ui/Elsewhere";
import { RepoSection } from "../src/ui/RepoSection";
import type { Ask } from "../src/ui/ConfirmCard";
import { Steps } from "./_steps";

const sha = (c: string) => c.repeat(40);
const daysAgo = (n: number) => Math.floor(Date.now() / 1000) - n * 86_400;
const probes = (rows: [string, boolean, number][]) => new Map(rows.map(([path, dirty, days]) => [path, { dirty, lastCommit: daysAgo(days) }]));

const web: Repo = {
  root: "/Users/me/dev/acme-web",
  list: parseWorktrees(`worktree /Users/me/dev/acme-web
HEAD ${sha("1")}
branch refs/heads/main

worktree /Users/me/dev/acme-web-search
HEAD ${sha("2")}
branch refs/heads/feature/search

worktree /Users/me/dev/acme-web-login
HEAD ${sha("3")}
branch refs/heads/feature/login

worktree /Users/me/dev/acme-web-checkout
HEAD ${sha("4")}
branch refs/heads/feature/checkout

worktree /Users/me/dev/acme-web-hotfix
HEAD ${sha("5")}
branch refs/heads/fix/cart-total
locked release freeze

worktree /Users/me/dev/acme-web-bisect
HEAD ${sha("6")}
detached

worktree /Users/me/dev/acme-web-old
HEAD ${sha("7")}
branch refs/heads/experiment/old
prunable gitdir file points to non-existent location
`),
  facts: { defaultBranch: "origin/main", defaultTip: sha("1"), merged: new Set(["main", "feature/search", "feature/login", "experiment/old"]), gone: new Set(["feature/login"]) },
  probes: probes([
    ["/Users/me/dev/acme-web", false, 1],
    ["/Users/me/dev/acme-web-search", false, 5],
    ["/Users/me/dev/acme-web-login", false, 40],
    ["/Users/me/dev/acme-web-checkout", true, 2],
    ["/Users/me/dev/acme-web-hotfix", false, 12],
    ["/Users/me/dev/acme-web-bisect", false, 90],
  ]),
  checked: true,
};

const api: Repo = {
  root: "/Users/me/dev/acme-api",
  list: parseWorktrees(`worktree /Users/me/dev/acme-api
HEAD ${sha("a")}
branch refs/heads/main

worktree /Users/me/dev/acme-api-limits
HEAD ${sha("b")}
branch refs/heads/feature/rate-limit

worktree /Users/me/dev/acme-api-audit
HEAD ${sha("c")}
branch refs/heads/feature/audit-log

worktree /Users/me/dev/acme-api-webhooks
HEAD ${sha("d")}
branch refs/heads/feature/webhooks
`),
  facts: { defaultBranch: "origin/main", defaultTip: sha("a"), merged: new Set(["main", "feature/rate-limit", "feature/audit-log"]), gone: new Set(["feature/audit-log"]) },
  probes: probes([
    ["/Users/me/dev/acme-api", false, 1],
    ["/Users/me/dev/acme-api-limits", false, 3],
    ["/Users/me/dev/acme-api-audit", false, 21],
    ["/Users/me/dev/acme-api-webhooks", false, 4],
  ]),
  checked: true,
};

// Found by a scan, outside the projects.
const side: Repo = {
  root: "/Users/me/Desktop/side-project",
  list: parseWorktrees(`worktree /Users/me/Desktop/side-project
HEAD ${sha("e")}
branch refs/heads/main

worktree /Users/me/Desktop/side-project-fix
HEAD ${sha("f")}
branch refs/heads/fix/typo

worktree /Users/me/Desktop/side-project-spike
HEAD ${sha("0")}
branch refs/heads/spike/offline
`),
  facts: { defaultBranch: "origin/main", defaultTip: sha("e"), merged: new Set(["main", "fix/typo"]), gone: new Set(["fix/typo"]) },
  probes: new Map(),
  checked: false,
};

const legacy: Repo = {
  root: "/Volumes/Backup/code/legacy-api",
  list: parseWorktrees(`worktree /Volumes/Backup/code/legacy-api
HEAD ${sha("9")}
branch refs/heads/master

worktree /Volumes/Backup/code/legacy-api-v2
HEAD ${sha("8")}
branch refs/heads/v2
prunable gitdir file points to non-existent location
`),
  facts: { defaultBranch: "origin/master", defaultTip: sha("9"), merged: new Set(["master"]), gone: new Set() },
  probes: new Map(),
  checked: false,
};

const asked: Ask = { kind: "bulk", root: api.root, paths: ["/Users/me/dev/acme-api-limits", "/Users/me/dev/acme-api-audit"] };
const inPane = ["/Users/me/dev/acme-web-checkout/src"];
const noop = () => {};
const section = (repo: Repo, ask: Ask | null, onConfirm: () => Promise<unknown> = () => Promise.resolve()) => (
  <RepoSection repo={repo} rows={rowsOf(repo, inPane)} asked={ask} working={false} deleteBranch={false} onDeleteBranch={noop} onAsk={noop} onConfirm={onConfirm} onCancel={noop} onOpen={noop} />
);

// A repo outside your projects: the page gives it no `onOpen`, so it has no "Open in new pane".
const elsewhere = (repo: Repo, ask: Ask | null = null) => (
  <RepoSection repo={repo} rows={rowsOf(repo, [])} asked={ask} working={false} deleteBranch={false} onDeleteBranch={noop} onAsk={noop} onConfirm={() => Promise.resolve()} onCancel={noop} />
);
const minutesAgo = (n: number) => Date.now() - n * 60_000;
const around = (props: { at: number; scanning?: Scanning | null; found?: number }, children?: ReactNode) => (
  <>
    {section(web, null)}
    <Elsewhere at={props.at} scanning={props.scanning ?? null} error={null} found={props.found ?? 0} onScan={noop} onStop={noop}>
      {children}
    </Elsewhere>
  </>
);

export { page } from "../src/pages/sidebar";
export const states = {
  // Two repos: the second asking to remove its merged worktrees.
  rest: {
    element: (
      <>
        {section(web, null)}
        {section(api, asked)}
      </>
    ),
  },
  // Before the first scan: what it does, and the button.
  "elsewhere-new": { element: around({ at: 0 }) },
  // A scan is looking at the disks, with a repo still listed from the one before.
  "elsewhere-scanning": { element: around({ at: minutesAgo(52), scanning: { step: "finding", count: 184_203 }, found: 1 }, elsewhere(side)) },
  // A scan that found two repos with worktrees outside the projects.
  elsewhere: { element: around({ at: minutesAgo(3), found: 2 }, <>{elsewhere(side)}{elsewhere(legacy)}</>) },
  // Removing one worktree of a repo outside the projects: the card warns first, since git then runs in that repo.
  "elsewhere-confirm": {
    element: around(
      { at: minutesAgo(3), found: 2 },
      <>
        {elsewhere(side, { kind: "remove", root: side.root, paths: ["/Users/me/Desktop/side-project-fix"] })}
        {elsewhere(legacy)}
      </>,
    ),
  },
  // A scan that found none.
  "elsewhere-none": { element: around({ at: minutesAgo(1) }) },
  // Removing one worktree failed: the card stays open with the reason above its buttons.
  "confirm-failed": {
    element: (
      <Steps steps={[{ click: "Remove" }]}>
        {section(web, { kind: "remove", root: web.root, paths: ["/Users/me/dev/acme-web-search"] }, () => Promise.reject(new Error("~/dev/acme-web-search: contains modified or untracked files")))}
      </Steps>
    ),
  },
};
