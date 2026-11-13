//! The Changes sidebar: the changed files of the focused pane's repo as a folder tree, and its commit history. It
//! runs git itself and keeps what it shows fresh. A click asks the main script to open the diff viewer.

use crate::{
    COMMIT, DONE, GRAPH, OPEN, POLL_MS,
    changes::{Changes, FileChange, Mode, changes, forget_repos},
    history::{history_page, with_first_page},
    log::{Commit, describe_ref},
};
use serde_json::{Value, json};
use std::{cell::RefCell, collections::BTreeSet, future::Future, pin::Pin, rc::Rc};
use wings_plugin::{
    format::{ago, basename, plural},
    git::{branch_label, ok, on_change, repo_root},
    host, messages, poll,
    task::{join, join_all, spawn},
    ui::{Node, card, checkbox, chips, empty, list, list_button, list_row, loading, muted, notice, page, row, search_field, spinner, stack, text, toolbar_with},
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
    view: View,
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
        panes: Vec::new(), followed: None, view: View::Uncommitted, collapsed: BTreeSet::new(), repo: None, answered: false, changes: None,
        history: Shown::default(), pending: true, loading_more: false, history_polled: false, loading: false, error: None, open_error: None,
        query: String::new(), typed: String::new(), all: false, loads: 0, typing: 0, asks: 0,
    });
}

fn with<T>(change: impl FnOnce(&mut Page) -> T) -> T {
    PAGE.with(|page| change(&mut page.borrow_mut()))
}

type Root = Pin<Box<dyn Future<Output = Option<String>>>>;

/// The selected pane's repo, else the one the sidebar followed while a pane is still in it, else the first pane's that
/// is in one.
async fn followed_root() -> Option<String> {
    let panes = with(|page| page.panes.clone());
    let asked: Vec<Root> = panes.iter().map(|(_, cwd, _)| -> Root { let cwd = cwd.clone(); Box::pin(async move { repo_root(&cwd).await }) }).collect();
    let roots = join_all(asked).await;
    let selected = panes.iter().position(|pane| pane.2).and_then(|at| roots[at].clone());
    with(|page| {
        let kept = page.followed.take().filter(|kept| roots.iter().flatten().any(|root| root == kept));
        page.followed = selected.or(kept).or_else(|| roots.into_iter().flatten().next());
        page.followed.clone()
    })
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
    if view == View::History {
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
    let button = Node::new("Button").prop("variant", variant).on("onClick", move |_| on_click()).text(label);
    if disabled { button.prop("disabled", true) } else { button }
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

/// Folder and file rows, a folder with only one folder in it shown as `a/b`.
fn tree_rows(folder: &Folder, depth: u32, root: &str, mode: Mode, collapsed: &BTreeSet<String>, out: &mut Vec<Node>) {
    for sub in &folder.folders {
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
        if open {
            tree_rows(f, depth + 1, root, mode, collapsed, out);
        }
    }
    for file in &folder.files {
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
    let summary = text(format!("{} ", plural(c.files.len(), "file")))
        .child(text(format!("+{}", c.added)).prop("tone", "success").prop("numeric", true))
        .text(" ")
        .child(text(format!("−{}", c.removed)).prop("tone", "danger").prop("numeric", true))
        .maybe(c.partial.then(|| muted(" (some new files not counted)")));
    let root = c.root.clone();
    let all = button("View all", "outline", false, move || ask(OPEN, json!({ "root": root, "mode": mode.name(), "path": null })));
    let mut rows = Vec::new();
    tree_rows(&tree(&c.files), 0, &c.root, mode, collapsed, &mut rows);
    vec![
        card().key("summary").child(row().child(stack().prop("gap", "none").prop("grow", true).child(summary).child(muted(c.against.clone()).prop("truncate", true))).child(all)),
        list().key("tree").children(rows),
    ]
}

fn commit_row(c: &Commit, key: String, root: &str) -> Node {
    let tiny = |node: Node| node.prop("size", "tiny");
    let pill = |label: String, tone: Option<&str>| {
        let pill = Node::new("Badge").prop("variant", "outline").text(label);
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
                .child(tiny(muted(ago(c.at * 1000.0, host::now())))),
        ],
        move || ask(COMMIT, asked.clone()),
    )
    .maybe((!refs.is_empty()).then(|| row().prop("gap", "xs").prop("wrap", true).children(refs)));
    list_row(key, open.prop("layout", "column"))
}

fn history_body(p: &Page, root: &str) -> Vec<Node> {
    let graph_root = root.to_owned();
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
            })))
            .child(button("Graph", "outline", false, move || ask(GRAPH, json!({ "root": graph_root })))),
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
        let refresh = button("Refresh", "outline", false, || {
            forget_repos();
            spawn(reload());
        });
        let actions = row().prop("gap", "xs").maybe(p.loading.then(|| spinner().prop("label", "Refreshing"))).child(refresh);
        let tabs = chips("Show", &[("uncommitted", "Changes"), ("branch", "Branch"), ("history", "History")], p.view.name(), |picked| {
            show(match picked {
                "branch" => View::Branch,
                "history" => View::History,
                _ => View::Uncommitted,
            });
        });
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

#[unsafe(no_mangle)]
pub fn start() {
    wings_plugin::run(view);
    host::on_panes(|list| {
        let text = |pane: &Value, name: &str| pane[name].as_str().unwrap_or_default().to_owned();
        let next: Vec<_> = list.iter().map(|pane| (text(pane, "paneId"), text(pane, "cwd"), pane["focused"] == true)).collect();
        if with(|page| std::mem::replace(&mut page.panes, next.clone()) == next && page.answered) {
            return;
        }
        // Only when the panes moved in a way that moves the sidebar's repo, or nothing shows yet.
        spawn(async {
            let (before, answered) = with(|page| (page.followed.clone(), page.answered));
            if followed_root().await != before || !answered {
                reload().await;
            }
        });
    });
    // Edits to files in the working tree, which Wings doesn't watch, and the history of a repo it doesn't watch.
    poll::while_focused(POLL_MS, false, || {
        if with(|page| page.view != View::History || page.history_polled) {
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
