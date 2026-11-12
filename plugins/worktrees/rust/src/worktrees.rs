//! The logic of the Worktrees page that needs no git: reading `git worktree list --porcelain`, deciding what each
//! worktree is, and which ones may be removed.

use std::collections::{BTreeSet, HashMap};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Worktree {
    pub path: String,
    pub head: Option<String>,
    /// The short branch name, none when detached or bare.
    pub branch: Option<String>,
    pub bare: bool,
    pub detached: bool,
    /// The lock reason: empty when locked without one, none when not locked.
    pub locked: Option<String>,
    /// Why git would prune it (its folder is gone), none when it wouldn't.
    pub prunable: Option<String>,
    /// The first entry of the list: the repo's own checkout.
    pub main: bool,
}

/// What `git worktree list --porcelain` prints: blocks of `key value` lines separated by an empty line.
pub fn parse_worktrees(text: &str) -> Vec<Worktree> {
    let mut out: Vec<Worktree> = Vec::new();
    let mut wt = Worktree::default();
    for line in text.lines().map(|line| line.trim_end_matches('\r')).chain([""]) {
        if line.is_empty() {
            let done = std::mem::take(&mut wt);
            if !done.path.is_empty() {
                out.push(done);
            }
            continue;
        }
        let (key, value) = line.split_once(' ').unwrap_or((line, ""));
        match key {
            "worktree" => wt.path = value.to_owned(),
            "HEAD" => wt.head = Some(value.to_owned()),
            "branch" => wt.branch = Some(value.strip_prefix("refs/heads/").unwrap_or(value).to_owned()),
            "bare" => wt.bare = true,
            "detached" => wt.detached = true,
            "locked" => wt.locked = Some(value.to_owned()),
            "prunable" => wt.prunable = Some(value.to_owned()),
            _ => {}
        }
    }
    if let Some(first) = out.first_mut() {
        first.main = true;
    }
    out
}

/// Whether a repo has anything to manage besides its own checkout: a linked worktree, or a record whose folder is gone.
pub fn has_worktrees(list: &[Worktree]) -> bool {
    list.len() > 1 || list.iter().any(|wt| wt.prunable.is_some())
}

/// What `git status` and `git log` said about one worktree.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Probe {
    /// None when git couldn't say.
    pub dirty: Option<bool>,
    /// The newest commit's time in seconds, none when unknown.
    pub last_commit: Option<f64>,
}

/// What one repo says about its branches.
#[derive(Clone, Debug, Default)]
pub struct Facts {
    /// The default branch as a ref, like `origin/main`.
    pub default_branch: Option<String>,
    /// The commit the default branch is at.
    pub default_tip: Option<String>,
    /// Local branches merged into the default branch.
    pub merged: BTreeSet<String>,
    /// Local branches whose upstream is gone.
    pub gone: BTreeSet<String>,
}

#[derive(Clone, Debug)]
pub struct Row {
    pub wt: Worktree,
    pub missing: bool,
    /// The branch is merged into the default branch and has commits of its own on it.
    pub merged: bool,
    /// The branch sits exactly on the default branch's commit: new, or merged by fast-forward.
    pub at_default: bool,
    pub gone: bool,
    pub dirty: Option<bool>,
    /// It could be removed, but its repo is outside your projects, so `git status` wasn't run there and nothing says
    /// whether it has changes.
    pub unchecked: bool,
    pub locked: bool,
    pub in_pane: bool,
    pub last_commit: Option<f64>,
    /// `git worktree remove` (without --force) can be offered.
    pub removable: bool,
    /// Why not.
    pub why: Option<&'static str>,
    /// Part of "Remove all merged & clean".
    pub bulk: bool,
}

fn trim(path: &str) -> &str {
    if path.len() > 1 { path.trim_end_matches('/') } else { path }
}

/// Whether `path` is `root` or inside it.
pub fn contains(root: &str, path: &str) -> bool {
    let (root, path) = (trim(root), trim(path));
    path == root || if root == "/" { path.starts_with('/') } else { path.strip_prefix(root).is_some_and(|rest| rest.starts_with('/')) }
}

/// Decides what each worktree is. A pane belongs to the deepest worktree that holds its folder. `skipped` is for a
/// repo outside your projects, where `git status` and `git log` weren't run because a repo's own settings can make
/// them run programs: whether a worktree has changes isn't known then, which doesn't stop it from being removed one at
/// a time (git itself refuses one that has changes), but keeps it out of "Remove all merged & clean".
pub fn describe(list: &[Worktree], facts: &Facts, probes: &HashMap<String, Probe>, pane_cwds: &[String], skipped: bool) -> Vec<Row> {
    let mut open = BTreeSet::new();
    for cwd in pane_cwds {
        let mut best: Option<&Worktree> = None;
        for wt in list {
            if contains(&wt.path, cwd) && best.is_none_or(|best| trim(&wt.path).len() > trim(&best.path).len()) {
                best = Some(wt);
            }
        }
        if let Some(best) = best {
            open.insert(best.path.as_str());
        }
    }
    // The default branch's own name, `main` for `origin/main`.
    let default_name = facts.default_branch.as_deref().map(|name| name.strip_prefix("origin/").unwrap_or(name));
    list.iter()
        .map(|wt| {
            let probe = probes.get(&wt.path);
            let missing = wt.prunable.is_some();
            let in_default = !wt.main && wt.branch.as_deref().is_some_and(|branch| Some(branch) != default_name && facts.merged.contains(branch));
            let at_default = in_default && wt.head.is_some() && wt.head == facts.default_tip;
            let merged = in_default && !at_default;
            let dirty = if missing || wt.bare {
                Some(false)
            } else if skipped {
                None
            } else {
                probe.and_then(|probe| probe.dirty)
            };
            let locked = wt.locked.is_some();
            let in_pane = open.contains(wt.path.as_str());
            let why = if wt.main {
                Some("The main worktree stays.")
            } else if locked {
                Some("Locked.")
            } else if missing {
                Some("Its folder is gone: prune it.")
            } else if in_pane {
                Some("Open in a pane.")
            } else if dirty.is_none() && !skipped {
                Some("Couldn't read its status.")
            } else if dirty == Some(true) {
                Some("Has uncommitted changes.")
            } else {
                None
            };
            let removable = why.is_none();
            Row {
                wt: wt.clone(),
                missing,
                merged,
                at_default,
                gone: wt.branch.as_ref().is_some_and(|branch| facts.gone.contains(branch)),
                dirty,
                unchecked: removable && skipped,
                locked,
                in_pane,
                last_commit: probe.and_then(|probe| probe.last_commit),
                removable,
                why,
                bulk: removable && merged && !skipped,
            }
        })
        .collect()
}

/// `/Users/me/dev/app` as `~/dev/app`.
pub fn shorten_path(path: &str) -> String {
    for home in ["/Users/", "/home/"] {
        if let Some(rest) = path.strip_prefix(home)
            && !rest.is_empty()
            && !rest.starts_with('/')
        {
            return format!("~{}", rest.find('/').map_or("", |at| &rest[at..]));
        }
    }
    match path.strip_prefix("/root") {
        Some(rest) if rest.is_empty() || rest.starts_with('/') => format!("~{rest}"),
        _ => path.to_owned(),
    }
}

/// The lines of `git branch --format=%(refname:short)%09%(upstream:track)` whose upstream is gone.
pub fn gone_branches(text: &str) -> BTreeSet<String> {
    text.lines()
        .filter_map(|line| line.split_once('\t'))
        .filter(|(name, track)| !name.is_empty() && track.split('\t').next().unwrap_or_default().trim() == "[gone]")
        .map(|(name, _)| name.trim().to_owned())
        .collect()
}

/// One branch name per line.
pub fn lines(text: &str) -> BTreeSet<String> {
    text.lines().map(str::trim).filter(|line| !line.is_empty()).map(str::to_owned).collect()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub const PORCELAIN: &str = "worktree /Users/me/dev/app
HEAD 1111111111111111111111111111111111111111
branch refs/heads/main

worktree /Users/me/dev/app-feature
HEAD 2222222222222222222222222222222222222222
branch refs/heads/feature/x

worktree /Users/me/dev/app-detached
HEAD 3333333333333333333333333333333333333333
detached

worktree /Users/me/dev/app-gone
HEAD 4444444444444444444444444444444444444444
branch refs/heads/old
locked testing the lock
prunable gitdir file points to non-existent location

worktree /Users/me/dev/has space
HEAD 5555555555555555555555555555555555555555
branch refs/heads/spaced
locked
";

    #[test]
    fn parses_git_worktree_list_porcelain_and_the_first_entry_is_the_main_worktree() {
        let list = parse_worktrees(PORCELAIN);
        assert_eq!(list.iter().map(|w| w.main).collect::<Vec<_>>(), [true, false, false, false, false]);
        assert_eq!(list[1].branch.as_deref(), Some("feature/x"));
        assert!(list[2].detached);
        assert_eq!(list[2].branch, None);
        assert_eq!(list[3].locked.as_deref(), Some("testing the lock"));
        assert_eq!(list[3].prunable.as_deref(), Some("gitdir file points to non-existent location"));
        assert_eq!(list[4].path, "/Users/me/dev/has space");
        assert_eq!(list[4].locked.as_deref(), Some(""));
        assert!(parse_worktrees("").is_empty());
        assert_eq!(parse_worktrees(&PORCELAIN.replace('\n', "\r\n")), list);
    }

    #[test]
    fn a_bare_main_worktree_is_read() {
        let list = parse_worktrees("worktree /srv/repo.git\nbare\n");
        assert!(list[0].bare && list[0].main);
    }

    fn facts(tip: char) -> Facts {
        Facts {
            default_branch: Some("origin/main".into()),
            default_tip: Some(tip.to_string().repeat(40)),
            merged: ["main", "feature/x", "old"].map(String::from).into(),
            gone: ["feature/x".to_owned()].into(),
        }
    }
    const CLEAN: Probe = Probe { dirty: Some(false), last_commit: Some(1000.0) };
    fn probes(over: &[(&str, Probe)]) -> HashMap<String, Probe> {
        parse_worktrees(PORCELAIN).into_iter().map(|w| (w.path, CLEAN)).chain(over.iter().map(|(path, probe)| ((*path).to_owned(), *probe))).collect()
    }
    fn by_branch<'a>(rows: &'a [Row], branch: &str) -> &'a Row {
        rows.iter().find(|row| row.wt.branch.as_deref() == Some(branch)).unwrap()
    }

    #[test]
    fn a_merged_clean_closed_linked_worktree_is_removable_and_part_of_the_bulk_set() {
        let rows = describe(&parse_worktrees(PORCELAIN), &facts('1'), &probes(&[]), &[], false);
        let r = by_branch(&rows, "feature/x");
        assert!(r.merged && r.gone && r.removable && r.bulk);
        assert_eq!(r.why, None);
    }

    #[test]
    fn main_locked_missing_dirty_unknown_and_open_worktrees_are_never_removable() {
        let list = parse_worktrees(PORCELAIN);
        let feature = "/Users/me/dev/app-feature";
        let rows = describe(&list, &facts('1'), &probes(&[(feature, Probe { dirty: Some(true), last_commit: Some(1.0) })]), &[], false);
        assert!(!by_branch(&rows, "main").removable);
        assert_eq!(by_branch(&rows, "main").why, Some("The main worktree stays."));
        let dirty = by_branch(&rows, "feature/x");
        assert_eq!((dirty.removable, dirty.dirty, dirty.bulk), (false, Some(true), false));
        let old = by_branch(&rows, "old");
        assert!(old.missing && old.locked && !old.removable);
        assert!(!by_branch(&rows, "spaced").removable);

        let unknown = describe(&list, &facts('1'), &probes(&[(feature, Probe { dirty: None, last_commit: None })]), &[], false);
        assert!(!by_branch(&unknown, "feature/x").removable);

        let open = describe(&list, &facts('1'), &probes(&[]), &[format!("{feature}/src")], false);
        assert!(by_branch(&open, "feature/x").in_pane && !by_branch(&open, "feature/x").removable);
    }

    #[test]
    fn a_missing_folder_that_is_not_locked_is_prunable_not_removable() {
        let list = parse_worktrees("worktree /r\nbranch refs/heads/main\n\nworktree /r-x\nbranch refs/heads/x\nprunable gone\n");
        let rows = describe(&list, &facts('1'), &HashMap::new(), &[], false);
        assert!(rows[1].missing && !rows[1].removable);
    }

    #[test]
    fn a_branch_sitting_on_the_default_branchs_commit_is_not_called_merged() {
        let rows = describe(&parse_worktrees(PORCELAIN), &facts('2'), &probes(&[]), &[], false);
        let r = by_branch(&rows, "feature/x");
        assert_eq!((r.at_default, r.merged, r.removable, r.bulk), (true, false, true, false));
    }

    #[test]
    fn a_detached_head_is_never_merged() {
        let rows = describe(&parse_worktrees(PORCELAIN), &facts('1'), &probes(&[]), &[], false);
        let r = rows.iter().find(|row| row.wt.detached).unwrap();
        assert_eq!((r.merged, r.removable, r.bulk), (false, true, false));
    }

    #[test]
    fn a_pane_belongs_to_the_deepest_worktree_holding_its_folder() {
        let list = parse_worktrees("worktree /r\nbranch refs/heads/main\n\nworktree /r/.wt/a\nbranch refs/heads/a\n");
        let rows = describe(&list, &facts('1'), &HashMap::from([("/r/.wt/a".to_owned(), CLEAN)]), &["/r/.wt/a/src".to_owned()], false);
        assert_eq!((rows[0].in_pane, rows[1].in_pane), (false, true));
        assert!(!contains("/r", "/rx"));
        assert!(contains("/r/", "/r"));
        assert!(contains("/", "/anything"));
    }

    #[test]
    fn paths_are_shortened_with_a_tilde_and_gone_upstreams_are_found() {
        assert_eq!(shorten_path("/Users/me/dev/app"), "~/dev/app");
        assert_eq!(shorten_path("/home/me"), "~");
        assert_eq!(shorten_path("/Users/mex"), "~");
        assert_eq!(shorten_path("/root/x"), "~/x");
        assert_eq!(shorten_path("/rooted"), "/rooted");
        assert_eq!(shorten_path("/srv/app"), "/srv/app");
        assert_eq!(gone_branches("a\t[gone]\nb\t[ahead 1]\nc\t\nd\t[gone]\n").into_iter().collect::<Vec<_>>(), ["a", "d"]);
    }

    #[test]
    fn a_repo_has_worktrees_to_manage_when_it_has_another_checkout_or_a_record_whose_folder_is_gone() {
        assert!(!has_worktrees(&parse_worktrees("worktree /r\nHEAD 1\nbranch refs/heads/main\n")));
        assert!(has_worktrees(&parse_worktrees("worktree /r\nHEAD 1\n\nworktree /r-x\nHEAD 2\n")));
        assert!(has_worktrees(&parse_worktrees("worktree /r\nHEAD 1\nprunable gitdir file points to non-existent location\n")));
        assert!(!has_worktrees(&[]));
    }

    #[test]
    fn outside_your_projects_a_worktree_can_be_removed_one_at_a_time_never_as_part_of_the_merged_and_clean() {
        let rows = describe(&parse_worktrees(PORCELAIN), &facts('1'), &HashMap::new(), &[], true);
        let r = by_branch(&rows, "feature/x");
        assert_eq!((r.dirty, r.unchecked, r.removable, r.merged, r.bulk), (None, true, true, true, false));
        // What stops a worktree anywhere still stops it, and those aren't called unchecked.
        assert_eq!(rows.iter().map(|row| (row.removable, row.unchecked)).collect::<Vec<_>>(), [(false, false), (true, true), (true, true), (false, false), (false, false)]);
        assert!(rows.iter().all(|row| !row.bulk));
    }
}
