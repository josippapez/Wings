# Wings

Wings is a terminal workspace for coding agents such as Claude Code and opencode. Your projects sit in a sidebar with each agent and what it is doing, and every project has tabs and split panes of real terminals. It is a Tauri 2 and React 19 app, MIT licensed.

## Features

- Projects in a sidebar, with the agents running in them and whether each one is working, waiting or needs you.
- Tabs and split panes per project. Rename, reorder and move them by dragging or from the right-click menus.
- Past sessions: browse history and resume a session.
- Command palette on ⌘K (Ctrl+Shift+K elsewhere) for commands, projects, tabs, agents and sessions. It lists every shortcut. Tab, Pane and View menus show them too.
- A global shortcut, ⌃⌥J by default (change it in Settings, Keyboard), that brings Wings forward and goes to the agent that needs you. ⌘J does the same inside the app.
- Worktrees: start an agent in a new git worktree from a project's right-click menu, and list or clean up worktrees with the Worktrees plugin.
- Diff review: comment on lines of a diff and send the comments to the agent in that repo.
- A notification list for when agents needed you or finished.
- Plugins that add badges, sidebars and actions to panes.

## Install

Download the latest build from the [releases page](https://github.com/josippapez/Wings/releases) (macOS). Wings checks for new releases on start and every six hours, and offers "Install and restart" in the app.

The app is signed with a self-signed certificate and is not notarized, so the first time you open it macOS shows a Gatekeeper warning. Right-click Wings in Applications and choose Open. See [docs/releasing.md](docs/releasing.md).

## Build from source

You need Rust (`rustup`), Node 24 and pnpm.

```sh
cd apps/desktop
pnpm install
pnpm tauri dev      # run the app
pnpm build          # type-check and build the UI
pnpm test           # UI tests
cargo test          # Rust tests, run in src-tauri/
```

`pnpm build:mac` builds the macOS app. Signing and releasing are in [docs/signing.md](docs/signing.md) and [docs/releasing.md](docs/releasing.md). More about the desktop app is in [apps/desktop/README.md](apps/desktop/README.md).

## Plugins

A plugin is a folder with a `wings-plugin.json` and some TypeScript, run in a sandbox and limited to the permissions it declares. See [plugins/README.md](plugins/README.md) for installing and the plugins in this repo (Changes, Dev servers, HUD, PR tracker, Session detail, Work item, Worktrees). To write one, use [`wings-plugin-sdk`](https://www.npmjs.com/package/wings-plugin-sdk) and [`wings-plugin-ui`](https://www.npmjs.com/package/wings-plugin-ui), whose sources are in `packages/`.

## License

MIT. See [LICENSE](LICENSE).
