//! The background tasks Claude Code runs for the session: a group in the side panel with each running one, how long it
//! has run and a Stop button, and a dialog over the chat with the end of what one printed, which a click on it opens.

use wings_plugin::{
    format::truncate,
    ui::{Node, button, card, dialog as panel, icon, list_button, loading, muted, notice, row, spinner, stack, text},
};

use crate::{
    chat::{self, Chat},
    session::Task,
};

const WIDTH: u32 = 640;

/// The most ended tasks the group keeps listing.
const DONE_LISTED: usize = 5;

/// The side panel's group of tasks: each running one, then the newest that ended with how long they ran. A subagent that
/// ended is in the Subagents group, so it isn't listed twice. None when there is nothing to show.
pub fn section(c: &Chat) -> Option<Node> {
    let running: Vec<&Task> = c.session.tasks.iter().filter(|task| task.ended.is_none()).collect();
    let done: Vec<&Task> = c.session.tasks.iter().rev().filter(|task| task.ended.is_some() && task.kind != "local_agent").take(DONE_LISTED).collect();
    if running.is_empty() && done.is_empty() {
        return None;
    }
    let title = match (running.len(), done.len()) {
        (0, done) => format!("Background tasks · {done} done"),
        (running, 0) => format!("Background tasks · {running} running"),
        (running, done) => format!("Background tasks · {running} running · {done} done"),
    };
    Some(
        stack()
            .prop("gap", "xs")
            .child(muted(title).prop("size", "small"))
            .child(stack().prop("gap", "none").children(running.into_iter().map(task_row)).children(done.into_iter().map(done_row))),
    )
}

/// One task that ended: how it ended, its name and how long it ran, with its output on a click when Claude Code keeps it.
fn done_row(task: &Task) -> Node {
    let name = if task.description.is_empty() { task.id.as_str() } else { task.description.as_str() };
    let ended = task.ended.as_deref().unwrap_or_default();
    let mark = match ended {
        "failed" => text("✗").prop("tone", "danger"),
        "completed" => text("✓").prop("tone", "success"),
        _ => muted("■"),
    };
    let how = match ended {
        "failed" => "Failed",
        "completed" => "Done",
        "stopped" | "killed" => "Stopped",
        _ => "Ended",
    };
    let time = match task.ended_at.saturating_sub(task.started) {
        ms if task.started > 0 && ms >= 1000 => format!("{how} · {}", crate::view::took(ms)),
        _ => how.to_owned(),
    };
    let line = row()
        .prop("gap", "s")
        .prop("align", "center")
        .child(mark)
        .child(stack().prop("grow", true).prop("gap", "none").child(text(name).prop("size", "small").prop("clamp", true).prop("title", name)).child(muted(time).prop("size", "small")));
    match task.has_output() {
        true => {
            let id = task.id.clone();
            list_button([line], move || chat::open_task(&id)).prop("title", "What it printed").key(task.id.as_str())
        }
        false => line.key(task.id.as_str()),
    }
}

/// One running task: its name, how long it has run, small Message and Stop buttons, and on a click on the name its output
/// when Claude Code keeps it, or a background agent's conversation in a tab of its own.
fn task_row(task: &Task) -> Node {
    let name = if task.description.is_empty() { task.id.as_str() } else { task.description.as_str() };
    let time = match task.stopping {
        true => muted("stopping"),
        false => muted("running").prop("since", task.started),
    };
    // The name has the whole row, three lines at most; under it the time, and the small buttons at the right.
    let title = row().prop("gap", "s").prop("align", "center").child(spinner()).child(stack().prop("grow", true).child(text(name).prop("size", "small").prop("clamp", true).prop("title", name)));
    let id = task.id.clone();
    let title = match (task.has_output(), task.kind == "local_agent") {
        (true, _) => list_button([title], move || chat::open_task(&id)).prop("title", "What it printed"),
        (false, true) => list_button([title], move || open_subagent(&id)).prop("title", "Its conversation, in a tab of its own"),
        _ => title,
    };
    let id = task.id.clone();
    let actions = row()
        .prop("gap", "xs")
        .prop("align", "center")
        .maybe(crate::actions_view::message_button(task))
        .child(
            button("", "ghost", task.stopping, move || chat::stop_task(&id))
                .prop("size", "icon-sm")
                .prop("label", "Stop")
                .prop("title", "Stop this task")
                .child(icon(14, 2.0, &[STOP])),
        );
    stack()
        .prop("gap", "none")
        .key(task.id.as_str())
        .child(title)
        .child(row().prop("gap", "xs").prop("align", "center").child(stack().prop("grow", true).child(time.prop("size", "small"))).child(actions))
        .maybe(task.error.as_deref().map(|why| text(why).prop("size", "small").prop("tone", "danger").prop("clamp", true).prop("title", why)))
}

/// The small square on Stop, in a 14-point box.
const STOP: &str = "M3.5 3.5h7v7h-7z";

/// A click on a subagent, by the id of the call that started it or a background agent's own id: its conversation, in a
/// tab of its own right after the chat's. One place, so what it does can change in one place.
pub fn open_subagent(id: &str) {
    crate::subagent_relay::open(id);
}

/// The dialog with the end of what the open task printed, while it is open: how it stands, Refresh and Stop, and the
/// output as plain text, as Claude Code says to show it.
pub fn dialog(c: &Chat) -> Option<Node> {
    let task = c.session.tasks.iter().find(|task| c.task.as_deref() == Some(task.id.as_str()))?;
    let state = match task.ended.as_deref() {
        None if task.stopping => muted("Stopping"),
        None => muted("Running").prop("since", task.started),
        Some("completed") => muted("Done"),
        Some("failed") => text("Failed").prop("tone", "danger"),
        Some("stopped" | "killed") => muted("Stopped"),
        Some(_) => muted("Ended"),
    };
    let id = task.id.clone();
    let stop = task.ended.is_none().then(|| button("Stop", "outline", task.stopping, move || chat::stop_task(&id)).prop("size", "sm"));
    let head = row()
        .prop("gap", "s")
        .prop("align", "center")
        .child(stack().prop("grow", true).child(state.prop("size", "small")))
        .child(button("Refresh", "ghost", false, chat::refresh_task).prop("size", "sm").prop("title", "Read what it printed again"))
        .maybe(stop);
    let body = match &task.output {
        None => loading(),
        Some(output) if output.text.trim().is_empty() => muted("It hasn't printed anything yet."),
        // One text, however many lines: a line a node would take a long output past what the page may draw.
        Some(output) => card().child(text(output.text.trim_end()).prop("mono", true).prop("size", "small")),
    };
    let cut = task.output.as_ref().filter(|output| output.truncated).map(|output| muted(format!("The last 8 KB of the {} KB it printed.", output.bytes.div_ceil(1024))).prop("size", "small"));
    let error = task.error.as_deref().map(|why| notice("danger", why));
    let title = if task.description.is_empty() { task.id.clone() } else { truncate(&task.description, 80) };
    Some(panel(&title, WIDTH, chat::close_task).child(head).maybe(error).maybe(cut).child(body))
}
