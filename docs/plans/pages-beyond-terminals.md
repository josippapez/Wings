# Pages beyond terminals

Wings is becoming a workspace for a project, not only a terminal with plugins. A tab can hold a terminal or a plugin's page, and the + button offers both.

## What exists today

- `views` in a plugin manifest can list `"tab"` in `opens`, and `wings.openView(id, { as: "tab" })` opens the page as a tab next to the terminals. Wings saves and restores it like a terminal tab (`plugins/README.md`, `views`).
- A tab page gets the same `wings` object as a sidebar: `exec`, `fetch`, `storage`, `secrets`, `broadcast`, `onPanes`, `onFocus`.
- Plugins can give Claude tools through Wings' MCP server (`mcpTools`, `wings.onTool`).
- The + button (`apps/desktop/src/components/title-bar.tsx`, `onNew`) always opens a terminal.

## What to build

### 1. A + menu

- Click + opens a terminal, as now. A chevron or long-press opens a menu: Terminal, then each plugin page that can open as a tab, with its icon.
- Manifest: a view that lists `"tab"` in `opens` and sets `"newTab": true` shows in the menu. Wings shows at most one entry per view.
- The command palette lists the same entries ("New tab: Tasks").
- A setting picks what plain + opens: a terminal (default) or a plugin page.

### 2. Tab pages that know their project

- A tab page belongs to the project it was opened in. Add `wings.project()` and `wings.onProject(fn)` to the SDK: `{ id, name, path, branch }` of the tab's project, so the page shows that project's data without guessing from panes.
- Today a view tab is one per project per view. Keep that, and allow `"multiple": true` for pages where several make sense, like two boards with different filters, each with its own saved state (`wings.tabState()` / `setTabState()`).

### 3. Room for bigger pages in `wings-plugin-ui`

- Layouts for a full tab: list and detail side by side, a toolbar row, a board with columns, a resizable split. Same look as the app.
- A page that opens both as a sidebar and as a tab gets a `wings.placement()` of `sidebar` or `tab`, so it can show more detail when there's room.

### 4. Plugins that use this

- **Tasks.** A task list per project: title, notes, status, links to panes or branches. Stored with `wings.storage` per project. MCP tools so an agent can list tasks, add one, and mark one done. A sidebar for the short list, a tab for the full board.
- **Azure DevOps in a tab.** The Work item and Sprint board plugins open their views as tabs: the sprint board with columns, a work item with its full description, comments and linked PRs, and the update feed from the 2-minute poll as an inbox.
- **Project notes.** Notes per project that you write and agents read. MCP tools `notes__read` and `notes__write` (write asks first, or only appends). Open question: keep notes in Wings' storage, or in a file in the repo (`.wings/notes.md`) so they travel with the project and agents can read them without Wings. The file is simpler and lets Claude read it directly. Storage keeps them out of git.

### 5. Projects in the sidebar, redesigned

A project row expands into what belongs to it, not only its terminals and agents:

- **Pull requests:** the project's open PRs (yours first, then ones waiting for your review), with status, checks and unread comments. Clicking one opens it as a tab in the main view: the description, the diff in the diff viewer with its review comments, threads, and actions to approve, comment or send comments to an agent. A PR has an agent working on it when a pane's branch matches the PR's branch.
- **Worktrees:** each worktree with its branch and agent, and New agent in worktree.
- **Plugin sections:** a plugin can add a section to a project, such as Tasks, Work items or Notes. Each section is a short list of rows with a badge, and clicking a row opens the plugin's page for it as a tab. SDK sketch: `contributes.projectSections: [{ id, title, icon? }]`, filled with `wings.setProjectSection(projectId, sectionId, rows)`, where a row is `{ id, label, detail?, badge?, open: { view, args } }`.
- Sections collapse and remember it per project, so a busy project stays readable. The project's own row still shows the branch and the agent state roll-up.

The PR list and the PR tab can start inside the PR tracker plugin, which already knows each pane's PR on GitHub, Azure DevOps and GitLab. That way they arrive through the same plugin API that other plugins use.

## SDK additions, all in one place

| Addition | What it's for |
|---|---|
| `"newTab": true` on a view | The view shows in the + menu and the palette |
| `wings.project()`, `wings.onProject(fn)` | The project a tab page belongs to |
| `"multiple": true` on a view, `wings.tabState()`, `wings.setTabState()` | Several tabs of one page, each with its own state |
| `wings.placement()` | Whether the page is in a sidebar or a tab, to show more detail when there's room |
| `contributes.projectSections`, `wings.setProjectSection(projectId, sectionId, rows)` | A plugin's own section under a project in the sidebar, like Pull requests, Tasks or Notes |
| `wings.openView(id, { as: "tab", args })` and `wings.viewArgs()` | Open a page for one item, like PR 123 or task 7, and read which item it was opened for |
| Full-tab layouts in `wings-plugin-ui` | List and detail, toolbar, board columns, a resizable split |

## Order

All seven shipped: 1 and 2 in Wings 0.1.22, 3 and 4 in 0.1.22 and 0.1.23, 5 and 6 in 0.1.23, 7 in 0.1.24.


1. The + menu and `newTab` (small, uses existing view tabs).
2. `wings.project()` / `onProject` and per-tab state.
3. Full-tab layouts in `wings-plugin-ui`.
4. Project notes, the smallest of the plugins and the one that proves MCP reading and writing.
5. Project sections in the sidebar, with PRs from the PR tracker and a PR review tab.
6. Tasks, then the Azure DevOps tabs.
7. The Project page in Wings, with plugin blocks (below).

## Next: the Project page in Wings itself

Project notes grow into a page that every project has, built into Wings rather than a plugin. It's where you and your agents keep what a project is, and where plugins put live views of it.

### What's on it

- **Facts:** a table of key project details, like the dev command, test command, staging URL and package manager.
- **Sections:** named Markdown sections with tables, lists, links and images, edited in place.
- **Blocks:** live plugin views placed between sections, like the sprint board from Work item, the Tasks board or open pull requests. Each block is a plugin page in its own frame, sized to fit, that shows the page's project.
- **Add:** "+ Add" between any two items offers a section, a fact, or one of the blocks the plugins offer.

### SDK

| Addition | What it's for |
|---|---|
| `contributes.blocks: [{ id, title, page, icon?, minHeight? }]` | A plugin page that can be placed on a Project page |
| `wings.placement()` returns `"block"` | The page draws a compact version for a block |
| `wings.setHeight(px)` | A block grows with its content, within limits |
| `wings.blockState()`, `wings.setBlockState()` | Settings per block, like which board or which filter |

A block gets `wings.project()` like a tab page. A block page is a normal plugin page, so the same page can also open as a tab.

### Agents

Wings' own MCP server gets `project_info` tools: read the page, set a fact, write or add to a section, and list the blocks. A tool call uses the project of the pane it came from, as Notes does now. The text an agent reads is the user's data, not instructions.

### Why in Wings and not a plugin

- **Storage:** the page lives in Wings' app data, so it's not bound by a plugin's 1 MB of storage. Images can be real files instead of links only.
- **Blocks:** only Wings can place one plugin's frame inside another page.
- **Tools:** the tools work with no plugin turned on.

The Notes plugin's facts and sections move into the page the first time it opens. After that, the plugin is retired.

### Performance

Each block is a frame, so a page holds at most 8 blocks. A block loads when it scrolls into view, pauses its polling while the page is hidden (`pollWhileFocused` plus visibility), and unloads after 5 minutes hidden, like sidebars.

## Open questions

- Should a plugin page be able to replace the terminal as the first tab of a new project?
- Project notes: settled. They live in Wings' app data as the Project page, and agents reach them through Wings' MCP tools.
- Do tab pages unload when hidden, like sidebars do after 5 minutes? Blocks do (5 minutes out of view, and unfocused while hidden). Tab pages still follow the view tab rule.
