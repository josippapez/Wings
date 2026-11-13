//! What the pane badge shows of a work item, and what the main script keeps for it. No host calls, so tests drive it.

use serde_json::{Value, json};
use std::collections::HashMap;

/// A pane's work item: its org, its id from the branch, and `${org} ${id}`.
#[derive(Clone, Debug, PartialEq)]
pub struct Ref {
    pub org: String,
    pub id: String,
    pub key: String,
}

impl Ref {
    pub fn new(org: &str, id: &str) -> Self {
        Self { org: org.to_owned(), id: id.to_owned(), key: format!("{org} {id}") }
    }
}

/// One state of a work item type, from `workItemTypeStates`.
#[derive(Clone, Debug, PartialEq)]
pub struct TypeState {
    pub name: String,
    pub category: String,
}

impl TypeState {
    /// `workItemTypeStates` as `az devops invoke` prints it.
    pub fn list(answer: &Value) -> Vec<Self> {
        answer["value"].as_array().into_iter().flatten().map(|s| Self { name: s["name"].as_str().unwrap_or_default().to_owned(), category: s["category"].as_str().unwrap_or_default().to_owned() }).collect()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct WorkItem {
    pub title: String,
    pub state: String,
    pub kind: String,
    pub category: Option<String>,
    pub states: Vec<TypeState>,
    pub sprint: String,
    pub assigned: String,
    pub parent: Option<String>,
    pub url: String,
}

/// A normalized work item, none there, or why it couldn't be read.
#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    Found(WorkItem),
    Missing,
    Error(String),
}

/// Forgets the panes that are gone: their refs, and the items no ref points at, so a closed pane's work item isn't
/// looked up again.
pub fn drop_closed(refs: &mut HashMap<String, Ref>, items: &mut HashMap<String, Item>, open: &[String]) {
    refs.retain(|pane, _| open.contains(pane));
    items.retain(|key, _| refs.values().any(|r| &r.key == key));
}

/// Process categories: Proposed, InProgress, Resolved, Completed, Removed. Review and QA states wait on someone else.
fn tone_of(item: &WorkItem) -> &'static str {
    match item.category.as_deref() {
        Some("InProgress") => {
            let state = item.state.to_lowercase();
            if ["review", "qa", "test"].iter().any(|word| state.contains(word)) { "warning" } else { "info" }
        }
        Some("Resolved") => "merged",
        Some("Completed") => "success",
        _ => "neutral",
    }
}

/// The next two states in the type's workflow order, skipping Removed.
pub fn next_states(item: &WorkItem) -> Vec<String> {
    let forward: Vec<&TypeState> = item.states.iter().filter(|s| s.category != "Removed").collect();
    let Some(at) = forward.iter().position(|s| s.name == item.state) else { return Vec::new() };
    forward.iter().skip(at + 1).take(2).map(|s| s.name.clone()).filter(|name| name.chars().count() <= 32).collect()
}

/// Every other state of the type, in workflow order, as Azure DevOps' own state menu offers them.
pub fn other_states(state: &str, states: &[TypeState]) -> Vec<String> {
    states.iter().map(|s| s.name.clone()).filter(|name| name != state).collect()
}

/// The badge of a pane, or none for one with no work item. `item` is none while it loads.
pub fn badge_for(r: &Ref, item: Option<&Item>) -> Value {
    let refresh = json!({ "id": "refresh", "label": "Refresh" });
    let label = format!("#{}", r.id);
    let found = match item {
        // Until the item loads there's no link to open.
        None => return json!({ "label": label, "tone": "neutral", "title": "Loading work item…", "subtitle": format!("Work item #{}", r.id), "actions": [refresh] }),
        Some(Item::Error(error)) => {
            return json!({
                "label": label, "tone": "neutral", "title": format!("Work item #{}", r.id),
                "rows": [{ "label": "Status", "value": error, "tone": "danger" }],
                "actions": [{ "id": "refresh", "label": "Refresh", "primary": true }],
            });
        }
        Some(Item::Missing) => return Value::Null,
        Some(Item::Found(item)) => item,
    };
    let tone = tone_of(found);
    let mut rows = vec![
        json!({ "label": "Type", "value": found.kind }),
        json!({ "label": "State", "value": found.state, "tone": tone }),
        json!({ "label": "Sprint", "value": found.sprint }),
        json!({ "label": "Assigned", "value": found.assigned }),
    ];
    if let Some(parent) = &found.parent {
        rows.push(json!({ "label": "Parent", "value": parent }));
    }
    let mut actions = vec![json!({ "id": "details", "label": "Open details", "primary": true }), json!({ "id": "open", "label": "Open in Azure DevOps" }), refresh];
    actions.extend(next_states(found).into_iter().map(|s| json!({ "id": format!("move:{s}"), "label": format!("Move to {s}") })));
    json!({ "label": format!("#{} {}", r.id, found.state), "tone": tone, "title": found.title, "subtitle": format!("{} #{}", found.kind, r.id), "rows": rows, "actions": actions })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn states(names: &[&str]) -> Vec<TypeState> {
        names.iter().map(|name| TypeState { name: (*name).into(), category: "InProgress".into() }).collect()
    }

    fn found(state: &str, category: &str, all: &[(&str, &str)]) -> WorkItem {
        WorkItem {
            title: "Login".into(),
            state: state.into(),
            kind: "Bug".into(),
            category: Some(category.into()),
            states: all.iter().map(|(name, category)| TypeState { name: (*name).into(), category: (*category).into() }).collect(),
            sprint: "Sprint 5".into(),
            assigned: "Ada Lovelace".into(),
            parent: Some("#9 Epic".into()),
            url: "https://dev.azure.com/o/P/_workitems/edit/1".into(),
        }
    }

    #[test]
    fn a_closed_panes_ref_goes_and_so_does_an_item_no_ref_points_at_so_nothing_polls_it() {
        let (a, b) = (Ref::new("https://dev.azure.com/o", "1234"), Ref::new("https://dev.azure.com/o", "5678"));
        let mut refs = HashMap::from([("p1".to_owned(), a.clone()), ("p2".to_owned(), b.clone()), ("p3".to_owned(), a.clone())]);
        let mut items = HashMap::from([(a.key.clone(), Item::Missing), (b.key.clone(), Item::Missing)]);
        let keys = |refs: &HashMap<String, Ref>| {
            let mut keys: Vec<_> = refs.keys().cloned().collect();
            keys.sort();
            keys
        };
        drop_closed(&mut refs, &mut items, &["p2".into(), "p3".into()]);
        assert_eq!(keys(&refs), ["p2", "p3"]);
        assert_eq!(items.len(), 2, "p3 still points at A");
        drop_closed(&mut refs, &mut items, &["p2".into()]);
        assert_eq!(keys(&refs), ["p2"]);
        assert_eq!(items.keys().collect::<Vec<_>>(), [&b.key]);
        drop_closed(&mut refs, &mut items, &[]);
        assert_eq!(refs.len() + items.len(), 0);
    }

    #[test]
    fn the_item_tab_offers_every_other_state_of_the_type_not_just_the_next_two() {
        let all = states(&["New", "Approved", "Under Analysis", "In Progress", "Done", "Removed"]);
        assert_eq!(other_states("New", &all), ["Approved", "Under Analysis", "In Progress", "Done", "Removed"]);
        assert_eq!(other_states("In Progress", &all), ["New", "Approved", "Under Analysis", "Done", "Removed"]);
    }

    #[test]
    fn the_badge_moves_to_the_next_two_states_skipping_removed_and_names_its_tone() {
        let item = found("Active", "InProgress", &[("New", "Proposed"), ("Active", "InProgress"), ("Resolved", "Resolved"), ("Closed", "Completed"), ("Removed", "Removed")]);
        assert_eq!(next_states(&item), ["Resolved", "Closed"]);
        let badge = badge_for(&Ref::new("https://dev.azure.com/o", "1"), Some(&Item::Found(item)));
        assert_eq!((badge["label"].as_str(), badge["tone"].as_str(), badge["subtitle"].as_str()), (Some("#1 Active"), Some("info"), Some("Bug #1")));
        let ids: Vec<_> = badge["actions"].as_array().unwrap().iter().map(|a| a["id"].as_str().unwrap()).collect();
        assert_eq!(ids, ["details", "open", "refresh", "move:Resolved", "move:Closed"]);
        assert_eq!(badge["rows"].as_array().unwrap().len(), 5);
        assert_eq!(tone_of(&found("Ready for QA", "InProgress", &[])), "warning");
        assert_eq!(tone_of(&found("Closed", "Completed", &[])), "success");
    }

    #[test]
    fn the_badge_while_loading_and_when_it_failed() {
        let r = Ref::new("https://dev.azure.com/o", "7");
        assert_eq!(badge_for(&r, None)["title"], "Loading work item…");
        let failed = badge_for(&r, Some(&Item::Error("boom".into())));
        assert_eq!((failed["rows"][0]["value"].as_str(), failed["actions"][0]["primary"].as_bool()), (Some("boom"), Some(true)));
        assert!(badge_for(&r, Some(&Item::Missing)).is_null());
    }
}
