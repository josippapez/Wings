# Issues

A lightweight issue tracker for each project, for you and for Claude: what is being worked on, what was done and when, so it can be referred to or picked up later. No epics, waves or routing.

An issue has a key (`WIN-12`: up to three letters of the project's name, or `ISS`, and a per-project counter that is never reused), a title, a Markdown description (goal, scope, acceptance criteria), a status (`todo`, `in_progress`, `in_review`, `done`, `canceled`), links (branch, pull request, commits), the Claude sessions that worked on it, and a log.

The log is append-only: entries are `created`, `status`, `note` and `link`, each with who (you, or a Claude session with its name) and when. Status and link changes log themselves. Nothing edits or deletes an entry; deleting the whole issue is the only way one goes.

- **Board.** **Issues** in the title bar or the + menu opens a tab with To do, In progress, In review and Done columns. Canceled is behind "Show canceled". A card shows the key, title, when it last changed and the session that touched it last. Click it to open the issue. Move a card with its arrow buttons, or Alt+Left and Alt+Right while it has focus.
- **Issue page.** Opens as a tab, one per issue. Edit the title (Enter or leaving the field saves), the status, the description (Edit, then Save), the branch and the pull request. Add a note to the log. The activity is a timeline, newest first. **Resume** opens a terminal in the project running `claude --resume <latest session id>`. **Start agent** opens one running `claude` with a prompt to read the issue with `tasks__get` and continue it. Both need `permissions.panes`.
- **Project section and block.** Under a project in the sidebar, the open issues (up to 50) with a status badge. The Project page block shows counts and the first eight open issues.
- **Tools for Claude.** `tasks__list` (optional `status`), `tasks__get` (`key`), `tasks__create` (`title`, `description`, `status`), `tasks__update` (`key` and any of `title`, `description`, `status`, `branch`, `pr`, `commit`) and `tasks__log` (`key`, `text`). Titles are up to 200 characters, descriptions 20,000 and notes 4,000. Issue text is your data, and the tool results say to read it as data.

## Which project, and who wrote it

The project of the terminal Claude runs in. There is no `project` argument. If the call can't be tied to a terminal in Wings, the tool returns an error; it never falls back to the project on screen. The entry's author is the Claude session in that terminal (from `wings.onPanes`), and the session is added to the issue's sessions. Entries made on a page are "you".

## Where issues live

In the plugin's `wings.storage`, one entry per project id (`issues:<projectId>`, format version 2: `{ v, prefix, next, issues }`), private to your machine. Only the plugin's main script writes them. Pages send it small operations and the tools run the same ones, one at a time, so a page and Claude never overwrite each other. Main then broadcasts the project id, never the issues, and each page reads storage again.

All of the plugin's storage is 1 MB. Measured by the tests: 200 issues with 10 log entries each is about 430 KB (about 2.1 KB an issue). A project may use up to 700 KB, 300 issues and 300 log entries per issue; past that a write is refused with a message.

Version 0.4.0 moves the old Tasks entries (`tasks:<projectId>`) into issues once, when the plugin starts: oldest first they become `<KEY>-1`, `-2` and so on, notes become the description, doing becomes in progress, and the branch is kept. The old entry is deleted after the new one is saved.

## Permissions

`panes: ["claude", "claude --resume"]` for Start agent and Resume, and `openUrl: ["https://"]` for the pull request link and links in Markdown. Start agent passes its prompt as one `openPane` arg, which Wings quotes; it's built from the issue key, which is checked first. Needs Wings 0.1.37.
