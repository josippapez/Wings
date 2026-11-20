//! Small mocks of the chat the Settings page draws beside its choices, so a choice shows what it does. They are made from
//! the same plain nodes the page uses, with made-up data, and are not the chat's own views (those belong to the chat's frame).

use wings_plugin::ui::{Node, card, disclosure, gauge, muted, row, stack, text};

use crate::{folds::Kind, view_mode::Mode};

/// What a view mode shows of the mock turn: `thought` is None when it is left out, else whether it shows open.
#[derive(Debug, PartialEq, Eq)]
pub struct Shows {
    pub thought: Option<bool>,
    pub steps: bool,
    pub command: bool,
    pub read: bool,
    pub edit: bool,
}

/// What `mode` opens, given which kinds of fold Verbose is set to open (`prefs::opens`). Normal shuts every fold and leaves
/// the thoughts out, Thinking adds them open, and Verbose opens what is ticked.
pub fn shows(mode: Mode, opens: impl Fn(Kind) -> bool) -> Shows {
    match mode {
        Mode::Normal => Shows { thought: None, steps: false, command: false, read: false, edit: false },
        Mode::Thinking => Shows { thought: Some(true), steps: false, command: false, read: false, edit: false },
        Mode::Verbose => {
            let steps = opens(Kind::Steps);
            Shows { thought: Some(opens(Kind::Thought)), steps, command: steps && opens(Kind::Command), read: steps && opens(Kind::Read), edit: steps && opens(Kind::Edit) }
        }
    }
}

/// One code line in the tone of what it holds, as the chat draws a diff.
fn line(code: &str, tone: Option<&str>) -> Node {
    let node = Node::new("Code").prop("code", code);
    match tone {
        Some(tone) => node.prop("tone", tone),
        None => node.prop("variant", "band"),
    }
}

/// A tool call's line, as the chat draws it: a mark, the tool and what it ran on.
fn call(name: &str, what: &str) -> Node {
    row()
        .prop("gap", "s")
        .prop("align", "baseline")
        .child(text("✓").prop("tone", "success"))
        .child(text(name).prop("weight", "medium"))
        .child(text(what).prop("mono", true).prop("size", "small").prop("tone", "muted"))
}

fn steps_body(shows: &Shows) -> Node {
    let diff = stack().prop("gap", "none").child(line("-    input.trim()", Some("danger"))).child(line("+    input.trim_end_matches('\\r')", Some("success")));
    stack()
        .prop("gap", "s")
        .child(call("Bash", "cargo test parser"))
        .maybe(shows.command.then(|| line("test result: FAILED. 3 passed; 1 failed", None)))
        .child(call("Read", "src/parser.rs"))
        .maybe(shows.read.then(|| line("41  fn parse(input: &str) -> Ast {", None)))
        .child(call("Edit", "src/parser.rs"))
        .maybe(shows.edit.then_some(diff))
}

/// One made-up turn as `mode` shows it: your message, what Claude thought, its run of tool calls, and its reply.
pub fn turn(mode: Mode, opens: impl Fn(Kind) -> bool) -> Node {
    let shows = shows(mode, opens);
    let thought = shows.thought.map(|open| {
        disclosure("Thought for 4s", muted("The test expects the trailing newline, and the parser trims it.")).prop("open", open).key(format!("thought-{open}"))
    });
    let steps = disclosure("Ran 1 command · Read 1 file · Edited 1 file", steps_body(&shows)).prop("open", shows.steps).key(format!("steps-{}", shows.steps));
    stack()
        .prop("gap", "s")
        .key(format!("turn-{}", mode.name()))
        .child(row().prop("gap", "s").child(muted("You")).child(text("Fix the failing parser test")))
        .maybe(thought)
        .child(steps)
        .child(text("The parser now keeps the trailing newline, and the test passes."))
}

/// A sample of the conversation's text at a text size: the scale the chat gives its scroll.
pub fn sample(scale: f32) -> Node {
    let lines = stack()
        .prop("gap", "xs")
        .child(text("Fix the failing parser test").prop("weight", "medium"))
        .child(text("The parser now keeps the trailing newline, and the test passes."))
        .child(muted("Edited src/parser.rs"));
    Node::new("Scroll").prop("textScale", scale as f64).child(lines)
}

/// A gauge filled to `percent`, amber for where Claude pauses and red for where a chat stops.
pub fn limit(label: &str, percent: f64, stops: bool) -> Node {
    let levels = if stops { (percent as u32, percent as u32) } else { (percent as u32, 101) };
    gauge(label, percent, levels, None)
}

/// The session panel's two places as a mock of Wings' window: the chat's tab, and the sidebar beside it.
pub fn places(sidebar: bool) -> Node {
    let panel = || card().child(stack().prop("gap", "xs").child(text("Session").prop("weight", "medium")).child(muted("Changes +3 −2")).child(muted("Context 42%")));
    let chat = card().child(
        row().prop("gap", "s").prop("align", "start")
            .child(stack().prop("grow", true).prop("gap", "xs").child(text("Chat").prop("weight", "medium")).child(muted("Fix the failing parser test")))
            .maybe((!sidebar).then(|| stack().prop("width", 150).child(panel()))),
    );
    let side = if sidebar { stack().prop("width", 150).child(panel()) } else { stack().prop("width", 150).child(muted("Wings sidebar")) };
    row().prop("gap", "s").prop("align", "start").child(stack().prop("grow", true).child(chat)).child(side)
}

/// A diff of two lines changed, side by side or in one column.
pub fn diff(split: bool) -> Node {
    let old = line("-    input.trim()", Some("danger"));
    let new = line("+    input.trim_end()", Some("success"));
    if split {
        row().prop("gap", "xs").prop("align", "start").child(stack().prop("grow", true).child(old)).child(stack().prop("grow", true).child(new))
    } else {
        stack().prop("gap", "none").child(old).child(new)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_view_mode_opens_a_little_more_and_verbose_follows_what_is_ticked() {
        let all = |_| true;
        assert_eq!(shows(Mode::Normal, all), Shows { thought: None, steps: false, command: false, read: false, edit: false });
        assert_eq!(shows(Mode::Thinking, all).thought, Some(true));
        assert!(!shows(Mode::Thinking, all).steps);
        let verbose = shows(Mode::Verbose, all);
        assert!(verbose.steps && verbose.command && verbose.read && verbose.edit && verbose.thought == Some(true));
        // Clearing the run's own fold keeps its calls' outputs shut, as in the chat; a cleared kind stays shut.
        let no_steps = shows(Mode::Verbose, |kind| kind != Kind::Steps);
        assert!(!no_steps.steps && !no_steps.command && !no_steps.edit);
        let no_command = shows(Mode::Verbose, |kind| kind != Kind::Command);
        assert!(no_command.steps && !no_command.command && no_command.read);
    }

    #[test]
    fn a_limit_gauge_is_amber_for_the_pause_and_red_for_the_stop() {
        let pause = wings_plugin::ui::tree_json(limit("Pause", 90.0, false));
        assert_eq!(pause["props"]["levels"], serde_json::json!([90, 101]));
        let stop = wings_plugin::ui::tree_json(limit("Stop", 95.0, true));
        assert_eq!(stop["props"]["levels"], serde_json::json!([95, 95]));
    }
}
