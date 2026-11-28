//! The Changes sidebar: the changed files of the focused pane's repo as a folder tree, and its commit history. It
//! runs git itself and keeps what it shows fresh. A click asks the main script to open the diff viewer.

use crate::{
    COMMIT, DONE, GRAPH, OPEN, POLL_MS,
    changes::{Changes, FileChange, Mode, changes, forget_repos},
    graph_log,
    history::{history_page, with_first_page},
    log::{Commit, describe_ref},
    mode::{self, Section},
};
use serde_json::{Value, json};
use std::{cell::RefCell, collections::BTreeSet, future::Future, pin::Pin, rc::Rc};
use wings_plugin::{
    format::{ago_now, basename, grouped, plural},
    git::{branch_label, ok, on_change, repo_root},
    host, messages, poll,
    task::{join, join_all, spawn},
    ui::{Node, checkbox, chips, empty, icon_button, icons, list, list_button, list_row, loading, muted, notice, page, row, search_field, spinner, stack, text, toolbar_with},
};

/// How long the search waits after the last key before it asks git.
const SEARCH_MS: u32 = 250;
/// Refs shown under a commit. The rest are a count.
const MAX_REFS: usize = 3;

/// What the chips choose between: the two lists of changes, and the commit history.
#[derive(Clone, Copy, PartialEq)]
enum View {
    Uncommitted,
    Branch,
    History,
}

impl View {
    fn name(self) -> &'static str {
        match self {
            Self::Uncommitted => "uncommitted",
            Self::Branch => "branch",
            Self::History => "history",
        }
    }

    fn mode(self) -> Mode {
        if self == Self::Branch { Mode::Branch } else { Mode::Uncommitted }
    }
}

#[derive(Clone, PartialEq)]
struct Repo {
    root: String,
    name: String,
    branch: String,
}

/// The History tab's rows, and the repo, search and scope they are of.
#[derive(Default)]
struct Shown {
    rows: Vec<Commit>,
    more: bool,
    source: String,
}

struct Page {
    /// Each pane's id, folder and whether it is the selected one.
    panes: Vec<(String, String, bool)>,
    /// The repo the sidebar follows, which it keeps while the selected pane isn't in one.
    followed: Option<String>,
    /// The folder of the project on screen, which the sidebar follows while no terminal pane is selected: a Claude chat
    /// tab or another page is on screen.
    here: Option<String>,
    view: View,
    /// Files, or the commit graph, as picked for the repo the sidebar follows.
    section: Section,
    /// Folders you collapsed, by path.
    collapsed: BTreeSet<String>,
    /// The repo the header shows, kept for the moment after a switch when the new answer isn't here yet.
    repo: Option<Repo>,
    /// The tab on screen has answered. With no `repo`, no pane is in one.
    answered: bool,
    changes: Option<Rc<Changes>>,
    history: Shown,
    /// The first page of this search and scope isn't here yet, so the rows may be of another.
    pending: bool,
    loading_more: bool,
    /// The history shown is of a repo Wings doesn't watch, so nothing says when it moves and the poll reloads it.
    history_polled: bool,
    loading: bool,
    error: Option<String>,
    /// Why the last diff, graph or commit didn't open.
    open_error: Option<String>,
    query: String,
    /// What the search box holds, which becomes the query after a pause.
    typed: String,
    all: bool,
    /// Go up with each load and each key in the search box: an older one that finds a later number drops out.
    loads: u64,
    typing: u64,
    asks: u64,
}

thread_local! {
    static PAGE: RefCell<Page> = RefCell::new(Page {
        panes: Vec::new(), followed: None, here: None, view: View::Uncommitted, section: Section::Files, collapsed: BTreeSet::new(), repo: None, answered: false, changes: None,
        history: Shown::default(), pending: true, loading_more: false, history_polled: false, loading: false, error: None, open_error: None,
        query: String::new(), typed: String::new(), all: false, loads: 0, typing: 0, asks: 0,
    });
}

impl Page {
    /// Drops what the sidebar shows when it is of another repo than `root`, so the new repo's header never sits over the
    /// old repo's files: the loading state shows until the new answer (or its error) arrives.
    fn leave_other_repo(&mut self, root: &str) {
        let shown = self.repo.as_ref().map(|repo| repo.root.as_str()).or(self.changes.as_ref().map(|c| c.root.as_str()));
        if shown.is_some_and(|shown| shown != root) {
            (self.repo, self.changes, self.answered, self.error, self.open_error) = (None, None, false, None, None);
            self.history = Shown::default();
            self.pending = true;
            self.loading_more = false;
            self.collapsed.clear();
        }
    }
}

fn with<T>(change: impl FnOnce(&mut Page) -> T) -> T {
    PAGE.with(|page| change(&mut page.borrow_mut()))
}

type Root = Pin<Box<dyn Future<Output = Option<String>>>>;

/// The selected pane's repo, else the one the sidebar followed while a pane is still in it, else the first pane's that
/// is in one. While no pane is selected, as when a Claude chat tab is on screen, the repo of the project on screen
/// comes first.
async fn followed_root() -> Option<String> {
    let (panes, here) = with(|page| (page.panes.clone(), page.here.clone()));
    let asked: Vec<Root> = panes.iter().map(|(_, cwd, _)| -> Root { let cwd = cwd.clone(); Box::pin(async move { repo_root(&cwd).await }) }).collect();
    let (roots, here) = join(join_all(asked), async move {
        match here {
            Some(here) => repo_root(&here).await,
            None => None,
        }
    })
    .await;
    let focused: Vec<bool> = panes.iter().map(|pane| pane.2).collect();
    with(|page| {
        page.followed = choose(&focused, roots, here, page.followed.take());
        page.followed.clone()
    })
}

/// Where the project event says the tab on screen works: a page tab's own folder (a Claude chat in a worktree or
/// another repo), else the project's.
fn place(project: &Value) -> Option<String> {
    project["folder"].as_str().or(project["path"].as_str()).map(str::to_owned)
}

/// The repo to follow, from each pane's repo (`roots`) and whether it is the selected one (`focused`), the repo of the
/// project on screen (`here`), and the repo followed until now (`kept`).
fn choose(focused: &[bool], roots: Vec<Option<String>>, here: Option<String>, kept: Option<String>) -> Option<String> {
    let at = focused.iter().position(|focused| *focused);
    let selected = at.and_then(|at| roots.get(at).cloned().flatten());
    // Without a selected pane the panes listed are other tabs' or other projects', so the project on screen says.
    let here = if at.is_none() { here } else { None };
    let kept = kept.filter(|kept| roots.iter().flatten().any(|root| root == kept));
    selected.or(here).or(kept).or_else(|| roots.into_iter().flatten().next())
}

/// Whether Wings watches the repo at `root`: it does when a project is in it.
async fn watched(root: &str) -> bool {
    let projects = host::projects().await.unwrap_or_default();
    for project in projects.as_array().into_iter().flatten() {
        if repo_root(project["path"].as_str().unwrap_or_default()).await.as_deref() == Some(root) {
            return true;
        }
    }
    false
}

fn source_of(root: &str, query: &str, all: bool) -> String {
    format!("{root}\0{query}\0{all}")
}

/// Reads the tab on screen again.
async fn reload() {
    let (mine, view, query, all) = with(|page| {
        page.loads += 1;
        page.loading = true;
        (page.loads, page.view, page.query.clone(), page.all)
    });
    let Some(root) = followed_root().await else {
        return with(|page| {
            if page.loads == mine {
                (page.repo, page.answered, page.loading, page.error) = (None, true, false, None);
            }
        });
    };
    let section = mode::saved(&root);
    with(|page| {
        page.leave_other_repo(&root);
        page.section = section;
    });
    if section == Section::Graph {
        let (branch, head) = join(ok(&root, &["branch", "--show-current"]), ok(&root, &["rev-parse", "--verify", "--quiet", "HEAD"])).await;
        let (name, polled) = (basename(&root).to_owned(), !watched(&root).await);
        let log = graph_log::begin(Some(root.clone()), Some(name.clone()));
        let found = graph_log::read(Some(&root)).await;
        with(|page| {
            if page.loads != mine {
                return;
            }
            (page.loading, page.history_polled) = (false, polled);
            page.repo = Some(Repo { name, branch: branch_label(branch.as_deref(), head.as_deref()), root });
            (page.answered, page.error) = (true, None);
        });
        graph_log::finish(log, found);
    } else if view == View::History {
        let (branch, head) = join(ok(&root, &["branch", "--show-current"]), ok(&root, &["rev-parse", "--verify", "--quiet", "HEAD"])).await;
        let head = head.filter(|head| !head.is_empty());
        let found = if head.is_some() || all { history_page(&root, &query, all, 0).await } else { Ok((Vec::new(), false)) };
        let polled = !watched(&root).await;
        with(|page| {
            if page.loads != mine {
                return;
            }
            page.loading = false;
            page.history_polled = polled;
            match found {
                Ok((top, more)) => {
                    let source = source_of(&root, &query, all);
                    let grown = page.history.source == source && page.history.rows.len() > top.len();
                    let rows = if page.history.source == source { with_first_page(&page.history.rows, top) } else { top };
                    page.history = Shown { more: if grown && rows.len() == page.history.rows.len() { page.history.more } else { more }, rows, source };
                    page.repo = Some(Repo { name: basename(&root).to_owned(), branch: branch_label(branch.as_deref(), head.as_deref()), root });
                    (page.answered, page.pending, page.error) = (true, false, None);
                }
                Err(error) => page.error = Some(error),
            }
        });
    } else {
        // A refresh, a reload and a tab switch all load the same way, so each one reads git again.
        let found = changes(&root, view.mode(), true).await;
        with(|page| {
            if page.loads != mine {
                return;
            }
            page.loading = false;
            match found {
                Ok(c) => {
                    page.repo = Some(Repo { root, name: c.name.clone(), branch: c.branch.clone() });
                    (page.changes, page.answered, page.error) = (Some(c), true, None);
                }
                Err(error) => page.error = Some(error),
            }
        });
    }
}

/// Load more on the History tab: the page after the rows the list already has.
fn load_more() {
    let Some((root, query, all, source, shown)) = with(|page| {
        let repo = page.repo.as_ref()?;
        page.loading_more = true;
        Some((repo.root.clone(), page.query.clone(), page.all, page.history.source.clone(), page.history.rows.len()))
    }) else {
        return;
    };
    spawn(async move {
        let found = history_page(&root, &query, all, shown).await;
        with(|page| {
            page.loading_more = false;
            match found {
                // Dropped when the list moved on while this was on its way: another repo or search, or a reload that changed the rows.
                Ok((rows, more)) if page.history.source == source && page.history.rows.len() == shown => {
                    page.history.rows.extend(rows);
                    page.history.more = more;
                }
                Ok(_) => {}
                Err(error) => page.error = Some(error),
            }
        });
    });
}

/// Asks main to open something, under an id main answers with.
fn ask(name: &str, mut what: Value) {
    what["id"] = with(|page| {
        page.open_error = None;
        page.asks += 1;
        page.asks
    })
    .into();
    messages::tell(name, &what);
}

/// Switches between Files and the graph, and keeps the pick for the repo the sidebar follows.
fn pick(section: Section) {
    if let Some(root) = with(|page| page.followed.clone()) {
        mode::save(&root, section);
    }
    with(|page| page.section = section);
    spawn(reload());
}

fn show(view: View) {
    with(|page| {
        if page.view.mode() != view.mode() {
            page.changes = None;
        }
        page.view = view;
        page.answered = if view == View::History { !page.history.source.is_empty() } else { page.changes.is_some() };
    });
    spawn(reload());
}

fn button(label: &str, variant: &str, disabled: bool, on_click: impl Fn() + 'static) -> Node {
    wings_plugin::ui::button(label, variant, false, on_click).disabled(disabled)
}

struct Folder<'a> {
    name: &'a str,
    path: String,
    folders: Vec<Folder<'a>>,
    files: Vec<&'a FileChange>,
}

fn tree(files: &[FileChange]) -> Folder<'_> {
    let mut root = Folder { name: "", path: String::new(), folders: Vec::new(), files: Vec::new() };
    for file in files {
        let mut at = &mut root;
        let mut parts: Vec<&str> = file.path.split('/').collect();
        parts.pop();
        for part in parts {
            let path = if at.path.is_empty() { part.to_owned() } else { format!("{}/{part}", at.path) };
            let found = match at.folders.iter().position(|folder| folder.name == part) {
                Some(found) => found,
                None => {
                    at.folders.push(Folder { name: part, path, folders: Vec::new(), files: Vec::new() });
                    at.folders.len() - 1
                }
            };
            at = &mut at.folders[found];
        }
        at.files.push(file);
    }
    root
}

/// The most folder and file rows the tree sends. The app refuses a page's tree over 2 MiB, and a row is about 400
/// bytes (see `a_five_thousand_file_change_...`), so this leaves most of that room to the rest of the page.
const MAX_ROWS: usize = 400;

/// What is left of the row budget, and how many file rows were drawn within it.
struct Budget {
    left: usize,
    files: usize,
}

/// Folder and file rows, a folder with only one folder in it shown as `a/b`. Stops when `budget` is used up.
fn tree_rows(folder: &Folder, depth: u32, root: &str, mode: Mode, collapsed: &BTreeSet<String>, budget: &mut Budget, out: &mut Vec<Node>) {
    for sub in &folder.folders {
        if budget.left == 0 {
            return;
        }
        let (mut f, mut label) = (sub, sub.name.to_owned());
        while f.files.is_empty() && f.folders.len() == 1 {
            f = &f.folders[0];
            label = format!("{label}/{}", f.name);
        }
        let open = !collapsed.contains(&f.path);
        let path = f.path.clone();
        let toggle = list_button([text(label).prop("grow", true).prop("truncate", true)], move || {
            with(|page| if open { page.collapsed.insert(path.clone()) } else { page.collapsed.remove(&path) });
        });
        out.push(list_row(format!("d:{}", f.path), toggle.prop("indent", depth * 12).prop("aria-expanded", open)));
        budget.left -= 1;
        if open {
            tree_rows(f, depth + 1, root, mode, collapsed, budget, out);
        }
    }
    for file in &folder.files {
        if budget.left == 0 {
            return;
        }
        budget.left -= 1;
        budget.files += 1;
        let tone = match file.status.as_str() {
            "D" | "U" => "danger",
            "A" | "?" => "success",
            _ => "warn",
        };
        let count = |n: Option<u64>, sign: char, tone: &str| n.filter(|n| *n > 0).map(|n| text(format!("{sign}{n}")).prop("tone", tone).prop("size", "tiny").prop("numeric", true));
        let (asked_root, asked_path) = (root.to_owned(), file.path.clone());
        let open = list_button(
            [text(file.status.clone()).prop("tone", tone).prop("size", "tiny").prop("mono", true), text(basename(&file.path)).prop("grow", true).prop("truncate", true)],
            move || ask(OPEN, json!({ "root": asked_root, "mode": mode.name(), "path": asked_path })),
        )
        .maybe(file.added.is_none().then(|| muted(if file.removed.is_none() { "binary" } else { "new" })))
        .maybe(count(file.added, '+', "success"))
        .maybe(count(file.removed, '−', "danger"));
        out.push(list_row(format!("f:{}", file.path), open.prop("indent", 12 + depth * 12)));
    }
}

fn changes_body(c: &Changes, mode: Mode, collapsed: &BTreeSet<String>) -> Vec<Node> {
    if c.files.is_empty() {
        return vec![empty(if mode == Mode::Branch { "Nothing changed on this branch." } else { "No uncommitted changes." }, &c.against)];
    }
    let total = |n: u64, sign: char, tone: &str| (n > 0).then(|| text(format!(" {sign}{n}")).prop("tone", tone).prop("numeric", true));
    let summary = text(plural(c.files.len(), "file"))
        .maybe(total(c.added, '+', "success"))
        .maybe(total(c.removed, '−', "danger"))
        .maybe(c.partial.then(|| muted(" (some new files not counted)")));
    let root = c.root.clone();
    let all = button("View all", "secondary", false, move || ask(OPEN, json!({ "root": root, "mode": mode.name(), "path": null }))).prop("size", "sm");
    let mut rows = Vec::new();
    let mut budget = Budget { left: MAX_ROWS, files: 0 };
    tree_rows(&tree(&c.files), 0, &c.root, mode, collapsed, &mut budget, &mut rows);
    // Past the cap the list ends in what it left out, with the way to the full diff.
    let more = (budget.files < c.files.len() && budget.left == 0).then(|| {
        let root = c.root.clone();
        list_row(
            "more",
            row()
                .prop("align", "center")
                .child(muted(format!("Showing {} of {} files", grouped(budget.files as u64), grouped(c.files.len() as u64))).prop("grow", true).prop("truncate", true))
                .child(button("View all", "secondary", false, move || ask(OPEN, json!({ "root": root, "mode": mode.name(), "path": null }))).prop("size", "sm")),
        )
    });
    rows.extend(more);
    vec![
        // The totals over what they're against, with no card around them, as Orca's git panel has its branch: a card and a
        // large button took a narrow sidebar's room from the files.
        // The summary is the list's first row, so "View all" ends where the rows' stats do.
        list().key("tree").child(list_row(
            "summary",
            row()
                .prop("align", "center")
                .prop("grow", true)
                .child(stack().prop("gap", "none").prop("grow", true).child(summary).child(muted(c.against.clone()).prop("size", "small").prop("truncate", true)))
                .child(all),
        )).children(rows),
    ]
}

fn commit_row(c: &Commit, key: String, root: &str) -> Node {
    let tiny = |node: Node| node.prop("size", "tiny");
    let pill = |label: String, tone: Option<&str>| {
        // A pill never runs past its row: the host cuts a long name short, and the title says all of it.
        let pill = Node::new("Badge").prop("variant", "outline").prop("title", label.clone()).text(label);
        match tone {
            Some(tone) => pill.prop("tone", tone),
            None => pill,
        }
    };
    let mut refs: Vec<Node> = c.refs.iter().take(MAX_REFS).map(|name| describe_ref(name)).map(|(label, tone)| pill(label, tone)).collect();
    if c.refs.len() > MAX_REFS {
        refs.push(pill(format!("+{}", c.refs.len() - MAX_REFS), None));
    }
    let asked = json!({ "root": root, "hash": c.hash, "subject": c.subject, "author": c.author, "at": c.at });
    let open = list_button(
        [
            text(c.subject.chars().take(200).collect::<String>()).prop("truncate", true),
            row()
                .child(tiny(muted(c.hash.chars().take(7).collect::<String>())).prop("mono", true))
                .child(tiny(muted(c.author.chars().take(80).collect::<String>())).prop("grow", true).prop("truncate", true))
                .child(tiny(muted(ago_now(c.at * 1000.0)))),
        ],
        move || ask(COMMIT, asked.clone()),
    )
    .maybe((!refs.is_empty()).then(|| row().prop("gap", "xs").prop("wrap", true).children(refs)));
    list_row(key, open.prop("layout", "column"))
}

fn history_body(p: &Page, root: &str) -> Vec<Node> {
    let mut out = vec![
        row()
            .key("search")
            .child(stack().prop("grow", true).child(search_field("Search commits", &p.typed, |typed| {
                let (mine, typed) = (with(|page| { page.typed = typed.to_owned(); page.typing += 1; page.typing }), typed.trim().to_owned());
                spawn(async move {
                    host::sleep(SEARCH_MS).await;
                    if with(|page| page.typing == mine && page.query != typed) {
                        with(|page| (page.query, page.pending) = (typed, true));
                        reload().await;
                    }
                });
            }))),
        checkbox("All branches", p.all, |all| {
            with(|page| (page.all, page.pending) = (all, true));
            spawn(reload());
        })
        .key("all"),
    ];
    let shown = &p.history;
    if shown.rows.is_empty() && !p.pending {
        let searched = format!("Searched messages, authors and hashes for “{}”.", p.query);
        out.push(if p.query.is_empty() { empty("No commits yet.", "") } else { empty("No commits match.", &searched) }.key("none"));
        return out;
    }
    // Keys for commit rows: the hash, made unique if a page repeats one.
    let mut seen: Vec<&str> = Vec::new();
    let rows: Vec<Node> = shown
        .rows
        .iter()
        .map(|c| {
            let again = seen.iter().filter(|hash| **hash == c.hash).count();
            seen.push(&c.hash);
            commit_row(c, if again == 0 { c.hash.clone() } else { format!("{}:{again}", c.hash) }, root)
        })
        .collect();
    out.push(list().key("commits").children(rows));
    if shown.more {
        out.push(button(if p.loading_more { "Loading…" } else { "Load more" }, "ghost", p.loading_more, load_more).key("more"));
    }
    out
}

fn view() -> Vec<Node> {
    PAGE.with(|state| {
        let p = &*state.borrow();
        let Some(repo) = &p.repo else {
            return vec![if p.answered {
                empty("No git repo in your panes.", "Open a terminal in a repo and its changes show here.")
            } else if let Some(error) = &p.error {
                notice("warn", error)
            } else {
                loading()
            }];
        };
        // An icon, as the switch and the pop-out left a 250 point sidebar no room for the word.
        let refresh = icon_button("Refresh", &icons::REFRESH_CW, || {
            forget_repos();
            spawn(reload());
        });
        let switch = chips("Show in the sidebar", &[("files", "Files"), ("graph", "Graph")], p.section.name(), |picked| pick(if picked == "graph" { Section::Graph } else { Section::Files })).segmented();
        let graph_root = repo.root.clone();
        let pop_out = icon_button("Open in a window", &icons::EXTERNAL_LINK, move || ask(GRAPH, json!({ "root": graph_root })));
        let actions = row().prop("gap", "xs").maybe(p.loading.then(|| spinner().prop("label", "Refreshing"))).child(switch).child(refresh).child(pop_out);
        if p.section == Section::Graph {
            let notice = p.open_error.as_ref().map(|error| notice("warn", error).key("open-error"));
            let shown = graph_log::with(|log| graph_log::body(log, |asked| ask(COMMIT, asked)));
            return vec![page(toolbar_with(&repo.name, muted(repo.branch.clone()).prop("truncate", true), Some(actions)), notice.into_iter().chain(shown).collect::<Vec<_>>()).prop("flush", true)];
        }
        let tabs = chips("Show", &[("uncommitted", "Changes"), ("branch", "Branch"), ("history", "History")], p.view.name(), |picked| {
            show(match picked {
                "branch" => View::Branch,
                "history" => View::History,
                _ => View::Uncommitted,
            });
        })
        .segmented();
        let body = match (p.view, &p.changes) {
            (View::History, _) => history_body(p, &repo.root),
            (_, Some(c)) => changes_body(c, p.view.mode(), &p.collapsed),
            _ => Vec::new(),
        };
        let content = [tabs.key("show")]
            .into_iter()
            .chain(p.error.as_ref().map(|error| notice("warn", error).key("error")))
            .chain(p.open_error.as_ref().map(|error| notice("warn", error).key("open-error")))
            .chain(body);
        vec![page(toolbar_with(&repo.name, muted(repo.branch.clone()).prop("truncate", true), Some(actions)), content.collect::<Vec<_>>())]
    })
}

/// Reloads when the panes or the project on screen moved in a way that moves the sidebar's repo, or nothing shows yet.
async fn follow() {
    let (before, answered) = with(|page| (page.followed.clone(), page.answered));
    if followed_root().await != before || !answered {
        reload().await;
    }
}

#[cfg_attr(not(test), unsafe(no_mangle))]
pub fn start() {
    wings_plugin::run(view);
    host::on_panes(|list| {
        let text = |pane: &Value, name: &str| pane[name].as_str().unwrap_or_default().to_owned();
        let next: Vec<_> = list.iter().map(|pane| (text(pane, "paneId"), text(pane, "cwd"), pane["focused"] == true)).collect();
        if with(|page| std::mem::replace(&mut page.panes, next.clone()) == next && page.answered) {
            return;
        }
        spawn(follow());
    });
    host::on("project", |_, project| {
        let here = place(project);
        if with(|page| std::mem::replace(&mut page.here, here.clone()) != here) {
            spawn(follow());
        }
    });
    // Edits to files in the working tree, which Wings doesn't watch, and the history of a repo it doesn't watch.
    poll::while_focused(POLL_MS, false, || {
        if with(|page| (page.view != View::History && page.section == Section::Files) || page.history_polled) {
            spawn(reload());
        }
    });
    // A commit, checkout or pull in the repo it shows.
    on_change(|_, roots| async move {
        if with(|page| page.followed.as_ref().is_some_and(|followed| roots.contains(followed))) {
            reload().await;
        }
    });
    messages::on(DONE, |done| {
        if let Some(error) = done["error"].as_str() {
            with(|page| {
                if done["id"] == page.asks {
                    page.open_error = Some(error.to_owned());
                }
            });
        }
    });
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{Repo, choose, commit_row, place, view, with};
    use crate::log::Commit;

    fn repo(root: &str) -> Option<String> {
        Some(root.to_owned())
    }

    /// The header's Files / Graph choice and the Changes / Branch / History choice are segmented tracks, and the
    /// header's icon buttons carry a tooltip.
    #[test]
    fn the_sidebars_choices_are_segmented_tracks_and_its_icon_buttons_have_tooltips() {
        with(|page| page.repo = Some(Repo { root: "/shop".into(), name: "shop".into(), branch: "main".into() }));
        let tree = wings_plugin::ui::tree_json(view().remove(0)).to_string();
        assert_eq!(tree.matches(r#""segmented":true"#).count(), 2, "{tree}");
        assert!(tree.contains(r#""title":"Refresh""#) && tree.contains(r#""title":"Open in a window""#), "{tree}");
        assert!(tree.contains(r#""size":"icon-sm""#), "{tree}");
    }

    /// A ghost button reads as plain text, so View all is a small secondary one.
    #[test]
    fn view_all_is_a_small_secondary_button() {
        let c = crate::changes::Changes {
            root: "/shop".into(),
            name: "shop".into(),
            branch: "main".into(),
            mode: crate::changes::Mode::Uncommitted,
            base: String::new(),
            against: "Working tree against HEAD".into(),
            files: vec![],
            added: 1,
            removed: 0,
            partial: false,
        };
        let file = crate::changes::FileChange { path: "a.rs".into(), old_path: None, status: "M".into(), added: Some(1), removed: Some(0), untracked: false };
        let c = crate::changes::Changes { files: vec![file], ..c };
        let tree = wings_plugin::ui::tree_json(super::changes_body(&c, crate::changes::Mode::Uncommitted, &Default::default()).remove(0)).to_string();
        let at = tree.find("View all").expect(&tree);
        let button = &tree[at..at + tree[at..].find(r#""type":"Button""#).expect(&tree)];
        assert!(button.contains(r#""variant":"secondary""#) && button.contains(r#""size":"sm""#) && !button.contains("ghost"), "{tree}");
    }

    #[test]
    fn a_total_of_zero_is_not_shown() {
        let c = crate::changes::Changes { added: 3, removed: 0, ..big(1, 1) };
        let tree = wings_plugin::ui::tree_json(super::changes_body(&c, crate::changes::Mode::Branch, &Default::default()).remove(0)).to_string();
        assert!(tree.contains("+3") && !tree.contains("−0"), "{tree}");
        let c = crate::changes::Changes { added: 0, removed: 2, ..c };
        let tree = wings_plugin::ui::tree_json(super::changes_body(&c, crate::changes::Mode::Branch, &Default::default()).remove(0)).to_string();
        assert!(tree.contains("−2") && !tree.contains("+0"), "{tree}");
    }

    fn big(count: usize, dirs: usize) -> crate::changes::Changes {
        let files = (0..count)
            .map(|i| crate::changes::FileChange {
                path: format!("src/BoundedContext/Module{}/Application/Handlers/Prescriptions/Tracker{i:05}Handler.cs", i % dirs),
                old_path: None,
                status: "M".into(),
                added: Some(12),
                removed: Some(3),
                untracked: false,
            })
            .collect();
        crate::changes::Changes {
            root: "/be".into(),
            name: "be".into(),
            branch: "feature/196116".into(),
            mode: crate::changes::Mode::Branch,
            base: String::new(),
            against: "Since origin/main at 1234567".into(),
            files,
            added: 1,
            removed: 0,
            partial: false,
        }
    }

    /// The app refuses a page's tree over 2 MiB, so the sidebar sends a bounded number of rows, however many files changed
    /// and however they are spread over folders, and ends the list with what it left out.
    #[test]
    fn a_five_thousand_file_change_renders_a_tree_under_the_limit_with_a_showing_row() {
        for dirs in [1, 400] {
            let c = big(5_000, dirs);
            let nodes = super::changes_body(&c, crate::changes::Mode::Branch, &Default::default());
            let json: usize = nodes.into_iter().map(|node| wings_plugin::ui::tree_json(node).to_string().len()).sum();
            eprintln!("dirs {dirs}: {json} bytes for the sidebar body");
            assert!(json < 400_000, "{json}");
            let tree = wings_plugin::ui::tree_json(super::changes_body(&c, crate::changes::Mode::Branch, &Default::default()).remove(0)).to_string();
            assert!(tree.contains("of 5,000 files"), "{}", &tree[tree.len().saturating_sub(600)..]);
        }
        // Under the cap there is no such row.
        let small = wings_plugin::ui::tree_json(super::changes_body(&big(50, 1), crate::changes::Mode::Branch, &Default::default()).remove(0)).to_string();
        assert!(!small.contains("of 50 files"), "{small}");
        assert!(!small.contains("Showing"), "{small}");
    }

    /// Switching to another repo drops the old one's files, history and error, so they never show under the new header.
    #[test]
    fn switching_repos_clears_the_previous_repos_files() {
        with(|page| {
            page.repo = Some(Repo { root: "/fe".into(), name: "fe".into(), branch: "main".into() });
            page.changes = Some(std::rc::Rc::new(crate::changes::Changes { root: "/fe".into(), ..big(3, 1) }));
            page.answered = true;
            page.error = Some("old".into());
            page.leave_other_repo("/fe");
            assert!(page.changes.is_some() && page.repo.is_some());
            page.leave_other_repo("/be");
            assert!(page.changes.is_none() && page.repo.is_none() && !page.answered && page.error.is_none());
        });
        let tree = wings_plugin::ui::tree_json(view().remove(0)).to_string();
        assert!(!tree.contains("fe"), "{tree}");
    }

    #[test]
    fn a_commits_ref_pills_name_the_whole_ref_as_their_title() {
        let commit = Commit { hash: "abc".into(), refs: vec!["b:feature/196116-be-prescription-tracker-list".into()], ..Default::default() };
        let tree = wings_plugin::ui::tree_json(commit_row(&commit, "k".into(), "/shop")).to_string();
        assert!(tree.contains(r#""title":"feature/196116-be-prescription-tracker-list""#), "{tree}");
    }

    #[test]
    fn with_a_chat_tab_on_screen_the_sidebar_follows_the_project_on_screen() {
        // A terminal of wings' is still open in its own tab, none selected: a Claude chat tab of another project is.
        assert_eq!(choose(&[false], vec![repo("/wings")], repo("/sciensus"), repo("/wings")), repo("/sciensus"));
        // The project on screen isn't a repo: the one followed stays.
        assert_eq!(choose(&[false], vec![repo("/wings")], None, repo("/wings")), repo("/wings"));
    }

    #[test]
    fn a_selected_pane_decides_over_the_project_on_screen() {
        assert_eq!(choose(&[false, true], vec![repo("/wings"), repo("/web")], repo("/wings"), repo("/wings")), repo("/web"));
        // A selected pane outside any repo keeps the repo followed while a pane is in it.
        assert_eq!(choose(&[false, true], vec![repo("/wings"), None], repo("/web"), repo("/wings")), repo("/wings"));
        assert_eq!(choose(&[true], vec![None], None, None), None);
    }

    #[test]
    fn a_chat_tab_working_in_another_folder_is_followed_there() {
        // The project event as Wings sends it while a Claude chat tab in a worktree of the project is on screen.
        let project = json!({ "id": "/wings", "name": "wings", "path": "/wings", "folder": "/wings-worktrees/fix", "hiddenSections": [] });
        assert_eq!(place(&project), repo("/wings-worktrees/fix"));
        // With a terminal on screen there is no folder: the project's own.
        assert_eq!(place(&json!({ "id": "/wings", "path": "/wings" })), repo("/wings"));
        assert_eq!(place(&json!(null)), None);
    }
}
