//! The Worktrees sidebar: the git worktrees of every project and of each repo a pane is in, what state each is in, and
//! removal after a confirm. Below them, on request, the repos elsewhere on this Mac that have worktrees (see
//! `elsewhere.rs`). It runs git from this page (see `git.rs`) and only offers what git itself would remove without
//! --force.

use crate::{
    elsewhere::{self, Saved, Scan, elsewhere_repos, find_repo_folders, repos_with_worktrees},
    git::{Outcome, Removal, Repo, load_repos, prune, remove_worktrees, rows_of},
    worktrees::{Row, shorten_path},
};
use serde_json::Value;
use std::{cell::RefCell, future::Future, pin::Pin, rc::Rc};
use wings_plugin::{
    format::{ago, basename, grouped, plural},
    git::{forget_default_branch, forget_repo_roots, on_change, repo_root},
    host,
    task::{join, join_all, spawn},
    ui::{Node, badge, card, checkbox, confirm, empty, loading, muted, notice, page, row, spinner, stack, text, toolbar},
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

fn button(label: impl Into<String>, variant: &str, disabled: bool, on_click: impl Fn() + 'static) -> Node {
    let button = Node::new("Button").prop("variant", variant).on("onClick", move |_| on_click()).text(label);
    if disabled { button.prop("disabled", true) } else { button }
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
        (Kind::Bulk, _) => Some("Also delete their branches (git branch -d)".to_owned()),
        (Kind::Remove, Some(branch)) => Some(format!("Also delete branch {branch} (git branch -d)")),
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
            button(if *kind == Kind::Prune { "Prune" } else { "Remove" }, "destructive", working, move || do_confirm(checked)),
            button("Cancel", "ghost", working, || with(|page| page.asked = None)),
        ],
    )
}

/// One worktree: its branch, where it is, what state it is in, and what can be done with it. `confirm` is the card
/// asking to remove it, which takes the place of its buttons. Without `can_open` there is no "Open in new pane".
fn worktree_item(root: &str, r: &Row, working: bool, confirm: Option<Node>, can_open: bool) -> Node {
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
    let flags: Vec<Node> = flags.into_iter().filter(|flag| flag.0).map(|(_, label, tone)| badge(label, tone).key(label)).collect();
    let name = match (&wt.branch, &wt.head) {
        (Some(branch), _) => branch.clone(),
        _ if wt.bare => "(bare)".into(),
        (None, Some(head)) => format!("detached {}", head.chars().take(7).collect::<String>()),
        _ => "(no branch)".into(),
    };
    // git prints seconds.
    let when = r.last_commit.map(|seconds| muted(ago(seconds * 1000.0, host::now())).prop("size", "small"));
    let actions = confirm.unwrap_or_else(|| {
        let open = (!r.missing && can_open).then(|| {
            let path = wt.path.clone();
            button("Open in new pane", "ghost", false, move || {
                let opened = host::open_pane(&path);
                spawn(async move {
                    if let Err(error) = opened.await {
                        with(|page| page.notes = vec![("danger", error)]);
                    }
                });
            })
        });
        let remove = if r.removable {
            let asked = Ask { kind: Kind::Remove, root: root.to_owned(), paths: vec![wt.path.clone()] };
            button("Remove", "outline", working, move || ask(asked.clone()))
        } else {
            muted(r.why.unwrap_or_default()).prop("size", "tiny")
        };
        row().prop("gap", "s").prop("justify", "end").prop("wrap", true).maybe(open).child(remove)
    });
    card().key(&wt.path).child(
        stack()
            .prop("gap", "xs")
            .child(row().child(text(name).prop("weight", "strong").prop("truncate", true).prop("grow", true)).maybe(when))
            .child(muted(shorten_path(&wt.path)).prop("size", "small").prop("mono", true).prop("truncate", true))
            .maybe((!flags.is_empty()).then(|| row().prop("wrap", true).prop("gap", "xs").children(flags)))
            .child(actions),
    )
}

/// One repo: its name and path, the buttons that act on several worktrees at once, and a card for each worktree.
/// Whatever is being confirmed in the repo shows here too, under those buttons or in its worktree. A repo outside
/// your projects gets no "Open in new pane": Wings opens panes only in your projects.
fn repo_section(p: &Page, repo: &Repo, can_open: bool) -> Node {
    let rows = rows_of(repo, &p.cwds);
    let asked = p.asked.as_ref().filter(|asked| asked.root == repo.root);
    let working = p.busy.as_ref().is_some_and(|busy| busy.ends_with(&format!(":{}", repo.root)));
    let several = |kind: Kind, label: String, paths: Vec<String>| {
        let asked = Ask { kind, root: repo.root.clone(), paths };
        button(label, "outline", working, move || ask(asked.clone()))
    };
    let paths = |rows: Vec<&Row>| rows.into_iter().map(|row| row.wt.path.clone()).collect::<Vec<_>>();
    let bulk = paths(rows.iter().filter(|row| row.bulk).collect());
    let missing = paths(rows.iter().filter(|row| row.missing && !row.locked).collect());
    let default = repo.facts.default_branch.as_ref().map(|branch| format!(" · default {branch}")).unwrap_or_default();
    let buttons = (rows.len() > 1 && (!missing.is_empty() || !bulk.is_empty())).then(|| {
        row()
            .prop("wrap", true)
            .prop("gap", "s")
            .maybe((!missing.is_empty()).then(|| several(Kind::Prune, format!("Prune {} missing", missing.len()), missing.clone())))
            .maybe((!bulk.is_empty()).then(|| several(Kind::Bulk, format!("Remove {} merged & clean", bulk.len()), bulk.clone())))
    });
    stack()
        .key(&repo.root)
        .prop("gap", "s")
        .child(Node::new("Heading").text(basename(&repo.root)))
        .child(muted(format!("{}{default}", shorten_path(&repo.root))).prop("truncate", true))
        .maybe(buttons)
        .maybe(asked.filter(|asked| asked.kind != Kind::Remove).map(|asked| confirm_card(p, asked, repo, None, working)))
        .child(stack().prop("gap", "s").children(rows.iter().map(|row| {
            let removing = asked.filter(|asked| asked.kind == Kind::Remove && asked.paths[0] == row.wt.path);
            worktree_item(&repo.root, row, working, removing.map(|asked| confirm_card(p, asked, repo, row.wt.branch.as_deref(), working)), can_open)
        })))
}

/// The "Elsewhere on this Mac" section: the button that scans the disks for repos with worktrees, what a scan is
/// doing or found, and the repos it found.
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
    let stop = p.scan.clone().map(|scan| button("Stop", "outline", false, move || scan.stop()));
    stack()
        .key("elsewhere")
        .prop("gap", "s")
        .child(
            row()
                .prop("justify", "between")
                .prop("wrap", true)
                .child(Node::new("Heading").text("Elsewhere on this Mac"))
                .child(row().prop("gap", "xs").child(button(if at == 0.0 { "Scan this Mac" } else { "Scan again" }, "outline", p.scanning.is_some(), scan_mac)).maybe(stop)),
        )
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
        let removable: usize = repos.iter().map(|repo| rows_of(repo, &p.cwds).iter().filter(|row| row.bulk).count()).sum();
        let refresh_button = button("Refresh", "outline", false, || {
            // The button only: focus and pane changes keep what was found.
            forget_repo_roots();
            forget_default_branch(None);
            spawn(refresh());
        });
        let actions = row().prop("gap", "xs").maybe(p.loading.then(|| spinner().prop("label", "Refreshing"))).child(refresh_button);
        let summary = if removable > 0 { format!("{} merged and clean", plural(removable, "worktree")) } else { "Of your projects".into() };
        let none = repos.is_empty().then(|| empty("None of your projects is a git repo.", "Add a project that is a git repo, or open a terminal in one, and its worktrees show here."));
        let body = [muted(summary).prop("truncate", true)]
            .into_iter()
            .chain(p.notes.iter().enumerate().map(|(at, (tone, note))| notice(tone, note).key(format!("note{at}"))))
            .chain(p.errors.iter().map(|error| notice("warn", error).key(error)))
            .chain(none)
            .chain(repos.iter().map(|repo| repo_section(p, repo, true)))
            .chain([elsewhere_section(p, &elsewhere_repos(&p.others, repos))]);
        vec![page(toolbar("Worktrees", Some(actions)), body.collect::<Vec<_>>())]
    })
}

#[unsafe(no_mangle)]
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
