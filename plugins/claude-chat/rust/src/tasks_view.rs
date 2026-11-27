//! The background tasks Claude Code runs for the session: a group in the side panel with each running one, how long it
//! has run and a Stop button, and a dialog over the chat with the end of what one printed, which a click on it opens.

use wings_plugin::{
    format::truncate,
    ui::{Entry, Node, button, card, dialog as panel, entry, icon, loading, muted, notice, row, spinner, stack, text},
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
    let failed = done.iter().filter(|task| task.ended.as_deref() == Some("failed")).count();
    let mut parts = vec![String::from("Background tasks")];
    let busy = running.iter().filter(|task| task.active()).count();
    if busy > 0 {
        parts.push(format!("{busy} running"));
    }
    if done.len() > failed {
        parts.push(format!("{} done", done.len() - failed));
    }
    if failed > 0 {
        parts.push(format!("{failed} failed"));
    }
    let title = parts.join(" · ");
    // A workflow's phases and agents, and what a subagent started, are under it.
    let tree = crate::workflow::tree(&c.session, &running, &task_row);
    Some(crate::view::fold(c, "tasks", title, tree.into_iter().chain(done.into_iter().map(done_row))))
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
    let line = entry(Entry { lead: Some(mark), title: text(name).prop("size", "small").prop("clamp", true).prop("title", name), detail: Some(muted(time).prop("size", "small")), trailing: None }).key(task.id.as_str());
    match task.has_output() {
        true => {
            let id = task.id.clone();
            line.on("onClick", move |_| chat::open_task(&id)).prop("title", "What it printed")
        }
        false => line,
    }
}

/// One running task: its name, how long it has run, small Message and Stop buttons, and on a click on the name its output
/// when Claude Code keeps it, or a background agent's conversation in the sidebar.
fn task_row(task: &Task) -> Node {
    let name = if task.description.is_empty() { task.id.as_str() } else { task.description.as_str() };
    let time = match task.stopping {
        true => muted("Stopping"),
        false => muted("Running").prop("since", task.started),
    };
    // The spinner, the name over how long it has run, and the small buttons at the right, on one row as a subagent's
    // is: the time and buttons sat on a line of their own, out of line with the name.
    let main = entry(Entry { lead: Some(spinner()), title: text(name).prop("size", "small").prop("clamp", true).prop("title", name), detail: Some(time.prop("size", "small")), trailing: None });
    let id = task.id.clone();
    let main = match (task.has_output(), task.kind == "local_agent") {
        (true, _) => main.on("onClick", move |_| chat::open_task(&id)).prop("title", "What it printed"),
        (false, true) => main.on("onClick", move |_| open_subagent(&id)).prop("title", "Its conversation, in the sidebar"),
        // In the inset a clickable row has, so its name starts where theirs do.
        _ => main,
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
        .child(row().prop("gap", "xs").prop("align", "center").child(stack().prop("grow", true).child(main)).child(actions))
        .maybe(task.error.as_deref().map(|why| text(why).prop("size", "small").prop("tone", "danger").prop("clamp", true).prop("title", why)))
}

/// The small square on Stop, in a 14-point box.
pub(crate) const STOP: &str = "M3.5 3.5h7v7h-7z";

/// A click on a subagent, by the id of the call that started it or a background agent's own id: its conversation, in Wings'
/// right sidebar, which has a button that pops it out into a tab beside the chat's. One place, so what it does can change in
/// one place.
pub fn open_subagent(id: &str) {
    crate::subagent_relay::open_in_sidebar(id);
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
    let stop = task.ended.is_none().then(|| button("Stop", "outline", task.stopping, move || chat::stop_task(&id)).prop("size", "sm").prop("title", "Stop this task"));
    let head = row()
        .prop("gap", "s")
        .prop("align", "center")
        .child(stack().prop("grow", true).child(state.prop("size", "small")))
        .child(button("Refresh", "outline", false, chat::refresh_task).prop("size", "sm").prop("title", "Read what it printed again"))
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

#[cfg(test)]
mod tests {
    use wings_plugin::ui::tree_json;

    use super::*;

    fn task(id: &str, ended: Option<&str>) -> Task {
        Task { id: id.into(), kind: "local_bash".into(), description: format!("build {id}"), call: String::new(), started: 1000, ended: ended.map(str::to_owned), ended_at: 8000, stopping: false, output: None, error: None, ambient: false }
    }

    #[test]
    fn the_group_has_the_heading_of_the_other_groups_and_a_running_task_says_running_in_a_fixed_icon_box() {
        chat::with(|c| {
            c.session.tasks = vec![task("a", None), task("b", Some("completed"))];
            let drawn = tree_json(section(c).expect("a group")).to_string();
            // A section, as the Subagents group is, so its heading is on the rows' left edge.
            assert!(drawn.contains("\"Section\"") && drawn.contains("Background tasks · 1 running · 1 done"), "{drawn}");
            assert!(drawn.contains("\"Running\"") && !drawn.contains("\"running\""), "{drawn}");
            // The spinner and the mark are each a row's lead, which the app sets in one box, so the names start at one x.
            assert_eq!(drawn.matches("\"type\":\"ListRow\"").count(), 2, "{drawn}");
            assert_eq!(drawn.matches("\"name\":\"lead\"").count(), 2, "{drawn}");
            c.session.tasks.clear();
        });
    }

    #[test]
    fn a_workflow_run_is_a_tree_of_its_phases_and_agents_and_a_task_sits_under_the_subagent_that_started_it() {
        chat::with(|c| {
            let hear = |c: &mut Chat, line: serde_json::Value| c.session.hear(&line);
            hear(c, serde_json::json!({ "type": "system", "subtype": "background_tasks_changed", "tasks": [
                { "task_id": "w1", "task_type": "local_workflow", "description": "Survey the API" },
                { "task_id": "a1", "task_type": "local_agent", "subagent_type": "general-purpose", "description": "Reader" },
                { "task_id": "b1", "task_type": "local_bash", "description": "Watch the build", "parent_task_id": "a1" },
            ] }));
            // As Claude Code 2.1.330 sends a run's rows with its progress.
            hear(c, serde_json::json!({ "type": "system", "subtype": "task_progress", "task_id": "w1", "description": "Survey the API", "usage": { "total_tokens": 9, "tool_uses": 3, "duration_ms": 5000 },
                "workflow_progress": [
                    { "type": "workflow_phase", "index": 0, "title": "Search", "kind": "phase" },
                    { "type": "workflow_phase", "index": 1, "title": "Verify", "kind": "phase" },
                    { "type": "workflow_agent", "index": 0, "label": "find sources", "phaseIndex": 0, "phaseTitle": "Search", "state": "start" },
                    { "type": "workflow_agent", "index": 0, "label": "find sources", "phaseIndex": 0, "phaseTitle": "Search", "state": "done", "tokens": 4200, "toolCalls": 7 },
                    { "type": "workflow_agent", "index": 1, "label": "check claim 1", "phaseIndex": 1, "phaseTitle": "Verify", "state": "progress", "toolCalls": 2 },
                    { "type": "workflow_agent", "index": 2, "label": "check claim 2", "phaseIndex": 1, "phaseTitle": "Verify", "state": "error", "error": "rate limited" },
                ] }));
            let drawn = tree_json(section(c).expect("a group")).to_string();
            let at = |needle: &str| drawn.find(needle).unwrap_or_else(|| panic!("{needle} is not drawn: {drawn}"));
            // The run, then its phases each with its agents, in order, the way the terminal's progress view lists them.
            assert!(at("Survey the API") < at("Search") && at("Search") < at("find sources") && at("find sources") < at("Verify") && at("Verify") < at("check claim 1"), "{drawn}");
            assert!(drawn.contains("1 of 1 done") && drawn.contains("0 of 2 done · 1 failed") && drawn.contains("rate limited"), "{drawn}");
            // The command is under the subagent that started it, which is under no one: the run is not drawn between them.
            assert!(at("Reader") < at("Watch the build"), "{drawn}");
            assert!(drawn.contains("\"width\":16"), "a nested row is moved in: {drawn}");
            c.session.tasks.clear();
            c.session.flows = Default::default();
        });
    }

    #[test]
    fn a_task_that_failed_is_counted_as_failed_not_done() {
        chat::with(|c| {
            c.session.tasks = vec![task("a", Some("failed")), task("b", Some("completed"))];
            let drawn = tree_json(section(c).expect("a group")).to_string();
            assert!(drawn.contains("Background tasks · 1 done · 1 failed"), "{drawn}");
            c.session.tasks.clear();
        });
    }

    #[test]
    fn the_output_dialogs_refresh_and_stop_are_both_outline_buttons() {
        chat::with(|c| {
            c.session.tasks = vec![task("a", None)];
            c.task = Some("a".into());
            let drawn = tree_json(dialog(c).expect("a dialog")).to_string();
            assert_eq!(drawn.matches("\"variant\":\"outline\"").count(), 2, "{drawn}");
            c.task = None;
            c.session.tasks.clear();
        });
    }
}
