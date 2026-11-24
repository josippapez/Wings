//! The Issues tab: a board of to do, in progress, in review and done, with canceled behind a toggle, in rows by parent epic or
//! feature unless that's turned off. Click a card to open the issue. Alt+Left and Alt+Right on a card move it between
//! columns, and so do its buttons. List shows them as rows grouped by status instead. The view and the rows choice start as
//! they were last left, or from the Default view setting, and the folded rows are kept with the tab. The search box
//! filters what's shown.

use crate::{
    add_issue,
    delete_confirm::{ask_to_delete, asking, node as confirm_node},
    domain::{Issue, Op, Status, Store, Update},
    feed::{self, Project},
    orderings::{Column, Lane, Sort, board_columns, board_lanes, issue_tree},
    parts::{child_progress, error_text, is_delete_key, line, muted, priority_icon, row, session_marker, stack, status_dot, status_summary, type_icon, when, column},
    relations::{children_of, progress},
    search::Index,
    settings::{self, Layout},
};
use serde_json::{Value, json};
use std::{cell::RefCell, collections::HashMap, future::Future, rc::Rc};
use wings_plugin::{
    host,
    task::spawn,
    ui::{Node, checkbox, chips, empty, loading, page_with, search_field, slot, title_with_context, toolbar_titled},
};

/// How many cards a column of the board shows at first, and rows the list does, and how many more each Show more adds.
const COLUMN_CARDS: usize = 30;
const LIST_ROWS: usize = 150;

/// The width of a row's priority and key, so the titles after them line up: the widest priority and 52 points for a key.
const PRIORITY_WIDTH: u32 = 20;
const KEY_WIDTH: u32 = 52;

const EMPTY: [(Status, &str); 5] = [
    (Status::Todo, "Nothing waiting"),
    (Status::InProgress, "Nothing in progress"),
    (Status::InReview, "Nothing in review"),
    (Status::Done, "Nothing finished yet"),
    (Status::Canceled, "Nothing canceled"),
];

/// The list's groups, in Linear's order: what's closest to done first.
const LIST_ORDER: [Status; 5] = [Status::InReview, Status::InProgress, Status::Todo, Status::Done, Status::Canceled];

struct Page {
    show_canceled: bool,
    layout: Layout,
    /// The rows folded shut: their parent's key, "" for the issues with no parent, or `status:<status>` for a list group.
    /// None until the tab's state is read.
    folded: Option<Vec<String>>,
    query: String,
    /// An error from the last change.
    error: Option<String>,
    /// What a search looks at, made again when the issues change.
    index: Option<(u64, Rc<Index>)>,
    /// How many more than the first step each column, lane cell or the list shows, by its key, after a Show more.
    more: HashMap<String, usize>,
}

thread_local! {
    static PAGE: RefCell<Option<Page>> = const { RefCell::new(None) };
}

fn with<T>(change: impl FnOnce(&mut Page) -> T) -> T {
    PAGE.with(|page| change(page.borrow_mut().as_mut().expect("started")))
}

/// Does `job` and shows why when it failed.
fn run(job: impl Future<Output = Result<(), String>> + 'static) {
    with(|p| p.error = None);
    spawn(async move {
        if let Err(error) = job.await {
            with(|p| p.error = Some(error));
        }
    });
}

fn pick(layout: Layout) {
    with(|p| p.layout = layout);
    drop(settings::set_last_layout(Some(layout)));
}

fn fold(lane: &str, open: bool) {
    let folded = with(|p| {
        let folded = p.folded.get_or_insert_with(Vec::new);
        if open {
            folded.retain(|k| k != lane);
        } else if !folded.iter().any(|k| k == lane) {
            folded.push(lane.to_owned());
        }
        folded.clone()
    });
    drop(host::set_tab_state(&json!({ "folded": folded })));
}

fn is_closed(i: &Issue) -> bool {
    matches!(i.status, Status::Done | Status::Canceled)
}

fn move_issue(key: &str, status: Status, by: i32) {
    let at = Status::FLOW.iter().position(|s| *s == status).map_or(-1, |at| at as i32);
    let Some(next) = usize::try_from(at + by).ok().and_then(|to| Status::FLOW.get(to)) else {
        return;
    };
    run(feed::apply(Op::Update(Update { key: key.to_owned(), status: Some(next.as_str().to_owned()), ..Update::default() })));
}

fn open_issue(key: &str) {
    if let Some(Some(project)) = feed::project() {
        feed::open_issue(key, &feed::with(|f| f.only.clone()).unwrap_or(project.id), None, false);
    }
}

// ---------- the pieces ----------

/// A card: key and the session marker, the title (a button that opens the issue), when it last changed and buttons to move
/// it a column or delete it. Alt+Left and Alt+Right on the title move it too, Delete asks to delete it.
fn card(i: &Issue, issues: &[Issue], hide_parent: bool) -> Node {
    let index = Status::FLOW.iter().position(|s| *s == i.status);
    let target = |by: i32| index.and_then(|at| usize::try_from(at as i32 + by).ok()).and_then(|to| Status::FLOW.get(to)).unwrap_or(&i.status).label();
    let (status, key) = (i.status, i.key.clone());
    let (for_keys, for_open, for_delete, for_back, for_forward) = (key.clone(), key.clone(), key.clone(), key.clone(), key);
    let keys = move |event: &Value| {
        if is_delete_key(event) {
            return ask_to_delete(&for_keys);
        }
        if index.is_none() || event["altKey"] != true {
            return;
        }
        match event["key"].as_str() {
            Some("ArrowRight") => move_issue(&for_keys, status, 1),
            Some("ArrowLeft") => move_issue(&for_keys, status, -1),
            _ => {}
        }
    };
    let key_line = row("xs")
        .maybe(i.priority.map(|p| priority_icon(Some(p), true)))
        .child(type_icon(i.kind, false, 14))
        .child(line(i.key.as_str(), Some("muted"), Some("tiny")).prop("mono", true))
        .maybe(i.parent.as_ref().filter(|_| !hide_parent).map(|p| line(format!("· {p}"), Some("muted"), Some("tiny")).prop("mono", true)));
    let (done, total) = if i.kind.can_parent() { progress(children_of(issues, &i.key)) } else { (0, 0) };
    let mut moves = row("s");
    moves = moves.child(
        Node::new("Button")
            .prop("size", "icon-xs")
            .prop("variant", "ghost")
            .prop("label", format!("Delete {}", i.key))
            .on("onClick", move |_| ask_to_delete(&for_delete))
            .child(Node::new("Icon").prop("name", "trash")),
    );
    if let Some(at) = index {
        moves = moves
            .child(Node::new("Button").prop("size", "xs").prop("variant", "ghost").prop("disabled", at == 0).prop("label", format!("Move {} to {}", i.key, target(-1))).on("onClick", move |_| move_issue(&for_back, status, -1)).text("←"))
            .child(Node::new("Button").prop("size", "xs").prop("variant", "ghost").prop("disabled", at == Status::FLOW.len() - 1).prop("label", format!("Move {} to {}", i.key, target(1))).on("onClick", move |_| move_issue(&for_forward, status, 1)).text("→"));
    }
    Node::new("Card")
        .key(i.key.as_str())
        .child(row("s").prop("justify", "between").prop("wrap", true).child(key_line).maybe(session_marker(i)))
        .child(Node::new("ListButton").on("onClick", move |_| open_issue(&for_open)).on("onKeyDown", keys).child(Node::new("Text").prop("clamp", true).text(i.title.as_str())))
        .maybe((total > 0).then(|| child_progress(done, total)))
        .child(row("s").prop("justify", "between").child(line(when(if i.status == Status::Done { "Done " } else { "Updated " }, i.updated_at), Some("muted"), Some("tiny"))).child(moves))
}

fn column_title(status: Status) -> Node {
    row("s").child(status_dot(status)).child(Node::new("Text").text(status.label()))
}

/// One row's open/close bar and title, for a list group.
fn list_group(open: bool, group: &str, summary: Node, body: Vec<Node>) -> Node {
    let group = group.to_owned();
    stack("s")
        .child(Node::new("Card").child(Node::new("ListButton").prop("aria-expanded", open).on("onClick", move |_| fold(&group, !open)).child(Node::new("Text").prop("weight", "strong").child(summary))))
        .maybe(open.then(|| stack("s").children(body)))
}

fn delete_button(key: &str) -> impl Fn(&Value) + 'static {
    let key = key.to_owned();
    move |event: &Value| {
        if is_delete_key(event) {
            ask_to_delete(&key);
        }
    }
}

/// One issue as a row: key, type and title, then its parent (in the status list), its children's progress, the session
/// marker and when it changed. With a `depth` it sits in the tree by parent: indented, with its status shown. Delete on the
/// row asks to delete it.
fn issue_row(i: &Issue, issues: &[Issue], parent: Option<&Issue>, depth: Option<usize>) -> Node {
    let (done, total) = if i.kind.can_parent() { progress(children_of(issues, &i.key)) } else { (0, 0) };
    let for_open = i.key.clone();
    let mut button = Node::new("ListButton").prop("label", format!("Open {}: {}", i.key, i.title)).on("onClick", move |_| open_issue(&for_open)).on("onKeyDown", delete_button(&i.key));
    if let Some(depth) = depth.filter(|d| *d > 0) {
        button = button.prop("indent", 41 + (depth - 1) * 24);
    }
    if depth.is_some_and(|d| d > 1) {
        button = button.child(muted("└").prop("size", "small"));
    }
    button = button
        .child(column(PRIORITY_WIDTH).child(priority_icon(i.priority, true)))
        .child(column(KEY_WIDTH).child(key_text(&i.key)))
        .child(type_icon(i.kind, false, 14))
        .child(title_text(i, false))
        .maybe(parent.map(|p| row("s").child(type_icon(p.kind, false, 14)).child(line(p.title.as_str(), Some("muted"), Some("tiny")).prop("truncate", true).prop("grow", true))))
        .maybe((total > 0).then(|| child_progress(done, total)))
        .maybe(session_marker(i))
        .maybe(depth.map(|_| status_column(i.status)))
        .child(time_column(Some(i.updated_at)));
    Node::new("ListItem").key(i.key.as_str()).child(button)
}

/// A row's title: one line, muted for an issue that is finished, and strong for a group's head.
fn title_text(i: &Issue, strong: bool) -> Node {
    let node = Node::new("Text").prop("grow", true).prop("truncate", true).text(i.title.as_str());
    let node = if is_closed(i) { node.prop("tone", "muted") } else { node };
    if strong { node.prop("weight", "strong") } else { node }
}

fn key_text(key: &str) -> Node {
    line(key, Some("muted"), Some("tiny")).prop("mono", true)
}

/// A row's status, muted and as wide as the column that keeps them in line down the list.
fn status_column(status: Status) -> Node {
    column(92).child(row("s").child(status_dot(status)).child(line(status.label(), Some("muted"), Some("tiny"))))
}

/// A row's time, as wide as the column that keeps them in line; the empty one is a group head's, which takes 13 points more.
fn time_column(updated_at: Option<f64>) -> Node {
    match updated_at {
        Some(at) => column(56).child(row("s").prop("justify", "end").child(line(crate::parts::ago_text(at), Some("muted"), Some("tiny")))),
        None => column(43),
    }
}

fn lane_node(lane: &Lane, issues: &[Issue], folded: &[String]) -> Node {
    let lane_key = lane.parent.map_or("", |p| p.key.as_str()).to_owned();
    let open = !folded.contains(&lane_key);
    let label = lane.parent.map_or_else(|| "No parent".to_owned(), |p| format!("{} {}", p.key, p.title));
    let toggle_key = lane_key.clone();
    let title = match lane.parent {
        Some(parent) => {
            let for_open = parent.key.clone();
            row("s")
                .child(status_dot(parent.status))
                .child(type_icon(parent.kind, true, 14))
                .child(key_text(&parent.key))
                .child(Node::new("Button").prop("variant", "ghost").prop("size", "xs").on("onClick", move |_| open_issue(&for_open)).child(line(parent.title.as_str(), None, None).prop("grow", true).prop("truncate", true)))
        }
        None => muted("No parent").prop("size", "small"),
    };
    let actions = lane.parent.map(|parent| {
        let (done, total) = progress(children_of(issues, &parent.key));
        row("s").child(muted(parent.status.label()).prop("size", "small")).maybe((total > 0).then(|| child_progress(done, total)))
    });
    let header = row("s")
        .child(Node::new("Button").prop("variant", "ghost").prop("label", label.as_str()).on("onClick", move |_| fold(&toggle_key, !open)).text(if open { "▾" } else { "▸" }))
        .child(title)
        .child(Node::new("Badge").prop("variant", "secondary").text(lane.count.to_string()))
        .maybe(actions);
    let cells = lane.columns.iter().map(|c| {
        let key = format!("{lane_key}:{}", c.status.as_str());
        let shown = limit(&key, COLUMN_CARDS).min(c.issues.len());
        Node::new("BoardColumn")
            .key(c.status.as_str())
            .prop("label", c.status.label())
            .prop("cell", true)
            .child(slot("body", stack("s").children(c.issues.iter().take(shown).map(|i| card(i, issues, true))).maybe(show_more(&key, c.issues.len(), shown, COLUMN_CARDS))))
    });
    stack("s").key(lane_key.as_str()).child(header).maybe(open.then(|| Node::new("BoardRow").children(cells)))
}

fn search_index(store: &Store, version: u64) -> Rc<Index> {
    with(|p| match &p.index {
        Some((v, index)) if *v == version => index.clone(),
        _ => {
            let index = Rc::new(Index::new(&store.issues));
            p.index = Some((version, index.clone()));
            index
        }
    })
}

// ---------- the page ----------


fn view() -> Vec<Node> {
    let project = feed::project();
    let Some(project) = project else {
        return vec![page_with(vec![header("", None)], [loading().text("Loading issues")])];
    };
    let Some(project) = project else {
        return vec![page_with(vec![header("", None)], [empty("No project open", "Open a project to see its issues.")])];
    };
    let (store, read_error, version) = feed::with(|f| (f.store.clone(), f.error.clone(), f.version));
    if let (None, Some(error)) = (&store, &read_error) {
        return vec![page_with(vec![header(&project.name, None)], [empty("Couldn't read the issues", error)])];
    }
    let tab_ready = with(|p| p.folded.is_some());
    if !tab_ready {
        return vec![page_with(vec![header(&project.name, None)], [loading().text("Loading issues")])];
    }
    build(&project, store.as_deref(), version)
}

fn build(project: &Project, store: Option<&Store>, version: u64) -> Vec<Node> {
    let (layout, show_canceled, query, error, folded) = with(|p| (p.layout, p.show_canceled, p.query.clone(), p.error.clone(), p.folded.clone().unwrap_or_default()));
    let all: Option<&[Issue]> = store.map(|s| s.issues.as_slice());
    let searching = !query.trim().is_empty();
    // Summary counts and the Group by parent chip stay on every issue; what's below the toolbar is what matches. Each issue's
    // text is lowercased once (`Index`), so a keystroke is a substring test per issue.
    let shown: Option<Vec<&Issue>> = all.map(|all| match store.filter(|_| searching) {
        Some(store) => search_index(store, version).matching(&query).into_iter().map(|at| &all[at]).collect(),
        None => all.iter().collect(),
    });
    let has_parents = all.is_some_and(|all| all.iter().any(|i| i.parent.is_some()));
    let count = |status: Status| all.map_or(0, |all| all.iter().filter(|i| i.status == status).count());
    let by_parent = has_parents && layout.by_parent && !layout.list;

    let search = all.map(|_| search_field("Search issues", &query, |text| with(|p| p.query = text.to_owned())));
    let mut top = vec![header(&project.name, search)];
    if let Some(all) = all {
        top.push(filter_bar(layout, has_parents, show_canceled, count(Status::Canceled), all));
    }
    top.extend(add_issue::bar());

    let mut body: Vec<Node> = Vec::new();
    if let Some(error) = &error {
        body.push(stack("s").child(error_text(error)));
    }
    if let (Some(shown), Some(all)) = (&shown, all) {
        let doomed = asking().and_then(|key| shown.iter().find(|i| i.key == key).copied());
        if let Some(confirm) = doomed.and_then(|issue| confirm_node(issue, all)) {
            body.push(confirm);
        }
    }
    match (&shown, all) {
        (Some(shown), Some(all)) => {
            if searching && shown.is_empty() {
                body.push(stack("s").child(muted(format!("No issues match \"{}\".", query.trim())).prop("role", "status")));
            } else if layout.list {
                body.push(issue_list(project, shown, all, by_parent_list(layout, has_parents), show_canceled, layout.sort, &folded));
            } else if by_parent {
                body.push(lanes_board(project, shown, all, &folded, show_canceled, layout.sort));
            } else {
                body.push(plain_board(project, shown, all, show_canceled, layout.sort));
            }
        }
        _ => body.push(loading().text("Loading issues")),
    }
    let flush = all.is_some() && !layout.list;
    let page = page_with(top, body);
    vec![if flush { page.prop("flush", true) } else { page }]
}

/// How wide the header's search field is.
const SEARCH_WIDTH: u32 = 220;

/// The tab's header, as Claude chat's: "Issues" strong with the project muted after it, and the search and New issue at
/// the end.
fn header(project: &str, search: Option<Node>) -> Node {
    let actions = row("s")
        .prop("align", "center")
        .maybe(search.map(|search| stack("none").prop("width", SEARCH_WIDTH).child(search)))
        .child(add_issue::button());
    toolbar_titled(title_with_context("Issues", project), Some(actions))
}

/// How the issues are shown, under the header: the view and the order on tracks, what's left out as ticks, and the counts
/// of the whole project at the end.
fn filter_bar(layout: Layout, has_parents: bool, show_canceled: bool, canceled: usize, all: &[Issue]) -> Node {
    let view = chips("View", &[("board", "Board"), ("list", "List")], if layout.list { "list" } else { "board" }, move |value| pick(Layout { list: value == "list", ..layout }))
        .segmented();
    let sort = row("s")
        .prop("align", "center")
        .child(muted("Sort").prop("size", "small"))
        .child(chips("Sort", &Sort::OPTIONS, layout.sort.as_str(), move |value| pick(Layout { sort: Sort::parse(value).unwrap_or_default(), ..layout })).segmented());
    let choices = row("l")
        .prop("wrap", true)
        .prop("align", "center")
        .child(view)
        .child(sort)
        .maybe(has_parents.then(|| checkbox("Group by parent", layout.by_parent, move |on| pick(Layout { by_parent: on, ..layout }))))
        .child(checkbox(format!("Show canceled ({canceled})"), show_canceled, |on| with(|p| p.show_canceled = on)));
    Node::new("Bar")
        .prop("edge", "bottom")
        .child(row("m").prop("wrap", true).prop("align", "center").prop("justify", "between").child(choices).child(status_summary(all)))
}

fn by_parent_list(layout: Layout, has_parents: bool) -> bool {
    has_parents && layout.by_parent
}

/// How many of the column, lane cell or list `key` shows: `step` at first, and `step` more after each Show more.
fn limit(key: &str, step: usize) -> usize {
    step + with(|p| p.more.get(key).copied().unwrap_or(0))
}

/// What's past the `shown` of `total`, and a button that shows `step` more of `key`.
fn show_more(key: &str, total: usize, shown: usize, step: usize) -> Option<Node> {
    let left = total.saturating_sub(shown);
    (left > 0).then(|| {
        let key = key.to_owned();
        row("s")
            .key(format!("more-{key}"))
            .prop("wrap", true)
            .child(muted(format!("{left} more not shown")).prop("size", "small"))
            .child(Node::new("Button").prop("variant", "ghost").prop("size", "sm").on("onClick", move |_| with(|p| *p.more.entry(key.clone()).or_default() += step)).text(format!("Show {} more", left.min(step))))
    })
}

fn plain_board(project: &Project, shown: &[&Issue], all: &[Issue], show_canceled: bool, sort: Sort) -> Node {
    let columns = board_columns(shown.iter().copied(), show_canceled, sort);
    Node::new("Board").prop("label", format!("Issues for {}", project.name)).children(columns.into_iter().map(|Column { status, issues: column }| {
        let empty = EMPTY.iter().find(|(s, _)| *s == status).map_or("", |(_, text)| *text);
        let shown = limit(status.as_str(), COLUMN_CARDS).min(column.len());
        let more = show_more(status.as_str(), column.len(), shown, COLUMN_CARDS);
        Node::new("BoardColumn")
            .key(status.as_str())
            .prop("count", column.len())
            .child(slot("title", column_title(status)))
            .child(slot("body", stack("s").maybe(column.is_empty().then(|| muted(empty).prop("size", "small"))).children(column.iter().take(shown).map(|i| card(i, all, false))).maybe(more)))
    }))
}

fn lanes_board(project: &Project, shown: &[&Issue], all: &[Issue], folded: &[String], show_canceled: bool, sort: Sort) -> Node {
    let lanes = board_lanes(shown, show_canceled, sort);
    let statuses: &[Status] = if show_canceled { &Status::ALL } else { &Status::FLOW };
    let header = Node::new("BoardRow").children(statuses.iter().map(|status| {
        let count = shown.iter().filter(|i| i.status == *status).count();
        Node::new("BoardColumn").key(status.as_str()).prop("count", count).child(slot("title", column_title(*status)))
    }));
    Node::new("Board")
        .prop("label", format!("Issues for {}, by parent", project.name))
        .prop("lanes", true)
        .child(slot("header", header))
        .child(Node::new("Slot").prop("name", "body").children(lanes.iter().map(|lane| lane_node(lane, all, folded))))
}

/// Issues as rows like Linear's list: by parent, each epic over its children indented under it, or by status. Groups fold.
fn issue_list(project: &Project, shown: &[&Issue], all: &[Issue], by_parent: bool, show_canceled: bool, sort: Sort, folded: &[String]) -> Node {
    let is_open = |group: &str| !folded.iter().any(|k| k == group);
    let mut budget = limit("list", LIST_ROWS);
    let mut take = |wanted: usize| {
        let given = wanted.min(budget);
        budget -= given;
        given
    };
    let mut out = stack("l").prop("maxWidth", 1100);
    let mut hidden = 0;
    if by_parent {
        let (groups, loose) = issue_tree(shown, show_canceled, sort);
        if groups.len() + loose.len() == 0 {
            return muted(format!("No issues for {} yet.", project.name));
        }
        for group in &groups {
            let root = group.root;
            let open_group = is_open(&root.key);
            let fold_key = root.key.clone();
            let (for_open, for_title) = (root.key.clone(), root.title.clone());
            let (done, total) = progress(children_of(all, &root.key));
            let head = Node::new("Card").child(
                row("s")
                    .child(Node::new("Button").prop("variant", "ghost").prop("size", "icon-xs").prop("label", for_title).on("onClick", move |_| fold(&fold_key, !open_group)).child(Node::new("Icon").prop("name", "chevron-down")))
                    .child(
                        Node::new("ListButton")
                            .prop("label", format!("Open {}: {}", root.key, root.title))
                            .on("onClick", move |_| open_issue(&for_open))
                            .on("onKeyDown", delete_button(&root.key))
                            .child(column(PRIORITY_WIDTH).child(priority_icon(root.priority, true)))
                            .child(type_icon(root.kind, true, 14))
                            .child(column(KEY_WIDTH).child(key_text(&root.key)))
                            .child(title_text(root, true))
                            .maybe((total > 0).then(|| child_progress(done, total)))
                            .child(status_column(root.status))
                            .child(time_column(None)),
                    ),
            );
            let rows = take(group.rows.len());
            hidden += group.rows.len() - rows;
            let by_key: Vec<Node> = group.rows.iter().take(rows).map(|(i, depth)| issue_row(i, all, None, Some(*depth))).collect();
            out = out.child(stack("s").key(root.key.as_str()).child(head).maybe(open_group.then(|| Node::new("List").children(by_key))));
        }
        if !loose.is_empty() {
            let rows = take(loose.len());
            hidden += loose.len() - rows;
            let summary = row("s").child(line("No parent", None, None)).child(muted(loose.len().to_string()).prop("size", "small"));
            let body = vec![Node::new("List").children(loose.iter().take(rows).map(|i| issue_row(i, all, None, Some(0))))];
            out = out.child(list_group(is_open(""), "", summary, body));
        }
    } else {
        let mut groups: Vec<Column> = board_columns(shown.iter().copied(), show_canceled, sort).into_iter().filter(|g| !g.issues.is_empty()).collect();
        groups.sort_by_key(|g| LIST_ORDER.iter().position(|s| *s == g.status));
        if groups.is_empty() {
            return muted(format!("No issues for {} yet.", project.name));
        }
        let by_key: std::collections::HashMap<&str, &Issue> = all.iter().map(|i| (i.key.as_str(), i)).collect();
        for Column { status, issues: rows } in &groups {
            let group = format!("status:{}", status.as_str());
            let shown_rows = take(rows.len());
            hidden += rows.len() - shown_rows;
            let summary = row("s").child(status_dot(*status)).child(line(status.label(), None, None)).child(muted(rows.len().to_string()).prop("size", "small"));
            let body = vec![Node::new("List").children(rows.iter().take(shown_rows).map(|i| issue_row(i, all, i.parent.as_deref().and_then(|p| by_key.get(p).copied()), None)))];
            out = out.child(list_group(is_open(&group), &group, summary, body));
        }
    }
    out.maybe(show_more("list", shown.len(), shown.len() - hidden, LIST_ROWS))
}

#[cfg_attr(not(test), unsafe(no_mangle))]
pub fn start() {
    let layout = settings::last_layout().unwrap_or_else(|| settings::start_layout(&settings::default_view()));
    PAGE.with(|page| *page.borrow_mut() = Some(Page { show_canceled: false, layout, folded: None, query: String::new(), error: None, index: None, more: HashMap::new() }));
    wings_plugin::run_filled(view);
    feed::watch(None);
    // The folded rows are kept with the tab.
    spawn(async {
        let saved = host::tab_state().await.unwrap_or(Value::Null);
        let folded: Vec<String> = saved["folded"].as_array().map(|k| k.iter().filter_map(|k| k.as_str().map(str::to_owned)).collect()).unwrap_or_default();
        with(|p| p.folded = Some(folded));
    });
}


#[cfg(test)]
mod tests {
    use super::*;

    fn json(node: Node) -> String {
        wings_plugin::ui::tree_json(node).to_string()
    }

    /// The header is the design system's: "Issues" strong and the project muted after it, never "Issues, web", with the
    /// search at a fixed width and New issue as an icon button at the end.
    #[test]
    fn the_header_names_the_page_and_its_project_apart_with_search_and_new_issue_at_the_end() {
        let header = wings_plugin::ui::tree_json(header("web", Some(search_field("Search issues", "", |_| {}))));
        let text = header.to_string();
        assert!(!text.contains("Issues, web"), "{text}");
        assert!(text.contains(r#""weight":"strong""#) && text.contains(r#""tone":"muted""#), "{text}");
        assert!(text.contains(&format!(r#""width":{SEARCH_WIDTH}"#)) && text.contains(r#""label":"New issue""#), "{text}");
    }

    /// A column longer than its first 30 cards shows 30 more a click, each column on its own, until all show.
    #[test]
    fn a_long_column_shows_more_in_steps_until_all_show() {
        PAGE.with(|page| *page.borrow_mut() = Some(Page { show_canceled: false, layout: settings::start_layout("board"), folded: None, query: String::new(), error: None, index: None, more: HashMap::new() }));
        assert_eq!(limit("todo", COLUMN_CARDS), 30);
        let more = json(show_more("todo", 75, 30, COLUMN_CARDS).unwrap());
        assert!(more.contains("45 more not shown") && more.contains("Show 30 more"), "{more}");
        with(|p| *p.more.entry("todo".into()).or_default() += COLUMN_CARDS);
        assert_eq!(limit("todo", COLUMN_CARDS), 60);
        assert!(json(show_more("todo", 75, 60, COLUMN_CARDS).unwrap()).contains("Show 15 more"));
        assert!(show_more("todo", 75, 75, COLUMN_CARDS).is_none());
        assert_eq!(limit("done", COLUMN_CARDS), 30, "each column keeps its own");
    }

    /// How the issues are shown sits in a bar under the header: the view and the order on tracks, the rest as ticks.
    #[test]
    fn the_filter_bar_puts_view_and_sort_on_tracks_and_the_toggles_on_ticks() {
        let bar = json(filter_bar(settings::start_layout("board"), true, false, 2, &[]));
        assert!(bar.contains(r#""type":"Bar""#), "{bar}");
        assert_eq!(bar.matches(r#""segmented":true"#).count(), 2, "{bar}");
        assert_eq!(bar.matches(r#""type":"Checkbox""#).count(), 2, "{bar}");
        assert!(bar.contains("Show canceled (2)") && bar.contains("Group by parent"), "{bar}");
        // No parents: no Group by parent.
        assert_eq!(json(filter_bar(settings::start_layout("board"), false, false, 0, &[])).matches(r#""type":"Checkbox""#).count(), 1);
    }
}
