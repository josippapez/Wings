//! Small pieces the Issues pages share, as the nodes `wings-plugin-ui/native` made of them: icons, the session marker, a
//! card, the counts, and the layout the TypeScript build's `layout-native` gave its boxes.

use crate::{
    actor::last_session,
    commands::agent_label,
    domain::{Issue, IssueType, Priority, Status},
};
use serde_json::{Value, json};
use wings_plugin::{format::ago, host, ui::Node};

/// A line of text with a tone and size.
pub fn line(content: impl Into<String>, tone: Option<&str>, size: Option<&str>) -> Node {
    let mut node = Node::new("Text").text(content);
    if let Some(tone) = tone {
        node = node.prop("tone", tone);
    }
    if let Some(size) = size {
        node = node.prop("size", size);
    }
    node
}

pub fn muted(content: impl Into<String>) -> Node {
    line(content, Some("muted"), None)
}

pub fn row(gap: &str) -> Node {
    Node::new("Row").prop("gap", gap)
}

pub fn stack(gap: &str) -> Node {
    Node::new("Stack").prop("gap", gap)
}

/// A column of a fixed width, in points: the native app's Stack takes one.
pub fn column(width: u32) -> Node {
    Node::new("Stack").prop("width", width)
}

/// An error from the last change, in the danger colour.
pub fn error_text(message: &str) -> Node {
    line(message, Some("danger"), None).prop("role", "alert")
}

/// A button of the given `size` and `variant`.
pub fn button(label: &str, variant: &str, size: &str, on_click: impl Fn() + 'static) -> Node {
    Node::new("Button").prop("variant", variant).prop("size", size).on("onClick", move |_| on_click()).text(label)
}

/// Whether a key event is one that asks to delete the focused issue: Delete, or Backspace without a modifier.
pub fn is_delete_key(event: &Value) -> bool {
    matches!(event["key"].as_str(), Some("Delete" | "Backspace")) && ["altKey", "ctrlKey", "metaKey", "shiftKey"].iter().all(|m| event[m] != true)
}

/// How long ago a time was, or nothing for a time that can't be read.
pub fn ago_text(ms: f64) -> String {
    ago(ms, host::now())
}

// ---------- icons ----------

fn shapes(list: Vec<Value>) -> Vec<Value> {
    list
}

fn svg(size: u32, tone: Option<&str>, label: Option<&str>, paint: Value, shapes: Vec<Value>) -> Node {
    let mut node = Node::new("Svg").prop("viewBox", "0 0 16 16").prop("size", size).prop("shapes", Value::Array(shapes));
    if let Some(tone) = tone {
        node = node.prop("tone", tone);
    }
    if let Some(label) = label {
        node = node.prop("label", label);
    }
    for (name, value) in paint.as_object().into_iter().flatten() {
        node = node.prop(name, value.clone());
    }
    node
}

/// A small icon for an issue's type, coloured by it. Decoration unless `label`: the card or page names the type in text too.
pub fn type_icon(kind: IssueType, label: bool, size: u32) -> Node {
    let list = match kind {
        IssueType::Task => vec![json!(["rect", { "x": 2.5, "y": 2.5, "width": 11, "height": 11, "rx": 2.5 }]), json!(["path", { "d": "m5.5 8.2 1.8 1.8 3.2-3.6" }])],
        IssueType::Bug => vec![json!(["ellipse", { "cx": 8, "cy": 9, "rx": 3, "ry": 4 }]), json!(["path", { "d": "M5 9H2.5M11 9h2.5M5.5 5.5 4 4M10.5 5.5 12 4M5.5 12.5 4 14M10.5 12.5 12 14M6 5a2 2 0 0 1 4 0" }])],
        IssueType::Feature => vec![json!(["path", { "d": "M8 2.2 9.6 6.4 13.8 8 9.6 9.6 8 13.8 6.4 9.6 2.2 8 6.4 6.4Z" }])],
        IssueType::Epic => vec![json!(["path", { "d": "M9 1.8 3.5 9H8l-1 5.2L12.5 7H8Z" }])],
    };
    let tone = match kind {
        IssueType::Task => "muted",
        IssueType::Bug => "danger",
        IssueType::Feature => "success",
        IssueType::Epic => "warn",
    };
    svg(size, Some(tone), label.then(|| kind.label()), json!({ "fill": "none", "stroke": "currentColor", "strokeWidth": 1.4, "strokeLinecap": "round", "strokeLinejoin": "round" }), shapes(list))
}

/// The urgent icon's square (x 1.5 to 14.5, corners of 3) with the exclamation mark's two strokes (1.8 wide, round) as holes.
const URGENT_CUT: &str = "M4.5 1.5h7a3 3 0 0 1 3 3v7a3 3 0 0 1-3 3h-7a3 3 0 0 1-3-3v-7a3 3 0 0 1 3-3ZM8 3.6a.9.9 0 0 0-.9.9V9a.9.9 0 0 0 1.8 0V4.5a.9.9 0 0 0-.9-.9ZM8 10.3a.9.9 0 0 0-.9.9v.3a.9.9 0 0 0 1.8 0v-.3a.9.9 0 0 0-.9-.9Z";

pub fn priority_name(priority: Option<Priority>) -> &'static str {
    match priority {
        Some(Priority::Urgent) => "Urgent",
        Some(Priority::High) => "High",
        Some(Priority::Medium) => "Medium",
        Some(Priority::Low) => "Low",
        None => "No priority",
    }
}

/// Linear's priority icon: three bars filled by level, an exclamation mark in a square for urgent, a dash for none. Named for
/// a screen reader with `label`, otherwise decoration next to text that says it.
pub fn priority_icon(priority: Option<Priority>, label: bool) -> Node {
    let filled = match priority {
        Some(Priority::Urgent | Priority::High) => 3,
        Some(Priority::Medium) => 2,
        Some(Priority::Low) => 1,
        None => 0,
    };
    // The app draws in one colour, so urgent's mark is cut out of its square, showing the page as the desktop's mark does.
    let (list, paint) = match priority {
        Some(Priority::Urgent) => (vec![json!(["path", { "d": URGENT_CUT, "fill": "currentColor", "fillRule": "evenodd" }])], json!({})),
        None => (vec![json!(["path", { "d": "M3 8h2M7 8h2M11 8h2", "stroke": "currentColor", "strokeWidth": 1.5, "strokeLinecap": "round" }])], json!({})),
        _ => (
            (0..3).map(|i| json!(["rect", { "x": 2.0 + f64::from(i) * 4.5, "y": 10.0 - f64::from(i) * 3.5, "width": 3, "height": 4.0 + f64::from(i) * 3.5, "rx": 1, "fill": "currentColor", "opacity": if i < filled { 1.0 } else { 0.3 } }])).collect(),
            json!({}),
        ),
    };
    let tone = match priority {
        Some(Priority::Urgent) => Some("warn"),
        Some(Priority::High) => None,
        _ => Some("muted"),
    };
    svg(14, tone, label.then(|| priority_name(priority)), paint, shapes(list))
}

/// A dot for an issue's status. Decoration: the column or the text names it.
pub fn status_dot(status: Status) -> Node {
    match status {
        Status::Todo | Status::Canceled => Node::new("Dot").prop("tone", "muted"),
        Status::InProgress => Node::new("Dot").prop("tone", "info"),
        Status::InReview => Node::new("Dot").prop("tone", "warn"),
        Status::Done => Node::new("Dot"),
    }
}

// ---------- what a card and a row hold ----------

/// Marks an issue an agent has worked on, with the name of the session that touched it last. Nothing for one only you have touched.
pub fn session_marker(issue: &Issue) -> Option<Node> {
    let last = last_session(issue)?;
    let label = match last.name.as_deref().filter(|n| !n.is_empty()) {
        Some(name) if name.chars().count() > 14 => format!("{}…", name.chars().take(13).collect::<String>()),
        Some(name) => name.to_owned(),
        None => agent_label(last.kind.as_deref()),
    };
    Some(Node::new("Badge").prop("variant", "outline").text(label))
}

/// How many of an epic's or feature's children are done, as a short bar and "3/7".
pub fn child_progress(done: usize, total: usize) -> Node {
    row("s").child(column(40).child(Node::new("Meter").prop("label", format!("{done} of {total} child issues done")).prop("percent", done as f64 / total as f64 * 100.0).prop("tone", "success"))).child(line(format!("{done}/{total}"), Some("muted"), Some("tiny")))
}

/// How many issues are to do, in progress, in review and done.
pub fn status_summary(issues: &[Issue]) -> Node {
    let count = |status: Status| issues.iter().filter(|i| i.status == status).count();
    let chip = |text: String, tone: Option<&str>| {
        let badge = Node::new("Badge").prop("variant", "outline").text(text);
        match tone {
            Some(tone) => badge.prop("tone", tone),
            None => badge,
        }
    };
    row("m")
        .prop("wrap", true)
        .child(chip(format!("{} to do", count(Status::Todo)), None))
        .child(chip(format!("{} in progress", count(Status::InProgress)), Some("info")))
        .child(chip(format!("{} in review", count(Status::InReview)), Some("warn")))
        .child(chip(format!("{} done", count(Status::Done)), Some("success")))
}

/// The time an issue changed, "Updated 5 min ago".
pub fn when(prefix: &str, ms: f64) -> String {
    format!("{prefix}{}", ago_text(ms))
}
