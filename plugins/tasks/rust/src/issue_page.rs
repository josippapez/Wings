//! The Issue tab: one issue of a project, opened with args { key, projectId, trail }; trail is the issues opened before it in
//! this tab, for the breadcrumb back. Edit its title, status, description and links, add a note, resume any agent session that
//! worked on it, or start a new agent on it.

use wings_plugin::markdown::markdown;
use crate::{
    actor::{actor_name, last_session},
    commands::{AGENTS, agent_command, agent_label},
    delete_confirm::{self, ask_to_delete},
    domain::{Issue, IssueType, Op, Priority, Relations, Status, Update, limits, normalize_key, visible_description},
    feed::{self, Project},
    file_diff::line_counts,
    ops::description_to_edit,
    orderings::{TRAIL_LENGTH, trail_to},
    parts::{button, child_progress, error_text, line, muted, priority_icon, priority_name, row, stack, status_dot, type_icon},
    relations::{blocks_of, children_of, parent_choices, progress},
};
use serde_json::{Value, json};
use std::{cell::RefCell, collections::HashMap, future::Future, rc::Rc};
use wings_plugin::{
    format::ago_now,
    host, messages,
    task::spawn,
    ui::{self, Choice, Combobox, Node, combobox_submit, empty, icon_button, icons, loading, page, select, slot},
};

/// The log entries the activity shows at first, newest first, and how many more each Show earlier adds.
const LOG_SHOWN: usize = 100;
/// Rows of Files changed shown before the rest fold away.
const FILE_ROWS: usize = 10;

struct Args {
    key: String,
    project_id: Option<String>,
    trail: Vec<String>,
}

/// What is being typed into one editor, and the last save's outcome.
#[derive(Default)]
struct Edit {
    draft: String,
    busy: bool,
    error: Option<String>,
}

#[derive(Default)]
struct Page {
    args: Option<Args>,
    /// An error from the last change.
    error: Option<String>,
    edits: HashMap<&'static str, Edit>,
    /// What is typed in each issue picker.
    queries: HashMap<&'static str, String>,
    /// The text last copied, for the "Copied" a button shows for a moment.
    copied: Option<String>,
    /// The issue whose activity shows more than its newest entries, and how many more, after Show earlier.
    earlier: Option<(String, usize)>,
    /// The page is narrower than `SIDE_BY_SIDE`: the properties go above the text instead of beside it.
    narrow: bool,
    /// The relation whose issue picker is open: a field shows only after its button is pressed.
    picking: Option<&'static str>,
    /// The issue whose commits all show, not only the first few.
    commits_open: Option<String>,
}

/// The least width at which the properties sit beside the description. Under it the text and its tables would be left
/// with less than a table needs, so the properties stack above it.
const SIDE_BY_SIDE: f32 = 760.0;
/// How wide one property is, beside the description and in the grid above it.
const PROPERTY_WIDTH: f32 = 300.0;
/// The widest the text column is, centred in what the properties leave.
const TEXT_WIDTH: f32 = 700.0;
/// Commits shown before "+N more".
const COMMITS_SHOWN: usize = 3;
/// The width of a property's name, so the values line up in a column.
const NAME_WIDTH: u32 = 76;

thread_local! {
    static PAGE: RefCell<Page> = RefCell::new(Page::default());
}

fn with<T>(change: impl FnOnce(&mut Page) -> T) -> T {
    PAGE.with(|page| change(&mut page.borrow_mut()))
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

fn update(key: &str) -> Update {
    Update { key: key.to_owned(), ..Update::default() }
}

fn send(change: Update) {
    run(feed::apply(Op::Update(change)));
}

fn project_id() -> String {
    with(|p| p.args.as_ref().and_then(|a| a.project_id.clone())).or_else(|| feed::project().flatten().map(|p| p.id)).unwrap_or_default()
}

// ---------- editing in place ----------

fn begin(name: &'static str, value: &str) {
    with(|p| p.edits.insert(name, Edit { draft: value.to_owned(), busy: false, error: None }));
}

fn end(name: &'static str) {
    with(|p| p.edits.remove(name));
}

/// Enter or leaving the field saves once, Escape cancels, a failed save keeps the draft and says why.
fn save(name: &'static str, current: String, required: bool, make: Rc<dyn Fn(String) -> Op>) {
    let Some(next) = with(|p| p.edits.get(name).filter(|e| !e.busy).map(|e| e.draft.trim().to_owned())) else {
        return;
    };
    if next == current || (required && next.is_empty()) {
        return end(name);
    }
    with(|p| {
        if let Some(edit) = p.edits.get_mut(name) {
            edit.busy = true;
            edit.error = None;
        }
    });
    spawn(async move {
        let done = feed::apply(make(next)).await;
        with(|p| match done {
            Ok(()) => drop(p.edits.remove(name)),
            Err(error) => {
                if let Some(edit) = p.edits.get_mut(name) {
                    edit.busy = false;
                    edit.error = Some(error);
                }
            }
        });
    });
}

/// A one-line field that edits in place.
fn inline_input(name: &'static str, label: &'static str, current: &str, max: usize, placeholder: &str, required: bool, make: Rc<dyn Fn(String) -> Op>) -> Node {
    let (draft, busy, error) = with(|p| p.edits.get(name).map(|e| (e.draft.clone(), e.busy, e.error.clone()))).unwrap_or_default();
    let (blur, enter, current_for_blur) = (make.clone(), make, current.to_owned());
    let current_for_enter = current.to_owned();
    let mut field = Node::new("TextInput")
        .prop("label", label)
        .prop("value", draft.as_str())
        .prop("maxLength", max)
        .prop("disabled", busy)
        .prop("autoFocus", true)
        .prop("selectOnFocus", true)
        .on("onChange", move |event| {
            with(|p| {
                if let Some(edit) = p.edits.get_mut(name) {
                    edit.draft = event["value"].as_str().unwrap_or_default().to_owned();
                    edit.error = None;
                }
            })
        })
        .on("onBlur", move |_| save(name, current_for_blur.clone(), required, blur.clone()))
        .on("onKeyDown", move |event| match event["key"].as_str() {
            Some("Enter") => save(name, current_for_enter.clone(), required, enter.clone()),
            Some("Escape") => end(name),
            _ => {}
        });
    if !placeholder.is_empty() {
        field = field.prop("placeholder", placeholder);
    }
    stack("xs").child(field).maybe(error.map(|e| Node::new("Notice").prop("tone", "danger").text(e)))
}

// ---------- the title ----------

fn title_row(issue: &Issue) -> Node {
    let key = issue.key.clone();
    let editing = with(|p| p.edits.contains_key("title"));
    let key_text = line(issue.key.as_str(), Some("muted"), Some("small")).prop("mono", true);
    if editing {
        let input = inline_input("title", "Title", &issue.title, limits::TITLE, "", true, Rc::new(move |title| Op::Update(Update { title: Some(title), ..update(&key) })));
        return row("m").child(type_icon(issue.kind, true, 18)).child(row("l").prop("align", "baseline").prop("grow", true).child(input).child(key_text));
    }
    let (title, for_key) = (issue.title.clone(), issue.title.clone());
    let button = Node::new("Button")
        .prop("variant", "ghost")
        .prop("wrap", true)
        .prop("label", "Edit the title")
        .on("onClick", move |_| begin("title", &title))
        .on("onKeyDown", move |event| {
            if event["key"] == "F2" {
                begin("title", &for_key);
            }
        })
        .child(Node::new("Text").prop("weight", "strong").text(issue.title.as_str()));
    row("none").child(type_icon(issue.kind, true, 18)).child(row("m").prop("align", "baseline").prop("grow", true).child(button).child(key_text))
}

// ---------- the properties column ----------

/// One property of the column, quiet: its muted name on the left, its value beside it and one small icon button at the end.
/// The name lines up with the value's first line, so a value of several lines doesn't float it in the middle.
/// Width of the slot a status dot or an icon is centred in, so the values beside them line up.
const MARK_SLOT: u32 = 16;

fn prop(label: &str, actions: Option<Node>, content: Vec<Node>) -> Node {
    row("s")
        .prop("align", "start")
        .child(ui_column(NAME_WIDTH).child(line(label, Some("muted"), Some("small"))))
        .child(stack("xs").prop("grow", true).children(content))
        .maybe(actions)
}

/// A fixed-width cell of a row.
fn ui_column(width: u32) -> Node {
    crate::parts::column(width)
}

fn copy(text: String) {
    spawn(async move {
        drop(host::copy_text(&text).await);
        with(|p| p.copied = Some(text.clone()));
        host::sleep(1500).await;
        with(|p| {
            if p.copied.as_ref() == Some(&text) {
                p.copied = None;
            }
        });
    });
}

fn copy_button(label: &str, text: &str, variant: &str, aria: String) -> Node {
    let copied = with(|p| p.copied.as_deref() == Some(text));
    let text = text.to_owned();
    Node::new("Button").prop("size", "xs").prop("variant", variant).prop("label", aria).on("onClick", move |_| copy(text.clone())).text(if copied { "Copied" } else { label })
}

/// An icon button that copies `text`, and says "Copied" for a moment after.
fn small(button: Node) -> Node {
    button.prop("size", "icon-xs")
}

fn copy_icon(text: &str, aria: &str) -> Node {
    let copied = with(|p| p.copied.as_deref() == Some(text));
    let text = text.to_owned();
    small(icon_button(if copied { "Copied" } else { aria }, &icons::COPY, move || copy(text.clone())))
}

/// An icon button that starts editing `name`, from `current`.
fn edit_icon(name: &'static str, aria: &str, current: &str) -> Node {
    let current = current.to_owned();
    small(icon_button(aria, &icons::PENCIL, move || begin(name, &current)))
}

fn branch_prop(issue: &Issue, project: &Project) -> Node {
    let branch = issue.links.as_ref().and_then(|l| l.branch.clone()).unwrap_or_default();
    let editing = with(|p| p.edits.contains_key("branch"));
    let key = issue.key.clone();
    let make: Rc<dyn Fn(String) -> Op> = Rc::new(move |value| Op::Update(Update { branch: Some(value), ..update(&key) }));
    let actions = (!editing).then(|| {
        row("none").maybe((!branch.is_empty()).then(|| copy_icon(&branch, "Copy the branch name"))).child(edit_icon("branch", "Edit the branch", &branch))
    });
    let value = if editing {
        inline_input("branch", "Branch", &branch, limits::BRANCH, "feature/name", false, make)
    } else if branch.is_empty() {
        muted("None").prop("size", "small")
    } else {
        line(branch.as_str(), None, Some("small")).prop("mono", true)
    };
    let use_current = project.branch.clone().filter(|current| !editing && *current != branch).map(|current| {
        let (key, name) = (issue.key.clone(), current.clone());
        Node::new("Button")
            .prop("size", "xs")
            .prop("variant", "ghost")
            .on("onClick", move |_| send(Update { branch: Some(name.clone()), ..update(&key) }))
            .text("Use ")
            .child(line(current.as_str(), None, Some("small")).prop("mono", true).prop("truncate", true))
    });
    stack("xs").child(prop("Branch", actions, vec![value])).maybe(use_current)
}

/// What a pull request link is called: "PR #12", or its host.
fn pr_label(url: &str) -> String {
    for marker in ["/pullrequests/", "/pullrequest/", "/pull/", "/merge_requests/"] {
        if let Some((_, rest)) = url.split_once(marker) {
            let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
            if !digits.is_empty() {
                return format!("PR #{digits}");
            }
        }
    }
    match url.split_once("://") {
        Some((_, rest)) => rest.split(['/', '?', '#']).next().unwrap_or(url).to_owned(),
        None => url.to_owned(),
    }
}

fn open_url(url: String) {
    run(async move { host::call("openUrl", json!({ "url": url })).await.map(drop) });
}

fn pr_prop(issue: &Issue) -> Node {
    let pr = issue.links.as_ref().and_then(|l| l.pr.clone()).unwrap_or_default();
    let editing = with(|p| p.edits.contains_key("pr"));
    let key = issue.key.clone();
    let actions = (!editing).then(|| edit_icon("pr", "Edit the pull request link", &pr));
    let value = if editing {
        inline_input("pr", "Pull request URL", &pr, limits::PR, "https://", false, Rc::new(move |value| Op::Update(Update { pr: Some(value), ..update(&key) })))
    } else if pr.is_empty() {
        muted("None").prop("size", "small")
    } else {
        let url = pr.clone();
        // In a row, so the app's column doesn't stretch the chip across it.
        row("s").child(Node::new("Button").prop("variant", "outline").prop("size", "xs").on("onClick", move |_| open_url(url.clone())).text(format!("{} ↗", pr_label(&pr))))
    };
    prop("Pull request", actions, vec![value])
}

/// Another issue as a chip, "KEY title", that opens it. With a remove, an × beside it takes the link away.
fn issue_chip(issue: Option<&Issue>, fallback: &str, on_remove: Option<Box<dyn Fn()>>) -> Node {
    let chip = match issue {
        Some(i) => {
            let (key, trail) = (i.key.clone(), ());
            let _ = trail;
            Node::new("Button")
                .prop("variant", "outline")
                .prop("size", "xs")
                .on("onClick", move |_| open_other(&key))
                .child(status_dot(i.status))
                .child(line(i.key.as_str(), Some("muted"), Some("tiny")).prop("mono", true))
                .child(Node::new("Text").prop("grow", true).prop("truncate", true).text(i.title.as_str()))
        }
        None => line(fallback, Some("muted"), Some("small")).prop("mono", true),
    };
    row("s").child(chip).maybe(on_remove.map(|remove| small(icon_button(&format!("Remove {fallback}"), &icons::CLOSE, move || remove()))))
}

/// Opens another issue in this tab, with the trail back to this one.
fn open_other(key: &str) {
    let (current, trail) = with(|p| p.args.as_ref().map(|a| (a.key.clone(), a.trail.clone()))).unwrap_or_default();
    feed::open_issue(key, &project_id(), Some(trail_to(&trail, &current, key)), false);
}

/// A field that finds an issue by key or title and hands back its key. It opens from its button and closes on a pick or Escape.
fn picker(name: &'static str, label: &'static str, placeholder: &'static str, options: Vec<&Issue>, on_pick: impl Fn(String) + 'static) -> Node {
    let value = with(|p| p.queries.get(name).cloned()).unwrap_or_default();
    combobox_submit(
        Combobox {
            label,
            placeholder,
            value,
            query: None,
            options: options.into_iter().map(|o| Choice { value: o.key.clone(), label: o.key.clone(), detail: Some(o.title.clone()) }).collect(),
            empty: "No issue matches",
            auto_focus: true,
        },
        move |text| {
            with(|p| p.queries.insert(name, text.unwrap_or_default()));
        },
        move |picked| {
            with(|p| {
                p.queries.insert(name, String::new());
                p.picking = None;
            });
            on_pick(picked.to_owned());
        },
        move |key| {
            if key == "Escape" {
                with(|p| p.picking = None);
            }
        },
    )
}

/// A relation: its chips, one small button at the end that opens `picker` under them, and "None" while it has no chips.
fn relation(name: &'static str, label: &'static str, add: &'static str, chips: Vec<Node>, options: Vec<&Issue>, on_pick: impl Fn(String) + 'static) -> Node {
    let open = with(|p| p.picking == Some(name));
    let icon = if add.starts_with("Change") { &icons::PENCIL[..] } else { &icons::PLUS[..] };
    let toggle = small(icon_button(add, icon, move || with(|p| p.picking = if p.picking == Some(name) { None } else { Some(name) })));
    let content = if chips.is_empty() { vec![muted("None").prop("size", "small")] } else { chips };
    stack("xs").child(prop(label, Some(toggle), content)).maybe(open.then(|| picker(name, label, "Find an issue", options, on_pick)))
}

fn links_props(issue: &Issue, issues: &[Issue]) -> Vec<Node> {
    let find = |key: &str| issues.iter().find(|i| i.key == key);
    let others: Vec<&Issue> = issues.iter().filter(|i| i.key != issue.key).collect();
    let key = issue.key.clone();
    let chips = |keys: &Option<Vec<String>>, remove: fn(&str, &str) -> Update| -> Vec<Node> {
        keys.iter()
            .flatten()
            .map(|k| {
                let (issue_key, other) = (key.clone(), k.clone());
                issue_chip(find(k), k, Some(Box::new(move || send(remove(&issue_key, &other)))))
            })
            .collect()
    };
    let mut props = Vec::new();
    let parent_remove = key.clone();
    let parent_chip = issue.parent.as_ref().map(|p| issue_chip(find(p), p, Some(Box::new(move || send(Update { rel: Relations { parent: Some(String::new()), ..Relations::default() }, ..update(&parent_remove) })))));
    let (pick_key, choices) = (key.clone(), parent_choices(issues, issue).into_iter().filter(|p| Some(&p.key) != issue.parent.as_ref()).collect::<Vec<_>>());
    props.push(relation(
        "parent",
        "Parent",
        if issue.parent.is_some() { "Change parent" } else { "Choose an epic or feature" },
        parent_chip.into_iter().collect(),
        choices,
        move |parent| send(Update { rel: Relations { parent: Some(parent), ..Relations::default() }, ..update(&pick_key) }),
    ));
    let related_key = key.clone();
    let related_options = others.iter().copied().filter(|o| !issue.related.as_ref().is_some_and(|r| r.contains(&o.key))).collect();
    props.push(relation(
        "related",
        "Related",
        "Add a related issue",
        chips(&issue.related, |k, other| Update { rel: Relations { related_remove: Some(vec![other.to_owned()]), ..Relations::default() }, ..update(k) }),
        related_options,
        move |k| send(Update { rel: Relations { related_add: Some(vec![k]), ..Relations::default() }, ..update(&related_key) }),
    ));
    let blocked_options = others.iter().copied().filter(|o| !issue.blocked_by.as_ref().is_some_and(|r| r.contains(&o.key))).collect();
    props.push(relation(
        "blocked",
        "Blocked by",
        "Add a blocking issue",
        chips(&issue.blocked_by, |k, other| Update { rel: Relations { blocked_by_remove: Some(vec![other.to_owned()]), ..Relations::default() }, ..update(k) }),
        blocked_options,
        move |k| send(Update { rel: Relations { blocked_by_add: Some(vec![k]), ..Relations::default() }, ..update(&key) }),
    ));
    let blocks = blocks_of(issues, &issue.key);
    if !blocks.is_empty() {
        props.push(prop("Blocks", None, blocks.into_iter().map(|b| issue_chip(Some(b), &b.key, None)).collect()));
    }
    props
}

fn options<T: Copy>(names: &[T], name: impl Fn(T) -> &'static str, label: impl Fn(T) -> &'static str) -> Vec<(String, String)> {
    names.iter().map(|n| (name(*n).to_owned(), label(*n).to_owned())).collect()
}

fn properties(issue: &Issue, issues: &[Issue], project: &Project, narrow: bool) -> Node {
    let last = last_session(issue).map(|s| s.id.clone());
    let commits = issue.links.as_ref().and_then(|l| l.commits.clone()).unwrap_or_default();
    let key = issue.key.clone();
    let (status_key, priority_key, type_key) = (key.clone(), key.clone(), key);
    let status_options = options(&Status::ALL, Status::as_str, Status::label);
    let priority_options: Vec<(String, String)> = Priority::NAMES.iter().map(|n| ((*n).to_owned(), priority_name(Priority::parse(n)).to_owned())).collect();
    let type_options = options(&IssueType::ALL, IssueType::as_str, IssueType::label);
    let picked = |label: &'static str, dot: Node, options: Vec<(String, String)>, value: &str, on_change: Box<dyn Fn(&str)>| {
        // The mark sits centred in a slot of one width so the three values start at the same x; the label centres on the select, whose own padding lifts its text.
        row("s")
            .prop("align", "center")
            .child(ui_column(NAME_WIDTH).child(line(label, Some("muted"), Some("small"))))
            .child(row("s").prop("align", "center").prop("grow", true).child(ui_column(MARK_SLOT).child(row("none").prop("justify", "center").child(dot))).child(select(label, &options, value, move |v| on_change(v)).prop("variant", "ghost")))
    };
    let mut sessions: Vec<_> = issue.sessions.iter().collect();
    sessions.sort_by(|a, b| b.seen.total_cmp(&a.seen));
    let session_rows = sessions.into_iter().map(|s| {
        // An unnamed session goes by its agent, so the line under it doesn't name the agent twice.
        let name = s.name.clone().filter(|n| !n.is_empty()).unwrap_or_else(|| agent_label(s.kind.as_deref()));
        let (id, kind) = (s.id.clone(), s.kind.clone());
        let latest = if last.as_deref() == Some(s.id.as_str()) && issue.sessions.len() > 1 { "latest · " } else { "" };
        let detail = format!("{latest}{} · {}", s.id.chars().take(8).collect::<String>(), ago_now(s.seen));
        stack("s")
            .key(s.id.as_str())
            .child(
                row("s")
                    .prop("justify", "between")
                    .child(line(name.as_str(), None, Some("small")).prop("truncate", true).prop("grow", true))
                    .child(small(icon_button(&format!("Resume {name}"), &icons::PLAY, move || resume(&id, kind.as_deref())))),
            )
            .child(line(detail, Some("muted"), Some("tiny")))
    });
    let open_commits = with(|p| p.commits_open.as_deref() == Some(issue.key.as_str()));
    let commits_key = issue.key.clone();
    let commit_chips = {
        let shown = if open_commits { commits.len() } else { COMMITS_SHOWN };
        let more = commits.len().saturating_sub(COMMITS_SHOWN);
        let toggle = (more > 0).then(|| {
            let label = if open_commits { "Show fewer".to_owned() } else { format!("+{more} more") };
            Node::new("Button").prop("variant", "ghost").prop("size", "xs").prop("label", label.as_str()).on("onClick", move |_| with(|p| p.commits_open = if open_commits { None } else { Some(commits_key.clone()) })).text(label)
        });
        row("xs").prop("wrap", true).children(commits.iter().take(shown).map(|c| copy_button(c, c, "secondary", format!("Copy commit {c}")).key(c.as_str()))).maybe(toggle)
    };
    // Groups split by hairlines, in one card: status, priority and type; the links; what the work produced; the dates.
    let group = |rows: Vec<Node>| stack("s").children(rows);
    let card = ui::rows_card([
        group(vec![
            picked("Status", status_dot(issue.status), status_options, issue.status.as_str(), Box::new(move |v| send(Update { status: Some(v.to_owned()), ..update(&status_key) }))),
            picked("Priority", priority_icon(issue.priority, false), priority_options, issue.priority.map_or("none", Priority::as_str), Box::new(move |v| send(Update { priority: Some(v.to_owned()), ..update(&priority_key) }))),
            picked("Type", type_icon(issue.kind, false, 14), type_options, issue.kind.as_str(), Box::new(move |v| send(Update { rel: Relations { kind: Some(v.to_owned()), ..Relations::default() }, ..update(&type_key) }))),
        ]),
        group(links_props(issue, issues)),
        group(vec![
            branch_prop(issue, project),
            pr_prop(issue),
            prop("Commits", None, vec![if commits.is_empty() { muted("None").prop("size", "small") } else { commit_chips }]),
            prop("Sessions", None, vec![if issue.sessions.is_empty() { muted("None yet").prop("size", "small") } else { stack("s").children(session_rows) }]),
        ]),
        group(vec![
            prop("Created", None, vec![line(ago_now(issue.created_at), None, Some("small"))]),
            prop("Updated", None, vec![line(ago_now(issue.updated_at), None, Some("small"))]),
        ]),
    ]);
    // Beside the description a column of its own; above it, the card takes the whole width.
    if narrow { card } else { stack("none").prop("width", PROPERTY_WIDTH).child(card) }
}

// ---------- the main column ----------

fn description(issue: &Issue) -> Node {
    let editing = with(|p| p.edits.contains_key("description"));
    if editing {
        let (draft, busy, error) = with(|p| p.edits.get("description").map(|e| (e.draft.clone(), e.busy, e.error.clone()))).unwrap_or_default();
        let key = issue.key.clone();
        let save_description = move || {
            let Some(text) = with(|p| p.edits.get("description").filter(|e| !e.busy).map(|e| e.draft.clone())) else {
                return;
            };
            with(|p| {
                if let Some(edit) = p.edits.get_mut("description") {
                    edit.busy = true;
                    edit.error = None;
                }
            });
            let key = key.clone();
            spawn(async move {
                let done = feed::apply(Op::Update(Update { description: Some(text), ..update(&key) })).await;
                with(|p| match done {
                    Ok(()) => drop(p.edits.remove("description")),
                    Err(error) => {
                        if let Some(edit) = p.edits.get_mut("description") {
                            edit.busy = false;
                            edit.error = Some(error);
                        }
                    }
                });
            });
        };
        return stack("s")
            .child(
                Node::new("Textarea")
                    .prop("label", "Description")
                    .prop("rows", 12)
                    .prop("maxLength", limits::DESCRIPTION)
                    .prop("value", draft.as_str())
                    .prop("placeholder", "Goal, scope and acceptance criteria. Markdown works.")
                    .prop("autoFocus", true)
                    .on("onChange", |event| {
                        with(|p| {
                            if let Some(edit) = p.edits.get_mut("description") {
                                edit.draft = event["value"].as_str().unwrap_or_default().to_owned();
                            }
                        })
                    })
                    .on("onKeyDown", |event| {
                        if event["key"] == "Escape" {
                            end("description");
                        }
                    }),
            )
            .maybe(error.map(|e| error_text(&e).prop("size", "small")))
            .child(row("s").prop("justify", "end").child(button("Cancel", "ghost", "sm", || end("description"))).child(Node::new("Button").prop("size", "sm").prop("disabled", busy).on("onClick", move |_| save_description()).text("Save")));
    }
    let shown = visible_description(issue.description.as_deref());
    let template = description_to_edit(issue);
    if shown.is_empty() {
        // In a row, as wide as its text: alone in the column it stretched across and centred its text, adrift.
        return row("none").child(Node::new("Button").prop("variant", "ghost").on("onClick", move |_| begin("description", &template)).text("Add a description…"));
    }
    // The edit button sits in a heading line over the text, so a table in it runs as wide as the card above, not 32 points short.
    stack("s").child(row("s").prop("align", "center").prop("justify", "between").child(Node::new("Heading").prop("level", 3).text("Description")).child(icon_button("Edit the description", &icons::PENCIL, move || begin("description", &template)))).child(markdown(&shown))
}

/// An epic's or feature's children with how many are done.
fn children_section(issue: &Issue, issues: &[Issue]) -> Node {
    let mut children = children_of(issues, &issue.key);
    children.sort_by_key(|c| crate::domain::key_number(&c.key));
    let (done, total) = progress(children.iter().copied());
    let list = Node::new("List").children(children.iter().map(|c| {
        let key = c.key.clone();
        Node::new("ListItem").key(c.key.as_str()).child(
            Node::new("ListButton")
                .prop("label", format!("Open {}: {}", c.key, c.title))
                .on("onClick", move |_| open_other(&key))
                .child(status_dot(c.status))
                .child(type_icon(c.kind, false, 14))
                .child(line(c.key.as_str(), Some("muted"), Some("tiny")).prop("mono", true))
                .child(Node::new("Text").prop("grow", true).prop("truncate", true).text(c.title.as_str()))
                .child(line(c.status.label(), Some("muted"), Some("tiny"))),
        )
    }));
    stack("s")
        .child(section_heading("Child issues", String::new()))
        .maybe((total > 0).then(|| child_progress(done, total)))
        .child(if children.is_empty() { muted("None yet. Open an issue and choose this one as its parent.").prop("size", "small") } else { list })
}

fn section_heading(title: &str, note: String) -> Node {
    row("s").prop("justify", "between").prop("wrap", true).child(Node::new("Heading").prop("level", 3).text(title)).child(line(note, Some("muted"), Some("small")))
}

fn file_row(file: &crate::domain::FileChange, first_commit: Option<String>) -> Node {
    let slash = file.path.rfind('/');
    let (name, dir) = match slash {
        Some(at) => (&file.path[at + 1..], Some(&file.path[..at]).filter(|d| !d.is_empty())),
        None => (file.path.as_str(), None),
    };
    let counts = (!file.what.is_empty()).then(|| line_counts(&file.what)).flatten();
    let who = file.session.as_ref().map_or(String::new(), |s| format!("{} · ", s.name.clone().filter(|n| !n.is_empty()).unwrap_or_else(|| agent_label(s.kind.as_deref()))));
    let path = file.path.clone();
    Node::new("ListItem").key(file.path.as_str()).child(
        Node::new("ListButton")
            .prop("title", format!("Open the diff of {}", file.path))
            .on("onClick", move |_| open_diff(first_commit.clone(), path.clone()))
            .child(row("s").prop("grow", true).child(line(name, None, None).prop("mono", true).prop("truncate", true)).maybe(dir.filter(|_| file.what.is_empty()).map(|d| line(d, Some("muted"), Some("small")).prop("mono", true).prop("truncate", true))))
            .maybe(counts.map(|(added, removed)| row("s").child(line(format!("+{added}"), Some("success"), Some("small"))).child(line(format!("−{removed}"), Some("danger"), Some("small")))))
            .maybe(if counts.is_none() && !file.what.is_empty() { Some(line(file.what.as_str(), Some("muted"), Some("small")).prop("truncate", true).prop("grow", true)) } else { None })
            .child(line(format!("{who}{}", ago_now(file.at)), Some("muted"), Some("tiny")).prop("truncate", true)),
    )
}

fn open_diff(commit: Option<String>, path: String) {
    let root = feed::project().flatten().map(|p| p.path).unwrap_or_default();
    let reply = messages::ask("open-diff", json!({ "commit": commit, "path": path, "root": root }));
    run(async move { reply.await.map(drop) });
}

/// The files agents' journals reported for the issue, newest first, with the rows past the first ten folded.
fn files_section(issue: &Issue) -> Option<Node> {
    let mut files: Vec<&crate::domain::FileChange> = issue.files.iter().flatten().collect();
    if files.is_empty() {
        return None;
    }
    files.sort_by(|a, b| b.at.total_cmp(&a.at));
    let first = issue.links.as_ref().and_then(|l| l.commits.as_ref()).and_then(|c| c.first().cloned());
    let rest = files.len().saturating_sub(FILE_ROWS);
    let more = (rest > 0).then(|| {
        wings_plugin::ui::disclosure(format!("{rest} more"), Node::new("List").children(files[FILE_ROWS..].iter().map(|f| file_row(f, first.clone()))))
    });
    Some(stack("s").child(section_heading("Files changed", files.len().to_string())).child(Node::new("List").children(files.iter().take(FILE_ROWS).map(|f| file_row(f, first.clone())))).maybe(more))
}

fn composer(issue: &Issue) -> Node {
    let (draft, busy, error) = with(|p| p.edits.get("note").map(|e| (e.draft.clone(), e.busy, e.error.clone()))).unwrap_or_default();
    let key = issue.key.clone();
    let submit = Rc::new(move || {
        let Some(text) = with(|p| p.edits.get("note").filter(|e| !e.busy && !e.draft.trim().is_empty()).map(|e| e.draft.clone())) else {
            return;
        };
        with(|p| {
            if let Some(edit) = p.edits.get_mut("note") {
                edit.busy = true;
            }
        });
        let key = key.clone();
        spawn(async move {
            let done = feed::apply(Op::Note { key, text }).await;
            with(|p| match done {
                Ok(()) => drop(p.edits.insert("note", Edit::default())),
                Err(error) => {
                    if let Some(edit) = p.edits.get_mut("note") {
                        edit.busy = false;
                        edit.error = Some(error);
                    }
                }
            });
        });
    });
    let (for_key, for_click) = (submit.clone(), submit);
    stack("s")
        .child(
            Node::new("Textarea")
                .prop("label", "Note")
                .prop("rows", 1)
                .prop("maxLength", limits::NOTE)
                .prop("value", draft.as_str())
                .prop("placeholder", "What was done, what's next")
                .on("onChange", |event| {
                    with(|p| {
                        let edit = p.edits.entry("note").or_default();
                        edit.draft = event["value"].as_str().unwrap_or_default().to_owned();
                        edit.error = None;
                    })
                })
                .on("onKeyDown", move |event| {
                    if event["key"] == "Enter" && (event["metaKey"] == true || event["ctrlKey"] == true) {
                        for_key();
                    }
                }),
        )
        .maybe(error.map(|e| error_text(&e).prop("size", "small")))
        .maybe((!draft.trim().is_empty()).then(|| {
            let add = Node::new("Button").prop("size", "sm").prop("disabled", busy).on("onClick", move |_| for_click());
            row("s").prop("justify", "end").child(if busy { add.child(Node::new("Spinner")).text("Add note") } else { add.text("Add note") })
        }))
}

/// How many of `issue`'s newest log entries its activity shows.
fn log_shown(issue: &Issue) -> usize {
    LOG_SHOWN + with(|p| p.earlier.as_ref().filter(|(key, _)| *key == issue.key).map_or(0, |(_, more)| *more))
}

fn activity(issue: &Issue) -> Node {
    let shown = log_shown(issue);
    let entries: Vec<&crate::domain::LogEntry> = issue.log.iter().rev().take(shown).collect();
    let items = entries.iter().enumerate().map(|(n, e)| {
        let by = actor_name(&e.by, &issue.sessions);
        let time = line(ago_now(e.at), Some("muted"), Some("small"));
        if e.kind == "note" {
            let tag = e.by.agent().filter(|a| a.name.as_ref().is_some_and(|n| !n.is_empty())).map(|a| Node::new("Badge").prop("variant", "secondary").text(agent_label(a.kind.as_deref())));
            Node::new("Card").key(format!("{}-{n}", e.at)).child(stack("s").child(row("s").child(Node::new("Avatar").prop("name", by.as_str()).prop("size", "small")).child(line(by.as_str(), None, None).prop("weight", "strong")).maybe(tag).child(time.prop("grow", true))).child(markdown(&e.text)))
        } else {
            let author = if e.by.agent().is_some_and(|a| a.via.as_deref() == Some("journal")) { format!("{by} · journal") } else { by };
            row("s").key(format!("{}-{n}", e.at)).prop("wrap", true).child(Node::new("Avatar").prop("name", author.as_str()).prop("size", "small")).child(line(author.as_str(), None, None).prop("weight", "strong")).child(muted(e.text.as_str())).child(time)
        }
    });
    let earlier = issue.log.len().saturating_sub(shown);
    let key = issue.key.clone();
    let more = (earlier > 0).then(|| {
        row("s")
            .prop("wrap", true)
            .child(muted(format!("{earlier} earlier {}", if earlier == 1 { "entry" } else { "entries" })).prop("size", "small"))
            .child(Node::new("Button").prop("variant", "ghost").prop("size", "sm").on("onClick", move |_| with(|p| p.earlier = Some((key.clone(), shown)))).text(format!("Show {} earlier", earlier.min(LOG_SHOWN))))
    });
    stack("m")
        .child(section_heading("Activity", format!("{} {}", issue.log.len(), if issue.log.len() == 1 { "entry" } else { "entries" })))
        .child(composer(issue))
        .child(stack("m").children(items))
        .maybe(more)
}

// ---------- the toolbar ----------

fn resume(id: &str, kind: Option<&str>) {
    let cwd = feed::project().flatten().map(|p| p.path).unwrap_or_default();
    let reply = messages::ask("open-session", json!({ "session": { "id": id, "kind": kind }, "cwd": cwd }));
    run(async move { reply.await.map(drop) });
}

fn start_agent(key: &str, kind: &str) {
    let cwd = feed::project().flatten().map(|p| p.path).unwrap_or_default();
    let key = key.to_owned();
    let kind = kind.to_owned();
    run(async move {
        let (command, args) = agent_command(&key, Some(&kind))?;
        host::OpenPane::new().command(&command, &args.iter().map(String::as_str).collect::<Vec<_>>()).cwd(&cwd).open().await.map(drop)
    });
}

/// A menu of `items` opened by `trigger`, lined up with its right edge. A destructive item is drawn red.
fn menu(trigger: Node, items: Vec<(String, bool, Rc<dyn Fn()>)>) -> Node {
    let items = items.into_iter().map(|(label, destructive, select)| {
        let item = Node::new("Button").key(label.clone()).on("onClick", move |_| select()).text(label);
        if destructive { item.prop("variant", "destructive") } else { item }
    });
    ui::menu(trigger, items, true)
}

fn toolbar(issue: Option<&Issue>, key: &str) -> Node {
    let (trail, _) = with(|p| (p.args.as_ref().map(|a| a.trail.clone()).unwrap_or_default(), ()));
    let sep = || muted("/").prop("size", "small");
    let mut crumbs = row("s").child(Node::new("Button").prop("variant", "ghost").prop("size", "sm").on("onClick", |_| feed::open_board()).text("← Issues"));
    for (at, entry) in trail.iter().enumerate() {
        let (back, project) = (trail[..at].to_vec(), entry.clone());
        crumbs = crumbs.child(sep()).child(Node::new("Button").key(format!("{at}-{entry}")).prop("variant", "ghost").prop("size", "sm").on("onClick", move |_| feed::open_issue(&project, &project_id(), Some(back.clone()), false)).text(entry.as_str()));
    }
    crumbs = crumbs.child(sep()).child(muted(issue.map_or(key, |i| i.key.as_str())).prop("size", "small"));
    let node = Node::new("Toolbar").child(slot("children", crumbs));
    let Some(issue) = issue else {
        return node;
    };
    let last = last_session(issue).map(|s| (s.id.clone(), s.kind.clone()));
    let resume_button = Node::new("Button").prop("variant", "outline").prop("size", "sm").prop("disabled", last.is_none()).on("onClick", move |_| {
        if let Some((id, kind)) = &last {
            resume(id, kind.as_deref());
        }
    });
    let key = issue.key.clone();
    let agents: Vec<(String, bool, Rc<dyn Fn()>)> = AGENTS
        .iter()
        .map(|a| {
            let (key, kind) = (key.clone(), a.kind);
            (a.label.to_owned(), false, Rc::new(move || start_agent(&key, kind)) as Rc<dyn Fn()>)
        })
        .collect();
    let delete_key = issue.key.clone();
    node.child(slot(
        "actions",
        row("s")
            .child(resume_button.text("Resume"))
            .child(menu(Node::new("Button").prop("size", "sm").prop("variant", "default").text("Start agent"), agents))
            .child(menu(Node::new("Button").prop("size", "icon-sm").prop("variant", "ghost").prop("label", "More").prop("title", "More").child(ui::icon(24, 2.0, &icons::ELLIPSIS).prop("size", 16)), vec![("Delete issue".to_owned(), true, Rc::new(move || ask_to_delete(&delete_key)) as Rc<dyn Fn()>)])),
    ))
}

// ---------- the page ----------

fn plain(toolbar: Node, body: Node) -> Vec<Node> {
    vec![page(toolbar, [body])]
}

fn view() -> Vec<Node> {
    let key = with(|p| p.args.as_ref().map(|a| a.key.clone()));
    let Some(key) = key else {
        return plain(Node::new("Toolbar").child(Node::new("Slot").prop("name", "title").text("Issue")), empty("No issue open", "Open an issue from the board or from a project's Issues."));
    };
    let Some(project) = feed::project() else {
        return plain(toolbar(None, &key), loading().text("Loading issue"));
    };
    let Some(project) = project else {
        return plain(toolbar(None, &key), empty("No project open", "Open a project to see its issues."));
    };
    let (store, read_error) = feed::with(|f| (f.store.clone(), f.error.clone()));
    let Some(store) = store else {
        return match read_error {
            Some(error) => plain(toolbar(None, &key), ui::error_state("Couldn't read the issues", &error, None)),
            None => plain(toolbar(None, &key), loading().text("Loading issue")),
        };
    };
    let wanted = normalize_key(&key);
    let Some(issue) = store.issues.iter().find(|i| i.key == wanted) else {
        let back = Node::new("Slot").prop("name", "children").child(button("Back to the board", "outline", "md", feed::open_board));
        return plain(toolbar(None, &key), empty(&format!("No issue {key}"), &format!("{} has no issue with that key. It may have been removed.", project.name)).child(back));
    };
    let issues = &store.issues;
    let error = with(|p| p.error.clone());
    let confirming = delete_confirm::node(issue, issues);
    let notices = error.map(|e| Node::new("Card").child(error_text(&e)));
    let main = stack("l")
        .child(description(issue))
        .maybe(issue.kind.can_parent().then(|| children_section(issue, issues)))
        .maybe(files_section(issue))
        .child(activity(issue));
    let narrow = with(|p| p.narrow);
    let head = title_row(issue);
    let (above, text) = if narrow { (Some(head), stack("l").prop("grow", true).child(main)) } else { (None, stack("l").prop("grow", true).child(head).child(main)) };
    // Beside the description the properties are a column of their own that stays in view: it is the split's fixed pane,
    // which scrolls on its own, while the text scrolls in the pane beside it. Under `SIDE_BY_SIDE` the split leaves the
    // pane out and the properties are above the text, in the same scroll, so the text keeps the whole width and a table
    // in it isn't cut off.
    let read = if narrow { stack("l").maybe(above).child(properties(issue, issues, &project, true)).child(text) } else { text };
    let read = stack("l").prop("maxWidth", TEXT_WIDTH).prop("pt", 14).maybe(notices).child(read).maybe(confirming);
    let side = if narrow { Vec::new() } else { vec![properties(issue, issues, &project, false)] };
    let split = Node::new("Split")
        .prop("side", "end")
        .prop("defaultSize", PROPERTY_WIDTH + 24.0)
        .prop("collapseBelow", SIDE_BY_SIDE)
        .on("onNarrow", |payload| with(|p| p.narrow = payload["narrow"].as_bool().unwrap_or(false)))
        .child(Node::new("Slot").prop("name", "start").child(Node::new("Scroll").child(read)))
        .child(Node::new("Slot").prop("name", "end").children(side));
    vec![page(toolbar(Some(issue), &key), [split]).prop("flush", true)]
}

/// The args the tab was opened with.
fn args_of(raw: &Value) -> Args {
    Args {
        key: raw["key"].as_str().unwrap_or_default().to_owned(),
        project_id: raw["projectId"].as_str().map(str::to_owned),
        trail: raw["trail"].as_array().map(|t| t.iter().filter_map(|k| k.as_str().map(str::to_owned)).collect::<Vec<_>>()).map(|t| t[t.len().saturating_sub(TRAIL_LENGTH)..].to_vec()).unwrap_or_default(),
    }
}

#[cfg_attr(not(test), unsafe(no_mangle))]
pub fn start() {
    let args = host::view_args().filter(Value::is_object).map(|raw| args_of(&raw));
    let only = args.as_ref().and_then(|a| a.project_id.clone());
    with(|p| p.args = args);
    wings_plugin::run_filled(view);
    feed::watch(only);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::{Actor, LogEntry},
        testing::{fresh, make},
    };

    /// An issue with more entries than its activity shows at first shows 100 more a click, until all show.
    #[test]
    fn a_long_activity_shows_earlier_entries_in_steps() {
        let mut issue = make(&fresh(), "A", |_| {}).issues[0].clone();
        issue.log = (0..250).map(|n| LogEntry { at: f64::from(n), by: Actor::You, kind: "status".into(), text: format!("e{n}") }).collect();
        let shown = || wings_plugin::ui::tree_json(activity(&issue)).to_string();
        let first = shown();
        assert!(first.contains("150 earlier entries") && first.contains("Show 100 earlier") && first.contains("e249") && !first.contains("e149"), "{first}");
        with(|p| p.earlier = Some((issue.key.clone(), 100)));
        let second = shown();
        assert!(second.contains("50 earlier entries") && second.contains("Show 50 earlier") && second.contains("e50") && !second.contains("e49\""), "{second}");
        with(|p| p.earlier = Some((issue.key.clone(), 200)));
        let all = shown();
        assert!(all.contains("e0") && !all.contains("earlier"), "{all}");
    }

    /// Under `SIDE_BY_SIDE` the properties are a card as wide as the page, above the text; wider they are a column of
    /// `PROPERTY_WIDTH` beside it.
    #[test]
    fn the_properties_fill_a_narrow_page_and_keep_their_column_in_a_wide_one() {
        let issue = make(&fresh(), "A", |_| {}).issues[0].clone();
        let project = Project { id: "p".into(), name: "web".into(), path: "/work/web".into(), branch: None };
        let narrow = wings_plugin::ui::tree_json(properties(&issue, &[issue.clone()], &project, true));
        assert_eq!((narrow["type"].as_str(), narrow["props"]["variant"].as_str(), narrow["props"]["width"].is_null()), (Some("Card"), Some("rows"), true), "{narrow}");
        let wide = wings_plugin::ui::tree_json(properties(&issue, &[issue.clone()], &project, false));
        assert_eq!((wide["type"].as_str(), wide["props"]["width"].as_f64()), (Some("Stack"), Some(f64::from(PROPERTY_WIDTH))), "{wide}");
    }

    /// The bar under "Child issues" says how many are done, so the heading doesn't say it again.
    #[test]
    fn the_child_issues_heading_does_not_repeat_the_progress_bar() {
        let store = make(&fresh(), "Epic", |c| c.rel.kind = Some("epic".into()));
        let store = make(&store, "Child", |c| c.rel.parent = Some("WEB-1".into()));
        let shown = wings_plugin::ui::tree_json(children_section(&store.issues[0], &store.issues)).to_string();
        assert!(shown.contains("0/1") && !shown.contains("0 of 1 done\""), "{shown}");
    }

    /// Twelve commits show three and a "+9 more" button, which opens them all.
    #[test]
    fn a_long_list_of_commits_shows_the_first_three_and_a_button_for_the_rest() {
        let mut issue = make(&fresh(), "A", |_| {}).issues[0].clone();
        issue.links = Some(crate::domain::Links { branch: None, pr: None, commits: Some((0..12).map(|n| format!("c{n:07}")).collect()) });
        let project = Project { id: "p".into(), name: "web".into(), path: "/work/web".into(), branch: None };
        let shown = |issue: &Issue| wings_plugin::ui::tree_json(properties(issue, std::slice::from_ref(issue), &project, true)).to_string();
        let first = shown(&issue);
        assert!(first.contains("c0000002") && !first.contains("c0000003") && first.contains("+9 more"), "{first}");
        with(|p| p.commits_open = Some(issue.key.clone()));
        let all = shown(&issue);
        assert!(all.contains("c0000011") && all.contains("Show fewer"), "{all}");
    }
}
