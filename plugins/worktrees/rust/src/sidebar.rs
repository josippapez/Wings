//! The Worktrees sidebar: the git worktrees of every project and of each repo a pane is in, what state each is in, and
//! removal after a confirm. Below them, on request, the repos elsewhere on this Mac that have worktrees (see
//! `elsewhere.rs`). It runs git from this page (see `git.rs`) and only offers what git itself would remove without
//! --force.

use crate::{
    elsewhere::{self, Saved, Scan, elsewhere_repos, find_repo_folders, repos_with_worktrees},
    git::{Outcome, Removal, Repo, load_repos, prune, remove_worktrees, rows_of},
    worktrees::{Row, Worktree, relative_path, shorten_path},
};
use serde_json::Value;
use std::{cell::RefCell, future::Future, pin::Pin, rc::Rc};
use wings_plugin::{
    format::{ago, basename, grouped, plural},
    git::{forget_default_branch, forget_repo_roots, on_change, repo_root},
    host,
    task::{join, join_all, spawn},
    ui::{Entry, Node, badge, button, checkbox, confirm, empty, entry, icon_button, icons, loading, muted, notice, page, row, rows_card, slot, spinner, stack, text},
};

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Remove,
    Bulk,
    Prune,
}

/// What the user is being asked to confirm: removing one worktree, removing the merged ones, or pruning the missing
/// ones.
#[derive(Clone)]
struct Ask {
    kind: Kind,
    root: String,
    paths: Vec<String>,
}

/// What a running scan is doing: looking at the files and folders on the disks, then asking git about the repos it
/// found.
enum Scanning {
    Finding(u64),
    Checking(usize),
}

#[derive(Default)]
struct Page {
    /// None before the first load.
    repos: Option<Vec<Repo>>,
    errors: Vec<String>,
    /// What the last scan found, loaded with git like the repos above: when it finished, and what went wrong loading
    /// them.
    others: Vec<Repo>,
    other_errors: Vec<String>,
    scanned_at: f64,
    scanning: Option<Scanning>,
    scan_error: Option<String>,
    scan: Option<Rc<Scan>>,
    loading: bool,
    /// The folders of the panes, and of every project, open in a pane or not. Only pane folders count as "open in a
    /// pane".
    cwds: Vec<String>,
    projects: Vec<String>,
    /// The repos the projects and panes are in. None until the first look, so a first look that finds no repo still
    /// loads (and shows) the empty state.
    known: Option<Vec<String>>,
    /// Goes up as a refresh starts: one that finds a later number here drops what it found.
    refreshes: u64,
    asked: Option<Ask>,
    /// Why what was confirmed couldn't be done. The confirm stays open and shows it.
    refused: Option<String>,
    delete_branch: bool,
    /// The confirmed work that is running, as `kind:root`.
    busy: Option<String>,
    /// What the last action did, each with its tone.
    notes: Vec<(&'static str, String)>,
}

thread_local! {
    static PAGE: RefCell<Page> = RefCell::new(Page::default());
}

fn with<T>(change: impl FnOnce(&mut Page) -> T) -> T {
    PAGE.with(|page| change(&mut page.borrow_mut()))
}

async fn refresh() {
    let (mine, folders) = with(|page| {
        page.refreshes += 1;
        page.loading = true;
        (page.refreshes, [page.cwds.clone(), page.projects.clone()].concat())
    });
    let saved = elsewhere::load();
    // The repos a scan found may be someone else's: only the git commands that run none of a repo's own programs run in them.
    let ((repos, errors), (others, other_errors)) = join(load_repos(folders, true), load_repos(saved.folders, false)).await;
    with(|page| {
        if page.refreshes != mine {
            return;
        }
        (page.repos, page.errors, page.others, page.other_errors) = (Some(repos), errors, others, other_errors);
        page.scanned_at = saved.at;
        page.loading = false;
    });
}

/// Only a change in which repos the projects and panes are in reloads. A pane moving inside one repo just draws "open
/// in a pane" again.
fn look() {
    spawn(async {
        let folders = with(|page| [page.cwds.clone(), page.projects.clone()].concat());
        let asked: Vec<Pin<Box<dyn Future<Output = Option<String>>>>> =
            folders.into_iter().map(|folder| -> Pin<Box<dyn Future<Output = Option<String>>>> { Box::pin(async move { repo_root(&folder).await }) }).collect();
        let mut roots: Vec<String> = join_all(asked).await.into_iter().flatten().collect();
        roots.sort();
        roots.dedup();
        if with(|page| page.known.replace(roots.clone()) == Some(roots)) {
            return;
        }
        refresh().await;
    });
}

/// The folders under `name` in a list the app sent, each once, sorted.
fn folders(list: &[Value], name: &str) -> Vec<String> {
    let mut out: Vec<String> = list.iter().filter_map(|item| item[name].as_str()).filter(|path| !path.is_empty()).map(str::to_owned).collect();
    out.sort();
    out.dedup();
    out
}

fn ask(asked: Ask) {
    with(|page| {
        page.notes.clear();
        page.delete_branch = false;
        page.refused = None;
        page.asked = Some(asked);
    });
}

fn summarize(outcome: &Outcome) -> Vec<(&'static str, String)> {
    let removed = (!outcome.removed.is_empty()).then(|| ("success", format!("Removed {}.", plural(outcome.removed.len(), "worktree"))));
    removed
        .into_iter()
        .chain(outcome.failed.iter().map(|(path, error)| ("danger", format!("{}: {error}", shorten_path(path)))))
        .chain(outcome.branch_errors.iter().map(|(branch, error)| ("danger", format!("Branch {branch} kept: {error}"))))
        .collect()
}

/// Runs what the confirm asked for. When nothing could be done the confirm stays open and shows why; otherwise it
/// closes and the notes show.
fn do_confirm(checked: bool) {
    let Some((asked, delete_branch)) = with(|page| {
        let asked = page.asked.clone()?;
        let kind = ["remove", "bulk", "prune"][asked.kind as usize];
        page.busy = Some(format!("{kind}:{}", asked.root));
        page.notes.clear();
        page.refused = None;
        Some((asked, page.delete_branch))
    }) else {
        return;
    };
    spawn(async move {
        let done = if asked.kind == Kind::Prune {
            prune(&asked.root).await.map(|said| vec![("success", if said.is_empty() { "Nothing to prune.".to_owned() } else { said })])
        } else {
            let opts = Removal { delete_branch, only_merged: asked.kind == Kind::Bulk, check: checked };
            remove_worktrees(&asked.root, &asked.paths, opts, || with(|page| page.cwds.clone())).await.and_then(|outcome| {
                let notes = summarize(&outcome);
                // Nothing was removed: the confirm stays so you can read why, and try again or cancel.
                if outcome.removed.is_empty() && !notes.is_empty() { Err(notes.into_iter().map(|(_, text)| text).collect::<Vec<_>>().join("\n")) } else { Ok(notes) }
            })
        };
        with(|page| {
            match done {
                Ok(notes) => (page.notes, page.asked) = (notes, None),
                Err(error) => page.refused = Some(error),
            }
            page.busy = None;
        });
        refresh().await;
    });
}

/// Looks for repos on every disk, keeps those with worktrees, and loads them. Stopping it keeps what the last scan
/// found.
fn scan_mac() {
    let scan = Rc::new(Scan::default());
    with(|page| {
        page.scan = Some(scan.clone());
        page.scan_error = None;
        page.scanning = Some(Scanning::Finding(0));
    });
    spawn(async move {
        let scanned = async {
            let folders = find_repo_folders(&scan, |count| with(|page| page.scanning = Some(Scanning::Finding(count)))).await?;
            with(|page| page.scanning = Some(Scanning::Checking(folders.len())));
            elsewhere::save(&Saved { at: host::now(), folders: repos_with_worktrees(folders, &scan).await? })?;
            refresh().await;
            Ok::<(), String>(())
        }
        .await;
        with(|page| {
            if let Err(error) = scanned
                && !scan.stopped()
            {
                page.scan_error = Some(error);
            }
            page.scan = None;
            page.scanning = None;
        });
    });
}

/// A repo's own settings and hooks can make git run programs, and `git worktree remove` (which looks for changes
/// first) and `git branch -d` do. That is why removing is a warning in a repo that isn't one of your projects.
const OUTSIDE_WARNING: &str = "This repo is outside your projects. Removing a worktree, and its branch if you tick that, runs git in it, and a repo's own settings and hooks can make git run programs. Only remove worktrees of repos you trust.";

/// The card that asks before anything is removed: what git will do, the branches to delete with it, and the two
/// buttons. `branch` is the branch of the one worktree being removed, which can be deleted with it.
fn confirm_card(p: &Page, asked: &Ask, repo: &Repo, branch: Option<&str>, working: bool) -> Node {
    let Ask { kind, paths, .. } = asked;
    let title = match kind {
        Kind::Prune => format!("Forget {} whose folder is gone?", plural(paths.len(), "worktree record")),
        Kind::Bulk => format!("Remove {}?", plural(paths.len(), "worktree")),
        Kind::Remove => format!("Remove {}?", shorten_path(&paths[0])),
    };
    let branches = match (kind, branch) {
        (Kind::Bulk, _) => Some(if paths.len() == 1 { "Also delete its branch" } else { "Also delete their branches" }.to_owned()),
        (Kind::Remove, Some(branch)) => Some(format!("Also delete branch {branch}")),
        _ => None,
    };
    let body = match &p.refused {
        Some(error) => vec![notice("danger", error)],
        None => [
            (*kind == Kind::Remove && !repo.checked).then(|| notice("warn", OUTSIDE_WARNING)),
            (*kind != Kind::Remove).then(|| stack().prop("gap", "xs").children(paths.iter().map(|path| text(shorten_path(path)).key(path).prop("mono", true).prop("size", "small")))),
            branches.map(|label| checkbox(label, p.delete_branch, |checked| with(|page| page.delete_branch = checked))),
        ]
        .into_iter()
        .flatten()
        .collect(),
    };
    let checked = repo.checked;
    confirm(
        title,
        body,
        [
            button(if *kind == Kind::Prune { "Prune" } else { "Remove" }, "destructive", false, move || do_confirm(checked)).disabled(working),
            button("Cancel", "outline", false, || with(|page| page.asked = None)).disabled(working),
        ],
    )
}

/// A small secondary button at the end of a section's label.
fn small_button(label: impl Into<String>, disabled: bool, on_click: impl Fn() + 'static) -> Node {
    button(label, "secondary", false, on_click).size("sm").disabled(disabled)
}

/// A section's label: its name, its context muted after it, and `actions` at the end, which wrap under the label when
/// there is no room for both.
fn section_label(name: &str, context: Option<String>, actions: Vec<Node>) -> Node {
    let title = row()
        .prop("gap", "m")
        .prop("align", "center")
        .child(text(name).prop("weight", "strong"))
        .maybe(context.map(|context| muted(&context).prop("size", "small").prop("truncate", true).prop("title", context)));
    row().prop("gap", "m").prop("align", "center").prop("justify", "between").prop("wrap", true).child(title).maybe((!actions.is_empty()).then(|| row().prop("gap", "s").prop("wrap", true).children(actions)))
}

/// What a worktree's branch is called on its row.
fn branch_name(wt: &Worktree) -> String {
    match (&wt.branch, &wt.head) {
        (Some(branch), _) => branch.clone(),
        _ if wt.bare => "(bare)".into(),
        (None, Some(head)) => format!("detached {}", head.chars().take(7).collect::<String>()),
        _ => "(no branch)".into(),
    }
}

/// One worktree as a row of its repo's card: the branch with its state badges after it, the folder muted and the time
/// of its last commit at the end, and the actions as ghost icon buttons. Without `can_open` there is no "Open in new
/// pane". What stops a removal is the disabled button's tooltip. `confirm` is the card asking to remove it, which
/// follows the row.
fn worktree_item(root: &str, r: &Row, working: bool, confirm: Option<Node>, can_open: bool) -> Vec<Node> {
    let wt = &r.wt;
    let flags = [
        (wt.main, "main", Some("info")),
        (r.missing, "folder missing", Some("danger")),
        (r.merged, "merged", Some("success")),
        (r.at_default, "no commits of its own", None),
        (r.gone, "upstream gone", Some("warn")),
        (r.dirty == Some(true), "uncommitted", Some("warn")),
        (r.unchecked, "changes not checked", None),
        (r.locked, "locked", Some("warn")),
        (r.in_pane, "open in a pane", Some("info")),
    ];
    let flags = flags.into_iter().filter(|flag| flag.0).map(|(_, label, tone)| badge(label, tone).key(label));
    let title = row().prop("gap", "s").prop("align", "center").prop("wrap", true).child(text(branch_name(wt)).prop("weight", "medium").prop("truncate", true)).children(flags);
    let path = relative_path(root, &wt.path);
    // git prints seconds.
    let when = r.last_commit.map(|seconds| muted(ago(seconds * 1000.0, host::now())).prop("size", "small"));
    let detail = muted(&path).prop("size", "small").prop("truncate", true).prop("title", shorten_path(&wt.path));
    let open = (!r.missing && can_open).then(|| {
        let path = wt.path.clone();
        icon_button("Open in new pane", &icons::SQUARE_TERMINAL, move || {
            let opened = host::open_pane(&path);
            spawn(async move {
                if let Err(error) = opened.await {
                    with(|page| page.notes = vec![("danger", error)]);
                }
            });
        })
    });
    let asked = Ask { kind: Kind::Remove, root: root.to_owned(), paths: vec![wt.path.clone()] };
    let remove = icon_button("Remove", &icons::TRASH_2, move || ask(asked.clone())).tooltip(r.why.unwrap_or("Remove worktree")).disabled(!r.removable || working);
    let actions = row().prop("gap", "xs").prop("align", "center").maybe(when).maybe(open).child(remove);
    let item = entry(Entry { lead: None, title, detail: Some(detail), trailing: Some(actions) }).key(&wt.path);
    let confirm = confirm.map(|confirm| entry(Entry { lead: None, title: confirm, detail: None, trailing: None }).key(format!("{}:confirm", wt.path)));
    [Some(item), confirm].into_iter().flatten().collect()
}

/// One repo: its name as a section label with its folder and default branch muted after it, the small buttons that
/// act on several worktrees at once at its end, and one card of a row for each worktree. Whatever is being confirmed
/// in the repo shows here too, under the label or under its worktree. A repo outside your projects gets no "Open in
/// new pane": Wings opens panes only in your projects.
fn repo_section(p: &Page, repo: &Repo, can_open: bool) -> Node {
    let rows = rows_of(repo, &p.cwds);
    let asked = p.asked.as_ref().filter(|asked| asked.root == repo.root);
    let working = p.busy.as_ref().is_some_and(|busy| busy.ends_with(&format!(":{}", repo.root)));
    let several = |kind: Kind, label: String, paths: Vec<String>| {
        let asked = Ask { kind, root: repo.root.clone(), paths };
        small_button(label, working, move || ask(asked.clone()))
    };
    let paths = |rows: Vec<&Row>| rows.into_iter().map(|row| row.wt.path.clone()).collect::<Vec<_>>();
    let bulk = paths(rows.iter().filter(|row| row.bulk).collect());
    let missing = paths(rows.iter().filter(|row| row.missing && !row.locked).collect());
    let default = repo.facts.default_branch.as_ref().map(|branch| format!(" · default {branch}")).unwrap_or_default();
    let buttons = if rows.len() > 1 {
        [
            (!missing.is_empty()).then(|| several(Kind::Prune, format!("Prune {} missing", missing.len()), missing.clone())),
            (!bulk.is_empty()).then(|| several(Kind::Bulk, format!("Remove {} merged & clean", bulk.len()), bulk.clone())),
        ]
        .into_iter()
        .flatten()
        .collect()
    } else {
        Vec::new()
    };
    stack()
        .key(&repo.root)
        .prop("gap", "s")
        .child(section_label(&basename(&repo.root), Some(format!("{}{default}", shorten_path(&repo.root))), buttons))
        .maybe(asked.filter(|asked| asked.kind != Kind::Remove).map(|asked| confirm_card(p, asked, repo, None, working)))
        .child(rows_card(rows.iter().flat_map(|row| {
            let removing = asked.filter(|asked| asked.kind == Kind::Remove && asked.paths[0] == row.wt.path);
            worktree_item(&repo.root, row, working, removing.map(|asked| confirm_card(p, asked, repo, row.wt.branch.as_deref(), working)), can_open)
        })))
}

/// The "Elsewhere on this Mac" section: its label with the button that scans the disks for repos with worktrees at its
/// end, what a scan is doing or found, and the repos it found.
fn elsewhere_section(p: &Page, found: &[&Repo]) -> Node {
    let at = p.scanned_at;
    let note = match &p.scanning {
        Some(Scanning::Finding(count)) => format!("{} files looked at", grouped(*count)),
        Some(Scanning::Checking(count)) => format!("Checking {} for worktrees…", plural(*count, "repo")),
        None if at == 0.0 => "Finds repos with worktrees anywhere on this Mac, not just your projects. It reads folder names only, and macOS may ask for access to some folders.".into(),
        None if !found.is_empty() => format!("Scanned {} · {} with worktrees", ago(at, host::now()), plural(found.len(), "repo")),
        None => format!("Scanned {}. No repo outside your projects has more than one worktree.", ago(at, host::now())),
    };
    let note = muted(note).prop("size", "small");
    let stop = p.scan.clone().map(|scan| small_button("Stop", false, move || scan.stop()));
    let actions = [Some(small_button(if at == 0.0 { "Scan this Mac" } else { "Scan again" }, p.scanning.is_some(), scan_mac)), stop].into_iter().flatten().collect();
    stack()
        .key("elsewhere")
        .prop("gap", "s")
        .child(section_label("Elsewhere on this Mac", None, actions))
        .child(if p.scanning.is_some() { note.prop("numeric", true) } else { note })
        .maybe(p.scan_error.as_ref().map(|error| notice("danger", error)))
        .children(p.other_errors.iter().map(|error| notice("warn", error).key(error)))
        .children(found.iter().map(|repo| repo_section(p, repo, false)))
}

fn view() -> Vec<Node> {
    PAGE.with(|state| {
        let p = &*state.borrow();
        let Some(repos) = &p.repos else {
            return vec![loading()];
        };
        let refresh_button = icon_button("Refresh", &icons::ROTATE_CW, || {
            // The button only: focus and pane changes keep what was found.
            forget_repo_roots();
            forget_default_branch(None);
            spawn(refresh());
        });
        let actions = row().prop("gap", "xs").prop("align", "center").maybe(p.loading.then(|| spinner().prop("label", "Refreshing"))).child(refresh_button);
                let title = row().prop("gap", "m").prop("align", "center").child(text("Worktrees").prop("weight", "strong"));
        let header = Node::new("Toolbar").child(slot("title", title)).child(slot("actions", actions));
        let none = repos.is_empty().then(|| empty("None of your projects is a git repo.", "Add a project that is a git repo, or open a terminal in one, and its worktrees show here."));
        let body = p
            .notes
            .iter()
            .enumerate()
            .map(|(at, (tone, note))| notice(tone, note).key(format!("note{at}")))
            .chain(p.errors.iter().map(|error| notice("warn", error).key(error)))
            .chain(none)
            .chain(repos.iter().map(|repo| repo_section(p, repo, true)))
            .chain([elsewhere_section(p, &elsewhere_repos(&p.others, repos))]);
        vec![page(header, body.collect::<Vec<_>>())]
    })
}

#[cfg_attr(not(test), unsafe(no_mangle))]
pub fn start() {
    wings_plugin::run(view);
    host::on_panes(|list| {
        let next = folders(list, "cwd");
        if with(|page| std::mem::replace(&mut page.cwds, next.clone()) != next) {
            look();
        }
    });
    host::on_projects(|list| {
        let next = folders(list, "path");
        if with(|page| std::mem::replace(&mut page.projects, next.clone()) != next) {
            look();
        }
    });
    // Heard once with the state the page starts in, so it also loads the first time; then each time you come back to Wings.
    host::on_focus(|focused| {
        if focused {
            spawn(refresh());
        }
    });
    // A commit, checkout or pull in any of the repos shown, from anywhere. Dirty state isn't seen: it reloads on focus
    // and after actions.
    on_change(|_, roots| async move {
        if with(|page| page.known.as_ref().is_some_and(|known| roots.iter().any(|root| known.contains(root)))) {
            refresh().await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worktrees::{Facts, Probe, describe, parse_worktrees};
    use wings_plugin::ui::tree_json;

    fn buttons(value: &Value, out: &mut Vec<Value>) {
        match value {
            Value::Object(map) => {
                if map.get("type") == Some(&Value::from("Button")) {
                    out.push(value["props"].clone());
                }
                map.values().for_each(|child| buttons(child, out));
            }
            Value::Array(list) => list.iter().for_each(|child| buttons(child, out)),
            _ => {}
        }
    }

    fn drawn(can_open: bool) -> Vec<Vec<Value>> {
        let list = parse_worktrees(LIST);
        let probes = list.iter().map(|wt| (wt.path.clone(), Probe { dirty: Some(false), last_commit: Some(1000.0) })).collect();
        describe(&list, &Facts::default(), &probes, &[], false)
            .iter()
            .map(|row| {
                let mut found = Vec::new();
                worktree_item("/dev/app", row, false, None, can_open).into_iter().for_each(|node| buttons(&tree_json(node), &mut found));
                found
            })
            .collect()
    }

    const LIST: &str = "worktree /dev/app\nHEAD 1\nbranch refs/heads/main\n\nworktree /dev/app-a\nHEAD 2\nbranch refs/heads/a\n\nworktree /dev/app-b\nHEAD 3\nbranch refs/heads/b\nlocked\n";

    #[test]
    fn the_branch_tick_in_a_bulk_confirm_is_singular_for_one_worktree_and_leaves_git_out_of_its_words() {
        let repo = Repo { root: "/dev/app".into(), list: vec![], facts: Facts::default(), probes: Default::default(), checked: true };
        let ticks = |paths: Vec<String>| {
            let asked = Ask { kind: Kind::Bulk, root: repo.root.clone(), paths };
            tree_json(confirm_card(&Page::default(), &asked, &repo, None, false)).to_string()
        };
        let one = ticks(vec!["/dev/app-a".into()]);
        assert!(one.contains("Also delete its branch") && !one.contains("git branch"), "{one}");
        let two = ticks(vec!["/dev/app-a".into(), "/dev/app-b".into()]);
        assert!(two.contains("Also delete their branches") && !two.contains("git branch"), "{two}");
    }

    #[test]
    fn a_worktrees_actions_are_ghost_icon_buttons_with_a_tooltip_and_what_stops_a_removal_is_the_tooltip() {
        host::testing::set_now(2_000_000.0);
        let rows = drawn(true);
        for button in rows.iter().flatten() {
            assert_eq!((button["variant"].as_str(), button["size"].as_str()), (Some("ghost"), Some("icon-sm")));
            assert!(button["label"].is_string() && button["title"].is_string(), "{button}");
        }
        // The main worktree's Remove is disabled; a free one has an enabled one; a locked one is disabled and says why.
        assert_eq!((rows[0][1]["disabled"].as_bool(), rows[0][1]["title"].as_str()), (Some(true), Some("The main worktree stays.")));
        assert_eq!(rows[1][1]["label"], "Remove");
        assert!(rows[1][1]["disabled"].is_null());
        assert_eq!((rows[2][1]["disabled"].as_bool(), rows[2][1]["title"].as_str()), (Some(true), Some("Locked.")));
        // Outside your projects there is no "Open in new pane".
        assert!(drawn(false)[1].iter().all(|b| b["label"] == "Remove"));
    }
}
