---
name: wings-cli-sign-in
description: Adds a one-click Sign in button to a Wings plugin badge for a CLI the plugin needs to be logged in (gh, az or similar), so the user never opens a terminal. Use when a plugin's CLI calls fail because it isn't signed in, or when adding a new provider CLI to a plugin.
---

# One-click CLI sign-in for Wings plugins

`plugins/pr-tracker/main.js` does this for GitHub (`gh`), GitLab (`glab`) and Azure DevOps (`az`). Copy its shape: `checkGhLogin`, `problems`, `problemBadge` and `signIn`. For the plugin API itself, see the `wings-plugin` skill and `plugins/README.md`.

## Steps

1. **Find the logged-out signal.** Reproduce it without signing the user out by pointing the CLI at an empty config folder in the scratchpad. For example, `GH_CONFIG_DIR=<empty dir> gh pr list` exits 4 and prints `To get started with GitHub CLI, please run:  gh auth login`. `az` prints a message that mentions `login`. `glab api` prints `401 Unauthorized`, but a private project answers `404` when you're signed out, so check `glab auth status --hostname <host>` on a 404. Match on stderr, because exit codes alone are shared with other failures.
2. **Record it per provider.** Keep `problems` keyed by provider (`github`, or the Azure org URL) as `{ login: true, message }`. Set it from every call that can hit it, and clear it when a call succeeds. GitLab is the exception: public projects answer some calls without sign-in, so its problem only clears on Try again or after signing in, or the badge would flip back and forth.
3. **Show the button.** A login problem replaces the pane's badge, even a PR badge, because nothing works until sign-in. The label is `Sign in to <Provider>`, with the actions Sign in (primary) and Try again. Each provider gets its own tone so the user can tell them apart: GitHub is `merged` (purple), GitLab is `info` (blue) and Azure DevOps is `warning` (amber). Give a new provider a different one.
4. **Read the CLI's login code before calling it.** Wings runs programs with no TTY and stdin closed, and many CLIs change behaviour without a TTY:
   - `gh auth login --web --clipboard --hostname github.com` doesn't open the browser without a TTY. It prints `One-time code (XXXX-XXXX) copied to clipboard` (or `First copy your one-time code: XXXX-XXXX`) and `Open this URL to continue in your web browser: <url>`, then waits. Source: `internal/authflow/flow.go` in cli/cli v2.102.0. Use `exec`'s `onOutput` to put the code in the badge label and `wings.openUrl` the URL. The URL's prefix must be in the manifest's `openUrl`.
   - `glab auth login --web --hostname <host>` goes straight to the browser OAuth flow without a TTY and opens the browser itself (`internal/commands/auth/login/login.go:587` in glab v1.121.0). A self-managed server needs an OAuth app ID first (`glab config set client_id`), and glab says so in its error.
   - `az login --allow-no-subscriptions --output none` opens the browser itself. Its subscription picker only appears with a TTY (`can_show_selector` in `azure/cli/command_modules/profile/custom.py`). `--allow-no-subscriptions` lets accounts that only use Azure DevOps sign in.
5. **Allow time.** Pass `timeoutMs: 300_000`, the maximum. The host waits up to 310 s for an action to finish, so the button spins for the whole sign-in.
6. **Refresh after.** On success, clear `problems`, the lookup cache and cached statuses, then refresh so the badges come back.
7. **Never touch tokens.** The plugin doesn't read, store or pass credentials. The CLI keeps its own.

## Check it

- `node --check` the plugin, then run the harness from the `wings-plugin` skill with the empty config folder. The lookup should record the login problem.
- The real browser sign-in needs the user's account. Ask them to click Sign in and tell you what the badge shows.
