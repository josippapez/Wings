# Claude accounts for Wings

Choose which Claude Code account each Wings project uses. Claude Code keeps an account in its own config folder, picked by `CLAUDE_CONFIG_DIR`: `~/.claude` is the default account and `~/.claude-<name>` is another one. This plugin sets that per project, so a terminal opened in "web" starts Claude with one account and a terminal in "api" with another, with no `export` to remember.

Adds a button to the Wings title bar that opens Claude accounts in the right sidebar. At the top is the project on screen and a picker for its account, labelled with the account's email when Claude Code knows it, else its name (Default for `~/.claude`). Under it, every account with its email or "Not signed in", the projects that use it, and "This project" on the one this project has.

Point at the button, or tab to it, and a popover shows the same picker without opening the sidebar.

Only new terminals use the account you pick. Claude sessions that are already open keep theirs, so after a switch the plugin says so and offers Open Claude, which opens a new terminal running `claude` in the project.

## Not signed in

Claude Code keeps who an account was in the folder's `.claude.json` after its sign-in is gone, so an email alone doesn't mean the account is signed in, and a Claude session on it then runs without your plan (its banner says API Usage Billing). The plugin asks `claude auth status` for each account instead, through Wings. An account that isn't signed in shows "Not signed in" in the warning colour, with who it was when the config still names them, and a Sign in button that opens a new terminal as that account running `claude auth login`. If the project on screen uses such an account, its card says new Claude sessions there won't use your plan until you sign in, with the same button. The list loads again when `claude` ends in that terminal or the terminal closes, and when Wings comes to the front.

## Start Claude as another account

Under the picker, Start Claude as has a button for each account the project doesn't use. It opens a new terminal in the project running `claude` as that account, and leaves the project's own account as it is. An account that isn't signed in says so on its button. Handy for a quick session on your other account without switching the project and back.

## Use my default setup

Each named account has a Use my default setup box, on until you turn it off. On, the account uses your default account's settings, plugins, skills and rules: Wings links `settings.local.json`, `CLAUDE.md` and the other Markdown files, `rules`, `skills`, `agents`, `commands`, `hooks`, `output-styles`, `plugins` and the status line scripts from `~/.claude` into the account's folder, copies `settings.json` into it with the account's own `model` kept (so an account on another plan keeps its model, and what you change in the default's settings reaches it the next time a terminal opens there), and copies your default MCP servers into it. The account's sign-in, sessions and history stay its own. What the account had at those names is moved to `.before-shared-setup/` in its folder and comes back when you turn the box off. Wings applies it each time it connects to the account, so an account you made before gets it the next time you use it.

## Add an account

Add account asks for a short name (lowercase letters, digits and dashes, up to 32, like `work`). Wings makes `~/.claude-<name>` with your default setup, the project switches to the new account, and `claude` opens in a new terminal in the project. Claude Code walks you through signing in the first time it runs in a new config folder. The other projects keep the account they have.

The list refreshes when Wings comes to the front and when the popover opens, so an account you signed in to in a terminal shows as signed in the next time you look.

## Install

Turn it on in Plugins, under Browse. It asks for the Claude accounts permission, and to open `claude` and `claude auth login` in a terminal.

The crate in `rust/` has one feature for each frame the manifest names (`main`, `sidebar`, `popover`). `sh build-rust.sh claude-accounts` in `plugins/` builds them into `dist/native/`. `cargo test` in `rust/` runs the tests.
