//! The dialog `/skills` opens over the chat: the session's skills with what each is for, and for the ones Claude Code lets
//! you change, how Claude and the `/` menu see each.

use wings_plugin::{
    format::tokens,
    host, task,
    ui::{Entry, Node, badge, dialog as panel, empty, entry, loading, muted, notice, search_field, select, stack, text},
};

use crate::{
    chat::{self, Chat},
    plugins_view::Budget,
    skills::{STATES, Skill},
};

const WIDTH: u32 = 640;

pub fn dialog(c: &Chat) -> Option<Node> {
    let skills = &c.session.skills;
    if !skills.open {
        return None;
    }
    let body = match &skills.list {
        None => loading(),
        Some(Err(why)) => notice("danger", why.as_str()),
        Some(Ok(list)) if list.is_empty() => empty("No skills", "Claude Code has no skills here."),
        Some(Ok(_)) => {
            let mut budget = Budget::default();
            let rows: Vec<Node> = skills.found().into_iter().filter_map(|skill| budget.take(row(skill, skills.saving.is_some()))).collect();
            let none = rows.is_empty().then(|| muted(format!("Nothing matches “{}”.", skills.query.trim())).prop("size", "small"));
            let more = (budget.left > 0).then(|| muted(format!("{} more. Search to find them.", budget.left)).prop("size", "small"));
            stack().prop("gap", "s").children(rows).maybe(none).maybe(more)
        }
    };
    let outcome = skills.outcome.as_ref().map(|(tone, said)| notice(tone, said.as_str()));
    Some(
        panel("Skills", WIDTH, chat::close_skills)
            .maybe(outcome)
            .child(search_field("Search skills", &skills.query, chat::search_skills))
            .child(body)
            .child(muted("On lists a skill with its description. Name only lists just its name. You only keeps it from Claude, and Off from you too. A change is kept in this project's local settings.").prop("size", "small")),
    )
}

/// A skill: its name over what it is for, where it is from and what it costs, and its state to change.
fn row(skill: &Skill, busy: bool) -> Node {
    let trailing = match &skill.locked {
        Some(by) => badge(lock_label(by), None).prop("title", format!("Set by {}, so it can't be changed here", lock_name(by))),
        None => {
            let options: Vec<(String, String)> = STATES.iter().map(|(value, label)| ((*value).to_owned(), (*label).to_owned())).collect();
            let name = skill.name.clone();
            select(&format!("How Claude sees {}", skill.name), &options, &skill.state, move |state| pick(&name, state)).prop("variant", "ghost").prop("disabled", busy)
        }
    };
    let detail = format!("{}{}", if skill.source.is_empty() { String::new() } else { format!("{} · ", skill.source) }, if skill.tokens > 0 { format!("{} tokens · {}", tokens(skill.tokens as f64), skill.description) } else { skill.description.clone() });
    entry(Entry {
        lead: None,
        title: text(skill.name.as_str()).prop("size", "small").prop("clamp", true).prop("title", skill.name.as_str()),
        detail: Some(muted(detail.as_str()).prop("size", "small").prop("clamp", true).prop("title", skill.description.as_str())),
        trailing: Some(trailing),
    })
    .key(skill.name.as_str())
}

fn lock_name(by: &str) -> &str {
    match by {
        "author" => "the skill's own settings",
        "plugin" => "its plugin",
        "policy" => "your organization",
        "flag" => "the command line",
        _ => "its name",
    }
}

fn lock_label(by: &str) -> &str {
    match by {
        "author" => "Set by skill author",
        "plugin" => "Set by plugin",
        "policy" => "Set by your organization",
        "flag" => "Set by command line",
        "reserved-name" => "Set by reserved name",
        _ => "Set by skill name",
    }
}

/// Makes the change you picked, from that pick and nothing else.
fn pick(name: &str, state: &str) {
    let Some((edit, account, cwd)) = chat::with(|c| Some((c.session.skills.change(name, state)?, chat::account(c, c.session.id.as_deref()), c.cwd.clone()))) else {
        return;
    };
    let (name, state) = (edit.0, state.to_owned());
    task::spawn(async move {
        let outcome = crate::skills::run(&account, &cwd, &edit.1).await;
        let saved = outcome.is_ok();
        chat::with(|c| c.session.skills.saved(&name, &state, outcome));
        if saved {
            // Claude Code's list, as it reads it now.
            host::sleep(1_500).await;
            chat::write(|c, _| Some(c.session.skills.reread()));
        }
    });
}

#[cfg(test)]
mod tests {
    use serde_json::{json, Value};
    use wings_plugin::ui::tree_json;

    use super::*;
    use crate::skills::{Skills, ID};

    fn locked_skills() -> Skills {
        let mut skills = Skills::default();
        skills.answered(
            ID,
            &json!({
                "subtype": "success",
                "response": { "skills": [
                    { "name": "plugin-skill", "description": "From a plugin", "state": "on", "locked_by": "plugin" },
                    { "name": "policy-skill", "description": "Set by policy", "state": "on", "locked_by": "policy" },
                    { "name": "editable-skill", "description": "Can change", "state": "on" }
                ] }
            }),
        );
        skills
    }

    fn has_visible_text(tree: &Value, expected: &str) -> bool {
        (tree["type"] == "#text" && tree["text"] == expected)
            || tree.get("children").and_then(Value::as_array).is_some_and(|children| children.iter().any(|child| has_visible_text(child, expected)))
    }

    #[test]
    fn polish_locked_skill_plugin_reason_is_visible() {
        let skills = locked_skills();
        let skill = &skills.list.as_ref().unwrap().as_ref().unwrap()[0];
        let tree = tree_json(row(skill, false));
        assert!(has_visible_text(&tree, "Set by plugin"), "{tree}");
    }

    #[test]
    fn polish_locked_skill_policy_reason_is_visible() {
        let skills = locked_skills();
        let skill = &skills.list.as_ref().unwrap().as_ref().unwrap()[1];
        let tree = tree_json(row(skill, false));
        assert!(has_visible_text(&tree, "Set by your organization"), "{tree}");
    }
}
