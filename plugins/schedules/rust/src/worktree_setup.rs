//! A project's worktree setup as the form edits it. The setting lives on the project in Wings, shared with Worktree setup
//! in the sidebar and Claude's worktree_setup tool; Schedules keeps no copy, it only reads it and writes the changes back.

use serde_json::{Value, json};

/// What `worktreeSetup` answers.
#[derive(Clone, Debug, PartialEq)]
pub struct WorktreeSetup {
    /// The project's own patterns, or none for the default list.
    pub include: Option<Vec<String>>,
    pub defaults: Vec<String>,
    /// The repo has a `.worktreeinclude`, which is used instead of `include`.
    pub has_include_file: bool,
    pub setup: Option<String>,
}

impl WorktreeSetup {
    pub fn from_json(value: &Value) -> Self {
        let list = |v: &Value| v.as_array().map(|list| list.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect::<Vec<_>>());
        WorktreeSetup { include: list(&value["include"]), defaults: list(&value["defaults"]).unwrap_or_default(), has_include_file: value["hasIncludeFile"] == true, setup: value["setup"].as_str().map(str::to_owned) }
    }
}

/// The fields to change; one left out stays as it is, `Some(None)` resets the list or clears the line.
#[derive(Debug, PartialEq, Default)]
pub struct WorktreeChange {
    pub include: Option<Option<Vec<String>>>,
    pub setup: Option<Option<String>>,
}

impl WorktreeChange {
    pub fn to_json(&self) -> Value {
        let mut out = json!({});
        if let Some(include) = &self.include {
            out["include"] = json!(include);
        }
        if let Some(setup) = &self.setup {
            out["setup"] = json!(setup);
        }
        out
    }
}

fn lines(text: &str) -> Vec<String> {
    text.split('\n').map(str::trim).filter(|line| !line.is_empty()).map(str::to_owned).collect()
}

/// What the textarea starts with: the project's list, or the defaults when it has none.
pub fn include_text(s: &WorktreeSetup) -> String {
    s.include.as_ref().unwrap_or(&s.defaults).join("\n")
}

/// What saving changes, or none when nothing: the default list given back is saved as none, and a field nobody touched is not saved.
pub fn worktree_change(loaded: &WorktreeSetup, include_field: &str, setup_field: &str) -> Option<WorktreeChange> {
    let mut change = WorktreeChange::default();
    let now = lines(include_field);
    if now != *loaded.include.as_ref().unwrap_or(&loaded.defaults) {
        change.include = Some(if now == loaded.defaults { None } else { Some(now) });
    }
    let setup = setup_field.trim();
    if setup != loaded.setup.as_deref().unwrap_or_default() {
        change.setup = Some(Some(setup.to_owned()).filter(|setup| !setup.is_empty()));
    }
    (change != WorktreeChange::default()).then_some(change)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> WorktreeSetup {
        WorktreeSetup { include: None, defaults: vec![".env".into(), ".mcp.json".into()], has_include_file: false, setup: None }
    }

    fn list(items: &[&str]) -> Option<Vec<String>> {
        Some(items.iter().map(|item| (*item).to_owned()).collect())
    }

    #[test]
    fn an_untouched_form_changes_nothing_and_starts_from_the_defaults_when_the_project_has_no_list() {
        assert_eq!(include_text(&base()), ".env\n.mcp.json");
        assert_eq!(worktree_change(&base(), ".env\n.mcp.json\n", ""), None);
        assert_eq!(worktree_change(&WorktreeSetup { include: list(&["a"]), setup: Some("make".into()), ..base() }, "a", " make "), None);
    }

    #[test]
    fn a_changed_list_or_line_is_saved_and_only_that_one() {
        assert_eq!(worktree_change(&base(), ".env\n!big/", ""), Some(WorktreeChange { include: Some(list(&[".env", "!big/"])), setup: None }));
        assert_eq!(worktree_change(&base(), ".env\n.mcp.json", "pnpm install"), Some(WorktreeChange { include: None, setup: Some(Some("pnpm install".into())) }));
    }

    #[test]
    fn the_default_list_given_back_is_saved_as_null_and_an_emptied_setup_clears_it() {
        let saved = WorktreeSetup { include: list(&["a"]), setup: Some("make".into()), ..base() };
        let change = worktree_change(&saved, ".env\n.mcp.json", "").unwrap();
        assert_eq!(change, WorktreeChange { include: Some(None), setup: Some(None) });
        assert_eq!(change.to_json(), json!({ "include": null, "setup": null }));
    }
}
