# wings-plugin-sdk

The `wings` API types, the page frame, helpers and the build command for [Wings](https://github.com/josippapez/Wings)
plugins. The components for plugin pages are in [wings-plugin-ui](../plugin-ui/README.md). A starter repo that
already has all of this set up is [templates/wings-plugin](../../templates/wings-plugin).

Wings itself gives each plugin the `wings` object at runtime. This package doesn't contain it: it types it, and
bundles your TypeScript and React into the plain scripts Wings loads.

## Set up

```sh
npm install --save-dev wings-plugin-sdk typescript
npm install --save-dev wings-plugin-ui react react-dom @types/react   # only for React pages
```

`tsconfig.json`:

```json
{
  "compilerOptions": {
    "target": "ES2022",
    "lib": ["ES2023", "DOM", "DOM.Iterable"],
    "module": "ESNext",
    "moduleResolution": "bundler",
    "jsx": "react-jsx",
    "strict": true,
    "noEmit": true,
    "skipLibCheck": true,
    "types": ["wings-plugin-sdk"]
  },
  "include": ["src"]
}
```

`wings-plugin.json` needs five fields: `id` (lowercase letters, digits and dashes), `name`, `version`, `api` (`1`)
and `main`. This one is enough for the badge below:

```json
{
  "id": "hello-wings",
  "name": "Hello Wings",
  "version": "0.1.0",
  "api": 1,
  "main": "dist/main.js",
  "permissions": { "exec": ["git branch --show-current"] },
  "contributes": { "ui": ["badges"] }
}
```

`src/main.ts` runs hidden, and sets a badge on each pane that is in a git repo:

```ts
// src/main.ts
import { gitOk } from "wings-plugin-sdk/git";
import { pollWhileFocused } from "wings-plugin-sdk/poll";

let panes: WingsPane[] = [];

async function look() {
  for (const { paneId, cwd } of panes) {
    const branch = await gitOk(cwd, ["branch", "--show-current"]);
    // Wings skips a badge a pane already shows, so every look can set every badge.
    await wings.setBadge(paneId, branch ? { label: branch, tone: "info" } : null).catch(console.warn);
  }
}

wings.onPanes((list) => {
  panes = list;
  void look();
});
pollWhileFocused(() => void look(), 15_000);
```

Then `npx wings-plugin pack` writes `hello-wings.wings-plugin`. Drop it on the Wings window, and Wings asks you to
approve the plugin's access before it runs.

## Build

Put the code in `src/`, next to `wings-plugin.json`:

```
<id>/
  wings-plugin.json
  icon.svg
  README.md        Wings shows it on the plugin's Settings page
  src/
    main.ts        the main script
    pages/         one file per page: sidebar.tsx, board.tsx, ...
    main/          the modules a large main script splits into
    ui/            React components the pages share
    lib/           logic with no UI, messages and settings, and their tests
    tools.ts       the tools Claude gets, if the plugin has any
  preview/         fixtures for `pnpm shot` in the Wings repo
  dist/            what the build writes
```

The entries are `src/main.ts` and each file in `src/pages/`. They build into `dist/` under their own names:
`src/main.ts` becomes `dist/main.js` and `src/pages/sidebar.tsx` becomes `dist/sidebar.js`. The manifest points
there, with `"main": "dist/main.js"` and `"page": "dist/sidebar.html"`. A page that uses `wings-plugin-ui` also gets
its `dist/<name>.html` written for it, unless you wrote your own there, and a plugin with no `src/main.ts` gets an
empty `dist/main.js`, since the manifest needs a `main`. Nothing else in `src/` is an entry. A page's CSS goes next
to what imports it.

```sh
npx wings-plugin pack            # builds and packs the plugin as <id>.wings-plugin, see Pack
npx wings-plugin build           # builds into dist/, for a repo that commits its built files
npx wings-plugin build --check   # writes nothing, and fails if a file in dist/ differs from what src/ builds
npx wings-plugin dev             # builds on each save and has the running Wings restart the plugin, see Dev
```

A plugin in its own repo uses `pack`, and its repo holds only source. `build` is for a folder that Wings loads
as it is, like the plugins in the Wings repo, which commit `dist/`. `--check` builds in memory and compares the
content of every output with the file on disk (a file's age doesn't count), and prints a line for each one that
differs or is missing. The `mcpTools` in the manifest count as an output too. Nothing runs it for you: add it to your CI or a git hook, so a source change can't ship
with an old `dist/`.

### Manifest checks

The build compares `wings-plugin.json` with the code it built, and fails with a line for each place they disagree.
A typo shows up when you build, not when Wings loads the plugin. It catches:

- a `main` that isn't `dist/main.js`, a page that isn't a file the build writes or the folder has, and an `icon`
  that isn't a file in the folder;
- a `wings.onTool("name")` with no `mcpTools` entry, or the reverse;
- a call of `wings.setBadge`, `setPaneStatus`, `openDiff` or `updateDiff` without `badges`, `status` or `diff` in
  `contributes.ui`;
- an id that no sidebar or view has, in `wings.openView("id")`, `closeView("id")`, `setSidebarLabel("id", ...)` or
  the `open: "id"` of a pane status;
- an id that no view with `tab` in its `opens` has, in the `open: { view: "id" }` of a project section row or a
  notification button.

The bundles are searched as text, so a name the code computes, like `wings.openView(id)`, is skipped.

### Tools sync

A plugin with a `src/tools.ts` doesn't write `contributes.mcpTools` by hand. `build` writes it into
`wings-plugin.json` from the tools the file exports, and nothing else in the manifest changes. `pack` writes it into
the package only. `mcpInstructions` stays yours. The build loads `src/tools.ts` in Node with a `wings` that does
nothing, so the file may register listeners when it loads but can't wait for an answer from Wings.

## Pages

A sidebar, popover, view, block, tab or settings page is a file in `src/pages/`. A React page default-exports its
component and says where it shows with `export const page = { place: "sidebar" }`. `place` is `sidebar`,
`popover`, `view`, `block`, `tab` or `settings`. The build writes the mount, which calls the matching `show*`
function of [wings-plugin-ui](../plugin-ui/README.md), and the page's HTML, so a test or a fixture can import the
page without it mounting.

- `fill: true` lays a page out edge to edge, with no padding. It does nothing for a block.
- A block mounts with `showBlock`, which keeps the block as tall as the page with `wings.setHeight`.
- A page in a tab or a view goes inside `Page` from `wings-plugin-ui`. A sidebar page doesn't.
- A page that builds its DOM itself, without React, is a `.ts` file built as it is, and gets the same frame from
  `mountShell` (see Shell).

Every `wings.on*` call returns a function that removes the listener, so `useBroadcast` from
`wings-plugin-sdk/react`, or your own `useEffect`, cleans up when a component goes. The API reference is
[plugins/README.md](https://github.com/josippapez/Wings/blob/main/plugins/README.md) in the Wings repo.

## Modules

| Module | For |
|---|---|
| `wings-plugin-sdk/messages` | `channel()`: questions, events and shared queries between main and the pages |
| `wings-plugin-sdk/react` | `useQuery`, `useSetting` and `useBroadcast`, for a page |
| `wings-plugin-sdk/settings` | `setting()`: a stored value that every frame reads and hears change |
| `wings-plugin-sdk/sessions` | `followSessions()`: the Claude sessions in panes |
| `wings-plugin-sdk/poll` | `pollWhileFocused()`: a timer that runs only while Wings is in front |
| `wings-plugin-sdk/async` | `cached`, `serial`, `singleFlight`, `mapLimit`, `latestOnly`, `errorMessage`, `openDiffWhile` |
| `wings-plugin-sdk/format` | `ago`, `plural`, `basename`, `truncate`, `tokens`, `clock` |
| `wings-plugin-sdk/cli` | `cli()`: `az`, `gh` and `glab`, with install and sign-in handled |
| `wings-plugin-sdk/git` | `git`, `gitOk`, `repoRoot`, `defaultBranch`, `onGitChange`, `parseRemote` |
| `wings-plugin-sdk/tools` | `tool`, `s` and `registerTools`: the tools Claude calls |
| `wings-plugin-sdk/shell` | `mountShell()`: the page frame, for a page without React |
| `wings-plugin-sdk/build` | `buildPlugin` and `pack`, for a script |

Each module's `.d.ts` says what every call does. The examples below are short, and each starts with the file it
goes in.

### Messages

Main and each page are separate frames. They talk through a `channel`, declared once in a module that all of them
import, with the types of what it carries. You don't write request ids, timeouts or a hello message. A channel has
three kinds of message:

- a question: a frame `ask`s, another `answer`s. For commands, like checking out a branch.
- an event: a frame `tell`s, the others `on` it. For "something changed" with no data.
- a query: main `serve`s a value by key, and every page that shows the key gets the same copy and its updates. For
  data. Main loads each key once, however many pages show it, and reloads it only while one does.

```ts
// src/lib/messages.ts
import { channel } from "wings-plugin-sdk/messages";

export const ch = channel<
  // Questions: a page asks, main answers.
  { checkout: { request: { cwd: string; branch: string }; response: void } },
  // Events: one frame tells, the others listen. `void` is no payload.
  { "head-moved": void },
  // Queries: main serves a value for each key. A `void` key holds one value.
  { branches: { key: { cwd: string }; value: string[] } }
>();
```

Main serves and answers:

```ts
// src/main/answers.ts
import { git, gitOk } from "wings-plugin-sdk/git";
import { ch } from "../lib/messages";

// `load` runs only here. A page that shows a key gets the kept value, and main reloads it every 30 s while one does.
ch.serve(
  "branches",
  async ({ cwd }) => {
    const out = await gitOk(cwd, ["for-each-ref", "--format=%(refname:short)", "refs/heads"]);
    return out ? out.split("\n") : [];
  },
  { refetchMs: 30_000, staleMs: 20_000 },
);

ch.answer("checkout", async ({ cwd, branch }) => {
  await git(cwd, ["switch", branch]);
  ch.refresh("branches"); // every page that shows a branches key gets the new list
  await ch.tell("head-moved");
});
```

A page reads the query with `useQuery`, and asks for the command:

```tsx
// src/pages/branches.tsx
import { useQuery } from "wings-plugin-sdk/react";
import { Empty, Heading, List, ListButton, ListItem, Loading, ProjectGate } from "wings-plugin-ui";
import { ch } from "../lib/messages";

export const page = { place: "sidebar" };

// The page is about the project on screen, so wait for Wings to say which.
export default function Page() {
  return <ProjectGate>{(project) => <Branches cwd={project.path} />}</ProjectGate>;
}

function Branches({ cwd }: { cwd: string }) {
  const { data, error, refresh } = useQuery(ch, "branches", { cwd });
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
      <ListButton onClick={refresh}>Refresh</ListButton>
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
| `ch.get(name, key, { maxAgeMs? })` | Main: the kept value if it is fresh enough, else one load, shared with any that is running |
| `ch.peek(name, key?)` | Main: `{ data, error, at }` for a key, or `undefined`. Never loads |
| `ch.watching(name)` | Main: the keys some page shows now |
| `ch.put(name, key, value)` | Any frame: sets a key's value for every frame at once. For a page that fetched something itself, so the other pages show it too |
| `ch.refresh(name, key?)` | Any frame: reloads a key now, or every key a page shows when you leave the key out |
| `ch.subscribe(name, key, listener)` | Any frame: calls `listener({ data, error, at, loading })` as the value changes. For a page without React |
| `useQuery(ch, name, key, { skip?, keepData?, timeoutMs? })` | In a page: `{ data, error, loading, at, refresh }`. `data` is `null` before the first value. `keepData` keeps the last key's value while a new key loads. `loading` is true only before the first value and while this hook's own `refresh()` waits, so a refetch in main never blinks. Without a value after `timeoutMs` (30 s), `error` is "Wings didn't answer. Try again." and a later value clears it |

A message is JSON that goes over `wings.broadcast`, and it reaches every other frame, never the one that sent it.
One that is too big for a broadcast (64,000 characters) is split and joined again, so a value of any size arrives
whole, but every frame gets every broadcast: keep a query to what the pages show. A page that loads before main, or
while main reloads, gets its value once main serves. A failed load keeps the last value and reaches every page as
`error`, until the next good load.

### Settings

A value that a settings page saves and other frames read is a `setting`, declared once in a module they import:

```ts
// src/lib/settings.ts
import { setting } from "wings-plugin-sdk/settings";

export const interval = setting("interval", {
  default: 60,
  // What storage holds is untrusted: return the value as a number, or undefined for the default.
  check: (value) => (typeof value === "number" && value >= 10 ? value : undefined),
});
```

```tsx
// src/pages/prefs.tsx
import { useSetting } from "wings-plugin-sdk/react";
import { interval } from "../lib/settings";

export const page = { place: "settings" };

export default function Prefs() {
  const [seconds, setSeconds] = useSetting(interval); // null until it is read
  return <button onClick={() => void setSeconds((seconds ?? 60) + 10)}>Poll every {seconds ?? "…"} s</button>;
}
```

Main reads it with `await interval.get()` and hears changes with `interval.onChange(listener)`. Declare each
setting once: two objects for one key in one frame don't hear each other.

### Sessions

`followSessions` tracks the Claude sessions that run in panes, and gives you each run's older entries and then the
live ones, with the overlap passed on once. It needs `permissions.transcript` for the entry types you read.

```ts
// src/main/sessions.ts
import { followSessions } from "wings-plugin-sdk/sessions";

interface Seen {
  id: string;
  replies: number;
}

followSessions<Seen>({
  create: (id) => ({ id, replies: 0 }),
  // A new `claude` process: start counting again.
  startRun: (seen) => {
    seen.replies = 0;
  },
  backlog: (_seen, run) => wings.transcript(run.sessionId, ["assistant"]),
  entry: (seen, entry) => {
    if (entry.type === "assistant") seen.replies += 1;
  },
  changed: (seen) => console.log(`${seen.id}: ${seen.replies} replies`),
});
```

### Poll

A timer that runs `git` or a network call costs battery while Wings is in the background. `pollWhileFocused` runs
`tick` every `ms` only while a Wings window is in front. When you come back it ticks at once if `ms` has passed since
the last tick, and otherwise waits out the time that was left, so switching windows doesn't fetch on every switch.
It doesn't tick for the state Wings starts in unless you pass `{ immediate: true }`.

```ts
// src/main/poll.ts
import { pollWhileFocused } from "wings-plugin-sdk/poll";

const stop = pollWhileFocused(() => console.log("tick"), 60_000, { immediate: true });
// stop() ends the timer and the focus listener.
export { stop };
```

### Async

Small helpers for the loads a plugin repeats:

```ts
// src/lib/branches.ts
import { cached, latestOnly, mapLimit, singleFlight } from "wings-plugin-sdk/async";
import { gitOk } from "wings-plugin-sdk/git";

/** One `git` run per folder, kept for a minute, however many ask. */
export const head = cached((cwd: string) => gitOk(cwd, ["rev-parse", "--short", "HEAD"]), { ttlMs: 60_000 });

/** At most 4 `git` runs at once. */
export const heads = (cwds: string[]) => mapLimit(cwds, 4, (cwd) => head.get(cwd));

/** A call while it runs makes it run once more when it is done, not twice at once. */
export const reload = singleFlight(async () => {
  head.clear();
});

/** Drops the answer of a load that a later one has replaced. */
const start = latestOnly();
export async function load(cwd: string) {
  const stale = start();
  const value = await head.get(cwd, { fresh: true });
  return stale() ? null : value;
}
```

`serial()` runs jobs one at a time in order, `errorMessage(error)` gives the text of anything thrown, and
`openDiffWhile(diff, load)` opens the diff viewer loading and fills it when `load` resolves.

### Format

```ts
// src/lib/labels.ts
import { ago, plural, tokens, truncate } from "wings-plugin-sdk/format";

export const label = (files: number, when: number) => `${plural(files, "file")}, ${ago(when)}`; // "2 files, 5 min ago"
export const short = (text: string) => truncate(text, 40);
export const size = (n: number) => tokens(n); // "116k"
```

`basename(path)` and `clock(ms)` are there too.

### CLI

`cli` wraps `az`, `gh` or `glab`. It tells "not installed" and "signed out" from a failed call, remembers the
problem, and builds the badge that offers Install, Sign in or Try again. List each command in `permissions.exec`,
`az login` or `gh auth login` included.

```ts
// src/lib/gh.ts
import { cli } from "wings-plugin-sdk/cli";

export const gh = cli("gh", { signedOut: /gh auth login|not logged in/i, signIn: ["auth", "login", "--web"] });

/** The numbers of the open PRs of the repo in `cwd`. `empty` is what a program that prints nothing means. */
export const openPulls = (cwd: string) => gh.json<{ number: number }[]>(["pr", "list", "--json", "number"], { cwd, empty: [] });
```

```ts
// src/main/gh.ts
import { lastLine } from "wings-plugin-sdk/cli";
import { gh, openPulls } from "../lib/gh";

wings.onPanes((panes) => {
  for (const { paneId, cwd } of panes) {
    openPulls(cwd)
      .then((pulls) => wings.setBadge(paneId, { label: `${pulls.length} PRs`, tone: "info" }))
      .catch((error) => {
        // The badge for a missing or signed-out `gh`, with its buttons, else the failure's last line.
        const problem = gh.problemBadge(undefined, { title: "GitHub", signedOut: "Sign in to see PRs", missing: "Install gh to see PRs" });
        return wings.setBadge(paneId, problem ?? { label: lastLine(String(error.message)), tone: "danger" });
      });
  }
});

wings.onAction(({ actionId }) => {
  if (actionId === "install") void gh.install();
  if (actionId === "signin") void gh.signIn();
  if (actionId === "retry") void gh.retry();
});
gh.onChange(() => console.log("a problem changed: draw the badges again"));
```

`CliError.kind` is `missing`, `signed-out` or `failed`, and its `message` holds all of stderr. `cli.exec` runs a
command and resolves `{ code, stdout, stderr }` whatever the exit code, for one whose output matters when it fails.

### Git

```ts
// src/lib/repo.ts
import { defaultBranch, git, onGitChange, parseRemote, repoRoot } from "wings-plugin-sdk/git";

export async function summary(cwd: string) {
  const root = await repoRoot(cwd); // null outside a repo
  if (!root) return null;
  const base = await defaultBranch(root); // "origin/main", or null
  const remote = parseRemote(await git(root, ["remote", "get-url", "origin"])); // { kind: "github", host, path }
  return { root, base, remote };
}

// A commit, checkout, pull, push or reset in a project's repo, from anywhere. It needs a `git` entry in `permissions.exec`.
export const stop = onGitChange((projectIds) => console.log("moved:", projectIds));
```

`git` throws git's first line of stderr, `gitOk` gives `null` instead of throwing, and `gitRun` gives the exit code
and both streams. `repoRoot.forget(cwd)` and `defaultBranch.forget(root)` clear what they found, for a Refresh
button. `onGitChange` needs Wings 0.1.50, and edits to files in the working tree don't count, so a plugin that shows
those keeps a slow poll.

### Tools

Claude calls a plugin's tools through `wings.onTool`. Define them in `src/tools.ts` with `s`, which gives both the
JSON Schema and the type of the input, and the build writes the manifest's `mcpTools` from it.

```ts
// src/tools.ts
import { s, tool } from "wings-plugin-sdk/tools";

export const tools = {
  branch_of: tool({
    description: "The git branch checked out in a folder.",
    input: s.object({ path: s.string().describe("An absolute folder path"), short: s.boolean().optional() }),
    // `context` says who called: { paneId, project, session }, each null when there is none. Wings 0.1.50.
    run: async ({ path, short }, context) => `${path}${short ? "" : " in " + (context.project?.name ?? "no project")}`,
  }),
};
```

```ts
// src/main/tools.ts
import { registerTools } from "wings-plugin-sdk/tools";
import { tools } from "../tools";

registerTools(tools); // checks each call against its schema, then runs the tool
```

### Shell

A page without React is a `.ts` file that builds its own DOM inside the frame `mountShell` returns:

```ts
// src/pages/hello.ts
import { mountShell } from "wings-plugin-sdk/shell";

mountShell("sidebar").textContent = "Hello from a sidebar";
```

## Pack

A plugin in its own repo doesn't need to commit what it builds. `wings-plugin pack` builds it into a temporary
folder and zips that as `<id>.wings-plugin`, the file you install in Wings' Plugins manager or attach to a GitHub
release:

```sh
npx wings-plugin pack   # writes hello-wings.wings-plugin and prints its path
```

The package holds `wings-plugin.json` at its top, the build in `dist/` (every entry, the generated pages and CSS),
and the files your plugin ships, like its icon, its own `.html`, `.css` and images, and its `README.md`, which
Wings shows in Settings. It leaves out `src/`, `preview/`, `node_modules`, `package.json` and lock files,
`tsconfig*.json`, `HANDOVER*`, dotfiles and other `.wings-plugin` files. Your folder gets no built files. From a
script, `pack(dir, { out })` in `wings-plugin-sdk/build` does the same and resolves the package's path.

## Dev

`npx wings-plugin dev` builds the plugin when a file changes, then tells the running Wings to read the folder again,
which restarts the plugin's frames. The first time it links the folder into Wings, and Wings asks you to approve the
plugin's access, like for any install. A failed build prints its error and keeps watching, and if Wings isn't open it
tries again every 2 seconds. Stop it with Ctrl+C: the plugin stays linked. `wings plugin link <folder>` (the command
that Wings ships) does the link once, without watching.
