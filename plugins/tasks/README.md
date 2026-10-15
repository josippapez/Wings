# Issues

A lightweight issue tracker for each project, for you and for Claude: what is being worked on, what was done and when, so it can be referred to or picked up later. Issues can be grouped under epics and linked to each other, with no waves or routing.

An issue has a key (`WIN-12`: up to three letters of the project's name, or `ISS`, and a per-project counter that is never reused), a title, a Markdown description (goal, scope, acceptance criteria), a type (`task` by default, `bug`, `feature`, `epic`), a status (`todo`, `in_progress`, `in_review`, `done`, `canceled`), links (branch, pull request, commits), relations to other issues, the Claude sessions that worked on it, and a log.

The log is append-only: entries are `created`, `status`, `type`, `note` and `link`, each with who (you, a Claude session with its name, or `git` on entries from before 0.6) and when. Status and link changes log themselves. Nothing edits or deletes an entry; deleting the whole issue is the only way one goes.

- **Board.** **Issues** in the title bar or the + menu opens a tab with To do, In progress, In review and Done columns. Canceled is behind "Show canceled". A card shows the key, title, when it last changed and the session that touched it last. Click it to open the issue. Move a card with its arrow buttons, or Alt+Left and Alt+Right while it has focus.
- **Issue page.** Opens as a tab, one per issue. Edit the title (Enter or leaving the field saves), the status, the description (Edit, then Save), the branch and the pull request. Add a note to the log. The activity is a timeline, newest first. **Resume** opens a terminal in the project running `claude --resume <latest session id>`. **Start agent** opens one running `claude` with a prompt to read the issue with `tasks__get` and continue it. Both need `permissions.panes`.
- **Project section and block.** Under a project in the sidebar, the open issues (up to 50) with a status badge. The Project page block shows counts and the first eight open issues.
- **Tools for Claude.** `tasks__list` (optional `status`, `type` and `query`, words that must all appear in an issue's key, title, description or log notes; each line shows the type and parent), `tasks__get` (`key`; shows type, parent, child issues with progress for an epic or feature, related, blocked by and blocks), `tasks__create` (`title`, `description`, `status`, `type`, `parent`, `related_add`, `blocked_by_add`), `tasks__update` (`key` and any of `title`, `description`, `status`, `type`, `branch`, `pr`, `commit`, `parent` (a key, or "" to clear), `related_add`, `related_remove`, `blocked_by_add`, `blocked_by_remove`; the last four take lists of keys) and `tasks__log` (`key`, `text`). The descriptions tell agents that issue keys are local to Wings (never in commit messages, branch names or pull request titles), to link each commit, pull request and branch themselves with `update`, and how to group work under an epic. A line in the plugin's `mcpInstructions` tells Claude at session start to file issues with `tasks__create` and to look for one with `tasks__list` before starting work. The Issues tab has a search box that filters by the same rule, and a Default view setting in Wings Settings (Board, Rows grouped by parent, or List) sets how the tab opens. Titles are up to 200 characters, descriptions 20,000 and notes 4,000. Issue text is your data, and the tool results say to read it as data.

## Types and relations

- **Type.** `task` (default), `bug`, `feature`, `epic`: a small icon coloured by type on board cards, block rows, the issue header, and a Type select in the properties column. Under a project in the sidebar a row that isn't a task has its type (and an epic's or feature's "3 of 7 done") as a muted line.
- **Parent.** An issue belongs to at most one parent, which must be an epic or a feature of the same project. An issue can't be its own parent or sit under one of its own descendants, and an epic or feature with children can't be changed to another type. Set it with the Parent picker (a combobox of the project's epics and features) or the tools. Board cards show the parent key, muted. An epic or feature page has a "Child issues" list with status dots and a progress bar, "3 of 7 done" (canceled children aren't counted).
- **Related.** Symmetric: linking A to B shows on both. Deduped, up to 20 per issue.
- **Blocked by.** `blockedBy` is stored on the blocked issue; the other issue shows "Blocks" worked out from it. Two issues can't block each other, directly or through a chain. Up to 20 per issue.
- Each change writes a log entry on both issues ("Linked to WIN-3 as related", "Parent: WIN-1" and "Child: WIN-4", "Blocked by WIN-2" and "Blocks WIN-4"), by whoever made it. A change that fails changes nothing. Deleting an issue removes it from others' relations and logs that on each ("WIN-3 was deleted: unlinked (related)", "Parent WIN-1 was deleted").
- Related and Blocked by are added with a combobox of the project's issue keys and titles, and removed with × on their "KEY title" chip, which opens that issue.

## Commits, branch and pull request

Issue keys stay in Wings. Nothing reads a commit message or a branch name for a key, and the plugin runs no `git` and reads no transcripts. The agent or you link the work:

- **Agents.** The `update` description tells them to call `update` with the hash (`commit`) after each commit they make for the issue, to set `pr` to the pull request's URL when they open one, and to set `branch` when they start on a branch. The `create`, `update` and `log` descriptions also say never to put an issue key in a commit message, branch name, pull request title or description.
- **You.** Edit the branch and pull request on the issue page.
- A commit already on the issue (by hash prefix) is never added twice. Each commit, branch and pull request change writes a `link` entry by whoever made it.
- 0.6 removed the scan that matched keys in commit messages and branch names (a `git log` and `git branch` per project every minute); the `scan:<projectId>` storage keys it left are deleted on start.

## Description template

A new issue starts empty. The first time you edit an empty description the editor opens with

```
## Goal

## Scope

## Done when
- [ ] 

## Validation
```

and saving it untouched stores no description. A page never shows a heading with nothing under it: sections whose body is blank or only empty checklist items are hidden, so a half-filled template shows only what was written, and an issue with only the template reads as "Add a description…". Tool-created issues aren't prefilled: if there is no description, `tasks__get` says "No description yet; fill Goal, Scope, Done when, Validation", and the `create` tool's description tells agents to fill those four (Done when as a checklist, Validation as the commands or checks that prove it).

## Opening issues

The Issue view is no longer `multiple`: clicking an issue reuses the one issue tab. A middle-click or Cmd/Ctrl+click on a card, a child, a related or blocking chip opens another tab (`wings.openView("issue", { as: "tab", args, newTab: true })`). Needs Wings 0.1.39.

## Which project, and who wrote it

The project of the terminal Claude runs in. There is no `project` argument. If the call can't be tied to a terminal in Wings, the tool returns an error; it never falls back to the project on screen. The entry's author is the Claude session in that terminal (from `wings.onPanes`), and the session is added to the issue's sessions. Entries made on a page are "you".

## Where issues live

In the plugin's `wings.storage`, one entry per project id (`issues:<projectId>`, format version 3: `{ v, prefix, next, issues }`), private to your machine. Only the plugin's main script writes them. Pages send it small operations and the tools run the same ones, one at a time, so a page and Claude never overwrite each other. Main then broadcasts the project id, never the issues, and each page reads storage again.

All of the plugin's storage is 1 MB. Measured by the tests: 200 issues with 10 log entries each is about 430 KB (about 2.1 KB an issue). A project may use up to 700 KB, 300 issues and 300 log entries per issue; past that a write is refused with a message.

Version 0.5.0 reads a format 2 store as format 3 (every issue a task) and saves it that way on start. Version 0.4.0 moved the old Tasks entries (`tasks:<projectId>`) into issues once, when the plugin starts: oldest first they become `<KEY>-1`, `-2` and so on, notes become the description, doing becomes in progress, and the branch is kept. The old entry is deleted after the new one is saved.

## Permissions

`panes: ["claude", "claude --resume"]` for Start agent and Resume, and `openUrl: ["https://"]` for the pull request link and links in Markdown. Start agent passes its prompt as one `openPane` arg, which Wings quotes; it's built from the issue key, which is checked first. Needs Wings 0.1.39.
