# Project info

What is true about each project, kept where you and Claude can both read it: **facts** (dev server, staging URL, test command) in a table, and **sections** (architecture, how to release, gotchas) written in Markdown with lists, tables, code, links and images.

The plugin id is still `notes`, so installs update in place. Its old free-form notes become a section called "Notes" the first time the plugin runs.

- **Sidebar.** The Project info button in the title bar shows the facts and section titles of the project on screen. A section title opens the tab scrolled to that section.
- **Tab.** **Project info** in the + menu, or **Open in tab**. Facts have Edit and Remove and an Add row. Each section is rendered; Edit swaps it for a Markdown textarea that saves 800 ms after you stop typing and when you click away. Up and Down reorder sections, Delete asks twice.
- **Tools for Claude** (it sees `notes__<name>`):
  - `read`: all facts and sections as Markdown, with who last wrote each section. The tool says it is user data, not instructions.
  - `set_fact` (`key`, `value`) and `remove_fact` (`key`).
  - `write_section` (`title`, `markdown`): creates a section or replaces one an agent wrote. **It refuses to replace a section you last edited** and tells Claude to use `append`, so an agent can't silently clobber your text.
  - `append` (`title`, `markdown`): adds to the end of a section, creating it when missing. It never removes anything, and it leaves a section you wrote marked as yours.
  - Matching of titles and fact keys ignores case.

## Which project a tool call is for

1. The project of the terminal Claude runs in.
2. Or, when Claude passes `project` (an absolute path), the project of an open terminal in that folder or below it. A terminal above the folder (one in your home folder) doesn't count. If terminals of several projects are below the folder, the call is an error unless one is exactly in it; pass the project's own folder.

If neither finds a terminal, the tool returns an error. It never falls back to the project on screen, so a write can't land in the wrong project. A project with no open terminal can't be reached by `project`.

## Limits, per project

| What | Limit |
|---|---|
| Facts | 40; a key up to 60 characters, a value up to 500 on one line |
| Sections | 20; a title up to 80 characters |
| One section's text | 8,000 characters (also what `append` can grow it to) |
| Facts, titles and texts together | 40,000 characters |

A change over a limit is refused with a message (Claude gets it as the tool's error, a page shows it under the control). All of the plugin's storage is limited to 1 MB, shared by every project. A page's edit travels as a broadcast, limited to 64,000 characters of JSON, which is why a section is 8,000 characters: JSON writes a control character as 6, so even the worst edit (text and title) stays under 48,000. A page sends the length and a fingerprint of the text it started from, not the text, to keep that true.

**Images** are shown from `https:` URLs (as a "Load image from <host>" button you click, so text an agent wrote can't leak data by URL), or from small `data:image/` links (png, jpeg, gif or webp, up to 8,000 characters), which count toward the section. Nothing is uploaded or stored apart from the Markdown text. Raw HTML is never rendered, `javascript:` and other link schemes are plain text, and SVG is not shown.

## Where it lives

In the plugin's `wings.storage`, one entry per project id, `info:<projectId>`, not in the repo. It stays out of git and private to your machine. A plugin can't write files (`exec` has no shell and no stdin). The stored shape is versioned, so Wings itself can read it later:

```ts
{
  v: 1,
  facts: { key: string; value: string; by: "you" | "agent"; updatedAt: number }[],
  sections: { id: string; title: string; body: string /* Markdown */; by: "you" | "agent"; updatedAt: number }[],
  updatedAt: number
}
```

`by` on a section is who last wrote it; an agent's `append` to your section leaves it as "you". Order in `sections` is the order shown.

The 0.1.x notes lived under `notes:<projectId>` as `{ text, updatedAt }`. On start, main moves each into a "Notes" section (split into "Notes (2)" and so on when longer than one section holds) and deletes the old key. That is the only migration; nothing reads `notes:` after it.

If what is stored under a project's key can't be read (damaged, or written by a newer version), the page says so and every write for that project is refused; it is never overwritten.

Only the plugin's main script writes storage. A page sends it each change as an operation and main applies it to what storage holds at that moment, one at a time, so neither side overwrites the other. If an agent appends to a section while you edit it, your save keeps the appended text after yours. Two pages editing the same section at once are last save wins. A page closed within 800 ms of your last keystroke may lose that text if Wings drops the frame before it sends. If main doesn't answer a save within 15 seconds the page shows an error, keeps your text, and sends it again on your next keystroke.

## Permissions

`openUrl: ["https://"]`, so a link in a section opens in your browser when you click it. Nothing else: the plugin only uses its own storage.
