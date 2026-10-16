---
name: wings-cli-sign-in
description: Adds a one-click Sign in button to a Wings plugin badge for a CLI the plugin needs to be logged in (gh, glab, az), so the user never opens a terminal. Use when a plugin's CLI calls fail because it isn't signed in, or when adding a new provider CLI to a plugin.
---

# One-click CLI sign-in for Wings plugins

Don't write the sign-in handling by hand. `cli()` from `wings-plugin-sdk/cli` runs `az`, `gh` or `glab`, tells "not installed" and "signed out" from a failed call, remembers the problem, builds the badge with Install, Sign in and Try again, and runs the login. The plugin gives it two things: how to see that the CLI is signed out, and how to sign in. Its `.d.ts` (`packages/plugin-sdk/cli.d.ts`) says what every call does. For the plugin API itself, see the `wings-plugin` skill and `plugins/README.md`.

Examples in this repo:

- `plugins/work-item/src/lib/az.ts` makes the handle, and `plugins/work-item/src/main/badge.ts` (`badgeOf`, `showBadges`) draws the badge and answers its actions.
- `plugins/pr-tracker/src/lib/forges/` has `gh`, `glab` and `az` handles for three hosts: `github.ts` for a login that prints a one-time code, `gitlab.ts` for a signed-out signal that needs a second command.

## Steps

1. **Find the logged-out signal.** Reproduce it without signing the user out by pointing the CLI at an empty config folder in the scratchpad. For example, `GH_CONFIG_DIR=<empty dir> gh pr list` exits 4 and prints `To get started with GitHub CLI, please run:  gh auth login`. `az` prints a message that mentions `login`. `glab api` prints `401 Unauthorized`, but a private project answers `404` when you're signed out, so check `glab auth status --hostname <host>` on a 404. Match on stderr, because exit codes alone are shared with other failures.
2. **Make the handle.** `cli(program, { signedOut, signIn })`. `signedOut` is a RegExp tested on stderr, or a function of `{ code, stdout, stderr }` and the key that answers true or false, for a signal a regexp can't see (glab's 404). `signIn` is the login's arguments, or a function of the key that gives them. Add `sticky: true` for a host that answers some calls without sign-in, like GitLab's public projects: its problem then only clears on Try again or after signing in, or the badge would flip back and forth.
3. **Call the CLI through it.** `run(args, { key, cwd })` resolves stdout, `json(args, { key, empty })` parses it (`empty` is what a program that prints nothing means; without it nothing printed is an error), and `exec` resolves `{ code, stdout, stderr }` whatever the exit code. They throw a `CliError` whose `kind` is `missing`, `signed-out` or `failed`, and whose `message` is all of stderr (`lastLine` cuts it to one line for a badge). `key` is what a problem is recorded under: the Azure organization, or a provider and host. A working call clears the problem. Pass `quiet: true` for a background read that must not turn badges into sign-in prompts.
4. **Show the button.** `problemBadge(key, { title, signedOut, missing, tone })` gives the badge for a pane while the key has a problem, or `null`. It replaces the pane's badge, even a PR badge, because nothing works until sign-in. Its actions have the ids `install`, `signin` (primary) and `retry`. Give each provider its own `tone` so the user can tell them apart: GitHub is `merged` (purple), GitLab is `info` (blue) and Azure DevOps is `warning` (amber, the default). Give a new provider a different one. In `wings.onAction`, answer the ids with `install()`, `signIn(key)` and `retry()`, and return the promise so the button spins.
5. **Redraw and reload.** `onChange(listener)` is called when a problem is recorded, changes or goes, or a sign-in starts or ends: draw the badges again there. `onRetry(listener)` is called by Try again and after a sign-in: clear the caches of what failed for lack of the CLI and load it again, and return the promise.
6. **Read the CLI's login code before calling it.** Wings runs programs with no TTY and stdin closed, and many CLIs change behaviour without a TTY:
   - `gh auth login --web --clipboard --hostname github.com` doesn't open the browser without a TTY. It prints `One-time code (XXXX-XXXX) copied to clipboard` (or `First copy your one-time code: XXXX-XXXX`) and `Open this URL to continue in your web browser: <url>`, then waits. Source: `internal/authflow/flow.go` in cli/cli v2.102.0. `signIn(key, { onOutput })` gets each line it prints: put the code in the badge label and `wings.openUrl` the URL (`plugins/pr-tracker/src/lib/forges/github.ts` does). The URL's prefix must be in the manifest's `openUrl`.
   - `glab auth login --web --hostname <host>` goes straight to the browser OAuth flow without a TTY and opens the browser itself (`internal/commands/auth/login/login.go:587` in glab v1.121.0). A self-managed server needs an OAuth app ID first (`glab config set client_id`), and glab says so in its error.
   - `az login --allow-no-subscriptions --output none` opens the browser itself. Its subscription picker only appears with a TTY (`can_show_selector` in `azure/cli/command_modules/profile/custom.py`). `--allow-no-subscriptions` lets accounts that only use Azure DevOps sign in.
7. **List the commands.** Every program and subcommand `cli` runs, the login included (`az login`, `gh auth login`), has to be in `permissions.exec`, since the user approves that list. A sign-in waits up to 5 minutes and then retries, and the host waits for an action up to 310 s, so the button spins for the whole sign-in.
8. **Never touch tokens.** The plugin doesn't read, store or pass credentials. The CLI keeps its own.

## Check it

- `cli` has a `wings` option for tests, so a test can hand it a fake `exec` that answers with the empty-config stderr from step 1 and check that `problem(key)` is recorded and `problemBadge` returns the badge. `packages/plugin-sdk/cli.test.mjs` shows the shape.
- Run the harness from the `wings-plugin` skill with the empty config folder on the built `dist/main.js`. The first call should set the Sign in badge.
- The real browser sign-in needs the user's account. Ask them to click Sign in and tell you what the badge shows.
