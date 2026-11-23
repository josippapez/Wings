//! The goal chip over the message box while a `/goal` is active: the condition, how long it has run, how many checks missed,
//! the latest check's reason, and Clear.

use wings_plugin::{
    format::plural,
    ui::{Node, badge, button, card, muted, row, stack, text},
};

use crate::chat::{self, Chat};

/// None when no goal is active.
pub fn chip(c: &Chat) -> Option<Node> {
    let goal = c.session.goal.as_ref()?;
    let head = row()
        .prop("gap", "s")
        .prop("align", "center")
        .child(badge("Goal", Some("info")))
        .child(text(goal.condition.as_str()).prop("size", "small").prop("truncate", true).prop("grow", true).prop("title", goal.condition.as_str()))
        // Counts up in Wings: the chat sends it once, not each second.
        .child(muted("Running").prop("size", "small").prop("since", goal.since))
        .maybe((goal.checks > 0).then(|| muted(format!("· {} not met", plural(goal.checks as usize, "check"))).prop("size", "small")))
        .child(button("Clear", "ghost", false, || chat::send_text("/goal clear")).prop("size", "sm").prop("title", "Stop working toward this goal (/goal clear)"));
    let reason = goal.reason.as_deref().map(|why| muted(why).prop("size", "small").prop("clamp", true).prop("title", why));
    Some(card().prop("variant", "raised").child(stack().prop("gap", "xs").child(head).maybe(reason)).key("goal"))
}

#[cfg(test)]
mod tests {
    use wings_plugin::ui::tree_json;

    use super::*;
    use crate::goal::Goal;

    #[test]
    fn the_chip_names_the_condition_counts_up_and_has_clear() {
        chat::with(|c| {
            assert!(chip(c).is_none());
            c.session.goal = Some(Goal { condition: "the build is green".into(), since: 4000, checks: 2, reason: Some("Two tests still fail".into()), awaiting: false });
            let drawn = tree_json(chip(c).expect("a chip")).to_string();
            assert!(drawn.contains("the build is green") && drawn.contains("\"since\":4000") && drawn.contains("· 2 checks not met"), "{drawn}");
            assert!(drawn.contains("Two tests still fail") && drawn.contains("Clear"), "{drawn}");
            c.session.goal = None;
        });
    }
}
