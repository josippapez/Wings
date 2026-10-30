# Wings plugins

A plugin is a folder with a `wings-plugin.json` manifest and a JavaScript file. Wings loads it into a sandboxed frame, and the plugin talks to Wings only through the `wings` object. Anything that touches your machine is checked in Rust against the permissions in the manifest.

`changes/` shows each pane's changed files as a badge and a sidebar tree, the repo's commit history, and a commit graph in a window of its own (`views`), all opening Wings' diff viewer.

`dev-servers/` puts a `localhost:<port>` badge on a pane while a program in it listens on a local port, and opens it in the browser from a button on the badge. It is one script with no pages, so it is the smallest plugin here to read.

`worktrees/` lists the git worktrees of your projects in a sidebar, open in a pane or not, with what each one is (merged into the default branch, upstream gone, uncommitted changes, locked, folder missing, open in a pane). It removes the ones you pick, or all merged and clean ones, after you confirm, with `git worktree remove` and never `--force`, prunes records of worktrees whose folder is gone, and can delete the branch with `git branch -d`. It can also look across the whole Mac: Scan this Mac finds, with `wings.diskFind`, the repos outside your projects that have more than one worktree, and lists them under their own heading, reading them only with git commands that don't run a repo's own programs. So it doesn't check them for changes, offers no "Remove all merged" and no pane there, and warns before it removes one worktree, since `git worktree remove` can run what a repo's settings and hooks name. Needs Wings 0.1.70.

`disk/` shows what takes the space on your disks, like DaisyDisk: a disks overview, a scan of a disk or folder drawn as a map you click into, a list of what's in each folder, hidden space, and a Collector that moves what you pick to the Trash. Claude scans with its `disk__*` tools and can fill the Collector for you to review. It scans with `wings.diskUsage`, in Wings, on several threads.

`work-item/` shows the Azure Boards work item named in each pane's branch, and moves it to its next state. A work item opens as a tab with its description, comments and linked pull requests, and the work items assigned to you in the current sprint show under each Azure DevOps project in the sidebar.

`tasks/` is a lightweight issue tracker for each project, shown as Issues: a key like `WIN-12`, a type (task, bug, feature or epic), a Markdown spec (new descriptions start from a Goal, Scope, Done when and Validation template), a status (to do, in progress, in review, done, canceled), a priority (urgent, high, medium, low, shown with Linear's icons and sorted first), links (branch, pull request, commits), a parent epic or feature with progress on the epic, related and blocking issues, the Claude sessions that worked on it and an append-only activity log of who did what and when. Issue keys stay local to Wings, never in commit messages or branch names; a commit an agent makes while an issue is in progress is linked by the agent or by you with the commit, branch and PR fields. A sidebar lists the open ones, a board in a tab shows them by status, in rows by parent epic or feature like Linear's sub-grouping (each row folds and shows how many children are done), an Issue page opens as a tab (one tab reused, a middle-click or Cmd+click opens another) to read, edit, resume or start an agent on, and the open ones show under the project, children indented under their epic or feature. Claude reads and writes them with `tasks__list`, `tasks__get`, `tasks__create`, `tasks__update` and `tasks__log`, for the project of the pane it came from. Issues also answers Claude Code's hooks: at session start it tells Claude to track its work here, and a "Keep issues local" setting (on by default) denies a commit or pull request command that holds an issue key. These need Wings 0.1.54.

`schedules/` starts a coding agent (Claude Code or OpenCode) with a prompt in one of your projects by itself, every few hours or at a set time on the days you pick, and opens each run in a new tab with a notification. A timer in its main script checks what is due every 30 seconds with plain date math, so it keeps running while Wings is in the background. A run missed while Wings was closed is not made up. Its Schedules page opens as a tab, and Wings 0.1.45 or newer runs it.

`pr-tracker/` is the largest plugin here, with five hosts and a lot of pages, so start from `dev-servers/` or a plugin made by `wings-plugin init` instead. It shows the pull request for each pane's branch as a badge with live status, checks and why they failed, the diff and review comments. It lists each project's pull requests in the sidebar (yours and the ones waiting for your review, or every open one, picked on its Settings page), can send a notification (`wings.notify`) when someone opens a new one or only when one asks for your review, checked every minute while Wings is in front and at an interval set on its Settings page while it's in the background, and in a tab or a Project page block every open one with All open, Mine, Review requested and Drafts chips (the choice is kept with `setTabState` / `setBlockState`). The PR tab's Checks view lists every check or policy grouped by state, with required or optional, kind, duration, an Open link and, for a failure with a log, the end of it fetched when the row opens. It opens one as a tab to read, reply to, approve and send to an agent. A tab opened with no pull request (from its title bar button) lists the project's open ones to pick from. "View in diff" on a thread opens the viewer at its file and line. It works with GitHub (github.com and GitHub Enterprise Server hosts `gh` is signed in to), GitLab (gitlab.com and self-managed hosts `glab` is signed in to) and Azure DevOps through their CLIs, and with Bitbucket Cloud and Codeberg through an API token saved on its Settings page. Replies work on every host: in the review thread on GitHub (github.com and Enterprise), GitLab and Azure DevOps, under the comment on Bitbucket Cloud, and on Codeberg as a new comment that quotes the one it answers, since the Gitea API has no threaded replies. The Approve button (confirmed first) works on GitHub, GitLab, Bitbucket Cloud and Codeberg; Azure DevOps has a Vote menu (Approve, Approve with suggestions, Wait for author, Reject, Reset vote) that runs `az repos pr set-vote`, which finds your reviewer identity itself. Bitbucket needs the `write:pullrequest:bitbucket` token scope, Codeberg `write:issue` to reply and `write:repository` to approve.

## Installing

Open Plugins (the puzzle icon in the title bar) and paste a GitHub repo link, choose a `.wings-plugin` file, or drop one on the window. The plugins in this folder that aren't installed are listed under Available, each with an Install button, and Wings shows a toast once when a new one lands on `main` (checked on start and every six hours, with the update check). A new plugin starts off. Turning it on shows what it can do and what it adds, and approves exactly that. An update that asks for more turns it off until you approve again.

From a GitHub link, Wings installs the newest release's `.wings-plugin` file, or the repo itself if there are no releases, and offers updates when a new release is out. A link to a folder in a repo of several plugins, `github.com/owner/name/plugins/<id>`, installs that folder from the default branch and offers an update when the plugin's own version there changes, so pushing a version bump ships it. An update that needs a newer Wings, by its `minWingsVersion`, isn't offered. Public repos need no sign-in. For a private repo Wings uses your `gh` sign-in, so `gh auth login` as someone who can see it. You can also share a private plugin as a `.wings-plugin` file.

The plugins in this folder install the same way:

| Plugin | Link |
| --- | --- |
| Changes | `github.com/josippapez/Wings/plugins/changes` |
| Dev servers | `github.com/josippapez/Wings/plugins/dev-servers` |
| Disk usage | `github.com/josippapez/Wings/plugins/disk` |
| HUD | `github.com/josippapez/Wings/plugins/hud` |
| Issues | `github.com/josippapez/Wings/plugins/tasks` |
| PR tracker | `github.com/josippapez/Wings/plugins/pr-tracker` |
| Session detail | `github.com/josippapez/Wings/plugins/session-detail` |
| Work item | `github.com/josippapez/Wings/plugins/work-item` |
| Worktrees | `github.com/josippapez/Wings/plugins/worktrees` |

From a terminal or a script, use the `wings plugin` command while Wings is open:

```sh
wings plugin install ~/Downloads/cyclops.wings-plugin   # or github.com/owner/name
wings plugin list            # --json for scripts
wings plugin link ~/code/my-plugin   # load a plugin folder in place, for development: `wings-plugin dev` does it on each save
wings plugin enable cyclops  # also update, disable, remove
```

Every plugin tool is also a command (Wings 0.1.94): `wings tools` lists them, and `wings <plugin> <tool> key=value…` runs one and prints its answer, like `wings tasks list status=todo`. A value that reads as JSON (a number, `true`, `false`, a `[list]`) goes as that, anything else as text. `--help` after a command shows its arguments.

`wings worktree-setup [project=<name|path>] [include='["a","b"]'] [setup=<command>]` (Wings 0.1.99) is Wings' own `worktree_setup` tool, which Claude also has: without `include` it prints what the project's new worktrees get (the local files copied in, a `.worktreeinclude` in the repo taking precedence, and the setup command); with it, it changes the file list, like Worktree setup in the sidebar. `include` lists patterns (`.gitignore` syntax, `!` leaves files out) and replaces the defaults, and an empty list resets it. `setup` proposes the command that runs in every new worktree before its agent starts (an empty value clears it): Wings asks you to allow it in a dialog, the call waits up to 5 minutes for your answer, and nothing is saved if you decline or don't answer. `project` defaults to the project of the pane you run in.

A plugin installed this way still waits for you to approve its access in Wings, which opens the approval. On first start Wings offers to add the `wings` command to `~/.local/bin`, and the Plugins sheet offers it later. With no arguments the command opens Wings rather than starting a second copy.

Plugins start and stop as you turn them on and off, with no restart. Installed plugins live in `<app data>/plugins/<id>/` (on macOS `~/Library/Application Support/dev.wings.app/plugins/`). Dev builds also load this repo's `plugins/` folder at startup, always on.

## Packaging

A `.wings-plugin` file is a zip of the plugin folder, with `wings-plugin.json` at its root or one folder down. A plugin written in TypeScript makes one with `wings-plugin pack` from `wings-plugin-sdk`, which builds `src/` and packs the built files in `dist/` with the manifest and the files the plugin ships. A plain JavaScript plugin can zip its git repo:

```sh
npx wings-plugin pack                                          # TypeScript: writes <id>.wings-plugin
git archive --format=zip --output my-plugin.wings-plugin HEAD  # plain JavaScript
```

`wings-plugin init <folder>` makes a starter repo with the SDK and a workflow that builds and attaches the package to a GitHub release when you push a version tag, so the repo holds only source. Wings installs a repo's release package. A TypeScript repo with no release yet can't be installed from its link, since its source has no built files.

## Writing a plugin

Wings runs plain JavaScript: `main` and each page's `<script>`. You write TypeScript and React, and `wings-plugin build` from the `wings-plugin-sdk` npm package bundles it into those files. The package also has the types of `wings` and the helpers below. `dev-servers/` is the smallest plugin here (one script, no pages), and `npx wings-plugin-sdk init <folder>` starts a plugin with the SDK set up (its starter is `packages/plugin-sdk/template/`).

Every `wings.on*` call returns a function that removes the listener, so a React effect can return it as its cleanup.

### Layout

```
<id>/
  wings-plugin.json
  icon.svg
  README.md        Wings shows it on the plugin's Settings page
  src/
    main.ts        the main script, which runs hidden
    pages/         one file per page: sidebar.tsx, board.tsx, ...
    main/          the modules a large main script splits into
    ui/            React components the pages share
    lib/           logic with no UI, messages and settings, and their tests
    tools.ts       the tools Claude gets, if the plugin has any
  preview/         fixtures that `wings-plugin shot` draws
  dist/            what the build writes
```

The entries are `src/main.ts` and each file in `src/pages/`. Everything else in `src/` is a module they import. Each entry builds into `dist/` under its own name: `src/main.ts` becomes `dist/main.js` and `src/pages/sidebar.tsx` becomes `dist/sidebar.js`, so the manifest says `"main": "dist/main.js"` and `"page": "dist/sidebar.html"`. A plugin with no `src/main.ts` gets an empty `dist/main.js`, since Wings needs a main.

A page default-exports its component and says where it shows with `export const page = { place: "sidebar" }`. `place` is `sidebar`, `popover`, `view`, `block`, `tab` or `settings`, and `fill: true` lays the page out edge to edge. A page built from `src/pages` mounts in one place, so a page that shows as a view and as a block is two files. It builds from `wings-plugin-ui`, and Wings' `plugin-ui.css` styles the components, so a Wings update restyles every plugin. The build writes the mount, which calls the matching `showSidebar`, `showView`, `showBlock` or other function, and the page's HTML. A `dist/<page>.html` of your own wins over the generated one. A page can bring its own components and CSS (`import "./sidebar.css"`, next to the page). A page that draws its own DOM, without React, is a `.ts` file, and gets its frame from `mountShell` in `wings-plugin-sdk/shell`. See the [README](../packages/plugin-ui/README.md) of `wings-plugin-ui` for the components, the `w-*` class contract and the `--wings-*` variables.

### Build and check

```sh
cd plugins
pnpm run deps   # installs plugin-sdk, plugin-ui and the plugins, each has its own lockfile
pnpm build   # writes each plugin's dist/
pnpm check   # type-checks, and fails if a committed file in dist/ is out of date
pnpm test    # runs the tests in every plugin's src/, which can import any module there
```

`dist/` is committed, because Wings installs a plugin folder straight from `main`. Commit it with the source change: `pnpm check` compares the content of each built file with the one on disk and fails when they differ, and a release stops on that. A plugin in its own repo commits only source and ships with `wings-plugin pack`.

The build also checks the manifest against what it built, and fails with a line for each place they disagree, so a typo doesn't wait for Wings to load the plugin. It catches a `main` that isn't `dist/main.js`; a page the build doesn't write and the folder doesn't have; an `icon` that isn't a file; a `wings.onTool("name")` with no `mcpTools` entry, or the reverse; a `wings.onHook("event")` with no `hooks` entry, or the reverse; a call of `wings.setBadge`, `setPaneStatus`, `setPaneRail`, `openDiff` or `updateDiff` without `badges`, `status`, `rail` or `diff` in `contributes.ui`; a `wings.openView("id")` that no sidebar or view has; and an id in an `open` of a pane status or rail, a project section row or a notification button that names no view it can open. The bundles are searched as text, so a name the code computes, like `wings.openView(id)`, is skipped.

While you work, `wings-plugin dev` in the plugin's folder builds on each save and has the running Wings restart the plugin. `wings-plugin shot` draws the pages (see Fixtures); in this repo, `pnpm --dir plugins shot` does it for every plugin.

### Talking to main

The main script and each page are separate frames, and they talk through `wings.broadcast`. Don't write request ids, timeouts or a hello message for that. Make a `channel` from `wings-plugin-sdk/messages` once, in a module that main and every page import, and give it the types of what it carries. A channel carries three kinds of message:

- **Queries** are data. Main `serve`s a value for a key, and every page that shows the key gets the same copy and each update. Main loads a key once, however many pages show it, and reloads it only while one does. A page that fetched something on its own can `put` it, and every other part gets it too. Reach for a query first: it replaces a page that polls main on its own schedule.
- **Questions** are commands and one-off reads: a page `ask`s, and main `answer`s, like checking out a branch or signing in.
- **Events** carry no data: a frame `tell`s, the others `on` it.

A value that a settings page saves and other frames read is a `setting()` from `wings-plugin-sdk/settings`, declared once the same way.

```ts
// src/lib/messages.ts
import { channel } from "wings-plugin-sdk/messages";
import { setting } from "wings-plugin-sdk/settings";

export const ch = channel<
  // Questions: a page asks, main answers.
  { checkout: { request: { cwd: string; branch: string }; response: void } },
  // Events: one frame tells, the others listen. `void` is no payload.
  { "branch-changed": void },
  // Queries: main serves a value for each key. A `void` key holds one value.
  { branches: { key: { cwd: string }; value: string[] } }
>();

/** A value in `wings.storage` that every frame reads and hears change. */
export const interval = setting("interval", { default: 60 });
```

Main serves the query and answers the question, and refreshes the query when something changes:

```ts
// src/main.ts
import { git, gitOk, onGitChange } from "wings-plugin-sdk/git";
import { ch } from "./lib/messages";

// `load` runs only in main. A page that shows a key gets the kept value, and main reloads it while one does.
ch.serve(
  "branches",
  async ({ cwd }) => {
    const out = await gitOk(cwd, ["for-each-ref", "--format=%(refname:short)", "refs/heads"]);
    return out ? out.split("\n") : [];
  },
  { staleMs: 20_000 },
);

ch.answer("checkout", async ({ cwd, branch }) => {
  await git(cwd, ["switch", branch]);
  ch.refresh("branches"); // reloads every key a page shows, and sends the new list to all of them
  await ch.tell("branch-changed");
});

// A commit or checkout from anywhere reloads the list too.
onGitChange(() => ch.refresh("branches"));
```

A page reads the query, and asks for the command:

```tsx
// src/pages/branches.tsx
import { useQuery } from "wings-plugin-sdk/react";
import { Empty, Heading, List, ListButton, ListItem, Loading, ProjectGate } from "wings-plugin-ui";
import { ch } from "../lib/messages";

export const page = { place: "sidebar" };

export default function Page() {
  return <ProjectGate>{(project) => <Branches cwd={project.path} />}</ProjectGate>;
}

function Branches({ cwd }: { cwd: string }) {
  const { data, error } = useQuery(ch, "branches", { cwd }); // `data` is null until main has sent the first value
  if (data === null && !error) return <Loading />;
  if (error) return <Empty title="Couldn't list the branches" description={error.message} />;
  return (
    <>
      <Heading>Branches</Heading>
      <List>
        {data?.map((branch) => (
          <ListItem key={branch}>
            <ListButton onClick={() => void ch.ask("checkout", { cwd, branch })}>{branch}</ListButton>
          </ListItem>
        ))}
      </List>
    </>
  );
}
```

| Call | Does |
|---|---|
| `ch.ask(name, request, { timeoutMs? })` | Asks whichever frame answers `name` and resolves its answer. It rejects with the answerer's error message, or with "Wings didn't answer. Try again." after 30 s |
| `ch.answer(name, handler)` | Answers `name` in this frame, usually main. A thrown error reaches the asker. One handler per name: a second call replaces the first |
| `ch.tell(name, payload?)`, `ch.on(name, listener)` | An event for every other frame that listens to `name` |
| `ch.serve(name, load?, { refetchMs, staleMs, throttleMs })` | Main: serves a query. `load(key)` runs only in main, one load per key at a time. `refetchMs` reloads a key that long after its last value, while a page shows it and Wings is in front. A page that starts to show a key gets the kept value, then a reload if it is older than `staleMs`. `throttleMs` folds the refreshes asked for within it into one load. Without `load`, main keeps what frames `put` |
| `ch.get(name, key, { maxAgeMs? })`, `ch.peek(name, key?)`, `ch.watching(name)` | Main: the kept value or one load, what is kept for a key without loading, and the keys some page shows now |
| `ch.put(name, key, value)` | Any frame: sets a key's value for every frame at once. For a page that fetched something itself |
| `ch.refresh(name, key?)` | Any frame: reloads a key now, or every key a page shows when you leave the key out |
| `ch.subscribe(name, key, listener)` | Any frame: calls `listener({ data, error, at, loading })` as the value changes, for a page without React |
| `useQuery(ch, name, key, { skip?, keepData?, timeoutMs? })` | In a page: `{ data, error, loading, at, refresh }`. `keepData` keeps the last key's value while a new key loads. `loading` is true only before the first value and while this hook's own `refresh()` waits, so a refetch in main never blinks |
| `setting(key, { default, check? })` | `get()` reads it from `wings.storage`, or gives `default` when nothing valid is stored (`check` turns what is stored into the value, or `undefined` for what isn't one). `set(value)` stores it and tells the other frames. `onChange(listener)` hears a `set` from any frame. Declare it once: two objects for one key in one frame don't hear each other |
| `useSetting(setting)` | In a page: `[value, set]`, with `value` `null` until it is read |

A message is JSON that goes over `wings.broadcast`, and it reaches every other frame, never the one that sent it. Messages are marked, so they never mix with your own `broadcast` calls. One that is too big for a broadcast (64,000 characters) is split and joined again, so a value of any size arrives whole. Every frame gets every broadcast, so keep a query to what the pages show. A page that loads before main, or while main reloads, gets its value once main serves. A failed load keeps the last value and reaches every page as `error` until the next good load.

### Helpers

Reach for these before writing your own. Each is a module of `wings-plugin-sdk`: its `.d.ts` says what every call does, and the [README](../packages/plugin-sdk/README.md) has the setup and an example of each. The last column is a plugin here that uses it, by its path under `plugins/`.

| Module | For | A plugin that uses it |
|---|---|---|
| `wings-plugin-sdk/messages` | `channel()`: questions, events and queries between main and the pages | `tasks/src/lib/messages.ts`, answered in `tasks/src/main.ts` |
| `wings-plugin-sdk/react` | `useQuery`, `useSetting` and `useBroadcast`, for a page | `hud/src/pages/sidebar.tsx` |
| `wings-plugin-sdk/settings` | `setting()`: a stored value that every frame reads and hears change | `tasks/src/lib/view-setting.ts` |
| `wings-plugin-sdk/sessions` | `followSessions()`: the Claude sessions in panes, each run's older entries and then its live ones | `hud/src/main.ts` |
| `wings-plugin-sdk/tools` | `tool()`, `s` and `registerTools()`: Claude's tools, defined once in `src/tools.ts` | `changes/src/tools.ts` |
| `wings-plugin-sdk/cli` | `cli()`: `gh`, `glab` and `az` calls that tell "not installed" and "signed out" from a failure, and the badge with Install, Sign in and Try again | `work-item/src/lib/az.ts` |
| `wings-plugin-sdk/azure` | `workItems()`: Azure DevOps work items, one `az rest` for every id asked at once | `work-item/src/main/badge.ts` |
| `wings-plugin-sdk/github` | `githubPulls()`: the open PRs of GitHub repositories, one `gh api graphql` for every repository asked at once | `pr-tracker/src/lib/forges/github.ts` |
| `wings-plugin-sdk/git` | `git`, `gitOk`, `repoRoot`, `parseRemote`, `onGitChange` and more: read a repo through `wings.exec`, and hear it change | `changes/src/lib/changes.ts` |
| `wings-plugin-sdk/poll` | `pollWhileFocused()`: a timer that runs only while Wings is in front | `dev-servers/src/main.ts` |
| `wings-plugin-sdk/async` | `cached`, `batched`, `serial`, `singleFlight`, `mapLimit`, `latestOnly`, `openDiffWhile` and `errorMessage` | `work-item/src/main/sections.ts` (`cached`, `singleFlight`), `worktrees/src/lib/git.ts` (`mapLimit`) |
| `wings-plugin-sdk/format` | `ago`, `plural`, `basename`, `truncate`, `tokens` and `clock` | `hud/src/lib/view.ts` |
| `wings-plugin-sdk/shell` | `mountShell()`: the frame of a page that builds its own DOM | `changes/src/pages/graph.ts` |
| `wings-plugin-ui` | The components, the page hooks (`useProject`, `useAction`, `usePoll` and more) and the `show*` functions | `tasks/src/pages/block.tsx` |

### Tools

Claude's tools, which are also `wings <plugin> <tool>` commands, are written once, in `src/tools.ts`, which exports `tools`: an object of `tool()` definitions keyed by the tool's name. `s` builds the input's schema, which is plain JSON Schema, and the type of `input` comes from it. The main script answers all of them with `registerTools(tools)` from `wings-plugin-sdk/tools`. The build writes `contributes.mcpTools` of the manifest from `tools`, so the two can't differ. `mcpInstructions`, the line that says when Claude should reach for them, stays in the manifest.

```ts
// src/tools.ts
import { git } from "wings-plugin-sdk/git";
import { s, tool } from "wings-plugin-sdk/tools";

export const tools = {
  list_branches: tool({
    description: "Lists the branches of this session's project, newest commit first.",
    input: s.object({ remote: s.boolean().describe("Include the remote branches.").optional() }),
    // Needs "git for-each-ref" in permissions.exec.
    async run({ remote }, { project }) {
      if (!project) throw new Error("This call didn't come from a pane in Wings.");
      const out = await git(project.path, ["for-each-ref", "--sort=-committerdate", "--format=%(refname:short)", remote ? "refs" : "refs/heads"]);
      return out.split("\n");
    },
  }),
};
```

A call's input is checked against its schema before `run`, so a wrong one comes back to Claude as an error that names the field. `run(input, { paneId, project, session })` also says who called, as `wings.onTool` does. `src/tools.ts` and what it imports run in Node while the manifest is built, with a `wings` that does nothing, so nothing at their top level may need Wings.

### Fixtures

A page is looked at before it ships. A file in `preview/` renders a page with made-up data, and `wings-plugin shot` in the plugin's folder (in this repo, `pnpm --dir plugins shot <plugin>`) draws every one in headless Chrome, 1000 and 640 px wide, and prints the PNG paths. It needs Chrome or Chromium (`CHROME=/path/to/chrome` if it isn't in the usual place), Node 22.4 or newer, and `wings-plugin-ui` and React installed in the plugin. `--width`, `--height` and `--out <dir>` change the sizes and where the PNGs go. `<plugin>/<file>` or `<plugin>/<file>/<state>` picks fewer, with the plugin's folder name for `<plugin>`. A file that exports `states` makes one screenshot for each, and a state sets what the page gets from Wings. Its fields go over the ones the file exports, so a state gives only what differs. They are `page`, `props`, `queries` (what main serves a `useQuery` with: a function of the key, or a value for every key), `answers` (what main answers an `ask` with), `storage`, `project`, `args`, `focused` (what `wings.focused` and `onFocus` say, true by default), `panes` and `projects` (what `onPanes` and `onProjects` hand their listener once, and `projects()` answers: none by default). A file that starts with `_` is shared data, not a fixture.

```tsx
// preview/branches.tsx
export { default, page } from "../src/pages/branches";

// Main serves two branches for any folder, and the page is the project "web".
export const queries = { branches: () => ["main", "fix-login"] };

export const states = {
  branches: {},
  // A folder with no branches.
  empty: { queries: { branches: () => [] } },
};
```

The header of [`shot.mjs`](../packages/plugin-sdk/shot.mjs) in the SDK lists every field and is where to look for the rest.

## Manifest

```json
{
  "id": "pr-tracker",
  "name": "PR tracker",
  "version": "0.1.0",
  "description": "Shows the pull request for each pane's branch.",
  "api": 1,
  "main": "dist/main.js",
  "permissions": {
    "exec": [
      "git branch --show-current [--show-current]",
      "gh pr list [--head --state --json]",
      "gh pr view [--json]"
    ],
    "transcript": [
      "pr-link"
    ],
    "openUrl": [
      "https://github.com/"
    ]
  },
  "contributes": {
    "ui": [
      "badges",
      "diff"
    ]
  }
}
```

| Permission | Allows |
|---|---|
| `exec` | Running these commands, with no shell and no stdin, 30 s timeout by default. Each entry is a program and the subcommand the arguments must start with, so `gh pr view` allows `gh pr view <url> --json title`. A bare program name allows any arguments. An entry can end with the only flags it allows, like `az boards work-item update [--id --state]`, which refuses `--title` or any other flag. Only flag names are listed; their values, and arguments that don't start with `-`, stay free. A flag in the command itself is listed too, like `git branch --show-current [--show-current]`. `[]` allows no flags at all, like `git remote get-url []` (Wings 0.1.108; older Wings refuse the plugin, so set `"minWingsVersion": "0.1.108"`). Give every entry a list: one without lets the plugin pass any flag Wings doesn't refuse outright, so list exactly the flags its calls pass, including the ones a helper adds, like `-o json` and `--organization` from `wings-plugin-sdk/cli` callers |
| `post` | POSTing to these API paths through a signed-in CLI with `wings.post`. Each entry is `gh`, `glab` or `az` and a path where `*` stands for one segment, like `gh repos/*/*/pulls/*/comments/*/replies`. `az` paths are full `https://` URLs. An entry may end with ` {field=a\|b}`, which pins that body field to one of those values, like `gh repos/*/*/pulls/*/reviews {event=APPROVE}` (an approval that can't be turned into a "request changes"); a request whose field has another value, or none, is refused. A `gh` or `glab` entry is a plain path, with no query, `=` or empty or `.` segment, and a post whose path ends in `graphql` may only read: a `mutation` or `subscription` is refused. `exec` refuses the write flags of `gh api` and `glab api` |
| `transcript` | Reading these Claude Code transcript entry types. `attachment:<kind>`, like `attachment:model`, allows one kind of attachment entry: attachments include whole files and hook output, so ask for the kinds you need rather than `attachment` |
| `openUrl` | Opening URLs that start with these prefixes: `https://` for any page, an https prefix whose host ends with `/` like `https://dev.azure.com/`, or http to this computer only (`localhost`, `127.0.0.1`, `[::1]`, without credentials) with a port or path next, like `http://localhost:`. Any other entry is refused when the plugin loads, since `https://dev.azure.com` would also match `https://dev.azure.com.example.com/` |
| `fetch` | Calling https URLs through `wings.fetch`. A bare entry is a prefix for any method, like an API's base URL ending in `/`. `GET <prefix>` allows only GET and HEAD. `POST <url>` allows one write: `*` stands for one path segment, there is no query, and the body must be a JSON object (or absent, when nothing is pinned) up to 64 KB, like `POST https://api.example.com/2.0/repos/*/*/pulls/*/comments`. A POST entry may end with ` {field=a\|b}` to pin body fields, like `POST https://codeberg.org/api/v1/repos/*/*/pulls/*/reviews {event=APPROVED}`. Wings refuses `PUT`, `PATCH` and `DELETE` on a `GET` or `POST` entry. Needs Wings 0.1.27 |
| `secrets` | The secrets the plugin may keep with `wings.secrets.set`, each with the one URL prefix `wings.fetch` sends it to as `bearer`, like `{ "azure-devops": "https://dev.azure.com/" }`. The prefix is an https URL whose host ends in `/`, like a `fetch` prefix. `wings.secrets.set` refuses a name that isn't here, and `wings.fetch` refuses a `bearer` for a URL outside its prefix, so a plugin that calls two hosts can't send one's token to the other. The approval dialog shows each binding. Needs Wings 0.1.108, which refuses `secrets.set` and `bearer` without it |
| `panes` | Opening terminals with `wings.openPane` and moving focus with `wings.focusPane`. Each entry is a command it may start in the new pane, matched like `exec`, so `npm run dev` allows `npm run dev --port=3000`. For `claude`, only the flags an entry names may be passed: `claude --resume` allows resuming, and `claude` alone allows no flags. `[]` allows plain shells only. Leave it out and the plugin can do neither |
| `notify` | `true` to show desktop notifications with `wings.notify` |
| `callTools` | Other plugins' tools the plugin calls with `wings.callTool`: a plugin id for all of its tools, or `<plugin> <tool>` for one, like `tasks list`. Only between Wings' own plugins, the ones installed from this repo's `plugins/`; a third-party plugin is refused whatever it declares. Needs Wings 0.1.94 |
| `events` | Events from other plugins the plugin hears with `wings.onEvent`: `<plugin> <event>`, like `pr-tracker pr-state`. Only between Wings' own plugins, both the sender and the receiver; a third-party plugin neither sends nor hears any. Wings 0.1.98, built-in plugins only |
| `worktrees` | `true` to make a git worktree of one of your projects with `wings.createWorktree`: a new branch in `<repo>-worktrees/<branch>` beside the repo, added to Wings as a project. Needs Wings 0.1.93 |
| `sendToAgents` | `true` to type a message into the prompt of the agent in a pane with `wings.sendToAgent`. Enter is only pressed while the user lets agents message each other (Settings, Agents). Needs Wings 0.1.87 |
| `ports` | `true` to see, with `wings.ports`, which local TCP ports the programs in your terminals listen on, with each program's name, pid and start time. Only ports that `localhost` reaches, in panes Wings started |
| `agentActivity` | `true` to read what an OpenCode session is doing with `wings.agentActivity` and hear when it changes with `wings.onAgentActivity`: its newest prompts and tool calls, model, context and token totals. OpenCode keeps no transcript, so Wings asks its background service; the plugin never sees the service's address or password. Needs Wings 0.1.63 |
| `diskUsage` | `true` to read the names and sizes of the files and folders on your disks with `wings.diskUsage`, look for files and folders by name with `wings.diskFind`, and list the mounted disks with `wings.disks`. Needs Wings 0.1.70 |
| `statusline` | `true` to read what Claude Code tells its status line with `wings.statusline`: your usage limits and each session's context and cache. See [Status line](#status-line) |

`icon`, optional, is an SVG file in the plugin folder, like `"icon.svg"`. The Plugins list shows it instead of the puzzle piece, and a sidebar, view or panel that leaves out its own `icon` uses it on its title bar button, in the overflow menu and on its tab. Wings draws it as a one-colour mask in the current text colour, so use shapes (`fill` or `stroke`, `currentColor` is fine) and expect colours and gradients to come out as a plain silhouette. It must be a relative path to an `.svg` file inside the plugin folder (no `..`, no absolute path, no link out of it), up to 32 KB, with `svg` as its root element, or Wings refuses to load the plugin. It is only ever drawn as an image, never inserted into a page, so scripts, `on*` attributes and links to other files in it do nothing. A sidebar's, view's or panel's `icon` is still one of the built-in names; it can't point to a file. Older Wings ignores the top-level `icon` and keeps the puzzle piece, so it needs no `minWingsVersion`. A sidebar or view that leaves out `icon` is refused by Wings 0.1.19 and older, so set `"minWingsVersion": "0.1.20"` when you do that. `wings-plugin pack` ships every file except `src/`, `preview/`, `node_modules`, package and lock files, `tsconfig*.json`, `HANDOVER*`, dotfiles and other `.wings-plugin` files, `icon.svg` included. Several pages of one plugin show the same icon: give all but one a built-in `icon`.

`minWingsVersion`, optional, is the oldest Wings that can run the plugin, like `"0.1.13"`. Set it when the plugin uses something an older Wings doesn't have. An older Wings refuses to install it and doesn't offer it as an update.

`contributes` lists what the plugin adds, which the manager shows next to it:

| Field | Means |
|---|---|
| `ui` | `badges` for pane header badges, `status` for the strip under a pane's terminal, `rail` for the card over it, `diff` for the diff viewer. Wings refuses `setBadge`, `setPaneStatus`, `setPaneRail` and `openDiff` without them. `status` needs Wings 0.1.32: set `"minWingsVersion": "0.1.32"`, since older Wings refuse the unknown `ui` value |
| `mcpTools` | `{ name, description, inputSchema? }` tools the plugin offers Claude through the Wings MCP server, handled with `wings.onTool`. Names are lowercase letters, digits and `_`, and Claude sees them as `<plugin id>__<name>`. `inputSchema` is a JSON Schema object for the arguments, `{ "type": "object" }` by default. A plugin with a `src/tools.ts` doesn't write this field: the build does, from the tools in that file (see Tools) |
| `hooks` | Claude Code hooks the plugin answers with `wings.onHook`: `"SessionStart"` to add context to a session, `"PreToolUse"` to refuse a Bash command (it only ever sees Bash), `"UserPromptSubmit"` to add context to each message the agent gets (Wings 0.1.77), `"Journal"` to get an agent's per-turn change journal (see [Claude Code hooks](#claude-code-hooks)). Anything else is refused. Needs Wings 0.1.54: set `"minWingsVersion": "0.1.54"`; `"Journal"` needs Wings 0.1.61 |
| `mcpInstructions` | A line for Claude, up to 500 characters, on when to reach for the plugin's `mcpTools`. Claude Code defers MCP tools behind tool search, so at session start it sees only tool names and the server's instructions, and never reads the tool descriptions until it searches. Wings adds this line to the instructions it sends when Claude starts a session (the cap is 4,096 characters in all, and a line that doesn't fit is left out), for plugins that are on. Name the tools and say what the user says that should trigger them. A change takes effect in new Claude sessions. Wings 0.1.48 and newer send it; older Wings ignore the field, so no `minWingsVersion` is needed |
| `sidebars` | Up to 3 `{ id, title, icon?, page }`: the plugin's own HTML `page`, shown in the right sidebar from a title bar button. It runs sandboxed like the plugin, with the same `wings` object, and Wings loads `plugin-ui.css` first so plain buttons, inputs and selects look native. Several open sidebars show as tabs, and each keeps running while hidden. A sidebar may add `popover: { page, width, height }`, a smaller page shown under its button when you point at it or focus it, 240 to 480 wide and 160 to 640 high; see [Popovers](#popovers) |
| `views` | Up to 3 `{ id, title, icon?, page, opens?, width?, height?, newTab?, multiple?, noProject?, button? }`: pages like `sidebars` that can also open in a modal over Wings, in a window of their own or as a tab. `opens` lists where, from `sidebar`, `modal`, `window` and `tab`, and the title bar button opens it in the first; it's `["sidebar"]` by default. `width` and `height` size the modal (560 by 480 by default) and the window when it first opens; Wings remembers a window's size and position after that. The sidebar and the modal offer to move a page that can open as a window into one. A page gets the same `wings` object wherever it opens, and a modal's page stops when the modal closes. `newTab: true` lists a view that opens as a tab in the + menu and the command palette (see Tabs). `multiple: true` lets it open several tabs per project. `noProject: true` marks a page that works with no project on screen, so the + menu and the command palette offer it then too, and open it as a tab that goes away once a project is added. All three need `tab` in `opens`, or Wings refuses the plugin. `button: false` leaves a view out of the title bar's plugin buttons, for a page that shows one item and only makes sense opened from a row with its `args` (Wings 0.1.37). Ids are unique across `sidebars` and `views`. Disabling or removing the plugin, or closing Wings, closes its windows |
| `projectSections` | Up to 8 `{ id, title, icon?, all?, sorts? }`: lists the plugin can show under each project in the sidebar, filled with `wings.setProjectSection`. `id` is like a view id, unique in the plugin, `title` is up to 40 characters, and `icon` is one of the built-in names, or left out to use the plugin's icon. `all` names one of the plugin's views that opens as a tab and lists everything: the section header's All button opens it in that project (Wings 0.1.29; older Wings ignore it). `sorts` is up to 8 `{ id, label }` orders the menu on the section header offers: the plugin sorts the rows itself, hears a pick with `wings.onSectionSort` and checks the current one with `wings.setSectionSort` (Wings 0.1.75). See Project sections. Needs Wings 0.1.23 |
| `blocks` | Up to 8 `{ id, title, page, icon?, minHeight? }`: pages the user can place on a project's Project page, each in a frame of its own. `id` is like a view id, unique among the blocks, `title` is up to 40 characters, `page` is an HTML file in the plugin, which a view may share, `icon` is one of the built-in names or left out for the plugin's icon, and `minHeight` is 80 to 800 CSS pixels (default 160). See Blocks. Needs Wings 0.1.24 |
| `settings` | `{ page }`: the plugin's own HTML page in Wings Settings, listed under Plugin settings by the plugin's name. It runs sandboxed like a sidebar, with the same `wings` object, fills the settings area and stops when you leave it or close Settings. Keep what it saves in `wings.storage`. See [Settings page](#settings-page) |
| `panels` | Up to 3 `{ id, title, icon?, url, width?, height? }` web pages, each opened in a popover from a title bar button. `icon` is one of `clock`, `globe`, `calendar`, `chart`, `list`, `git-compare`, `git-graph`, `git-branch`, `folder-tree`, `message`, `scroll`, `kanban`, `gauge`, `activity`, `timer`, `server`, `terminal`, `todo` or `puzzle` (Wings 0.1.20 and newer for all but the first five), and `url` must be https. The page runs as a normal website with no access to Wings, and keeps its cookies, so a sign-in sticks |

`id` is lowercase letters, digits and dashes. `api` must be `1`. A `page` is a built file, like `dist/sidebar.html`: the build fails on one it doesn't write and the folder doesn't have.

A command runs as you, with that CLI's own sign-in, so `gh api` can read anything your GitHub account can. Only declare commands you need, because that list is what the user approves. A command runs in its `cwd`, which has to be a folder inside one of your Wings projects once `..` and symlinks are resolved, or inside a git repo one of your open panes is in (a built-in plugin may use any git repo); git needs one, and any other program without one runs in `/`. On top of the manifest, Wings refuses, whatever it says:

- an option before a `git`, `gh`, `glab` or `az` command, like `git -C . config`, and the commands that change a CLI's own settings: `git config`, `gh config`, `gh alias`, `gh extension`, `glab config`, `glab alias`, `az config`, `az configure`, `az cloud`, `az extension` and `az devops configure`;
- flags that run other commands, touch files, change data on the server or print a token: `git -c`, `--upload-pack`, `--output`, `--no-index`, `gh`/`glab` `-X`, `--method`, `-f`, `-F`, `--input`, `--form`, `--show-token`, and for `az` `--method`, `--http-method`, the file flags (`--in-file`, `--out-file`, `--output-file`, `--file`, `--destination`, `--source` and their short forms) and `@path` values, which az reads as files;
- a git path that leads outside `cwd`: one with `..`, or one whose file or folder, symlinks resolved, is elsewhere, since `git diff <a> <b>` compares any two files once one is outside the repo, or anywhere outside a repo. A path that names nothing on disk, like a revision, is fine. Also a `git fetch` refspec with a destination or `+`;
- a `gh api` or `glab api` endpoint that isn't a plain path like `projects/g%2Fp/merge_requests?per_page=100` (glab sends its token to any host a full URL names), and `env` or `$ENV` in gh's `--jq`;
- an `az` call that reaches anything but Azure DevOps: `--organization` must be a `dev.azure.com` or `<org>.visualstudio.com` organization and `az rest --url` an Azure DevOps URL. `az account get-access-token` takes only `--resource 499b84ac-1321-427f-aa17-267ca6975798` and `-o`, `az login` only `--allow-no-subscriptions` and `-o`, `gh auth login` only `--web`, `--clipboard` and `--hostname`, and `glab auth login` only `--web` and `--hostname`.

git also ignores a bare repo it finds in a command's folder (`safe.bareRepository=explicit`), and a package with a `.git` folder or file isn't installed. That still can't cover every subcommand of every CLI, so treat an `exec` entry as trusting the plugin with that command.

A `panes` command is typed into the new pane's shell, the way Wings resumes a session, so you're back at a prompt when it exits. It runs as you in a terminal you can see, the same trust as `exec`, and the same checks run on everything typed after the program, `args` included. A `\` in a value is written outside its quotes, so fish reads it like the other shells. Since a shell reads it, every word of an entry and of a command may only use letters, digits and `-_./:=@%+,`: no spaces inside a word, quotes, `$`, globs or operators. Words past the declared entry come from the plugin and are single-quoted. A `cwd` has to be a folder inside one of your Wings projects once `..` and symlinks are resolved, so neither can lead out of it.

Plugins don't load on Windows yet. WebView2 gives child frames the app's IPC bridge, so each plugin needs its own webview there first.

## API

| Call | Does |
|---|---|
| `wings.onPanes(fn)` | Called with every terminal pane on each change, including a change of focus, and right away when Wings already sent them (Wings 0.1.54): `{ paneId, cwd, command, project, projectId, session, focused }[]`. `project` is the project's name and `projectId` its id, the same as `wings.project().id` (Wings 0.1.22). `cwd` follows `cd`. `focused` is true on the focused pane of the tab on screen. `session` is `{ sessionId, kind, name, state, startedAt }` while an agent runs in the pane, else `null`; `kind` is `"claude"` or `"opencode"` (Wings 0.1.60, which also started passing opencode sessions), and only a Claude session has a transcript. `startedAt` is when that `claude` started, in ms, since a resumed session's transcript also holds what came before |
| `wings.onFocus(fn)` | Called with `{ focused }` once with the current state, then on every change: `focused` is true while any Wings window (the main one or a view window) is in front, false while you're in another app or Wings is minimized or hidden. A short move between two Wings windows doesn't count as a change. Needs no permission. `wings.focused` reads the last state (true until Wings sends it, which is right after the frame is ready). Needs Wings 0.1.17: set `"minWingsVersion": "0.1.17"`. See Pausing polls |
| `wings.onTool(name, fn)` | Runs one of your `mcpTools` when Claude calls it. `fn(input, { paneId, project, session })` gets Claude's arguments and who called: `paneId` is the Wings pane Claude runs in, `project` is that pane's project `{ id, name, path }`, and `session` is the agent session in it `{ id, name, kind }`, with `name` `null` while it has none and `kind` `"claude"` or `"opencode"` (Wings 0.1.60; before it only Claude sessions were passed). Each is `null` when there is none: no pane, or Claude hasn't started its session yet. `project` and `session` need Wings 0.1.50. It returns a string or a JSON value. A thrown error goes back to Claude as a failed call. Register it in `main`, since sidebar pages don't get tool calls. With a `src/tools.ts`, `registerTools` does this for you (see Tools) |
| `wings.onHook(event, fn)` | Answers a Claude Code hook listed in `contributes.hooks`. `fn(input, { paneId, project, session })` gets the hook's JSON from Claude Code (`session_id`, `cwd`, `hook_event_name`, plus `source` for SessionStart, `tool_name` and `tool_input.command` for PreToolUse, `prompt` for UserPromptSubmit, or the journal fields for Journal) and who is asking, like `onTool`; `session.id` is `input.session_id`. It returns a string (context), `{ context?, deny? }`, `null` or `undefined`, or a promise of one. `deny` only counts for PreToolUse and is the reason Claude sees. Wings waits 1.5 s per plugin, and a thrown error, a late answer or an empty one counts as nothing, so Claude is never held up. Register it in `main`. Needs Wings 0.1.54 |
| `wings.setPaneRail(paneId, rail)` | Floats a card over a pane's terminal, or removes it with `null`. Needs `rail` in `contributes.ui`. See [Pane rail](#pane-rail). Needs Wings 0.1.54 |
| `wings.onAction(fn)` | Called with `{ paneId, actionId }` when a badge action is clicked. Return a promise: the button shows a spinner until it settles (up to 5 min), and a thrown error is shown in the card |
| `wings.exec(program, args, { cwd, timeoutMs, onOutput })` | Resolves `{ code, stdout, stderr }`. `timeoutMs` is 30 s by default, 5 min at most. `onOutput(line)` gets each stdout and stderr line while the program runs. A program that prints more than 8 MB to stdout or stderr is killed and the call rejects with `Output is larger than 8 MB`, not a truncated result: ask for less (`git diff` a few files at a time) |
| `wings.transcript(sessionId, types, { last? })` | Resolves the matching transcript entries, oldest first. `last` keeps only the newest that many (up to 1000), which is much faster on a long session |
| `wings.statusline()` | Resolves `{ rateLimits, sessions }`, what Claude Code last told `wings statusline`. `rateLimits` is `{ fiveHour, sevenDay, at }`, each window `{ usedPercentage, resetsAt }` with `resetsAt` in epoch seconds, or `null` until a session has reported them (only Pro and Max plans get them). `sessions` has, by session id, `{ model, contextWindowSize, usedPercentage, totalInputTokens, totalOutputTokens, currentUsage, promptCache, at }`, Claude Code's own numbers from the last response. `at` is when Wings got it, in ms |
| `wings.agentActivity(sessionId)` | Needs `permissions.agentActivity` (Wings 0.1.63). Resolves `{ model, context, tokens, cost, items }` for an OpenCode session (a pane's `session.kind` is `"opencode"`): `items` are its newest prompts (`{ kind: "prompt", id, at, text }`) and tool calls (`{ kind: "tool", id, at, name, input, status, ms, result, files }`, `status` is `running`, `completed` or `error`, `files` the absolute paths the call changed), oldest first; `tokens` is `{ input, output, reasoning, cacheRead, cacheWrite }` for the whole session; `context` is what the newest reply read. Wings reads one page of the latest messages, a few ms, never the whole history. Rejects when OpenCode's service isn't running or doesn't know the session |
| `wings.onAgentActivity(fn)` | Needs `permissions.agentActivity`. Called with `{ sessionId }` when an OpenCode session in a pane starts or ends a run or does a tool call, as one event per burst (about one a second at most). Read what changed with `wings.agentActivity`, instead of polling |
| `wings.onTranscript(fn)` | Called with `{ sessionId, paneId, entry }` for each transcript entry Claude Code writes while it runs in a pane, when the entry's `type` is in `permissions.transcript`. Only entries written after the plugin started, or after the session started in the pane: read earlier ones with `wings.transcript`. Wings looks for new entries twice a second and skips entries over 256 KB, which are large tool results |
| `wings.setBadge(paneId, badge)` | Shows a badge in the pane header; `null` removes it. Needs `badges` in `contributes.ui`. Sending what this frame last sent for the pane does nothing, `null` included, so call it on every poll. A call for a pane that has closed rejects with `no pane <id>`. See [Badges](#badges) |
| `wings.setPaneStatus(paneId, status)` | Shows a status strip under the pane's terminal; `null` removes it. Needs `status` in `contributes.ui`. See [Pane status](#pane-status) |
| `wings.openUrl(url)` | Opens the URL in the browser |
| `wings.ports()` | Needs `permissions.ports`. Resolves `{ paneId, pid, name, port, startedAt }[]`: the local TCP ports listening in each pane's shell and everything it started, sorted by pane and port. `startedAt` is in ms. Wings reads the process tree and runs `lsof` with fixed arguments, about 20 ms a call (macOS and Linux with `lsof`; rejects where it is missing), so poll every 15 s at most, and only while focused |
| `wings.diskUsage(path, { minBytes?, depth?, maxEntries?, onProgress?, signal? })` | Needs `permissions.diskUsage` (Wings 0.1.70). Scans a folder or a disk in Wings, on several threads, and resolves `{ root, floor, entries, restricted, ms }`. `path` is absolute, or starts with `~` for your home folder. `root` is a tree of `{ name, size, kind, items, children?, smaller?, restricted? }`: `size` is bytes on disk, like `du` (allocated blocks, a file with several hard links counted once, and from Wings 0.1.71 on macOS the blocks a file of 64 KB or more shares with its APFS clones counted once), `kind` is `dir`, `file`, `link` (never followed) or `other`, `items` counts the files and folders under a folder at any depth, and `children` are its entries of at least `floor` bytes, biggest first. Everything under the floor, or past `depth`, is added up in the folder's `smaller: { size, items }`, so a map can draw the rest as one slice. `minBytes` is the floor (1 MiB by default); `depth` is how many levels are kept under the root (0 to 64, 64 by default; 1 keeps only the root's own entries); `maxEntries` (1 to 100,000, 20,000 by default) caps what comes back by raising the floor, which `floor` reports. A folder Wings can't open, for its permissions or macOS privacy, is `restricted: true` with its own size and `items: 0`, is listed even under the floor (until `maxEntries` folds it away), and is counted in `restricted`. The scan stays on the volume `path` is on, and scanning `/` on macOS skips `/System/Volumes`, which the root already reaches through its firmlinks. `onProgress({ entries, bytes, path })` is called about five times a second with the count so far and a folder it is reading. Abort `signal` to stop it: the call rejects with `The scan was stopped`. A path that doesn't exist rejects with `There's no file or folder at <path>` |
| `wings.diskFind(path, { names, skip?, onProgress?, signal? })` | Needs `permissions.diskUsage` (Wings 0.1.70). Looks under a folder or a disk, on several threads, for files, folders and links called exactly one of `names` (1 to 16 names, each 1 to 255 characters with no `/`, like `.git`), and resolves `{ paths, entries, restricted, ms }`: their full paths, sorted, at most 10,000 (with that many it stops, and there may be more). It reads folder listings only, with no sizes, and never goes into a match, a folder named in `skip` (up to 32 names, like `node_modules`), a symbolic link or another volume, so a repo's `.git` is found without opening it. The root's own name isn't tested. `path` (`~`, a link as the root, the volume rule, `/System/Volumes` on macOS) is as in `diskUsage`, and so are `onProgress({ entries, bytes, path })`, about five times a second with `bytes` always 0, and `signal`: abort it and the call rejects with `The scan was stopped` |
| `wings.disks()` | Needs `permissions.diskUsage` (Wings 0.1.70). Resolves the mounted disks Finder shows, the startup disk first, as `{ name, mount, fs, kind, removable, readOnly, total, free }`: `kind` is `ssd`, `hdd` or `unknown`, `total` and `free` are bytes, and `free` counts what macOS can purge on its own as free, the way Finder does |
| `wings.highlight(code, lang)` | Wings 0.1.108, needs no permission. Colors `code` as `lang` (a name like `typescript`, `ts`, `rust`, `json`, `bash`; Wings loads the grammar the first time a language is used) and resolves one array per line of `[text, color?]` tokens in the dark theme, or `null` for a language Wings has no grammar for. It rejects over 100,000 characters. The page draws the spans itself, so no markup crosses over and no highlighter ships in the page. `CodeBlock` and `Markdown` fences in `wings-plugin-ui` call it for you, with the `lang` prop and `languageOf(path)` |
| `wings.openPane({ command?, args?, cwd?, placement? })` | Opens a terminal and resolves `{ paneId }` once it's in `onPanes`. `args` are values after `command`, each passed as one shell word even with spaces in it, like a prompt for `claude`: up to 4 KB together, none may start with `-` or hold a newline or other control character (Wings 0.1.37). `placement` is `tab` (the default), or `right` or `down` for a split beside the focused pane, which has to be in the project on screen. Without `cwd` it opens at the root of the project on screen. The new pane gets the keyboard, unless `focus: false` (Wings 0.1.93) opens a new tab behind the one on screen |
| `wings.focusPane(paneId)` | Switches to an open pane's project and tab and moves the keyboard to it |
| `wings.callTool(name, args?, { project? })` | Needs `permissions.callTools` (Wings 0.1.94), and both plugins have to be Wings' own. Runs another plugin's tool, `name` like `tasks__list`, as if Claude called it in `project` (the frame's project without it), and resolves `{ text, isError }`: the tool's text answer, as Claude would see it |
| `wings.emit(name, data?)` | Sends the event `name` (lowercase letters, digits and dashes) with JSON `data` up to 16 KB to every other running plugin that declared `<your plugin id> <name>` in `permissions.events`. Resolves once delivered; nothing happens for a plugin that didn't declare it. Wings 0.1.98, built-in plugins only |
| `wings.onEvent(listener)` | Needs `permissions.events`. Calls `listener({ from, name, data })` for each declared event another plugin sends, in the main script. Returns a function that removes the listener. Wings 0.1.98, built-in plugins only |
| `wings.createWorktree({ projectId?, branch, base? })` | Needs `permissions.worktrees` (Wings 0.1.93). Makes a worktree of a project (the one on screen without `projectId`) on a new `branch` from `base`, or the repo's default branch, beside the repo, adds it to Wings as a project and resolves `{ projectId, path }`. Wings copies the project's gitignored local setup into it first (`.claude/rules`, `.claude/skills`, `.mcp.json`, `.env*` and the like: the repo's `.worktreeinclude`, else the project's list in Worktree setup), and runs the project's setup command, if it has one, before the first command opened in it, so the agent starts only once setup succeeded. Plugins need do nothing for either. Open a terminal there with `openPane({ cwd: path })`. Rejects when the branch or folder already exists |
| `wings.worktreeSetup(projectId?)` | Needs `permissions.worktrees` (Wings 0.1.99). What a project's new worktrees get, the settings of Worktree setup in the sidebar: `{ include, defaults, hasIncludeFile, setup }`. `include` is the project's pattern lines, or null for the default list in `defaults`; `hasIncludeFile` says the repo's `.worktreeinclude` is used instead; `setup` is the shell line, or null |
| `wings.setWorktreeSetup(projectId, { include?, setup? })` | Needs `permissions.worktrees` (Wings 0.1.99). Changes those settings, only the fields given: `include` an array of pattern lines (`.gitignore` syntax, `!` leaves files out, a custom list replaces the defaults; null resets to them) and `setup` a one-line shell command (null clears it). `setup` is refused for a plugin that isn't one of Wings' own, since it runs in every new worktree. The sidebar dialog and Claude's `worktree_setup` tool show the change at once |
| `wings.sendToAgent(paneId, text)` | Needs `permissions.sendToAgents` (Wings 0.1.87). Pastes `text` into the prompt of the agent (Claude Code or opencode) in a pane, after a first line naming your plugin, and resolves `{ submitted }`: Wings presses Enter only while the user turned on "Let agents message each other" in Settings, Agents. Rejects for a pane without an agent (a shell would run the text), an agent waiting on a permission prompt, or one with text the user typed and hasn't sent. A message left unsent (Enter is off) stays, and the next one goes after it. Up to 8 KB; newlines and tabs are fine, other control characters are refused |
| `wings.installTool(program)` | Asks Wings to install a CLI the plugin runs, after `exec` rejected with `<program> is not installed or not on PATH`. See [Required tools](#required-tools). Resolves `true` once the install started or it's already there, `false` if you cancelled |
| `wings.notify({ title, body?, projectId?, actions? })` | A desktop notification, with your plugin's name before the title. The title is cut at 64 characters and the body at 256. Up to 20 a minute per plugin; more are refused with how long to wait. Clicking the desktop notification does nothing. It's also kept in the Inbox, where up to 3 `actions` are buttons: `{ label, open: { view, args? } }` opens one of your views that opens as a tab, in the project `projectId` names or else the one on screen, and `{ label, url }` opens an https link your `openUrl` permission allows. A label is cut at 32 characters. Buttons need Wings 0.1.45 |
| `wings.openDiff({ title, subtitle, cwd?, patch?, comments?, focus? })` | Opens the diff viewer and resolves `{ id }`. Without `patch` it opens in a loading state. `focus` is `{ path, line?, side?: "old" \| "new" }`: once the patch is shown the viewer opens that file, scrolls to the comment thread on that line and highlights the line for a moment (`side` defaults to `new`). It works when the patch arrives later through `updateDiff`, and a path or line the patch doesn't have just opens the diff as usual. Needs Wings 0.1.27. `cwd` is the repo or folder the diff is of: the viewer's review comments (written by the user on any line, sent to an agent with Send to agent) offer the agent working there first |
| `wings.updateDiff(id, { patch, comments, focus? } \| { error })` | Fills in or fails a viewer opened with `openDiff`; ignored once it's closed. A new `focus` is acted on once, so an update with only `comments` leaves where the user is |
| `wings.onReply(handler)` | A reply typed in the diff viewer under a comment with `canReply`: `handler({ diffId, replyTo, body })`, where `replyTo` is the thread's first comment. Post it, then add it with `updateDiff`. The reply box waits for the returned promise, and a thrown error shows under it with the text kept |
| `wings.post(program, url, fields, { host })` | POSTs `fields` (string or whole-number values, up to 64 KB) as JSON to a `permissions.post` path, as you: `gh api` (pass `host` for a GitHub Enterprise Server; github.com is the default) or `glab api` against `host`, or `az rest` for Azure DevOps. Wings builds the command itself. Resolves `{ code, stdout, stderr }` |
| `wings.fetch(url, { method, headers, body, bearer, binary })` | An HTTP call made by Wings to a `permissions.fetch` URL. `bearer` names a secret sent as `Authorization: Bearer <secret>`, refused for a URL outside the prefix `permissions.secrets` binds it to. Resolves `{ status, contentType, body }` for any status and doesn't follow redirects. With `binary: true` (Wings 0.1.108) `body` is the response as base64, up to 10 MB, for an image: `data:${contentType};base64,${body}` shows it in an `<img>`. The URL must already be in normal form, and its path may only use letters, digits and `-_.~/`. Headers are limited to `Accept`, `Accept-Language`, `Content-Type`, `Cache-Control`, `If-None-Match` and `If-Modified-Since` |
| `wings.secrets.set(name, value)`, `.delete(name)`, `.has(name)` | Secrets like API tokens, kept in the system keychain under the plugin's name. `set` takes only a name `permissions.secrets` declares. They can be sent by `fetch` but not read back, and go when the plugin is removed or replaced by one from elsewhere |
| `wings.storage.get(key)`, `.set(key, value)`, `.delete(key)`, `.keys()`, `.entries(prefix)` | The plugin's own storage for settings and state, since `localStorage` throws in plugin pages. Values are JSON (what `JSON.stringify` keeps), and `get` resolves `null` for a key that isn't set. Keys are 1 to 128 letters, digits and `- _ . : /`. `entries(prefix)` resolves `[key, value]` for every key that starts with `prefix`, sorted by key, in one call (Wings 0.1.50). A value is limited to 1 MB and everything together to 50 MB, and a `set` that would go over is refused. See [Storage](#storage) |
| `wings.openView(id, { as?, args?, newTab? })` | Opens one of your `sidebars` or `views`: `as` is `sidebar`, `modal`, `window` or `tab`, one of the view's `opens`, and the first by default. A window that's already open comes to the front, and so does the view's tab in the project on screen. Only one modal shows at a time. `args` is JSON, up to 16 KB, and only with `as: "tab"`: the tab's page reads it with `wings.viewArgs()`. A tab that's already open with different `args` takes the new ones, drops the state it saved with `setTabState` (it belonged to the old args) and reloads, and with the same args or none just comes to the front. A view with `multiple` always opens a new tab. `newTab: true` (with `as: "tab"` only, Wings 0.1.39) always opens a new tab, even when the view already has one open or a `multiple` view has a tab with the same `args`; use it for a middle-click or Cmd+click in your page. Wings does the same for its own buttons: a middle-click or Cmd+click on a plugin's title bar button, + menu entry or section row opens a new tab. Needs Wings 0.1.22 |
| `wings.viewArgs()` | What `openView` passed as `args` when it opened this tab, or `null`. Synchronous, and a copy. Needs Wings 0.1.22 |
| `wings.placement()` | Where this page shows: `"sidebar"`, `"tab"`, `"modal"`, `"window"` or `"block"` (on a Project page), so a page can show more detail when it has room. `null` in the main script. A sidebar's popover reports `"sidebar"` and the settings page `"modal"`. Synchronous. Needs Wings 0.1.22 |
| `wings.project()` | Resolves `{ id, name, path, branch, parentId? }` of the project the page is about, or `null` when none is open. A tab page's project is the one the tab belongs to and never changes. Any other page gets the project on screen. Needs Wings 0.1.22 |
| `wings.onProject(fn)` | Calls `fn` with the current project, then each time it changes: the project on screen switched (never for a tab page), or its branch or name changed. Returns a function that removes the listener. Needs Wings 0.1.22 |
| `wings.projects()` | Resolves every project, `{ id, name, path, branch, parentId? }[]`, in sidebar order. Worktrees are projects too, and one Wings made has `parentId`, the id of the project it is a worktree of (Wings 0.1.98). Works in the main script and in pages. Needs Wings 0.1.23 |
| `wings.onProjects(fn)` | Calls `fn` with every project, then each time one is added, removed or renamed, or a branch changes. Returns a function that removes the listener. Needs Wings 0.1.23 |
| `wings.setProjectSection(projectId, sectionId, rows)` | Shows `rows` as a section under that project in the sidebar, or removes it with `null`. `sectionId` is one the manifest declares in `contributes.projectSections`. A row is `{ id, label, detail?, badge?: { text, tone? }, icon?, parent?, open: { view, args? } }`; `icon` replaces the row's dot, in the badge's tone: `pr-open`, `pr-merged`, `pr-closed` or `pr-draft` (Wings 0.1.35), or a work status, `todo`, `in-progress`, `in-review`, `done` or `canceled` (Wings 0.1.40). `parent` is the id of another row in the section that this row sits under, indented; a row whose parent isn't listed shows at the top (Wings 0.1.70; older Wings ignore it). Rejects for an unknown project, an undeclared section, or an `open.view` that isn't one of the plugin's views with `tab` in `opens`. See Project sections. Needs Wings 0.1.23 |
| `wings.setSectionSort(sectionId, sort)` | Checks `sort`, one of the section's `sorts` in the manifest, in the menu on its header, for every project. Send it on start and when the order changes. Rejects for a sort the section didn't declare. Needs Wings 0.1.75 |
| `wings.onSectionSort(listener)` | In the main script: `listener({ sectionId, sort })` runs when the user picks an order in a section header's menu. Sort the rows, send them with `setProjectSection` and confirm with `setSectionSort`. Needs Wings 0.1.75 |
| `wings.tabState()` | In a tab page: resolves the state it last saved with `setTabState`, or `null`. Outside a tab it resolves `null`. Needs Wings 0.1.22 |
| `wings.setTabState(state)` | In a tab page: saves JSON state, up to 64 KB, on the tab. Wings saves it with the tab and restores it on the next start and when the page loads again after being unloaded. Rejects outside a tab and when the state is too big. Needs Wings 0.1.22 |
| `wings.blockState()` | In a block: the state it last saved with `setBlockState`, or `null`. `null` outside a block. See Blocks. Needs Wings 0.1.24 |
| `wings.setBlockState(state)` | In a block: saves JSON state, up to 16 KB, with the Project page, like which board or filter the block shows. Rejects outside a block and when the state is too big. Needs Wings 0.1.24 |
| `wings.setHeight(px)` | In a block: asks for a height in CSS pixels. Wings rounds it, clamps it between the block's `minHeight` and 1200 and applies it without reloading the page. Asking for the height already asked for does nothing, and calls in one tick send the last. Throws outside a block. Needs Wings 0.1.24 |
| `wings.closeView(id)` | Closes a sidebar or view wherever it's open |
| `wings.openSettings()` | Opens Wings Settings at the plugin's settings page. Refused for a plugin without `contributes.settings` |
| `wings.setSidebarLabel(sidebarId, label, { rows, tone })` | Takes a sidebar or view id. Up to 80 characters next to its title bar button, like a running timer; `null` clears it. Wings shows it as a green pill, cut with an ellipsis when it doesn't fit, or an amber or red one when `tone` is `"warning"` or `"danger"`. `rows` are up to 6 `{ label, value }` shown when you point at it |
| `wings.broadcast(message)`, `wings.onBroadcast(fn)` | Sends a JSON value, up to 64 KB, to every other frame of the plugin that is loaded: its main script, and its sidebars, popovers, modals, windows, tabs, blocks and settings page. A frame that isn't loaded hears nothing. Use it so a page refreshes when a tool call changed something. A `channel` from `wings-plugin-sdk/messages` is built on it (see Talking to main) |

### Badges

A badge is `{ label, tone, icon?, counts?, loading?, title?, subtitle?, rows?, actions? }`:

- `tone` is one of `neutral`, `info`, `success`, `warning`, `danger`, `merged`.
- `icon` is one of `pr-open`, `pr-merged`, `pr-closed`, `pr-draft`.
- `counts` are `{ icon, value }` chips after the label, where `icon` is `changed`, `push` or `pull`. Leave out zeros.
- `rows` are `{ label, value, tone? }`, shown in the card when you click the badge.
- `actions` are `{ id, label, primary? }` buttons in that card. A click calls `onAction`.
- `loading: true` shows a spinner in the pill while the plugin refreshes.

A review comment is `{ id, replyTo, path, line, side, author, body, createdAt, url, canReply }`, where `side` is `additions` or `deletions`, and `canReply` on a thread's first comment shows a reply box under it. `path` is `null` for a comment on the whole pull request, and `line` is `null` for comments on code that has changed since. Both show under Discussion in the viewer.

Wings keeps the books for badges, statuses and project sections, so a plugin keeps no copy of what it sent. A frame skips a `setBadge`, `setPaneStatus`, `setPaneRail` or `setProjectSection` that repeats what it sent last for the same pane or section, `null` included (Wings 0.1.17 and newer for badges and statuses, 0.1.50 for sections), and Wings keeps the same badge object for an identical one and ignores section rows that are identical to what the section already shows, so setting the same thing again costs nothing. So set them on every pass, with no signature map and no list of what is shown. Wings drops the badges, statuses and rails of a pane when it closes, so a plugin needs no cleanup for closed panes, and a `setBadge` for a pane that is gone rejects with `no pane <id>`. Send `null` only to clear the badge of a pane that stays open, like a pull request that was merged.

### Pane status

A status is a slim strip under a pane's terminal, for figures you want in view all the time, like usage. A badge is something to act on; a status is something to glance at. The strip shows only while at least one plugin has set a status for the pane, and its height stays fixed while the items update.

```js
await wings.setPaneStatus(paneId, {
  items: [
    { label: "5h", value: "8% · 4h19m", percent: 8, tone: "success" },
    { label: "ctx", value: "62%", percent: 62, tone: "warning" },
  ],
  rows: [{ label: "Model", value: "Opus" }],
  open: "hud",
});
```

- `items` are 1 to 4 figures, left to right: `label` (muted, up to 16 characters), `value` (up to 32), `percent` (0 to 100, draws a small meter) and `tone`, which colours the meter and the value.
- `rows` are up to 12 `{ label, value, tone? }` lines in the card shown when you point at the strip.
- `open` is the id of one of the plugin's sidebars or views. Clicking the strip opens it, in the first place its `opens` lists.
- One status per plugin per pane; `null` removes it. With several plugins, each plugin's items sit in plugin order, separated by a dot.
- Wings rejects a call that breaks a limit, with the reason, instead of cutting the text like a badge does. Sending what the frame sent last does nothing, so you can call it on every poll.
- Wings drops a status when its pane closes or the plugin stops. When several plugins add to the same place, the extra pane badges fold into a "+N" button and title bar buttons past the first three into a menu.

### Pane rail

A rail is a small card floating over a pane's terminal, for a list you want in view while the agent works, like its todo list. A status is a strip of figures; a rail is a list of steps. Needs Wings 0.1.54 and `"rail"` in `contributes.ui`.

```js
await wings.setPaneRail(paneId, {
  title: "Todo",
  items: [
    { text: "Read the hook docs", status: "done" },
    { text: "Draw the rail", status: "in_progress" },
    { text: "Write the docs", status: "pending" },
  ],
  open: "board",
});
```

- `title` is up to 40 characters. `items` are up to 30 `{ text, status }` lines, `text` up to 200 characters and `status` one of `pending`, `in_progress` (highlighted) or `done` (dimmed and struck through).
- `open` is the id of one of the plugin's sidebars or views. Clicking the title opens it, as with a pane status.
- The card starts in the top right. The user drags its header to move it, and it snaps to the nearest corner; minimises it to a pill that shows the title and `done/total`; and closes it, which hides it for that pane until the plugin sends a different rail, so a new step brings it back. The corner and the minimised state are one setting for every pane, and the card stays inside the pane when it resizes.
- It is drawn by Wings over the terminal and never takes its keyboard focus. Moving it doesn't resize the terminal.
- One rail per plugin per pane; `null` removes it. Rails of several plugins stack in the same corner, in plugin order.
- Wings rejects a call that breaks a limit, with the reason. Sending what the frame sent last does nothing, so you can call it on every poll. Wings drops a rail when its pane closes or the plugin stops.

Plugin tools reach Claude through one MCP server, `wings`, which you add once from the Plugins sheet with Connect. It runs `wings --mcp` at user scope, so every Claude Code session gets the tools of the plugins that are on, and the list updates as you turn plugins on and off. The tools only work while Wings is open.

Wings adds tools of its own to that server, with or without plugins:

| Tool | What it does |
| --- | --- |
| `search_history`, `read_session` | Find and read past Claude Code and OpenCode sessions. |
| `list_sessions` | The open panes: id, project, folder, program and, for Claude, its state (working, blocked, done or idle), what it waits for, session id and name. |
| `read_pane(pane_id, lines?)` | The last up to 200 lines (32 KB) of a pane's output as plain text. Marked as data, since a pane can print anything. |
| `open_pane(cwd?, command?, run?)` | A new tab with a command typed in it. A `cwd` inside a project you added opens in that project; any other folder opens in the calling pane's project. With `run`, Wings also presses Enter, but only while "Let Claude run commands in new tabs" is on in Settings, Agents (off by default). The reply says whether it ran. The command can't hold a newline or other control character. Needs Wings 0.1.55, and 0.1.69 for a folder outside your projects. |
| `notify(title, body?)` | A desktop notification titled "Claude: …", up to 3 a minute across all sessions. |

Mind what these allow. Every Claude Code session you start on this computer can call them, so one session can read what another shows, which can include secrets and prompts. With the run setting off, `open_pane` only types a command, so nothing runs outside Claude Code's own permission checks for Bash. With it on, a command Claude passes with `run` runs in the new tab without those checks. Claude Code still asks before a call to an MCP tool, and shows the command, unless you allowed `open_pane` without asking. The socket is readable by your user only (`0600`) and there is no network access.

### Claude Code hooks

A plugin can add context when a Claude Code session starts and refuse a Bash command, so Claude uses a plugin's tools without being told to. MCP instructions alone are often not enough. List the event in `contributes.hooks` and answer it in `main`:

```js
wings.onHook("SessionStart", (input, { project }) => (project ? `Track work in ${project.name} with the issues__* tools.` : null));
wings.onHook("PreToolUse", (input) => (/\bgit commit\b/.test(input.tool_input?.command ?? "") ? { deny: "Log the work in an issue first." } : null));
```

Connect adds `wings hook` to `SessionStart`, `UserPromptSubmit` (from 0.1.77) and, with the matcher `Bash`, to `PreToolUse` in `~/.claude/settings.json`, after the hooks you already have, which stay as they are and in order. Disconnect takes it out again. Wings answers only for the plugins that are on, approved and list the event; with none it replies at once. `wings hook` waits up to 2 s for Wings, and with Wings closed, slow or with nothing to say it prints nothing, so a hook never blocks Claude. Contexts from several plugins are joined with a blank line (Claude Code keeps up to 10,000 characters), and for PreToolUse any deny refuses the command, with the reasons joined. Needs Wings 0.1.54.

`Journal` is not a Claude Code hook and Wings never installs it there. It is for a tool that records, at the end of each agent turn (Claude Code or OpenCode), which files changed and which commits the turn made, and runs a sink command with that as one JSON object on stdin. Wings registers itself as the journal's sink: on start (release builds) and after Connect it writes `{ "command": ["<path to wings>", "journal"] }` to `~/.config/journal/sink.json`, keeping any other keys, and repoints it if the app moves. A sink of your own in that file (any other command) is never touched, and a file that isn't a JSON object is left alone. Disconnecting both Claude Code and OpenCode in Plugins takes Wings' own entry out again (and the file, if nothing else is in it). To set it by hand, the command is `/Applications/Wings.app/Contents/MacOS/wings journal`. Wings adds `"hook_event_name": "Journal"` and passes it to the plugins that list `"Journal"`, as the pane the agent runs in: the sink's parent processes must reach that pane, which they do when the journal hook is a child of the agent. The input has `session_id`, `cwd`, `agent` (`"claude-code"` or `"opencode"`, which is also the `session.kind` the handler sees), `prompt` (the first 200 characters of the turn's prompt), `files` (`{ path, what }` each), `more` (changed files beyond those listed) and `commits` (short hashes, oldest first); the last two may be missing. A handler's answer is ignored, and `wings journal` prints nothing and exits 0 whether or not Wings is open. Wings waits 1.5 s per plugin, so answer quickly. Needs Wings 0.1.61.

The `wings-plugin-sdk` package has types for all of this.

## Pausing polls

Wings is in the background most of the day, and a timer that runs `git`, `gh` or `lsof` then spends the user's battery on a badge nobody sees. Stop timers while `wings.focused` is false and check once when it turns true. `pollWhileFocused(tick, ms)` from `wings-plugin-sdk/poll` does this: it runs `tick` every `ms` while Wings is in front, and once as soon as you come back, if `ms` has passed since the last run. Within `ms` it waits for the time that was left, so switching windows doesn't fetch on every switch. A third argument `{ immediate: true }` also runs `tick` once at the start, when Wings is in front. Keep event-driven work (`onPanes`, `onTranscript`, actions, tools) running. `onFocus` also tells you when to call `wings.notify`: send it only when `wings.focused` is false, since a user who is looking doesn't need a notification.

## Required tools

A plugin needs the GitHub CLI (`gh`), the GitLab CLI (`glab`) or the Azure CLI (`az`) when its `exec` or `post` entries name it. Its page in Plugins lists them under Needs, each Installed or not. A missing one has a button: on a Mac with Homebrew, Wings asks, then opens a tab in the project on screen and runs the vendor's own Homebrew command there (`brew install gh`, `brew install glab`, `brew update && brew install azure-cli`), so you see its output and can answer it. Without Homebrew, or off macOS, it links to the vendor's install page and to brew.sh. When the command is done, Wings checks again, says so, and restarts the plugins that run it.

`exec` rejects with `<program> is not installed or not on PATH` for a missing program. Don't match on that text or write the sign-in handling by hand: `cli()` from `wings-plugin-sdk/cli` runs `az`, `gh` or `glab`, tells a missing program and a signed-out one from a failed call, and `problemBadge` gives the badge with Install, Sign in and Try again. Its actions call `install()`, which asks Wings through `wings.installTool(program)`, `signIn()` and `retry()`. List each command in `permissions.exec`, `az login` and `gh auth login` included. `work-item/src/lib/az.ts` makes the handle and `work-item/src/main/badge.ts` draws the badge and answers its actions. The commands come from Wings' own table, so a plugin only names the program: one in its manifest and in that table, and Wings always asks first.

## Views

A view is one of your pages that can open somewhere other than the sidebar. This one opens in a modal from its title bar button, and can move to a window of its own:

```json
"contributes": {
  "views": [{ "id": "board", "title": "Board", "icon": "list", "page": "dist/board.html", "opens": ["modal", "window"], "width": 720, "height": 520 }]
}
```

Any of the plugin's pages can open it, and keep it current while it's open in any of those places:

```js
await wings.openView("board", { as: "window" });
wings.broadcast({ refreshed: Date.now() }); // the board page hears it with wings.onBroadcast
```

A window shows the same sandboxed page as the sidebar would. Its calls go to Wings' main window and are checked there against your manifest, exactly like the sidebar's, so a window can't do more than the plugin could.

With `tab` in `opens`, the view opens as a tab in the project on screen, next to its terminal tabs, and fills the pane area. Each project has at most one tab per view, and opening it again shows that tab. The page keeps running while another tab shows, and stops when you close the tab, with its close button, a middle click or ⌘W, or when the plugin calls `wings.closeView`. Wings saves the tab with the others and opens it again on the next start, unless the plugin is gone or the view no longer lists `tab`.

Wings 0.1.14 and older refuse a plugin with `tab` in `opens`, so set `"minWingsVersion": "0.1.15"` to use it.

A view with `"newTab": true` also shows in the + menu, a chevron next to the + button, with the plugin's icon, and in the command palette as "New tab: <title>". Under Settings, General, "New tab button opens" picks what the + button itself opens: a terminal (the default) or one of these pages. `"multiple": true` makes an open of the view, from + or `openView`, a new tab instead of showing the project's existing one, except that an open with `args` equal to those of a tab of it already in the project shows that tab. Give each tab its own `wings.setTabState` so they don't share. `newTab`, `multiple`, `args`, `project`, `placement` and tab state need Wings 0.1.22, so set `"minWingsVersion": "0.1.22"` when you use them.

## Project sections

A plugin can list things under each project in the sidebar, like its open tasks or pull requests. Declare the section in the manifest, then fill it per project from the main script:

```json
"contributes": { "projectSections": [{ "id": "open", "title": "Open tasks", "icon": "todo" }] }
```

```ts
wings.onProjects((projects) => {
  for (const p of projects) {
    wings.setProjectSection(p.id, "open", [
      { id: "t1", label: "Fix the login", detail: "due Friday", badge: { text: "High", tone: "warning" }, open: { view: "board", args: { task: "t1" } } },
    ]);
  }
});
```

A section shows under its project after the project's row, with the plugin's icon, its title and the row count, only while it has rows. Clicking its header folds it shut, and Wings remembers that per project and section. Clicking a row opens `open.view` as a tab in that row's project, switching to the project, with `args` for `viewArgs()` and following the same rules as `openView`: a tab already open with other args takes the new ones and reloads, and a view with `multiple` opens a new tab unless a tab of it in that project already has equal `args`. `open.view` must be one of the plugin's views with `tab` in `opens`, or the call rejects. Wings cuts text for you (label and detail 120 characters, badge 24, ids 64), keeps the first 50 rows, leaves out a row with no `id` or `label` or an `id` already used, and rejects `args` over 16 KB. A badge `tone` is one of the badge tones. Sections are kept in memory, not saved: they go when the plugin is turned off or removed, so fill them again on start, and a call that repeats the last one for that project and section does nothing (the frame skips it from Wings 0.1.50, and Wings ignores identical rows before that), so call `setProjectSection` whenever you have looked and keep no copy of what you sent. They need Wings 0.1.23, so set `"minWingsVersion": "0.1.23"`.

## Blocks

A block is a plugin page the user places on a project's Project page, between its sections, like a sprint board or the open pull requests. Each block runs in its own sandboxed frame, with the same `wings` object as a tab page.

```json
"contributes": {
  "views": [{ "id": "board", "title": "Board", "page": "dist/board.html", "opens": ["tab"] }],
  "blocks": [{ "id": "board", "title": "Sprint board", "page": "dist/block.html", "minHeight": 200 }]
}
```

A page built from `src/pages` mounts in one place, so give a block its own page, `src/pages/block.tsx` with `export const page = { place: "block" }`, and share the components it draws from `src/ui/`, like `tasks/src/pages/block.tsx` does. A block's `page` can be the HTML of a view when you write it yourself, and then `wings.placement()` says `"block"` where a compact version is wanted. Declaring a block adds nothing to the sidebar or the + menu: it shows only in the Project page's "+ Add" menu, for plugins that are on and approved.

In a block:

- `wings.project()` is the Project page's project, and never changes.
- `wings.blockState()` and `wings.setBlockState(state)` keep a small JSON value (up to 16 KB) per block, like the filter it shows. Two blocks of the same page have their own state. Wings saves it with the Project page.
- `wings.setHeight(px)` sizes the block to its content, within `minHeight` and 1200. A page of your own that measures itself can call it from a `ResizeObserver` (`showBlock` does this for a `wings-plugin-ui` page); Wings applies the height without reloading the page, and the call returns at once.
- Outside a block, `blockState()` gives `null`, and `setBlockState` and `setHeight` fail with a message saying so.

Blocks cost a frame each, so Wings loads one only when it scrolls into view, unloads it after 5 minutes out of view (a page that saved its state with `setBlockState` comes back as it was) and shows at most 8 at once on a Project page. While a block is out of view, or its project's tab is hidden, its `wings.focused` is false from the first line of the page (not only after the first `onFocus`) and `onFocus` reports each change, so `pollWhileFocused` pauses its polling and checks once when the block is back. Gate polling on `wings.focused`, not `document.visibilityState`: a frame scrolled out of view or in a hidden tab still reports `"visible"`, because the browser only reports the state of the whole Wings window. The build mounts a block page with `showBlock` from `wings-plugin-ui`, which also keeps the block as tall as the page with `wings.setHeight`. Don't poll from a block's page faster than you would from a sidebar.

Blocks, `"block"` and the three calls need Wings 0.1.24, so set `"minWingsVersion": "0.1.24"`. Older Wings ignore `contributes.blocks`.

## Popovers

A sidebar can have a quick look: a second page shown under its title bar button after you point at it for a moment, or when the button gets the keyboard. Clicking the button still opens the sidebar, and the popover doesn't show while that sidebar is open. It closes shortly after the pointer leaves both, or with Escape on the button.

```json
"contributes": {
  "sidebars": [{ "id": "timer", "title": "Timer", "icon": "clock", "page": "dist/sidebar.html", "popover": { "page": "dist/popover.html", "width": 340, "height": 420 } }]
}
```

The page gets the same `wings` object, storage and broadcasts as the sidebar. It loads the first time it shows and keeps running while closed, so it opens at once after that and its timers keep going. Send the user to the full view with `wings.openView("timer")`.

## Settings page

A plugin can have a page in Wings Settings instead of a settings form in its sidebar:

```json
"contributes": {
  "settings": { "page": "dist/settings.html" }
}
```

Any of the plugin's pages, or its main script, can send the user there, and hear what changed with `wings.onBroadcast`:

```js
await wings.openSettings();
// the settings page, after saving. A value like this is a `setting()`, see Talking to main:
await wings.storage.set("interval", 60);
wings.broadcast({ settingsChanged: true });
```

Wings 0.1.14 and older ignore `contributes.settings` and have no `wings.openSettings`. Set `"minWingsVersion": "0.1.15"` when the plugin needs them, or check `typeof wings.openSettings === "function"` first.

## Status line

Claude Code sends its status line command a JSON status after each reply: the 5-hour and weekly usage limits, the context window and how much of it is used, and the prompt cache. It's the documented way to get the usage limits, with no sign-in token involved. Wings keeps the newest status for plugins with `permissions.statusline`, and only the fields listed under `wings.statusline()`.

When Wings starts, it adds itself to Claude Code: its MCP server, `wings statusline --pass` in front of the `statusLine` command in `~/.claude/settings.json`, and, from 0.1.54, `wings hook` after your `SessionStart` and `PreToolUse` hooks (see [Claude Code hooks](#claude-code-hooks)). Your own status line stays in that command and runs after Wings on the same input, so it still shows, your hooks stay as they were, and the rest of the file is left as it was. Disconnect in Plugins puts your status line back, takes the hooks out and stops Wings adding itself again. Dev builds don't connect on their own, since they'd point Claude Code at their own binary.

## Storage

`wings.storage` needs no permission: it's the plugin's own data, and no other plugin can read it. Wings keeps it in a folder per plugin, `<app data>/plugin-storage/<id>.<scope>/` (on macOS `~/Library/Application Support/dev.wings.app/plugin-storage/`), with one file per key, so a `set` writes only that key. Each change writes a new file and renames it into place, so a crash never leaves half a file. Before Wings 0.1.50 a plugin's storage was one file, `<id>.<scope>.json`; the first use after an update splits it into the folder and keeps the old file as `<id>.<scope>.json.migrated`. The main script and the sidebar pages share it, and writes they make at the same time all land.

It follows the same rule as secrets. An update from the same GitHub repo keeps it. Removing the plugin deletes it, and so does installing a plugin with the same id from a file or another repo, which gets a new `<scope>`, so a different package can't read what the old one stored. Keep tokens in `wings.secrets`, not here.
