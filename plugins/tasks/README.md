# Issues

A lightweight issue tracker for each project, for you and for Claude: what is being worked on, what was done and when, so it can be referred to or picked up later. Issues can be grouped under epics and linked to each other, with no waves or routing.

An issue has a key (`WIN-12`: up to three letters of the project's name, or `ISS`, and a per-project counter that is never reused), a title, a Markdown description (goal, scope, acceptance criteria), a type (`task` by default, `bug`, `feature`, `epic`), a status (`todo`, `in_progress`, `in_review`, `done`, `canceled`), a priority (`urgent`, `high`, `medium`, `low`, or none), links (branch, pull request, commits), relations to other issues, the agent sessions (Claude Code or OpenCode, each with its kind) that worked on it, and a log.

The log is append-only: entries are `created`, `status`, `type`, `priority`, `note` and `link`, each with who (you, an agent session with its name, or `git` on entries from before 0.6) and when. Status and link changes log themselves. Nothing edits or deletes an entry; deleting the whole issue is the only way one goes.

- **Board.** **Issues** in the title bar or the + menu opens a tab with To do, In progress, In review and Done columns. Canceled is behind "Show canceled". A card shows the key, title, when it last changed and the session that touched it last. Click it to open the issue. Move a card with its arrow buttons, or Alt+Left and Alt+Right while it has focus.
- **Issue page.** Opens as a tab, one per issue. Edit the title (Enter or leaving the field saves), the status, the description (Edit, then Save), the branch and the pull request. Add a note to the log. The activity is a timeline, newest first. **Resume** opens a terminal in the project that continues the latest session with the agent it ran in (`claude --resume <id>` or `opencode --session <id>`), and each session under Sessions has its own Resume. **Start agent** picks Claude Code or OpenCode and opens a terminal running it (`claude <prompt>` or `opencode --prompt <prompt>`) with a prompt to read the issue with `tasks__get` and continue it. The agents and their commands are one table, `AGENTS` in `src/lib/commands.ts`. Both need `permissions.panes`.
- **Project section and block.** Under a project in the sidebar, the open issues (up to 50) with a status badge. The Project page block shows counts and the first eight open issues.
- **Tools for Claude.** `tasks__list` (optional `status`, `type` and `query`, words that must all appear in an issue's key, title, description or log notes; each line shows the type, priority and parent), `tasks__get` (`key`; shows type, parent, child issues with progress for an epic or feature, related, blocked by and blocks), `tasks__create` (`title`, `description`, `status`, `priority`, `type`, `parent`, `related_add`, `blocked_by_add`), `tasks__update` (`key` and any of `title`, `description`, `status`, `priority`, `type`, `branch`, `pr`, `commit`, `parent` (a key, or "" to clear), `related_add`, `related_remove`, `blocked_by_add`, `blocked_by_remove`; the last four take lists of keys), `tasks__log` (`key`, `text`) and `tasks__todo` (`items`, `key`; see Todo). The descriptions tell agents that issue keys are local to Wings (never in commit messages, branch names or pull request titles), to link each commit, pull request and branch themselves with `update`, and how to group work under an epic. A line in the plugin's `mcpInstructions` tells Claude to use Issues for every piece of work, not only when asked. The Issues tab has a search box that filters by the same rule, and a Default view setting in Wings Settings (Board, Rows grouped by parent, or List) sets how the tab opens. Titles are up to 200 characters, descriptions 20,000 and notes 20,000. Issue text is your data, and the tool results say to read it as data. The tools are written once, in `src/tools.ts`: the build puts their descriptions and schemas, limits included, into the manifest, and every call is checked against its schema before anything runs, so a wrong input gets an error naming the field and what it allows.

## Session start, local issues and Todo

Needs Wings 0.1.54 (`contributes.hooks`, and `rail` in `contributes.ui`); the journal needs 0.1.61.

- **Session start.** At every start, resume, clear and compact in a project Issues knows, Claude is told that Issues is the local tracker for it, kept on this computer and never named in commits, branches or PRs. If the session is already on an issue, the line names it (key, title, status) and says to keep its status and log current; if the session has a todo with steps left, it says how many and that `tasks__todo` with no items reads it. Otherwise it says to find the issue with `tasks__list` or create one with `tasks__create` before work that changes files, and to keep its steps in `tasks__todo`. Nothing is returned for a session outside a known project.
- **Stale and finished.** The same start line also lists, for the project, issues In progress or In review whose last change is 3 or more days old ("WIN-3 is In progress but unchanged for 5 days."; the three oldest, then a count) and, when the session's todo has every step done while its linked issue is still In progress or In review, a line saying so. They are things to check: the hook never changes a status.
- **Journal.** Answers `Journal`, which an agent's per-turn change journal sends through `wings journal` (Wings registers itself as the journal's sink unless you have your own; needs Wings 0.1.61). It finds the session's issue the way session start does, only when that issue is In progress or In review, links each commit the turn made (a commit already on the issue is not added twice, and each writes a `link` log entry) and records the turn's files in the issue's **Files changed**: one row per path with the newest description, time and session, at most 100 (the oldest go), with paths and descriptions cut to 300 characters. The log gets no entry for files: it stays append-only for people and decisions, so a journal can't fill it or push your notes out of `tasks__get`, and an issue with a full log still takes its files. `tasks__get` prints "Files changed (N):" after Links, newest first, up to 30 lines and "and N more", and the Issue page shows the same list under the description, the first ten rows with the rest folded. Issues without it are unchanged. A Done or Canceled issue, or an unknown session, changes nothing. It never answers with context.
- **Keep issues local.** A setting in Wings Settings, on by default. While on, a Bash command that is a commit or a write to a host (`git commit`, `git tag -a` or `-m`, `gh pr create|edit|comment|review`, `gh issue create|comment`, `glab mr create|update|note`, `az repos pr create|update`) and holds a key like `WEB-12` of a project that has issues is denied, with a line telling Claude the key is a local Wings issue to take out of the message. Keys match as whole words and case-sensitively, with the prefixes the projects really use. Off, nothing is checked.
- **Todo.** `tasks__todo` is the agent's own step list: `items` (up to 30 of `{ text, status }`, `status` is `pending`, `in_progress` or `done`) replaces the session's whole list, no `items` reads it back, and `key` links it to an issue (by default the one the session is on, which also records the session on that issue). Each write keeps the earlier version, the newest 50 per session, in `todo/<slot>/<session id>`. While a step is open the list shows as a card in the corner of the pane running the session (`wings.setPaneRail`), and the **Todo** tab lists the project's sessions with their steps, the issue each is on and an expandable history. The issue page shows a session's steps under it. A Wings without `setPaneRail` just shows no card.

## Types, priority and relations

- **Priority.** `urgent`, `high`, `medium`, `low`, or none, which isn't stored. Linear's three-bar icon shows on cards, list rows and the issue page, a Priority select sits in the properties column, and within a board column and a group of the list by status the highest sorts first. Each change writes a `priority` log entry.
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
- 0.6 removed the scan that matched keys in commit messages and branch names (a `git log` and `git branch` per project every minute). The `scan:<projectId>` storage keys it left were deleted on every start until 0.11; nothing reads them.

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

The project of the terminal Claude runs in. There is no `project` argument. If the call can't be tied to a terminal in Wings, the tool returns an error; it never falls back to the project on screen. The entry's author is the agent session in that terminal (Claude Code's or OpenCode's), which Wings passes with the tool call with its kind, and the session is added to the issue's sessions. Entries made on a page are "you".

## Where issues live

In the plugin's `wings.storage`, private to your machine: one entry per issue, `issues/<slot>/<KEY>`, and one per project, `issues/<slot>` (`{ v: 3, path, prefix, next }`). The slot is 16 hex digits of a hash of the project's path, so keys stay short for any path; the project's entry keeps the path, and a second path that lands in the same slot is refused rather than mixed in. A change writes only the issues it changed, and the project's entry when the counter moves, before the issue. Pages read a project with one `wings.storage.entries("issues/<slot>")` call (Wings 0.1.50). Only the plugin's main script writes them. Pages send it small operations and the tools run the same ones, one at a time, so a page and Claude never overwrite each other. A page asks main to apply an operation and is answered once it is saved, and main then tells every page the project id, never the issues, so each reads storage again: a broadcast takes 64 KB and an issue can be bigger.

Wings holds 1 MB per entry and 50 MB for the whole plugin. Measured by the tests: 200 issues with 10 log entries each is about 430 KB (about 2.1 KB an issue). One issue may use up to 1,000 KB (a full description and about 48 notes of the full 20,000 characters), a project 300 issues and an issue 300 log entries; past that a write is refused with a message.

Version 0.12.0 moves each project from its one `issues:<projectId>` entry (format 3 or 2) or old Tasks entry to one entry per issue, once, when the plugin starts or first writes to the project. The project's entry is written last, so a move cut short reads the old entry and starts again; the old entry is deleted after it.

Version 0.5.0 reads a format 2 store as format 3 (every issue a task) and saves it that way on start. Version 0.4.0 moved the old Tasks entries (`tasks:<projectId>`) into issues once, when the plugin starts: oldest first they become `<KEY>-1`, `-2` and so on, notes become the description, doing becomes in progress, and the branch is kept. The old entry is deleted after the new one is saved.

## Permissions

`panes: ["claude", "claude --resume", "opencode --prompt", "opencode --session"]` for Start agent and Resume, and `openUrl: ["https://"]` for the pull request link and links in Markdown. Start agent passes its prompt as one `openPane` arg, which Wings quotes; it's built from the issue key, which is checked first. Needs Wings 0.1.39.
