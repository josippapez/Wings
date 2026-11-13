//! The pure half of the per-project "Work items" section and the Sprint block: which iterations are running now, the
//! query for the work items assigned to you in them, and the rows. No host calls, so tests drive it.

use crate::{branch::branch_has_id, item::{assignee_of, item_tab_args}};
use serde_json::{Value, json};
use std::collections::HashMap;
use wings_plugin::format::{parse_time, truncate};

pub const SECTION_ID: &str = "work-items";
/// The most rows Wings takes for a section.
pub const ROWS_CAP: usize = 50;

const DAY_MS: f64 = 86_400_000.0;

/// The paths, as work items carry them in `System.IterationPath` (`Project\Sprint 5`), of the iterations running at
/// `now`: dated, started, and not past the end of their last day. A running iteration that has a running child (a
/// release around a sprint) is left out, since items belong to the sprint. `root` is one node of
/// `classificationNodes/iterations`, as `az devops invoke` prints it.
pub fn current_iterations(root: &Value, now: f64) -> Vec<String> {
    fn walk(node: &Value, now: f64, running: &mut Vec<String>) {
        let time = |name: &str| parse_time(node["attributes"][name].as_str().unwrap_or_default());
        if let (Some(path), Some(start), Some(finish)) = (node["path"].as_str(), time("startDate"), time("finishDate"))
            && start <= now
            && now < finish + DAY_MS
        {
            running.push(path.to_owned());
        }
        for child in node["children"].as_array().into_iter().flatten() {
            walk(child, now, running);
        }
    }
    let mut running = Vec::new();
    walk(root, now, &mut running);
    running.iter().filter(|path| !running.iter().any(|other| other != *path && other.starts_with(&format!("{path}\\")))).map(|path| iteration_path(path)).collect()
}

/// `\Project\Iteration\Sprint 5` (a node's path) becomes `Project\Sprint 5` (a work item's).
pub fn iteration_path(node_path: &str) -> String {
    let mut parts: Vec<&str> = node_path.split('\\').filter(|part| !part.is_empty()).collect();
    if parts.get(1) == Some(&"Iteration") {
        parts.remove(1);
    }
    parts.join("\\")
}

fn quote(text: &str) -> String {
    text.replace('\'', "''")
}

fn path_list(paths: &[String]) -> String {
    paths.iter().map(|path| format!("'{}'", quote(path))).collect::<Vec<_>>().join(", ")
}

/// WIQL for what's assigned to you in `paths`, newest change first.
pub fn assigned_query(project: &str, paths: &[String]) -> String {
    format!(
        "SELECT [System.Id],[System.Title],[System.State],[System.WorkItemType] FROM WorkItems WHERE [System.TeamProject] = '{}' AND [System.IterationPath] IN ({}) AND [System.AssignedTo] = @Me AND [System.State] <> 'Removed' ORDER BY [System.ChangedDate] DESC",
        quote(project),
        path_list(paths)
    )
}

/// WIQL for every work item in `paths`, newest change first: the sprint block's "whole sprint".
pub fn sprint_query(project: &str, paths: &[String]) -> String {
    format!(
        "SELECT [System.Id],[System.Title],[System.State],[System.WorkItemType],[System.AssignedTo] FROM WorkItems WHERE [System.TeamProject] = '{}' AND [System.IterationPath] IN ({}) AND [System.State] <> 'Removed' ORDER BY [System.ChangedDate] DESC",
        quote(project),
        path_list(paths)
    )
}

/// A state's tone from its name: the section has no per-type workflow, and the names differ by process.
pub fn state_tone(state: &str) -> &'static str {
    let lower = state.to_lowercase();
    let has = |names: &[&str]| names.iter().any(|name| lower.contains(name));
    if ["done", "closed", "completed", "complete"].contains(&lower.as_str()) {
        "success"
    } else if has(&["resolved"]) {
        "merged"
    } else if has(&["review", "qa", "test"]) {
        "warning"
    } else if has(&["active", "progress", "doing", "committed", "develop"]) {
        "info"
    } else {
        "neutral"
    }
}

// ---------- which items a repo's section shows ----------

/// Per work item type (lower case), the category of each state (lower case): Proposed, InProgress, Resolved, Completed,
/// Removed.
pub type StateCategories = HashMap<String, HashMap<String, String>>;

/// `workItemTypes` as `az devops invoke` prints it, cut by `--query` to the names and categories.
pub fn state_categories(types: &Value) -> StateCategories {
    let mut out = StateCategories::new();
    for kind in types.as_array().into_iter().flatten() {
        let Some(name) = kind["name"].as_str().filter(|name| !name.is_empty()) else { continue };
        let states = kind["states"].as_array().into_iter().flatten().filter_map(|state| {
            let (name, category) = (state["name"].as_str().filter(|n| !n.is_empty())?, state["category"].as_str().filter(|c| !c.is_empty())?);
            Some((name.to_lowercase(), category.to_owned()))
        });
        out.insert(name.to_lowercase(), states.collect());
    }
    out
}

/// A finished item is in a Completed or Removed state category. When the category isn't known (the lookup failed, or a
/// custom type), the names processes give their last states decide.
pub fn is_finished(kind: &str, state: &str, categories: Option<&StateCategories>) -> bool {
    match categories.and_then(|categories| categories.get(&kind.to_lowercase())).and_then(|states| states.get(&state.to_lowercase())) {
        Some(category) => category == "Completed" || category == "Removed",
        None => ["done", "closed", "completed", "removed"].contains(&state.to_lowercase().as_str()),
    }
}

/// What a work item's relations say about the git repositories it touches: the repository ids in its branch, commit and
/// pull request links (`vstfs:///Git/<kind>/<project id>%2F<repository id>%2F...`), lower case.
pub fn repo_ids_of(relations: &Value) -> Vec<String> {
    let mut ids: Vec<String> = Vec::new();
    for relation in relations.as_array().into_iter().flatten() {
        if relation["rel"] != "ArtifactLink" {
            continue;
        }
        let url = relation["url"].as_str().unwrap_or_default().to_ascii_lowercase();
        let Some(rest) = ["vstfs:///git/ref/", "vstfs:///git/commit/", "vstfs:///git/pullrequestid/"].iter().find_map(|prefix| url.strip_prefix(prefix)) else { continue };
        let segment = rest.split('/').next().unwrap_or_default();
        let Some(first) = segment.get(1..).and_then(|tail| tail.find("%2f")).map(|at| at + 1) else { continue };
        let after = &segment[first + 3..];
        if let Some(end) = after.get(1..).and_then(|tail| tail.find("%2f")).map(|at| at + 1) {
            let id = after[..end].to_owned();
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
    }
    ids
}

/// Work items read with their relations, as repository ids by work item id. An item that couldn't be read is left out.
pub fn repo_links_of(items: &[Option<Value>]) -> HashMap<u64, Vec<String>> {
    items.iter().flatten().filter_map(|item| Some((item["id"].as_u64()?, repo_ids_of(&item["relations"])))).collect()
}

pub struct RepoRef {
    /// Its Azure repository id, when it could be read.
    pub id: Option<String>,
    /// Local and remote branch names.
    pub branches: Vec<String>,
}

pub const NOT_LINKED: &str = "Not linked to a repo";

#[derive(Clone, Debug)]
pub struct QueryRow {
    pub id: u64,
    pub fields: Value,
}

/// The rows `az boards query` printed.
pub fn query_rows(answer: &Value) -> Vec<QueryRow> {
    answer.as_array().into_iter().flatten().filter_map(|row| Some(QueryRow { id: row["id"].as_u64()?, fields: row["fields"].clone() })).collect()
}

fn field<'a>(row: &'a QueryRow, name: &str) -> Option<&'a str> {
    row.fields[name].as_str().filter(|text| !text.is_empty())
}

#[derive(Clone, Debug, PartialEq)]
pub struct SectionRow {
    pub id: String,
    pub label: String,
    pub detail: String,
    pub badge: Option<(String, &'static str)>,
    pub args: Value,
}

impl SectionRow {
    /// The row as `wings.setProjectSection` takes it: it opens the item tab for its work item.
    pub fn json(&self) -> Value {
        let mut row = json!({ "id": self.id, "label": self.label, "detail": self.detail, "open": { "view": "item", "args": self.args } });
        if let Some((text, tone)) = &self.badge {
            row["badge"] = json!({ "text": text, "tone": tone });
        }
        row
    }
}

/// The rows of a section: each opens the item tab for its work item.
pub fn section_rows(org: &str, rows: &[QueryRow]) -> Vec<SectionRow> {
    rows.iter()
        .take(ROWS_CAP)
        .map(|row| {
            let state = field(row, "System.State").unwrap_or_default();
            SectionRow {
                id: row.id.to_string(),
                label: truncate(field(row, "System.Title").map_or_else(|| format!("#{}", row.id), str::to_owned).as_str(), 120),
                detail: format!("{} #{}", row.fields["System.WorkItemType"].as_str().unwrap_or("Work item"), row.id),
                badge: (!state.is_empty()).then(|| (truncate(state, 24), state_tone(state))),
                args: item_tab_args(row.id, org),
            }
        })
        .collect()
}

/// The rows of one repository's section, from the team project's unfinished items. An item shows when it links to the
/// repository (a branch, commit or pull request there) or a branch of it has the item's id, first. An item linked to no
/// repository at all follows, marked "Not linked to a repo", under every repository of the team project. An item linked
/// to other repositories only isn't shown.
pub fn repo_rows(org: &str, rows: &[QueryRow], links: &HashMap<u64, Vec<String>>, repo: &RepoRef) -> Vec<SectionRow> {
    let (mut mine, mut loose) = (Vec::new(), Vec::new());
    for row in rows {
        let repos = links.get(&row.id).map_or(&[][..], Vec::as_slice);
        if repo.id.as_ref().is_some_and(|id| repos.contains(&id.to_lowercase())) || repo.branches.iter().any(|branch| branch_has_id(branch, row.id)) {
            mine.push(row.clone());
        } else if repos.is_empty() {
            loose.push(row.clone());
        }
    }
    let shown = mine.len();
    let all: Vec<QueryRow> = mine.into_iter().chain(loose).collect();
    let mut out = section_rows(org, &all);
    for row in out.iter_mut().skip(shown) {
        row.detail = format!("{} · {NOT_LINKED}", row.detail);
    }
    out
}

// ---------- the Sprint block ----------

/// The most work items a block shows.
pub const BLOCK_ITEMS_CAP: usize = 100;

/// Block state from `host::block_state`, which is any JSON the page once saved: `mine`, what's assigned to you, is the
/// default, and false is the whole sprint.
pub fn clean_block_state(raw: &Value) -> bool {
    raw["mine"] != false
}

#[derive(Clone, Debug, PartialEq)]
pub struct BlockItem {
    pub id: u64,
    pub title: String,
    pub kind: String,
    pub state: String,
    pub assignee: Option<String>,
}

pub fn block_items(rows: &[QueryRow]) -> Vec<BlockItem> {
    rows.iter()
        .take(BLOCK_ITEMS_CAP)
        .map(|row| BlockItem {
            id: row.id,
            title: truncate(&field(row, "System.Title").map_or_else(|| format!("#{}", row.id), str::to_owned), 120),
            kind: truncate(row.fields["System.WorkItemType"].as_str().unwrap_or("Work item"), 24),
            state: truncate(field(row, "System.State").unwrap_or("No state"), 24),
            assignee: assignee_of(row).map(|name| truncate(&name, 40)),
        })
        .collect()
}

pub struct StateColumn {
    pub state: String,
    pub items: Vec<BlockItem>,
}

const TONE_ORDER: [&str; 6] = ["neutral", "info", "warning", "merged", "success", "danger"];

/// One column per state, in the order work moves through them (not started, active, review, resolved, done), each
/// keeping the order the items came in.
pub fn group_by_state(items: &[BlockItem]) -> Vec<StateColumn> {
    let mut columns: Vec<StateColumn> = Vec::new();
    for item in items {
        match columns.iter_mut().find(|column| column.state == item.state) {
            Some(column) => column.items.push(item.clone()),
            None => columns.push(StateColumn { state: item.state.clone(), items: vec![item.clone()] }),
        }
    }
    columns.sort_by_key(|column| TONE_ORDER.iter().position(|tone| *tone == state_tone(&column.state)));
    columns
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: &str = "2026-10-13T10:00:00Z";

    fn now() -> f64 {
        parse_time(NOW).unwrap()
    }

    fn node(name: &str, dates: Option<(&str, &str)>, children: Vec<Value>, parent: &str) -> Value {
        let mut node = json!({ "name": name, "path": format!("{parent}\\{name}"), "children": children });
        if let Some((start, finish)) = dates {
            node["attributes"] = json!({ "startDate": start, "finishDate": finish });
        }
        node
    }

    const PROJ: &str = "\\Proj\\Iteration";

    #[test]
    fn iteration_path_drops_the_iteration_segment() {
        assert_eq!(iteration_path("\\Proj\\Iteration\\Sprint 5"), "Proj\\Sprint 5");
        assert_eq!(iteration_path("\\Proj\\Iteration"), "Proj");
    }

    #[test]
    fn current_iterations_are_running_by_date_with_the_last_day_included_and_undated_skipped() {
        let root = json!({ "path": PROJ, "children": [
            node("Sprint 4", Some(("2026-09-15T00:00:00Z", "2026-09-28T00:00:00Z")), vec![], PROJ),
            node("Sprint 5", Some(("2026-10-12T00:00:00Z", "2026-10-13T00:00:00Z")), vec![], PROJ),
            node("Backlog", None, vec![], PROJ),
        ] });
        assert_eq!(current_iterations(&root, now()), ["Proj\\Sprint 5"]);
        assert!(current_iterations(&root, parse_time("2026-10-14T00:00:01Z").unwrap()).is_empty());
        assert!(current_iterations(&Value::Null, now()).is_empty());
    }

    #[test]
    fn a_running_parent_with_a_running_child_gives_way_to_the_child() {
        let release = "\\Proj\\Iteration\\Release 2";
        let sprint = node("Sprint 5", Some(("2026-10-12T00:00:00Z", "2026-10-26T00:00:00Z")), vec![], release);
        let root = json!({ "path": PROJ, "children": [node("Release 2", Some(("2026-09-01T00:00:00Z", "2026-12-01T00:00:00Z")), vec![sprint], PROJ)] });
        assert_eq!(current_iterations(&root, now()), ["Proj\\Release 2\\Sprint 5"]);
    }

    #[test]
    fn the_queries_quote_names_and_the_sprint_one_has_no_assignee_filter_but_asks_for_the_assignee() {
        let q = assigned_query("O'Neil", &["O'Neil\\Sprint 1".into()]);
        assert!(q.contains("[System.TeamProject] = 'O''Neil'"));
        assert!(q.contains("IN ('O''Neil\\Sprint 1')"));
        assert!(q.contains("@Me"));
        let q = sprint_query("O'Neil", &["O'Neil\\Sprint 1".into(), "O'Neil\\Sprint 2".into()]);
        assert!(!q.contains("@Me"));
        assert!(q.contains("[System.AssignedTo] FROM"));
        assert!(q.contains("IN ('O''Neil\\Sprint 1', 'O''Neil\\Sprint 2')"));
    }

    fn row(id: u64, state: &str, kind: &str) -> QueryRow {
        QueryRow { id, fields: json!({ "System.Title": format!("Item {id}"), "System.State": state, "System.WorkItemType": kind }) }
    }

    #[test]
    fn section_rows_are_at_most_50_with_a_state_badge_and_open_the_item_view() {
        let rows: Vec<QueryRow> = (1..=60).map(|i| QueryRow { id: i, fields: json!({ "System.Title": format!("T{}", i - 1), "System.State": "Active", "System.WorkItemType": "Task" }) }).collect();
        let out = section_rows("https://dev.azure.com/o", &rows);
        assert_eq!(out.len(), 50);
        assert_eq!(out[0].json(), json!({ "id": "1", "label": "T0", "detail": "Task #1", "badge": { "text": "Active", "tone": "info" }, "open": { "view": "item", "args": { "id": 1, "org": "https://dev.azure.com/o" } } }));
    }

    // The badge, a section row and the Sprint block open one work item's tab with args that serialize the same.
    #[test]
    fn a_work_items_tab_opens_with_the_same_args_from_a_section_row_and_the_badge() {
        let org = "https://dev.azure.com/o";
        let from_section = section_rows(org, &[QueryRow { id: 1234, fields: json!({}) }])[0].args.to_string();
        assert_eq!(from_section, item_tab_args(1234, org).to_string());
    }

    #[test]
    fn state_tone_reads_the_state_names() {
        assert_eq!(["Done", "Resolved", "Ready for Peer Review/QA", "Active", "New"].map(state_tone), ["success", "merged", "warning", "info", "neutral"]);
    }

    fn types() -> Value {
        json!([
            { "name": "Task", "states": [{ "name": "To Do", "category": "Proposed" }, { "name": "Done", "category": "Completed" }, { "name": "Removed", "category": "Removed" }] },
            { "name": "Bug", "states": [{ "name": "Active", "category": "InProgress" }, { "name": "Resolved", "category": "Resolved" }, { "name": "Closed", "category": "Completed" }] },
        ])
    }

    #[test]
    fn is_finished_follows_the_states_category_completed_and_removed_are_finished_resolved_is_not() {
        let cats = state_categories(&types());
        assert!(is_finished("Task", "Done", Some(&cats)));
        assert!(is_finished("task", "removed", Some(&cats)));
        assert!(!is_finished("Task", "To Do", Some(&cats)));
        assert!(!is_finished("Bug", "Resolved", Some(&cats)));
        assert!(is_finished("Bug", "Closed", Some(&cats)));
    }

    #[test]
    fn is_finished_falls_back_to_the_states_name_without_the_categories_or_for_an_unknown_type() {
        assert!(is_finished("Task", "Done", None));
        assert!(!is_finished("Task", "Resolved", None));
        assert!(!is_finished("Task", "Active", None));
        let cats = state_categories(&types());
        assert!(is_finished("Custom", "Closed", Some(&cats)));
        assert!(!is_finished("Custom", "Doing", Some(&cats)));
        assert!(state_categories(&Value::Null).is_empty());
    }

    fn art(kind: &str, repo: &str, tail: &str) -> Value {
        json!({ "rel": "ArtifactLink", "url": format!("vstfs:///Git/{kind}/proj-guid%2F{repo}%2F{tail}") })
    }

    #[test]
    fn repo_ids_of_reads_the_repository_id_out_of_branch_commit_and_pull_request_links_and_nothing_else() {
        let relations = json!([art("Ref", "AAA", "GBfeature%2F12-x"), art("Commit", "bbb", "abc123"), art("PullRequestId", "AAA", "7"), { "rel": "ArtifactLink", "url": "vstfs:///Build/Build/5" }, { "rel": "Hyperlink", "url": "https://x" }]);
        assert_eq!(repo_ids_of(&relations), ["aaa", "bbb"]);
        assert!(repo_ids_of(&Value::Null).is_empty());
        let links = repo_links_of(&[Some(json!({ "id": 1, "relations": [art("Ref", "R1", "GBx")] })), Some(json!({ "id": 2, "relations": null })), None, Some(json!({ "id": 4 }))]);
        assert_eq!(links, HashMap::from([(1, vec!["r1".to_owned()]), (2, vec![]), (4, vec![])]));
    }

    #[test]
    fn repo_rows_put_items_linked_to_the_repo_or_with_a_branch_of_it_first_unlinked_ones_after_and_other_repos_items_are_left_out() {
        let rows: Vec<QueryRow> = (101..=105).map(|id| row(id, "Active", "Task")).collect();
        let links = HashMap::from([(101, vec!["repo-a".to_owned()]), (102, vec!["repo-b".to_owned()]), (103, vec![]), (104, vec!["repo-b".to_owned()]), (105, vec![])]);
        let org = "https://dev.azure.com/org";
        let a = repo_rows(org, &rows, &links, &RepoRef { id: Some("REPO-A".into()), branches: vec!["main".into(), "feature/104-thing".into()] });
        assert_eq!(a.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), ["101", "104", "103", "105"]);
        assert!(!a[0].detail.contains("Not linked"));
        assert!(a[2].detail.contains(NOT_LINKED));
        assert_eq!(a[3].detail, "Task #105 · Not linked to a repo");
        let b = repo_rows(org, &rows, &links, &RepoRef { id: Some("repo-b".into()), branches: vec![] });
        assert_eq!(b.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), ["102", "104", "103", "105"]);
    }

    #[test]
    fn repo_rows_without_the_repos_id_match_by_branch_only_and_keep_at_most_50_rows() {
        let rows: Vec<QueryRow> = (101..181).map(|id| row(id, "Active", "Task")).collect();
        let out = repo_rows("https://dev.azure.com/org", &rows, &HashMap::new(), &RepoRef { id: None, branches: vec!["feature/170-x".into()] });
        assert_eq!(out.len(), 50);
        assert_eq!(out[0].id, "170");
        assert!(repo_rows("https://dev.azure.com/org", &[row(101, "Active", "Task")], &HashMap::from([(101, vec!["other".to_owned()])]), &RepoRef { id: None, branches: vec![] }).is_empty());
    }

    #[test]
    fn block_items_are_capped_at_100_with_the_assignee_from_an_identity_or_a_string() {
        let item = |id: u64, who: Option<Value>| {
            let mut fields = json!({ "System.Title": format!("T{id}"), "System.State": "Active", "System.WorkItemType": "Bug" });
            if let Some(who) = who {
                fields["System.AssignedTo"] = who;
            }
            QueryRow { id, fields }
        };
        let mut rows = vec![item(1, Some(json!({ "displayName": "Ana" }))), item(2, Some(json!("Bo"))), item(3, None)];
        rows.extend((10..130).map(|id| item(id, None)));
        let out = block_items(&rows);
        assert_eq!(out.len(), 100);
        assert_eq!(out.iter().take(3).map(|i| i.assignee.clone()).collect::<Vec<_>>(), [Some("Ana".into()), Some("Bo".into()), None]);
        assert_eq!(out[2], BlockItem { id: 3, title: "T3".into(), kind: "Bug".into(), state: "Active".into(), assignee: None });
    }

    #[test]
    fn group_by_state_gives_columns_in_workflow_order_and_items_keep_their_order() {
        let it = |id: u64, state: &str| BlockItem { id, title: format!("T{id}"), kind: "Task".into(), state: state.into(), assignee: None };
        let cols = group_by_state(&[it(1, "Done"), it(2, "Active"), it(3, "New"), it(4, "Active"), it(5, "Ready for QA")]);
        assert_eq!(cols.iter().map(|c| c.state.as_str()).collect::<Vec<_>>(), ["New", "Active", "Ready for QA", "Done"]);
        assert_eq!(cols[1].items.iter().map(|i| i.id).collect::<Vec<_>>(), [2, 4]);
        assert!(group_by_state(&[]).is_empty());
    }

    #[test]
    fn the_block_state_is_yours_unless_it_says_otherwise() {
        assert!(clean_block_state(&Value::Null));
        assert!(!clean_block_state(&json!({ "mine": false })));
        assert!(clean_block_state(&json!({ "mine": "no" })));
    }
}
