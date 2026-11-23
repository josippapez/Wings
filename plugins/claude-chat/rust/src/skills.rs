//! What `/skills` shows: the session's skills with what each is for and how Claude and the `/` menu see it, from Claude
//! Code's `get_skills_dialog` read, and a change of that state, which Claude Code's own `claude edit-skill-overrides
//! --json` writes to the project's local settings (`skillOverrides`), as its terminal menu does. A skill a plugin, its own
//! author or your organization sets can't be changed, and Claude Code says so (`locked_by`).

use std::{cell::RefCell, rc::Rc};

use serde_json::{Value, json};
use wings_plugin::host;

use crate::session::request;

/// The id of the read that fills the dialog.
pub const ID: &str = "skills-dialog";

/// The states Claude Code's menu cycles a skill through, as (its word, what the chat says).
pub const STATES: [(&str, &str); 4] = [("on", "On"), ("name-only", "Name only"), ("user-invocable-only", "You only"), ("off", "Off")];

#[derive(Debug, Clone, PartialEq)]
pub struct Skill {
    pub name: String,
    pub description: String,
    /// Where it comes from: `user`, `project`, `plugin`, `built-in`...
    pub source: String,
    /// What its listing takes in the system prompt.
    pub tokens: u64,
    pub state: String,
    /// Why it can't be changed: `author`, `plugin`, `policy`, `flag` or `reserved-name`.
    pub locked: Option<String>,
    /// The other names its state may be kept under, which an edit sends back.
    handles: Value,
}

#[derive(Debug, Default)]
pub struct Skills {
    pub open: bool,
    /// The list, None until Claude Code answers, or why it couldn't.
    pub list: Option<Result<Vec<Skill>, String>>,
    pub query: String,
    /// The skill being changed, and how the last change went.
    pub saving: Option<String>,
    pub outcome: Option<(&'static str, String)>,
}

impl Skills {
    /// Opens the dialog, and gives the read that fills it.
    pub fn open(&mut self) -> String {
        self.open = true;
        self.list = None;
        self.outcome = None;
        self.reread()
    }

    pub fn reread(&self) -> String {
        request(ID, json!({ "subtype": "get_skills_dialog" }))
    }

    /// Takes in Claude Code's answer to the read. False when it isn't that.
    pub fn answered(&mut self, id: &str, response: &Value) -> bool {
        if id != ID {
            return false;
        }
        self.list = Some(match response["subtype"] == "success" {
            true => Ok(of(&response["response"]["skills"])),
            false => Err(response["error"].as_str().unwrap_or("Claude Code didn't answer.").to_owned()),
        });
        true
    }

    /// The edit that sets the skill `name` to `state`, which the dialog keeps from doing twice at once. None for a skill that
    /// is locked, a state Claude Code has no word for, or while another change is being made.
    pub fn change(&mut self, name: &str, state: &str) -> Option<(String, Value)> {
        if self.saving.is_some() || !STATES.iter().any(|(known, _)| *known == state) {
            return None;
        }
        let skill = self.list.as_ref()?.as_ref().ok()?.iter().find(|skill| skill.name == name && skill.locked.is_none())?;
        let mut edit = json!({ "name": skill.name, "state": state });
        if skill.handles.is_object() && !skill.handles.as_object().is_some_and(|handles| handles.is_empty()) {
            edit["handles"] = skill.handles.clone();
        }
        self.saving = Some(skill.name.clone());
        self.outcome = None;
        Some((skill.name.clone(), edit))
    }

    /// Takes in how Claude Code made the change.
    pub fn saved(&mut self, name: &str, state: &str, outcome: Result<(), String>) {
        self.saving = None;
        self.outcome = Some(match outcome {
            Ok(()) => {
                let label = STATES.iter().find(|(known, _)| *known == state).map_or(state, |(_, label)| label);
                if let Some(Ok(list)) = &mut self.list {
                    if let Some(skill) = list.iter_mut().find(|skill| skill.name == name) {
                        skill.state = state.to_owned();
                    }
                }
                ("success", format!("{name} is now {label}. It applies from your next message, and is kept in this project's local settings."))
            }
            Err(why) => ("danger", crate::setup::refused(&why)),
        });
    }

    /// The skills that match what is typed in the search, by name, description or source.
    pub fn found(&self) -> Vec<&Skill> {
        let wanted = self.query.trim().to_lowercase();
        let list = self.list.as_ref().and_then(|list| list.as_ref().ok()).map(Vec::as_slice).unwrap_or_default();
        list.iter().filter(|skill| wanted.is_empty() || [&skill.name, &skill.description, &skill.source].iter().any(|text| text.to_lowercase().contains(&wanted))).collect()
    }
}

fn of(rows: &Value) -> Vec<Skill> {
    rows.as_array()
        .into_iter()
        .flatten()
        .filter_map(|row| {
            Some(Skill {
                name: row["name"].as_str()?.to_owned(),
                description: row["description"].as_str().unwrap_or_default().to_owned(),
                source: row["source"].as_str().unwrap_or_default().to_owned(),
                tokens: row["tokens"].as_u64().unwrap_or(0),
                state: row["state"].as_str().unwrap_or("on").to_owned(),
                locked: row["locked_by"].as_str().map(str::to_owned),
                handles: row["handles"].clone(),
            })
        })
        .collect()
}

/// Makes the edit with Claude Code's own command, under the chat's account and in its folder, where the project's local
/// settings are. It runs only from the click that picked the state.
pub async fn run(account: &host::Account, cwd: &str, edit: &Value) -> Result<(), String> {
    let printed = Rc::new(RefCell::new(Value::Null));
    let kept = printed.clone();
    let process = host::spawn_as(account, "claude", &["edit-skill-overrides", "--json"], cwd, &edit.to_string(), move |line| *kept.borrow_mut() = line.clone());
    // It reads stdin to its end before it does anything.
    process.input().stop();
    let exit = process.await;
    crate::setup::outcome(&exit, &printed.borrow()).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Rows as Claude Code 2.1.330 answers `get_skills_dialog`.
    fn answer() -> Value {
        json!({ "subtype": "success", "request_id": ID, "response": { "skills": [
            { "name": "dev-core:agent-browser", "display_name": "dev-core:agent-browser", "description": "Fetch facts from the live web", "source": "plugin", "tokens": 120, "state": "on", "locked_by": "plugin", "advertised": true, "handles": { "unqualified_name": "agent-browser" } },
            { "name": "deploy", "display_name": "deploy", "description": "Ship it", "source": "project", "tokens": 40, "state": "on", "advertised": true, "handles": { "unqualified_name": "deploy" } },
            { "name": "anthropic-skills:docx", "description": "Word files", "source": "claude.ai sync", "tokens": 90, "state": "user-invocable-only", "advertised": true, "handles": {} },
        ] } })
    }

    #[test]
    fn the_read_lists_each_skill_with_its_state_and_what_locks_it() {
        let mut skills = Skills::default();
        assert!(skills.open().contains("get_skills_dialog"));
        assert!(!skills.answered("other", &answer()));
        assert!(skills.answered(ID, &answer()));
        let list = skills.list.as_ref().unwrap().as_ref().unwrap();
        assert_eq!(list.len(), 3);
        assert_eq!((list[0].locked.as_deref(), list[1].locked.as_deref(), list[2].state.as_str()), (Some("plugin"), None, "user-invocable-only"));
        skills.query = "WORD".into();
        assert_eq!(skills.found().iter().map(|skill| skill.name.as_str()).collect::<Vec<_>>(), ["anthropic-skills:docx"]);
    }

    #[test]
    fn a_state_is_written_by_the_edit_command_as_it_takes_it_and_a_locked_skill_has_none() {
        let mut skills = Skills::default();
        skills.answered(ID, &answer());
        assert_eq!(skills.change("dev-core:agent-browser", "off"), None, "a plugin's skill can't be changed");
        assert_eq!(skills.change("deploy", "sometimes"), None, "Claude Code has no such state");
        let (name, edit) = skills.change("deploy", "name-only").unwrap();
        assert_eq!(name, "deploy");
        assert_eq!(edit, json!({ "name": "deploy", "state": "name-only", "handles": { "unqualified_name": "deploy" } }));
        assert_eq!(skills.change("deploy", "off"), None, "one change at a time");
        skills.saved("deploy", "name-only", Ok(()));
        let list = skills.list.as_ref().unwrap().as_ref().unwrap();
        assert_eq!(list[1].state, "name-only");
        assert!(skills.outcome.as_ref().unwrap().1.contains("Name only"));
        // A skill with no other names sends none.
        assert_eq!(skills.change("anthropic-skills:docx", "on").unwrap().1, json!({ "name": "anthropic-skills:docx", "state": "on" }));
        skills.saved("anthropic-skills:docx", "on", Err("Not a skill state edit (state: Invalid option)".into()));
        assert_eq!(skills.outcome.as_ref().unwrap().0, "danger");
    }
}
