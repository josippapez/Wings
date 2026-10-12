# Changes

Shows what changed in the git repo of each pane, and opens the diffs in Wings' diff viewer.

- **Badge.** A pane in a repo with changes shows the changed lines, like `+120 −8`, and the number of changed files. Click it for **Open changes** (everything not committed yet, staged and untracked files included, against `HEAD`), **Changes on this branch** (everything since the branch left the default branch, uncommitted work included), **Show in sidebar** and **Refresh**. A clean repo has no badge.
- **Sidebar.** The Changes button in the title bar lists the changed files of the focused pane's repo as a folder tree, with each file's status (`M` modified, `A` added, `D` deleted, `R` renamed, `?` not tracked yet) and its added and removed lines. Switch between **Uncommitted** and **This branch**, click a file to see its diff, or **View all**. It refreshes when you focus another pane, every 15 seconds while it's open, and from its Refresh button.
- **History.** The sidebar's History tab lists the commits of the current branch, newest first, 50 at a time with **Load more**: subject, short hash, author, how long ago, and pills for `HEAD`, branches and tags. Search matches the message, the author or the start of a hash, ignoring case. **All branches** lists every branch's commits. Click a commit to see what it changed; a merge shows what it brought in, against its first parent. The list checks for new commits every 15 seconds while it's open.
- **Commit graph.** **Graph** in the History tab, or the Commit graph button in the title bar, opens every branch's commits in a window with their lanes, like `git log --graph --all`, up to the newest 10,000. Click a row to see its diff, or move between rows with the arrow keys. It only draws the rows on screen, so a long history scrolls smoothly. Refresh reads it again.
- **Tools for Claude.** `changes__changed_files` lists the changed files of the repo containing a folder, uncommitted or on the branch. `changes__recent_commits` lists the newest commits on its current branch, up to 200. Both only read.

The default branch is `origin/HEAD` when the remote has set it, else `origin/main`, `origin/master`, `main` or `master`. Run `git remote set-head origin --auto` once if yours isn't found.

## Limits

- Files that git doesn't track yet are read with `git grep`, since Wings doesn't let plugins run `git diff --no-index`. Up to 200 of them are shown with their contents, binary files show as binary, and a missing newline at the end of a new file isn't marked.
- **View all** stops at 1,500 files or 50,000 changed lines (about 2 MB) and says so. Open single files from the sidebar instead. A commit's diff stops at the same size.
- The sidebar lists as many files as fit in one 64 KB message to it, and says when there are more.
- The graph shows the newest 10,000 commits, and lanes past the 24th are cut off.

## Permissions

Only local, read-only `git` commands: `rev-parse`, `branch --show-current`, `merge-base`, `diff`, `ls-files`, `grep`, `log` and `show`, each but `merge-base` limited to the flags it uses. Nothing touches the network or changes the repo.
