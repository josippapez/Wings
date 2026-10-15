# Changes

Shows what changed in the git repo of each pane, and opens the diffs in Wings' diff viewer.

- **Badge.** A pane in a repo with changes shows the changed lines, like `+120 −8`, and the number of changed files. Click it for **Open changes** (everything not committed yet, staged and untracked files included, against `HEAD`), **Changes on this branch** (everything since the branch left the default branch, uncommitted work included), **Show in sidebar** and **Refresh**. A clean repo has no badge.
- **Sidebar.** The Changes button in the title bar lists the changed files of the focused pane's repo as a folder tree, with each file's status (`M` modified, `A` added, `D` deleted, `R` renamed, `?` not tracked yet) and its added and removed lines. Switch between **Uncommitted** and **This branch**, click a file to see its diff, or **View all**. It follows the focused pane, checks again on a timer while Wings is in front, and has a Refresh button.
- **History.** The sidebar's History tab lists the commits of the current branch, newest first, a page at a time with **Load more**: subject, short hash, author, how long ago, and pills for `HEAD`, branches and tags. Search matches the message, the author or the start of a hash, ignoring case. **All branches** lists every branch's commits. Click a commit to see what it changed; a merge shows what it brought in, against its first parent. The list checks for new commits on the sidebar's timer.
- **Commit graph.** **Graph** in the History tab, or the Commit graph button in the title bar, opens every branch's commits in a window with their lanes, like `git log --graph --all`. A very long history is cut to its newest commits, and the header says how many it shows. Click a row to see its diff, or move between rows with the arrow keys. It only draws the rows on screen, so a long history scrolls smoothly. Refresh reads it again.
- **Tools for Claude.** `changes__changed_files` lists the changed files of the repo containing a folder, uncommitted or on the branch. `changes__recent_commits` lists the newest commits on its current branch. Both only read, and both check what Claude passes against the schema they declare, so a `limit` out of range is an error that names the field, not a guess. A line in the plugin's `mcpInstructions` tells Claude at session start to use them for what changed and for the latest commits, instead of running `git status` or `git log`.

The default branch is `origin/HEAD` when the remote has set it, else `origin/main`, `origin/master`, `main` or `master`. Run `git remote set-head origin --auto` once if yours isn't found.

## Limits

- Files that git doesn't track yet are read with `git grep`, since Wings doesn't let plugins run `git diff --no-index`. Only the first batch of them is shown with its contents, and the viewer says how many were left out. Binary files show as binary, and a missing newline at the end of a new file isn't marked.
- **View all** stops at a size the viewer can draw quickly, and says so. Open single files from the sidebar instead. A commit's diff stops at the same size.
- The sidebar lists as many files as fit in one message to it, and says when there are more.
- The graph shows only the newest commits of a very long history, and a repo with very many branches side by side has its outermost lanes cut off.

## Permissions

Only local, read-only `git` commands: `rev-parse`, `branch --show-current`, `merge-base`, `diff`, `ls-files`, `grep`, `log` and `show`, each but `merge-base` limited to the flags it uses. Nothing touches the network or changes the repo.
