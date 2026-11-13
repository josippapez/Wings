//! The Issue tab: one issue of a project, opened with args { key, projectId, trail }; trail is the issues opened before it in
//! this tab, for the breadcrumb back. Edit its title, status, description and links, add a note, resume any agent session that
//! worked on it, or start a new agent on it.

use crate::{
    actor::{actor_name, last_session},
    commands::{AGENTS, agent_command, agent_label},
    delete_confirm::{self, ask_to_delete},
    domain::{Issue, IssueType, Op, Priority, Relations, Status, Update, limits, normalize_key, visible_description},
    feed::{self, Project},
    file_diff::line_counts,
    markdown::markdown,
    ops::description_to_edit,
    orderings::{TRAIL_LENGTH, trail_to},
    parts::{ago_text, button, child_progress, error_text, line, muted, priority_icon, priority_name, row, stack, status_dot, type_icon},
    relations::{blocks_of, children_of, parent_choices, progress},
};
use serde_json::{Value, json};
use std::{cell::RefCell, collections::HashMap, future::Future, rc::Rc};
use wings_plugin::{
    host,
    task::spawn,
    ui::{Choice, Combobox, Node, combobox, empty, loading, page, select, slot},
};

/// The log entries the activity shows, newest first: the app draws at most 5,000 nodes in a page, and an issue keeps up to 300.
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
    /// The menu in the toolbar that is open.
    menu: Option<&'static str>,
    edits: HashMap<&'static str, Edit>,
    /// What is typed in each issue picker.
    queries: HashMap<&'static str, String>,
    /// The text last copied, for the "Copied" a button shows for a moment.
    copied: Option<String>,
}

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
        .child(Node::new("Text").prop("size", "large").prop("weight", "strong").text(issue.title.as_str()));
    row("none").child(type_icon(issue.kind, true, 18)).child(row("m").prop("align", "baseline").prop("grow", true).child(button).child(key_text))
}

// ---------- the properties column ----------

/// One property of the column: a small muted label over its value.
fn prop(label: &str, actions: Option<Node>, content: Vec<Node>) -> Node {
    stack("s").child(row("s").prop("justify", "between").child(line(label, Some("muted"), Some("tiny")).prop("weight", "medium")).maybe(actions)).children(content)
}

fn copy(text: String) {
    spawn(async move {
        drop(host::call("copyText", json!({ "text": text })).await);
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

fn branch_prop(issue: &Issue, project: &Project) -> Node {
    let branch = issue.links.as_ref().and_then(|l| l.branch.clone()).unwrap_or_default();
    let editing = with(|p| p.edits.contains_key("branch"));
    let key = issue.key.clone();
    let make: Rc<dyn Fn(String) -> Op> = Rc::new(move |value| Op::Update(Update { branch: Some(value), ..update(&key) }));
    let actions = (!editing).then(|| {
        let for_edit = branch.clone();
        row("xs").maybe((!branch.is_empty()).then(|| copy_button("Copy", &branch, "ghost", "Copy the branch name".to_owned()))).child(
            Node::new("Button").prop("size", "xs").prop("variant", "ghost").prop("label", "Edit the branch").on("onClick", move |_| begin("branch", &for_edit)).text("Edit"),
        )
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
    prop("Branch", actions, vec![value]).maybe(use_current)
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
    let for_edit = pr.clone();
    let actions = (!editing).then(|| Node::new("Button").prop("size", "xs").prop("variant", "ghost").prop("label", "Edit the pull request link").on("onClick", move |_| begin("pr", &for_edit)).text("Edit"));
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
    row("s").child(chip).maybe(on_remove.map(|remove| Node::new("Button").prop("size", "icon-xs").prop("variant", "ghost").prop("label", format!("Remove {fallback}")).on("onClick", move |_| remove()).text("×")))
}

/// Opens another issue in this tab, with the trail back to this one.
fn open_other(key: &str) {
    let (current, trail) = with(|p| p.args.as_ref().map(|a| (a.key.clone(), a.trail.clone()))).unwrap_or_default();
    feed::open_issue(key, &project_id(), Some(trail_to(&trail, &current, key)), false);
}

/// A field that finds an issue by key or title and hands back its key.
fn picker(name: &'static str, label: &'static str, placeholder: &'static str, options: Vec<&Issue>, on_pick: impl Fn(String) + 'static) -> Node {
    let value = with(|p| p.queries.get(name).cloned()).unwrap_or_default();
    combobox(
        Combobox {
            label,
            placeholder,
            value,
            query: None,
            options: options.into_iter().map(|o| Choice { value: o.key.clone(), label: o.key.clone(), detail: Some(o.title.clone()) }).collect(),
            empty: "No issue matches",
            auto_focus: false,
        },
        move |text| {
            with(|p| p.queries.insert(name, text.unwrap_or_default()));
        },
        move |picked| {
            with(|p| p.queries.insert(name, String::new()));
            on_pick(picked.to_owned());
        },
    )
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
    props.push(prop(
        "Parent",
        None,
        parent_chip
            .into_iter()
            .chain([picker("parent", "Parent", if issue.parent.is_some() { "Change parent" } else { "Choose an epic or feature" }, choices, move |parent| send(Update { rel: Relations { parent: Some(parent), ..Relations::default() }, ..update(&pick_key) }))])
            .collect(),
    ));
    let related_key = key.clone();
    let related_options = others.iter().copied().filter(|o| !issue.related.as_ref().is_some_and(|r| r.contains(&o.key))).collect();
    props.push(prop(
        "Related",
        None,
        chips(&issue.related, |k, other| Update { rel: Relations { related_remove: Some(vec![other.to_owned()]), ..Relations::default() }, ..update(k) })
            .into_iter()
            .chain([picker("related", "Related", "Add a related issue", related_options, move |k| send(Update { rel: Relations { related_add: Some(vec![k]), ..Relations::default() }, ..update(&related_key) }))])
            .collect(),
    ));
    let blocked_options = others.iter().copied().filter(|o| !issue.blocked_by.as_ref().is_some_and(|r| r.contains(&o.key))).collect();
    props.push(prop(
        "Blocked by",
        None,
        chips(&issue.blocked_by, |k, other| Update { rel: Relations { blocked_by_remove: Some(vec![other.to_owned()]), ..Relations::default() }, ..update(k) })
            .into_iter()
            .chain([picker("blocked", "Blocked by", "Add a blocking issue", blocked_options, move |k| send(Update { rel: Relations { blocked_by_add: Some(vec![k]), ..Relations::default() }, ..update(&key) }))])
            .collect(),
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

fn properties(issue: &Issue, issues: &[Issue], project: &Project) -> Node {
    let last = last_session(issue).map(|s| s.id.clone());
    let commits = issue.links.as_ref().and_then(|l| l.commits.clone()).unwrap_or_default();
    let key = issue.key.clone();
    let (status_key, priority_key, type_key) = (key.clone(), key.clone(), key);
    let status_options = options(&Status::ALL, Status::as_str, Status::label);
    let priority_options: Vec<(String, String)> = Priority::NAMES.iter().map(|n| ((*n).to_owned(), priority_name(Priority::parse(n)).to_owned())).collect();
    let type_options = options(&IssueType::ALL, IssueType::as_str, IssueType::label);
    let picked = |label: &'static str, dot: Node, options: Vec<(String, String)>, value: &str, on_change: Box<dyn Fn(&str)>| {
        prop(label, None, vec![row("s").child(dot).child(select(label, &options, value, move |v| on_change(v)))])
    };
    let mut sessions: Vec<_> = issue.sessions.iter().collect();
    sessions.sort_by(|a, b| b.seen.total_cmp(&a.seen));
    let session_rows = sessions.into_iter().map(|s| {
        // An unnamed session goes by its agent, so the line under it doesn't name the agent twice.
        let name = s.name.clone().filter(|n| !n.is_empty()).unwrap_or_else(|| agent_label(s.kind.as_deref()));
        let (id, kind) = (s.id.clone(), s.kind.clone());
        let detail = format!("{}{} · seen {}", if s.name.as_ref().is_some_and(|n| !n.is_empty()) { format!("{} · ", agent_label(s.kind.as_deref())) } else { String::new() }, s.id.chars().take(8).collect::<String>(), ago_text(s.seen));
        stack("s")
            .key(s.id.as_str())
            .child(
                row("s")
                    .prop("justify", "between")
                    .child(row("s").child(line(name.as_str(), None, Some("small")).prop("truncate", true)).maybe((last.as_deref() == Some(s.id.as_str())).then(|| Node::new("Badge").prop("variant", "outline").text("Latest"))))
                    .child(Node::new("Button").prop("variant", "ghost").prop("size", "xs").prop("label", format!("Resume {name}")).on("onClick", move |_| resume(&id, kind.as_deref())).text("Resume")),
            )
            .child(line(detail, Some("muted"), Some("tiny")))
    });
    let mut column = Node::new("Stack").prop("gap", "l").prop("width", 240);
    column = column
        .child(picked("Status", status_dot(issue.status), status_options, issue.status.as_str(), Box::new(move |v| send(Update { status: Some(v.to_owned()), ..update(&status_key) }))))
        .child(picked("Priority", priority_icon(issue.priority, false), priority_options, issue.priority.map_or("none", Priority::as_str), Box::new(move |v| send(Update { priority: Some(v.to_owned()), ..update(&priority_key) }))))
        .child(picked("Type", type_icon(issue.kind, false, 14), type_options, issue.kind.as_str(), Box::new(move |v| send(Update { rel: Relations { kind: Some(v.to_owned()), ..Relations::default() }, ..update(&type_key) }))))
        .children(links_props(issue, issues))
        .child(branch_prop(issue, project))
        .child(pr_prop(issue))
        .child(prop(
            "Commits",
            None,
            vec![if commits.is_empty() { muted("None").prop("size", "small") } else { row("s").prop("wrap", true).children(commits.iter().map(|c| copy_button(c, c, "secondary", format!("Copy commit {c}")).key(c.as_str()))) }],
        ))
        .child(prop("Sessions", None, vec![if issue.sessions.is_empty() { muted("None yet").prop("size", "small") } else { stack("s").children(session_rows) }]))
        .child(prop("Created", None, vec![line(ago_text(issue.created_at), None, Some("small"))]))
        .child(prop("Updated", None, vec![line(ago_text(issue.updated_at), None, Some("small"))]));
    column
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
        return Node::new("Button").prop("variant", "ghost").on("onClick", move |_| begin("description", &template)).text("Add a description…");
    }
    row("s").prop("align", "start").prop("justify", "between").child(markdown(&shown)).child(Node::new("Button").prop("size", "xs").prop("variant", "ghost").on("onClick", move |_| begin("description", &template)).text("Edit"))
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
        .child(section_heading("Child issues", if total > 0 { format!("{done} of {total} done") } else { String::new() }))
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
            .child(row("s").child(line(name, None, None).prop("mono", true)).maybe(dir.map(|d| line(d, Some("muted"), Some("small")).prop("mono", true).prop("truncate", true))))
            .maybe(counts.map(|(added, removed)| row("s").child(line(format!("+{added}"), Some("success"), Some("small"))).child(line(format!("−{removed}"), Some("danger"), Some("small")))))
            .maybe(if counts.is_none() && !file.what.is_empty() { Some(line(file.what.as_str(), Some("muted"), Some("small")).prop("truncate", true)) } else { None })
            .child(line(format!("{who}{}", ago_text(file.at)), Some("muted"), Some("tiny")).prop("truncate", true)),
    )
}

fn open_diff(commit: Option<String>, path: String) {
    let root = feed::project().flatten().map(|p| p.path).unwrap_or_default();
    run(crate::ask::ask("open-diff", json!({ "commit": commit, "path": path, "root": root })));
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
                .prop("rows", 3)
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

fn activity(issue: &Issue) -> Node {
    let entries: Vec<&crate::domain::LogEntry> = issue.log.iter().rev().take(LOG_SHOWN).collect();
    let items = entries.iter().enumerate().map(|(n, e)| {
        let by = actor_name(&e.by, &issue.sessions);
        let time = line(ago_text(e.at), Some("muted"), Some("small"));
        if e.kind == "note" {
            let tag = e.by.agent().filter(|a| a.name.as_ref().is_some_and(|n| !n.is_empty())).map(|a| Node::new("Badge").prop("variant", "secondary").text(agent_label(a.kind.as_deref())));
            Node::new("Card").key(format!("{}-{n}", e.at)).child(stack("s").child(row("s").child(Node::new("Avatar").prop("name", by.as_str()).prop("size", "small")).child(line(by.as_str(), None, None).prop("weight", "strong")).maybe(tag).child(time.prop("grow", true))).child(markdown(&e.text)))
        } else {
            let author = if e.by.agent().is_some_and(|a| a.via.as_deref() == Some("journal")) { format!("{by} · journal") } else { by };
            row("s").key(format!("{}-{n}", e.at)).prop("wrap", true).child(Node::new("Avatar").prop("name", author.as_str()).prop("size", "small")).child(line(author.as_str(), None, None).prop("weight", "strong")).child(muted(e.text.as_str())).child(time)
        }
    });
    let earlier = issue.log.len().saturating_sub(LOG_SHOWN);
    stack("m")
        .child(section_heading("Activity", format!("{} {}", issue.log.len(), if issue.log.len() == 1 { "entry" } else { "entries" })))
        .child(composer(issue))
        .child(stack("m").children(items))
        .maybe((earlier > 0).then(|| muted(format!("{earlier} earlier entries are in the issue, not shown here.")).prop("size", "small")))
}

// ---------- the toolbar ----------

fn resume(id: &str, kind: Option<&str>) {
    let cwd = feed::project().flatten().map(|p| p.path).unwrap_or_default();
    run(crate::ask::ask("open-session", json!({ "session": { "id": id, "kind": kind }, "cwd": cwd })));
}

fn start_agent(key: &str, kind: &str) {
    let cwd = feed::project().flatten().map(|p| p.path).unwrap_or_default();
    let key = key.to_owned();
    let kind = kind.to_owned();
    run(async move {
        let (command, args) = agent_command(&key, Some(&kind))?;
        host::call("openPane", json!({ "command": command, "args": args, "cwd": cwd, "placement": "tab" })).await.map(drop)
    });
}

/// A button that shows its items under it when pressed: the app has no popup menu yet.
fn menu(name: &'static str, label: &str, aria: Option<&str>, variant: Option<&str>, items: Vec<(String, bool, Rc<dyn Fn()>)>) -> Node {
    let open = with(|p| p.menu == Some(name));
    let mut trigger = Node::new("Button").prop("size", "sm").on("onClick", move |_| with(|p| p.menu = if p.menu == Some(name) { None } else { Some(name) })).text(label);
    if let Some(variant) = variant {
        trigger = trigger.prop("variant", variant);
    }
    if let Some(aria) = aria {
        trigger = trigger.prop("label", aria);
    }
    stack("xs").child(trigger).maybe(open.then(|| {
        stack("xs").children(items.into_iter().map(|(label, destructive, select)| {
            Node::new("Button").key(label.clone()).prop("variant", if destructive { "destructive" } else { "ghost" }).on("onClick", move |_| {
                with(|p| p.menu = None);
                select();
            }).text(label)
        }))
    }))
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
            .child(menu("start", "Start agent", None, Some("default"), agents))
            .child(menu("more", "…", Some("More"), Some("ghost"), vec![("Delete issue".to_owned(), true, Rc::new(move || ask_to_delete(&delete_key)) as Rc<dyn Fn()>)])),
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
            Some(error) => plain(toolbar(None, &key), empty("Couldn't read the issues", &error)),
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
    let notices = (error.is_some() || confirming.is_some()).then(|| stack("s").maybe(error.map(|e| Node::new("Card").child(error_text(&e)))).maybe(confirming));
    let main = stack("l")
        .child(description(issue))
        .maybe(issue.kind.can_parent().then(|| children_section(issue, issues)))
        .maybe(files_section(issue))
        .child(activity(issue));
    let head = stack("s").child(title_row(issue));
    let grid = row("l").prop("align", "start").child(stack("l").prop("grow", true).child(head).child(main)).child(Node::new("Stack").prop("width", 16)).child(properties(issue, issues, &project));
    vec![page(toolbar(Some(issue), &key), [stack("l").prop("maxWidth", 1040).maybe(notices).child(grid)])]
}

/// The args the tab was opened with.
fn args_of(raw: &Value) -> Args {
    Args {
        key: raw["key"].as_str().unwrap_or_default().to_owned(),
        project_id: raw["projectId"].as_str().map(str::to_owned),
        trail: raw["trail"].as_array().map(|t| t.iter().filter_map(|k| k.as_str().map(str::to_owned)).collect::<Vec<_>>()).map(|t| t[t.len().saturating_sub(TRAIL_LENGTH)..].to_vec()).unwrap_or_default(),
    }
}

#[unsafe(no_mangle)]
pub fn start() {
    let args = host::view_args().filter(Value::is_object).map(|raw| args_of(&raw));
    let only = args.as_ref().and_then(|a| a.project_id.clone());
    with(|p| p.args = args);
    wings_plugin::run_filled(view);
    feed::watch(only);
}
