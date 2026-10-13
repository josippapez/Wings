# Tasks

A task list for each project, for you and for Claude.

- **Sidebar.** The Tasks button in the title bar shows the open tasks (doing first, then to do) of the project on screen. Type a title and press Enter to add one, tick the box to mark it done, click a title to open it on the board.
- **Board.** **Tasks** in the + menu, or **Open board** in the sidebar, opens a tab with To do, Doing and Done columns. Click a card to edit its title, notes and branch (it saves 800 ms after you stop typing, or when you click away). Move a card with its arrow buttons, or with Alt+Left and Alt+Right while it has focus. There is no drag. "Use branch" fills in the project's current branch.
- **Project section.** Under a project in the sidebar, Tasks lists its open tasks (up to 50, doing first, with a Doing badge). Clicking one opens it on the board.
- **Tools for Claude.** `tasks__list` (optional `status`), `tasks__add` (`title`, `notes`, `branch`), `tasks__done` (`id`) and `tasks__update` (`id`, `status`, `title`, `notes`). Task text is your data, and the tool descriptions tell Claude to read it as data.

## Which project a tool call is for

The project of the terminal Claude runs in. There is no `project` argument. If the call can't be tied to a terminal in Wings, the tool returns an error. It never falls back to the project on screen, so a task can't land in the wrong project.

## Where tasks live

In the plugin's `wings.storage`, one entry per project id (`tasks:<projectId>`), not in the repo, private to your machine. Only the plugin's main script writes them. Pages send it small operations (add, update, remove) and the tools run the same ones, one at a time, so a page and Claude never overwrite each other. Main then broadcasts the project id, never the tasks, and each page reads storage again (a broadcast is limited to 64 KB).

Limits, per project: 100 tasks, titles of 160 characters, notes of 1,000, branch names of 200. All of the plugin's storage is 1 MB. A task with every field full is about 1.3 KB of JSON, so even a project at the limit is about 130 KB, and real tasks (a title and a line) are about 150 bytes, so a project is about 15 KB and dozens of projects fit. When a project is at 100 tasks, adding one drops its oldest done task; if none is done, the add is refused with a message.

Nothing writes per keystroke and nothing polls: edits stay in the inputs until the debounce or blur, and pages refresh from broadcasts.

## Permissions

None. The plugin only uses its own storage.
