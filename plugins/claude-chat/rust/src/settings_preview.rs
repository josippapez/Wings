//! Small mocks of the chat the Settings page draws beside its choices, so a choice shows what it does. They are made from
//! the same plain nodes the page uses, with made-up data, and are not the chat's own views (those belong to the chat's frame).

use wings_plugin::ui::{Node, card, disclosure, gauge, icon, muted, row, stack, text};

use crate::{folds::Kind, view_mode::Mode};

/// What a view mode shows of the mock turn: `thought` is None when it is left out, else whether it shows open. Every
/// other field is whether that kind of fold shows open.
#[derive(Debug, PartialEq, Eq, Default)]
pub struct Shows {
    pub thought: Option<bool>,
    pub steps: bool,
    pub command: bool,
    pub read: bool,
    pub edit: bool,
    pub search: bool,
    pub web: bool,
    pub subagent: bool,
    pub other: bool,
    pub summary: bool,
    pub note: bool,
}

/// What `mode` opens, given which kinds of fold Verbose is set to open (`prefs::opens`). Normal shuts every fold and leaves
/// the thoughts out, Thinking adds them open, and Verbose opens what is ticked: a call's output only inside an open run.
pub fn shows(mode: Mode, opens: impl Fn(Kind) -> bool) -> Shows {
    match mode {
        Mode::Normal => Shows::default(),
        Mode::Thinking => Shows { thought: Some(true), ..Shows::default() },
        Mode::Verbose => {
            let steps = opens(Kind::Steps);
            let call = |kind| steps && opens(kind);
            Shows {
                thought: Some(opens(Kind::Thought)),
                steps,
                command: call(Kind::Command),
                read: call(Kind::Read),
                edit: call(Kind::Edit),
                search: call(Kind::Search),
                web: call(Kind::Web),
                subagent: call(Kind::Subagent),
                other: call(Kind::Other),
                summary: opens(Kind::Summary),
                note: opens(Kind::Note),
            }
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
    let diff = stack().prop("gap", "none").child(line("-    input.trim()", Some("danger"))).child(line("+    input.trim_end()", Some("success")));
    stack()
        .prop("gap", "s")
        .child(call("Bash", "cargo test parser"))
        .maybe(shows.command.then(|| line("test result: FAILED. 3 passed; 1 failed", None)))
        .child(call("Read", "src/parser.rs"))
        .maybe(shows.read.then(|| line("41  fn parse(input: &str) -> Ast {", None)))
        .child(call("Edit", "src/parser.rs"))
        .maybe(shows.edit.then_some(diff))
        .child(call("Grep", "parse_ident"))
        .maybe(shows.search.then(|| line("src/parser.rs:41\nsrc/lexer.rs:9", None)))
        .child(call("WebFetch", "docs.example.com/parser"))
        .maybe(shows.web.then(|| small("The parser guide: trailing newlines are kept unless trimmed.")))
        .child(call("Agent", "Audit the discount code"))
        .maybe(shows.subagent.then(|| small("Two checks skip the expiry date. Both are fixed.")))
        .child(call("Skill", "review"))
        .maybe(shows.other.then(|| small("Reviewed 3 files, no findings.")))
}

/// One made-up turn as `mode` shows it: your message, what Claude thought, its run of tool calls, its reply, and what came
/// after it (a finished agent's summary, a note from Claude Code and a subagent's report, whole when `report` says so in
/// every mode): every kind of fold Settings ticks.
pub fn turn(mode: Mode, opens: impl Fn(Kind) -> bool, report: bool) -> Node {
    let shows = shows(mode, opens);
    let thought = shows.thought.map(|open| {
        disclosure("Thought for 4s", muted("The test expects the trailing newline, and the parser trims it.")).prop("open", open).key(format!("thought-{open}"))
    });
    let steps = disclosure("Ran 1 command · Read 1 file · Edited 1 file · 4 more", steps_body(&shows)).prop("open", shows.steps).key(format!("steps-{}", shows.steps));
    stack()
        .prop("gap", "s")
        .key(format!("turn-{}", mode.name()))
        .child(row().prop("gap", "s").child(muted("You")).child(text("Fix the failing parser test")))
        .maybe(thought)
        .child(steps)
        .child(text("The parser now keeps the trailing newline, and the test passes."))
        .child(kind(Kind::Summary, shows.summary))
        .child(kind(Kind::Note, shows.note))
        .child(self::report(report))
}

/// A fold as the chat draws it: its line, and under it the body when it is open. The key carries the state, so a tick
/// remounts the fold open or shut.
fn fold(title: &str, body: Node, open: bool) -> Node {
    let body = if open { body } else { stack() };
    row().child(disclosure(title, body).prop("open", open).key(format!("fold-{title}-{open}")))
}

fn small(body: &str) -> Node {
    text(body).prop("tone", "muted").prop("size", "small")
}

/// The chat element a kind of fold opens or shuts, as it looks with the fold `open`, made of sample text.
pub fn kind(kind: Kind, open: bool) -> Node {
    let diff = || stack().prop("gap", "none").child(line("-    input.trim()", Some("danger"))).child(line("+    input.trim_end()", Some("success")));
    let under = |call: Node, title: &str, body: Node| stack().prop("gap", "xs").child(call).child(fold(title, body, open));
    let node = match kind {
        Kind::Steps => fold(
            "Ran 1 command · Read 1 file · Edited 1 file",
            stack().prop("gap", "xs").child(call("Bash", "cargo test parser")).child(call("Read", "src/parser.rs")).child(call("Edit", "src/parser.rs")),
            open,
        ),
        Kind::Command => under(call("Bash", "cargo test parser"), "Output", line("test result: FAILED. 3 passed; 1 failed", None)),
        Kind::Read => under(call("Read", "src/parser.rs"), "Output", line("41  fn parse(input: &str) -> Ast {", None)),
        Kind::Edit => under(call("Edit", "src/parser.rs"), "Changes", diff()),
        Kind::Search => under(call("Grep", "parse_ident"), "Output", line("src/parser.rs:41\nsrc/lexer.rs:9", None)),
        Kind::Web => under(call("WebFetch", "docs.example.com/parser"), "Output", small("The parser guide: trailing newlines are kept unless trimmed.")),
        Kind::Subagent => under(call("Agent", "Audit the discount code"), "Output", small("Two checks skip the expiry date. Both are fixed.")),
        Kind::Other => under(call("Skill", "review"), "Output", small("Reviewed 3 files, no findings.")),
        Kind::Thought => fold("Thought for 7s", small("The test expects the trailing newline, and the parser trims it."), open),
        Kind::Summary => stack()
            .prop("gap", "xs")
            .child(row().prop("gap", "s").child(text("✓").prop("tone", "success")).child(text("Agent \"Audit the discount code\" finished · 12s").prop("size", "small").prop("tone", "muted")))
            .child(fold("Summary", small("Checked the discount code: two cases skipped the expiry date, both fixed."), open)),
        Kind::Note => fold("⚙ Hook output", small("pre-commit: formatted 2 files."), open),
    };
    node.key(format!("kind-{}-{open}", kind.key()))
}

/// A subagent's report in the chat, whole when `open` and as its first line when not.
pub fn report(open: bool) -> Node {
    let head = row().prop("gap", "s").prop("align", "center").child(text("↩").prop("tone", "muted")).child(text("Report from Audit the discount code").prop("weight", "medium").prop("size", "small"));
    let first = "Two checks skip the expiry date.";
    let fold = if open {
        fold("Report", stack().prop("gap", "xs").child(text(first)).child(text("Both are fixed in discount.rs, and the tests cover them.")), true)
    } else {
        fold(first, stack(), false)
    };
    stack().prop("gap", "xs").key(format!("report-{open}")).child(head).child(fold)
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

/// Lucide's `smartphone`, as on the chat's Remote Control button (`view.rs`).
const PHONE: [&str; 2] = ["M7 2h10a2 2 0 0 1 2 2v16a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2z", "M12 18h.01"];

/// The foot of a new chat's message box: its Remote Control button lit and named when the chat starts with it on, else
/// the quiet icon you press to turn it on.
pub fn remote(on: bool) -> Node {
    let glyph = icon(24, 2.0, &PHONE).prop("size", 14);
    let button = if on {
        Node::new("Button").prop("variant", "secondary").prop("size", "sm").prop("label", "Remote Control").child(glyph).child(text("Remote"))
    } else {
        Node::new("Button").prop("variant", "ghost").prop("size", "icon-sm").prop("label", "Remote Control").child(glyph)
    };
    let said = if on { "On as the chat starts" } else { "Off until you press it" };
    row()
        .prop("gap", "m")
        .prop("align", "center")
        .key(format!("remote-{on}"))
        .child(small("Opus 5.5 · High"))
        .child(small("Auto"))
        .child(button)
        .child(small(said))
}

/// A gauge filled to `percent`, amber for where Claude pauses and red for where a chat stops.
pub fn limit(label: &str, percent: f64, stops: bool) -> Node {
    let levels = if stops { (percent as u32, percent as u32) } else { (percent as u32, 101) };
    gauge(label, percent, levels, None)
}

/// The session panel's two places as a mock of Wings' window: the chat's tab, and the sidebar beside it.
pub fn places(sidebar: bool) -> Node {
    let panel = || card().child(stack().prop("gap", "xs").child(text("Session").prop("weight", "medium")).child(muted("Changes +3 −2")).child(muted("Context 42%")));
    let chat = stack().prop("grow", true).prop("gap", "xs").child(text("Chat").prop("weight", "medium")).child(muted("Fix the failing parser test"));
    // In the chat, the panel sits inside the chat's own card, on its right.
    let chat = card().child(row().prop("gap", "m").prop("align", "start").child(chat).maybe((!sidebar).then(|| stack().prop("width", 150).child(panel()))));
    if !sidebar {
        return chat;
    }
    let side = stack().prop("width", 150).prop("gap", "xs").child(small("Wings sidebar")).child(panel());
    row().prop("gap", "m").prop("align", "start").child(stack().prop("grow", true).child(chat)).child(side)
}

/// A made-up chat session as the view options show it, for the pane beside the choices: a turn as the view mode and the fold
/// ticks show it, at the text size, then an edit's diff in its layout, and where the session panel sits.
pub fn session(mode: Mode, opens: impl Fn(Kind) -> bool, report: bool, scale: f32, split: bool, sidebar: bool) -> Node {
    let chat = Node::new("Scroll").prop("textScale", scale as f64).child(turn(mode, opens, report));
    stack().prop("gap", "m").child(card().child(chat)).child(card().child(diff(split))).child(places(sidebar))
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
        assert_eq!(shows(Mode::Normal, all), Shows::default());
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
    fn the_session_holds_the_turn_at_the_text_size_with_the_diff_in_its_layout_and_the_panel_where_it_sits() {
        let json = |split, sidebar| wings_plugin::ui::tree_json(session(Mode::Verbose, |_| true, true, 1.25, split, sidebar));
        let (one, two) = (json(false, false), json(true, true));
        assert!(one.to_string().contains("\"textScale\":1.25") && one.to_string().contains("Fix the failing parser test"));
        // The layout and the place change what is drawn.
        assert_ne!(one, two);
        // Wings' sidebar is named only where the panel is in it; in the chat the panel sits in the chat's card.
        assert!(two.to_string().contains("Wings sidebar") && !one.to_string().contains("Wings sidebar"));
        // The diff card shows the same edit as the turn above it.
        assert_eq!(one.to_string().matches("trim_end()").count(), 2, "{one}");
    }

    /// Every tick under "What starts open" changes the session beside it in Verbose, and the subagent reports' tick in
    /// every mode: the session had only the run, its command, read, edit and the thinking.
    #[test]
    fn every_fold_tick_changes_the_session_preview() {
        let json = |mode, opens: &dyn Fn(Kind) -> bool, report| wings_plugin::ui::tree_json(session(mode, opens, report, 1.0, false, false)).to_string();
        for (kind, _, _) in crate::folds::KINDS {
            let all = json(Mode::Verbose, &|_| true, true);
            let without = json(Mode::Verbose, &|other| other != kind, true);
            assert_ne!(all, without, "{kind:?} changes nothing in the session");
        }
        for mode in [Mode::Normal, Mode::Thinking, Mode::Verbose] {
            assert_ne!(json(mode, &|_| true, true), json(mode, &|_| true, false), "the report tick in {}", mode.name());
        }
        // Normal keeps every fold shut: a tick changes nothing there but the report.
        assert_eq!(json(Mode::Normal, &|_| true, true), json(Mode::Normal, &|_| false, true));
    }

    #[test]
    fn a_limit_gauge_is_amber_for_the_pause_and_red_for_the_stop() {
        let pause = wings_plugin::ui::tree_json(limit("Pause", 90.0, false));
        assert_eq!(pause["props"]["levels"], serde_json::json!([90, 101]));
        let stop = wings_plugin::ui::tree_json(limit("Stop", 95.0, true));
        assert_eq!(stop["props"]["levels"], serde_json::json!([95, 95]));
    }
}
