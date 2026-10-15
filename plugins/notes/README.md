# Project info (retired)

Project info moved into Wings as each project's Project page. Open it from a project in the sidebar, the + menu or the command palette. Agents read and write it with Wings' own `project_info_*` tools, so no plugin is needed.

The first time a project's page opens, Wings imports what this plugin stored for that project (`info:<projectId>`, `{ v: 1, facts, sections }`, and older `notes:<projectId>` text). The plugin has no tools and only points to the page, so you can turn it off.
