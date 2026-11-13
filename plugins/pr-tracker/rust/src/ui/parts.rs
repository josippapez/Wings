//! Small pieces the PR pages share: a check's mark, a tag, and how long ago a time was.

use crate::{checklist::iso_to_ms, types::CheckState};
use wings_plugin::{
    format::ago,
    host,
    ui::{Node, badge, muted},
};

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

/// The text at its large size, as a page's title.
pub fn large_text(text: impl Into<String>) -> Node {
    wings_plugin::ui::text(text).prop("size", "large")
}

/// How long ago `iso` was, or none when it isn't a time.
pub fn time_text(iso: &str) -> Option<String> {
    iso_to_ms(iso).map(|ms| ago(ms, host::now()))
}
