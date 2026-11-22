//! Small pieces the PR pages share: a check's mark, a tag, and how long ago a time was.

use crate::{checklist::iso_to_ms, types::CheckState};
use wings_plugin::{
    format::ago,
    host,
    ui::{Node, badge, icon, muted, row, slot, text},
};

/// Lucide's `refresh-cw` and `external-link`, which the pages' icon buttons draw.
pub const REFRESH: [&str; 4] = ["M3 12a9 9 0 0 1 9-9 9.75 9.75 0 0 1 6.74 2.74L21 8", "M21 3v5h-5", "M21 12a9 9 0 0 1-9 9 9.75 9.75 0 0 1-6.74-2.74L3 16", "M8 16H3v5"];
pub const EXTERNAL_LINK: [&str; 3] = ["M15 3h6v6", "M10 14 21 3", "M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6"];

/// A ghost icon button with a tooltip, as Claude chat's header has them: `label` is what it is read out as and what the
/// tooltip says.
pub fn icon_button(label: &str, paths: &[&str], disabled: bool, on_click: impl Fn() + 'static) -> Node {
    Node::new("Button")
        .prop("variant", "ghost")
        .prop("size", "icon-sm")
        .prop("label", label)
        .prop("title", label)
        .prop("disabled", disabled)
        .on("onClick", move |_| on_click())
        .child(icon(24, 2.0, paths).prop("size", 16))
}

/// A page whose `top` (the header and the bars under it) stays put over its `body`.
pub fn page_with(top: Vec<Node>, body: impl IntoIterator<Item = Node>) -> Node {
    Node::new("Page").child(Node::new("Slot").prop("name", "toolbar").children(top)).child(Node::new("Slot").prop("name", "body").children(body))
}

/// A header's title as the design system has it: the name strong and its context muted and small after it.
pub fn title_with_context(name: &str, context: &str) -> Node {
    slot(
        "title",
        row()
            .prop("gap", "m")
            .prop("align", "center")
            .child(text(name).prop("weight", "strong").prop("truncate", true).prop("title", name))
            .maybe((!context.is_empty()).then(|| muted(context).prop("size", "small").prop("truncate", true).prop("title", context))),
    )
}

/// A small muted label over a section of a page, with its count: "Conversation · 3".
pub fn section_label(label: impl Into<String>) -> Node {
    muted(label).prop("size", "small").prop("weight", "medium")
}

/// `parts` as one line of muted facts split by " · ", each keeping its own tone.
pub fn joined(parts: Vec<Node>) -> Node {
    let mut line = row().prop("gap", "s").prop("wrap", true).prop("align", "center");
    for (at, part) in parts.into_iter().enumerate() {
        if at > 0 {
            line = line.child(muted("·").prop("size", "small").key(format!("dot{at}")));
        }
        line = line.child(part.key(format!("part{at}")));
    }
    line
}

fn state_word(state: CheckState) -> &'static str {
    match state {
        CheckState::Failed => "Failed",
        CheckState::Running => "Running",
        CheckState::Queued => "Queued",
        CheckState::Passed => "Passed",
        CheckState::Skipped => "Not run",
    }
}

fn state_tone(state: CheckState) -> &'static str {
    match state {
        CheckState::Failed => "danger",
        CheckState::Running => "warn",
        CheckState::Queued | CheckState::Skipped => "muted",
        CheckState::Passed => "success",
    }
}

/// The marks as drawings, in a 16 point box.
fn state_shape(state: CheckState) -> serde_json::Value {
    use serde_json::json;
    match state {
        CheckState::Failed => json!([["path", { "d": "M3.5 3.5l9 9M12.5 3.5l-9 9" }]]),
        CheckState::Running => json!([["circle", { "cx": 8, "cy": 8, "r": 5.5, "fill": "currentColor", "stroke": "none" }]]),
        CheckState::Queued => json!([["circle", { "cx": 8, "cy": 8, "r": 4.8 }]]),
        CheckState::Passed => json!([["path", { "d": "M3.5 8.5l3 3 6-7" }]]),
        CheckState::Skipped => json!([["path", { "d": "M4.5 8h7" }]]),
    }
}

/// How a check, job or step stands: ✕, ●, ○, ✓ or –, in its colour. A glyph in the text's own weight is a thin ring and
/// cross in the native app, so the marks are drawn. Their box is 19 high, a line of 13px text, so a mark at the top of a
/// row is centred on its first line.
pub fn state_mark(state: CheckState) -> Node {
    Node::new("Svg")
        .prop("viewBox", "-1.5 -1.5 19 19")
        .prop("size", 19)
        .prop("tone", state_tone(state))
        .prop("label", state_word(state))
        .prop("fill", "none")
        .prop("stroke", "currentColor")
        .prop("strokeWidth", "1.6")
        .prop("strokeLinecap", "round")
        .prop("strokeLinejoin", "round")
        .prop("shapes", state_shape(state))
}

/// A small outlined label, muted unless it's `required`, which is in the warning colour.
pub fn tag(label: &str, required: bool) -> Node {
    if required {
        badge(label, Some("warn")).prop("size", "sm").prop("variant", "outline")
    } else {
        Node::new("Badge").prop("size", "sm").prop("variant", "outline").child(muted(label))
    }
}

/// A column of a row, which takes a fixed `width` in points.
pub fn column(width: u32) -> Node {
    Node::new("Stack").prop("width", width)
}

/// How long ago `iso` was, or none when it isn't a time.
pub fn time_text(iso: &str) -> Option<String> {
    iso_to_ms(iso).map(|ms| ago(ms, host::now()))
}

/// A queued check that has waited this long says so, since "1 h ago" reads as when it ran.
const LONG_QUEUE_MS: f64 = 15.0 * 60_000.0;

fn queued_ago(ms: f64, now: f64) -> String {
    if now - ms > LONG_QUEUE_MS { format!("queued {}", ago(ms, now).trim_end_matches(" ago")) } else { ago(ms, now) }
}

/// Like `time_text` for a check waiting to start: "queued 1 h" once it has waited a while.
pub fn queued_text(iso: &str) -> Option<String> {
    iso_to_ms(iso).map(|ms| queued_ago(ms, host::now()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_check_queued_for_long_says_it_is_queued_and_a_fresh_one_says_how_long_ago() {
        let now = 1_800_000_000_000.0;
        assert_eq!(queued_ago(now - 3_600_000.0, now), "queued 1 h");
        assert_eq!(queued_ago(now - 20.0 * 60_000.0, now), "queued 20 min");
        assert_eq!(queued_ago(now - 5.0 * 60_000.0, now), "5 min ago");
    }
}
