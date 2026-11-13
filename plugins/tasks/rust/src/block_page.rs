//! The Issues block of a Project page: counts and the project's open issues, compact. Add an issue on the line above, open one to
//! read it, or open the board tab for the rest.

use crate::{
    add_issue,
    domain::{Issue, Status},
    feed,
    orderings::open_issues,
    parts::{button, error_text, line, muted, row, session_marker, stack, status_dot, status_summary, type_icon},
};
use wings_plugin::ui::{Node, empty, loading};

/// The block lists this many open issues; the board has the rest.
const BLOCK_ROWS: usize = 8;

fn open_row(i: &Issue) -> Node {
    let key = i.key.clone();
    let id = feed::with(|f| f.only.clone().or_else(|| f.project.clone().flatten().map(|p| p.id))).unwrap_or_default();
    Node::new("ListItem").key(i.key.as_str()).child(
        Node::new("ListButton")
            .prop("label", format!("Open {}: {}", i.key, i.title))
            .on("onClick", move |_| feed::open_issue(&key, &id, None, false))
            .child(status_dot(i.status))
            .child(type_icon(i.kind, false, 14))
            .child(line(i.key.as_str(), Some("muted"), Some("tiny")).prop("mono", true))
            .child(Node::new("Text").prop("grow", true).prop("truncate", true).text(i.title.as_str()))
            .maybe((i.status != Status::Todo).then(|| line(i.status.label(), Some("muted"), Some("tiny"))))
            .maybe(session_marker(i)),
    )
}

fn view() -> Vec<Node> {
    let Some(project) = feed::project() else {
        return vec![loading().text("Loading issues")];
    };
    let Some(project) = project else {
        return vec![empty("No project open", "Open a project to see its issues.")];
    };
    let (store, error) = feed::with(|f| (f.store.clone(), f.error.clone()));
    if let (None, Some(error)) = (&store, &error) {
        return vec![empty("Couldn't read the issues", error)];
    }
    let top = row("m").prop("justify", "between").prop("wrap", true).child(add_issue::node()).child(button("Open board", "outline", "sm", feed::open_board));
    let body: Vec<Node> = match store.as_deref().map(|s| s.issues.as_slice()) {
        None => vec![loading().text("Loading issues")],
        Some([]) => vec![empty("No issues yet", "Add one above, or ask Claude to keep track of its work here.")],
        Some(issues) => {
            let open = open_issues(issues);
            let mut body = vec![status_summary(issues)];
            if open.is_empty() {
                body.push(muted(format!("Nothing open for {}.", project.name)).prop("size", "small"));
            } else {
                body.push(Node::new("List").children(open.iter().take(BLOCK_ROWS).map(|i| open_row(i))));
            }
            if open.len() > BLOCK_ROWS {
                body.push(Node::new("Button").prop("variant", "link").prop("size", "xs").on("onClick", |_| feed::open_board()).text(format!("{} more on the board", open.len() - BLOCK_ROWS)));
            }
            body
        }
    };
    vec![stack("s").child(top).children(body).maybe(error.map(|e| error_text(&e)))]
}

#[unsafe(no_mangle)]
pub fn start() {
    wings_plugin::run(view);
    feed::watch(None);
}
