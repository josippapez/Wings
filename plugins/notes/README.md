# Notes

Notes for each project that you write and Claude reads.

- **Sidebar.** The Notes button in the title bar shows the notes of the project on screen. Type, and it saves 800 ms after you stop, or when you click away.
- **Tab.** **Notes** in the + menu, or **Open in tab** in the sidebar, opens the same editor with more room.
- **Tools for Claude.** `notes__read` returns the project's notes. `notes__write` adds a timestamped section (`## 2026-10-13 14:05 (Claude)`) at the end and never changes or removes what is there. If you are typing while Claude adds a section, it lands at the end of your text.

## Which project a tool call is for

1. The project of the terminal Claude runs in.
2. Or, when Claude passes `project` (an absolute path to the project folder or a folder inside it), the project of an open terminal in that folder.

If neither finds a terminal, the tool returns an error. It never falls back to the project on screen, so a note can't land in the wrong project. A project with no open terminal can't be reached by `project`.

## Where notes live

In the plugin's `wings.storage`, one entry per project id, not in the repo. They stay out of git and are private to your machine. A repo file would travel with the project and let Claude read it without Wings, but a plugin can't write files (`exec` has no shell and no stdin). Notes are limited to 30,000 characters per project, and all of the plugin's storage to 1 MB.

Only the plugin's main script writes them. The pages send it their saves, and each save is applied after any section Claude added since the page last looked, so neither side overwrites the other. Two editors open on the same project (the sidebar and a tab) are last save wins. A page that closes within 800 ms of your last keystroke may lose that text if Wings drops the frame before it sends.

## Permissions

None. The plugin only uses its own storage.
