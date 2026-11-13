//! A project's section in the sidebar: its open issues, the ones an agent works on now first, then by status or latest change,
//! each epic's or feature's children under it. Wings skips rows identical to the last ones. The order is picked in the
//! section header's menu or on the settings page.

use crate::{
    domain::{Issue, Status},
    orderings::section_issues,
    relations::{children_of, progress},
    settings,
};
use serde_json::{Value, json};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    rc::Rc,
};
use wings_plugin::{host, task::spawn};

pub const SECTION: &str = "tasks";
/// The project section shows this many rows.
const SECTION_ROWS: usize = 50;

thread_local! {
    /// Each project's issues as last shown, so a pane change or the Sidebar order setting sorts them again without reading storage.
    static SHOWN: RefCell<HashMap<String, Rc<Vec<Issue>>>> = RefCell::new(HashMap::new());
    /// The agent sessions running in a pane now.
    static RUNNING: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
}

/// The sidebar row's status icon, which Wings draws in the badge's tone.
fn icon(status: Status) -> &'static str {
    match status {
        Status::Todo => "todo",
        Status::InProgress => "in-progress",
        Status::InReview => "in-review",
        Status::Done => "done",
        Status::Canceled => "canceled",
    }
}

/// A muted line under a row for what isn't a plain task: its type, and an epic's or feature's progress.
fn detail(issues: &[Issue], i: &Issue) -> String {
    if i.kind == crate::domain::IssueType::Task {
        return String::new();
    }
    let (done, total) = progress(children_of(issues, &i.key));
    if i.kind.can_parent() && total > 0 { format!("{} · {done} of {total} done", i.kind.label()) } else { i.kind.label().to_owned() }
}

/// `issues` with each one's children right after it, and theirs after them, the rest in the order given. Wings indents a
/// row under its `parent`, and one that doesn't know `parent` still lists an epic over its children. An issue whose parent
/// isn't in `issues` stays where it was.
fn children_after_parents<'a>(issues: &[&'a Issue]) -> Vec<&'a Issue> {
    let keys: HashSet<&str> = issues.iter().map(|i| i.key.as_str()).collect();
    let mut below: HashMap<&str, Vec<&'a Issue>> = HashMap::new();
    let mut top: Vec<&'a Issue> = Vec::new();
    for &i in issues {
        match i.parent.as_deref().filter(|p| keys.contains(p)) {
            Some(parent) => below.entry(parent).or_default().push(i),
            None => top.push(i),
        }
    }
    fn place<'a>(i: &'a Issue, below: &HashMap<&str, Vec<&'a Issue>>, placed: &mut HashSet<&'a str>, out: &mut Vec<&'a Issue>) {
        if !placed.insert(&i.key) {
            return;
        }
        out.push(i);
        for child in below.get(i.key.as_str()).into_iter().flatten() {
            place(child, below, placed, out);
        }
    }
    let mut out = Vec::new();
    let mut placed = HashSet::new();
    // Whatever no top-level issue reaches hangs off a loop of parents, which still shows.
    for &i in top.iter().chain(issues) {
        place(i, &below, &mut placed, &mut out);
    }
    out
}

/// The rows of a project's section for `issues`, or null for none.
pub fn rows_of(project_id: &str, issues: &[Issue], running: &HashSet<String>, order: &str) -> Value {
    let open = section_issues(issues, |i| i.sessions.iter().any(|s| running.contains(&s.id)), order == "date");
    let open: Vec<&Issue> = open.into_iter().take(SECTION_ROWS).collect();
    let listed: HashSet<&str> = open.iter().map(|i| i.key.as_str()).collect();
    let rows: Vec<Value> = children_after_parents(&open)
        .into_iter()
        .map(|i| {
            let mut row = json!({
                "id": i.key,
                "label": format!("{} {}", i.key, i.title).chars().take(120).collect::<String>(),
                "icon": icon(i.status),
                "open": { "view": "issue", "args": { "key": i.key, "projectId": project_id } },
            });
            let detail = detail(issues, i);
            if !detail.is_empty() {
                row["detail"] = detail.into();
            }
            if i.status != Status::Todo {
                row["badge"] = json!({ "text": i.status.label(), "tone": if i.status == Status::InProgress { "info" } else { "warning" } });
            }
            if let Some(parent) = i.parent.as_deref().filter(|p| listed.contains(p)) {
                row["parent"] = parent.into();
            }
            row
        })
        .collect();
    if rows.is_empty() { Value::Null } else { Value::Array(rows) }
}

/// Shows the open ones of `issues` under the project. A failure is dropped: it never fails the change that caused it.
pub async fn push_section(project_id: String, issues: Vec<Issue>) {
    SHOWN.with(|shown| shown.borrow_mut().insert(project_id.clone(), Rc::new(issues)));
    send(&project_id).await;
}

async fn send(project_id: &str) {
    let Some(issues) = SHOWN.with(|shown| shown.borrow().get(project_id).cloned()) else {
        return;
    };
    let rows = RUNNING.with(|running| rows_of(project_id, &issues, &running.borrow(), &settings::section_order()));
    drop(host::set_project_section(project_id, SECTION, &rows).await);
}

fn resort() {
    let projects: Vec<String> = SHOWN.with(|shown| shown.borrow().keys().cloned().collect());
    for project in projects {
        spawn(async move { send(&project).await });
    }
}

/// Called with every pane on each change: sorts the sections again when an agent session starts or stops.
pub fn on_section_panes(panes: &[Value]) {
    let now: HashSet<String> = panes.iter().filter_map(|p| p["session"]["sessionId"].as_str().map(str::to_owned)).collect();
    if RUNNING.with(|running| *running.borrow() == now) {
        return;
    }
    RUNNING.with(|running| *running.borrow_mut() = now);
    resort();
}

fn show_order(order: &str) {
    drop(host::call("setSectionSort", json!({ "sectionId": SECTION, "sort": order })));
}

/// Tells Wings the order the section starts in, sorts again when the setting changes, and hears the menu on the section's
/// header, which sets the same setting as the settings page.
pub fn watch_order() {
    show_order(&settings::section_order());
    settings::on_change(settings::SECTION_ORDER, || {
        show_order(&settings::section_order());
        resort();
    });
    host::on("sectionSort", |_, picked| {
        let sort = picked["sort"].as_str().unwrap_or_default();
        if picked["sectionId"] == SECTION && settings::SECTION_ORDERS.iter().any(|(value, _)| *value == sort) {
            drop(settings::set_section_order(sort));
            show_order(sort);
            resort();
        }
    });
}
