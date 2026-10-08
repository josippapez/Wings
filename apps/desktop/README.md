# Wings desktop (POC)

Tauri 2 + React 19 app. Rust owns the terminals (PTYs) and Claude Code detection; the UI is shadcn/ui on Base UI with xterm.js panes.

## Run

Needs Rust (`rustup`), Node 24 and pnpm.

```bash
cd apps/desktop
pnpm install
pnpm tauri dev
```

`cargo test` in `src-tauri/` runs the Rust tests.

## How Claude sessions show up

Run `claude` in any Wings pane. Wings sees it as the pane's foreground process (a process-tree scan on Windows) and reads Claude Code's own `~/.claude/sessions/<pid>.json` for the session id, name and busy/waiting/idle status. It changes no Claude Code settings.

## Shortcuts

macOS uses ⌘; Windows and Linux use Ctrl+Shift (plain Ctrl stays with the shell).

| macOS | Action |
|---|---|
| ⌘T | New tab |
| ⌘1–9 | Go to tab |
| ⌘D / ⇧⌘D | Split right / down |
| ⌥⌘ + arrow | Focus the pane in that direction |
| ⌘W / ⇧⌘W | Close pane / close window |
| ⌘+ / ⌘− / ⌘0 | Font size up / down / reset (works on any keyboard layout) |
| ⌘B | Toggle sidebar |
| ⌘-click | Open a link |

Tabs, splits and font size are saved and come back on the next launch; panes that were running Claude run `claude --resume <session>` again.

## Benchmark

`WINGS_BENCH=1 WINGS_BENCH_OUT=bench.json ./src-tauri/target/release/wings` (after `pnpm tauri build --no-bundle`) runs the terminal rendering benchmark in `src/bench.ts` and writes the results.

Dev builds print `[detect]`, `[focus]` and `[pane]` lines to the `pnpm tauri dev` output.
