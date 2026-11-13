//! The Checks tab: every check or policy of the pull request, grouped by how it stands. A failed check's log is asked
//! for only when its row is opened, and shown as plain text.

use crate::{
    checklist::{GroupId, duration_text, group_checks, has_log, log_tail, with_unresolved},
    pr_page,
    types::{CheckItem, CheckState, FailureDetail, Status},
    ui::{markdown::code_block, parts, run_view},
};
use serde_json::json;
use std::{cell::RefCell, collections::{HashMap, HashSet}};
use wings_plugin::{
    host,
    task::spawn,
    ui::{Node, muted, notice, row, stack, text},
};

#[derive(Clone)]
enum Load {
    Loading,
    Error(String),
    Done(Vec<FailureDetail>),
}

#[derive(Default)]
struct Ui {
    /// Groups the user opened or folded, against how they start.
    groups: HashMap<&'static str, bool>,
    /// Rows that show their failure.
    expanded: HashSet<String>,
    loads: HashMap<String, Load>,
}

thread_local! {
    static UI: RefCell<Ui> = RefCell::new(Ui::default());
}

fn ui<T>(change: impl FnOnce(&mut Ui) -> T) -> T {
    UI.with(|ui| change(&mut ui.borrow_mut()))
}

fn starts_open(id: GroupId) -> bool {
    matches!(id, GroupId::Failed | GroupId::Active)
}

/// The log belongs to the commit it ran for, so a new push asks again.
fn load_key(st: &Status, c: &CheckItem) -> String {
    format!("{}:{}", st.key, c.id)
}

fn ask(key: String, c: &CheckItem) {
    let Some(failing) = c.failing.clone() else { return };
    if ui(|u| matches!(u.loads.get(&key), Some(Load::Loading | Load::Done(_)))) {
        return;
    }
    ui(|u| u.loads.insert(key.clone(), Load::Loading));
    spawn(async move {
        let load = match pr_page::load_failure(failing).await {
            Ok(details) => Load::Done(details),
            Err(message) => Load::Error(message),
        };
        // A failed read is asked again the next time the row opens.
        ui(|u| u.loads.insert(key, load));
    });
}

pub fn build(st: &Status, unresolved: Option<usize>) -> Node {
    let items = with_unresolved(st.items.as_deref().unwrap_or_default(), unresolved);
    if items.is_empty() {
        let c = st.checks;
        return muted(if c.passed + c.failed + c.running > 0 { "This provider didn't list the checks by name." } else { "No checks reported." });
    }
    let can_run = pr_page::read(|p| p.can.run);
    if can_run && let Some(item) = run_view::opened_item(&items) {
        return run_view::build(&item, st);
    }
    stack().prop("gap", "m").children(group_checks(&items).into_iter().map(|g| {
        let open = ui(|u| u.groups.get(g.id.name()).copied()).unwrap_or(starts_open(g.id));
        let name = g.id.name();
        let head = Node::new("ListButton")
            .prop("aria-expanded", open)
            .on("onClick", move |_| ui(|u| {
                let now = u.groups.get(name).copied().unwrap_or(starts_open(g.id));
                u.groups.insert(name, !now);
            }))
            .child(text(g.label).prop("weight", "strong"))
            .child(Node::new("Badge").prop("variant", "secondary").prop("size", "sm").child(muted(g.items.len().to_string())));
        stack()
            .prop("gap", "s")
            .key(name)
            .child(head)
            .maybe(open.then(|| stack().prop("gap", "xs").children(g.items.iter().map(|c| check_row(st, c, can_run && c.run.is_some())))))
    }))
}

fn go() -> Node {
    Node::new("Svg")
        .prop("viewBox", "0 0 16 16")
        .prop("size", 16)
        .prop("tone", "muted")
        .prop("fill", "none")
        .prop("stroke", "currentColor")
        .prop("strokeWidth", "1.5")
        .prop("strokeLinecap", "round")
        .prop("strokeLinejoin", "round")
        .prop("shapes", json!([["path", { "d": "M6.5 4.5 10 8l-3.5 3.5" }]]))
}

fn check_row(st: &Status, c: &CheckItem, opens_run: bool) -> Node {
    let key = load_key(st, c);
    let expandable = !opens_run && c.state == CheckState::Failed && has_log(c.failing.as_ref());
    let open = ui(|u| u.expanded.contains(&c.id));
    let url = c.url.clone().filter(|url| url.starts_with("https://"));
    let when = c.at.as_deref().and_then(parts::time_text);
    let took = duration_text(c.seconds);
    let main = vec![
        parts::state_mark(c.state),
        stack()
            .prop("gap", "xs")
            .prop("grow", true)
            .child(
                row()
                    .prop("gap", "s")
                    .prop("wrap", true)
                    .child(text(c.name.as_str()).prop("weight", "medium"))
                    .maybe(match c.required {
                        Some(true) => Some(parts::tag("Required", true)),
                        Some(false) => Some(parts::tag("Optional", false)),
                        None => None,
                    })
                    .maybe(c.kind.as_ref().filter(|kind| **kind != c.name).map(|kind| muted(kind.as_str()).prop("size", "tiny"))),
            )
            .maybe(c.note.as_ref().map(|note| text(note.as_str()).prop("tone", if c.state == CheckState::Failed { "danger" } else { "muted" }).prop("size", "small"))),
        if opens_run { go() } else { row() },
    ];
    let line = row().prop("gap", "m").prop("align", "start").prop("grow", true).children(main);
    let id = c.id.clone();
    let lead = if opens_run {
        Node::new("ListButton").prop("title", format!("{}: see its jobs, steps and logs", c.name)).on("onClick", move |_| run_view::open(&id)).child(line)
    } else if expandable {
        let item = c.clone();
        let key = key.clone();
        Node::new("ListButton")
            .prop("aria-expanded", open)
            .on("onClick", move |_| {
                let now = ui(|u| {
                    if !u.expanded.remove(&item.id) {
                        u.expanded.insert(item.id.clone());
                    }
                    u.expanded.contains(&item.id)
                });
                if now {
                    ask(key.clone(), &item);
                }
            })
            .child(line)
    } else {
        row().prop("gap", "none").prop("grow", true).child(parts::column(7)).child(line).child(parts::column(7))
    };
    let time = [when, took].into_iter().flatten().collect::<Vec<_>>().join(" · ");
    let open_button = url.map(|url| Node::new("Button").prop("variant", "ghost").prop("size", "sm").prop("label", format!("Open {}", c.name)).on("onClick", move |_| host::open_url(&url)).text("Open"));
    Node::new("Bar")
        .key(c.id.clone())
        .prop("edge", "top")
        .prop("gap", "xs")
        .child(row().prop("gap", "m").child(lead).maybe((!time.is_empty()).then(|| muted(time).prop("size", "small"))).child(parts::column(52).child(row().prop("justify", "end").maybe(open_button))))
        .maybe(open.then(|| failure_body(&key)))
}

fn failure_body(key: &str) -> Node {
    let load = ui(|u| u.loads.get(key).cloned());
    let body: Vec<Node> = match load {
        None | Some(Load::Loading) => vec![muted("Loading the log…").prop("size", "small")],
        Some(Load::Error(message)) => vec![notice("danger", message)],
        Some(Load::Done(details)) => details.iter().enumerate().map(|(i, d)| detail(i, d)).collect(),
    };
    stack().prop("gap", "s").children(body)
}

fn detail(at: usize, d: &FailureDetail) -> Node {
    let name = d.step.as_deref().or(d.check.as_deref()).unwrap_or("the check");
    let log = (!d.log.is_empty()).then(|| log_tail(&d.log));
    stack()
        .prop("gap", "xs")
        .key(at.to_string())
        .child(
            Node::new("Text")
                .prop("size", "small")
                .child(text(d.step.as_deref().or(d.check.as_deref()).unwrap_or_default()).prop("weight", "strong"))
                .text(if d.summary.is_empty() { String::new() } else { format!(": {}", d.summary) }),
        )
        .child(match log {
            Some((tail, hidden)) => {
                let shown = tail.split('\n').count();
                stack()
                    .prop("gap", "xs")
                    .maybe((hidden > 0).then(|| muted(format!("Showing the last {shown} of {} lines.", shown + hidden)).prop("size", "tiny")))
                    .child(code_block(&tail))
            }
            None => muted(format!("No log for {name}.")).prop("size", "tiny"),
        })
}

/// Forgets what the tab asked for, for another pull request.
pub fn reset() {
    UI.with(|ui| *ui.borrow_mut() = Ui::default());
}
