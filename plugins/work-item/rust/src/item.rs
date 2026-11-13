//! The pure half of the work item tab: reading what `az rest` for a work item and the work item's updates return. No
//! host calls, so tests drive it.

use crate::{sections::QueryRow, urls::update_time};
use serde_json::{Value, json};
use std::{cmp::Ordering, collections::HashMap};
use wings_plugin::format::parse_time;

/// What the "item" view is opened with. The page learns the project from the work item.
#[derive(Clone, Debug, PartialEq)]
pub struct ItemArgs {
    pub id: u64,
    pub org: String,
}

/// The args for opening a work item's tab, built here and nowhere else: the host shows an open tab only when the args are
/// equal, so a click from anywhere has to give one work item the same ones.
pub fn item_tab_args(id: u64, org: &str) -> Value {
    json!({ "id": id, "org": org })
}

/// What `host::view_args` gave as `ItemArgs`, or none when it isn't one.
pub fn item_args(raw: &Value) -> Option<ItemArgs> {
    let id = raw["id"].as_f64().filter(|id| id.fract() == 0.0 && *id > 0.0 && *id < 9e15)?;
    let org = raw["org"].as_str().filter(|org| org.starts_with("https://"))?;
    Some(ItemArgs { id: id as u64, org: org.to_owned() })
}

#[derive(Clone, Debug, PartialEq)]
pub struct PrLink {
    pub id: u64,
    /// The repository's id: web links accept it where a name goes.
    pub repo_id: String,
}

/// The pull requests linked to a work item: `ArtifactLink`s named "Pull Request",
/// `vstfs:///Git/PullRequestId/<project>%2F<repo>%2F<id>`.
pub fn pull_request_links(relations: &Value) -> Vec<PrLink> {
    let mut out: Vec<PrLink> = Vec::new();
    for relation in relations.as_array().into_iter().flatten() {
        if relation["rel"] != "ArtifactLink" || relation["attributes"]["name"] != "Pull Request" {
            continue;
        }
        let url = relation["url"].as_str().unwrap_or_default();
        const PREFIX: &str = "vstfs:///git/pullrequestid/";
        if !url.to_ascii_lowercase().starts_with(PREFIX) {
            continue;
        }
        let rest = &url[PREFIX.len()..];
        // `<project>%2F<repo>%2F<id>`: the id is what follows the last `%2F`, and the project the first part.
        let lower_rest = rest.to_ascii_lowercase();
        let Some(last) = lower_rest.rfind("%2f") else { continue };
        let Some(first) = lower_rest[1.min(last)..last].find("%2f").map(|at| at + 1) else { continue };
        let (repo, id) = (&rest[first + 3..last], &rest[last + 3..]);
        if rest.contains('/') || repo.is_empty() || id.is_empty() || !id.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        let Ok(id) = id.parse() else { continue };
        if !out.iter().any(|link| link.id == id) {
            out.push(PrLink { repo_id: repo.to_owned(), id });
        }
    }
    out
}

#[derive(Clone, Debug, PartialEq)]
pub struct Comment {
    pub id: u64,
    pub author: String,
    pub at: String,
    /// HTML, as Azure DevOps keeps it. Never put it in a page as it is: see `html`.
    pub html: String,
}

/// Oldest first, and none for a time that isn't a date, which keeps the order it came in.
fn time_order(a: &str, b: &str) -> Ordering {
    match (parse_time(a), parse_time(b)) {
        (Some(x), Some(y)) => x.partial_cmp(&y).unwrap_or(Ordering::Equal),
        _ => Ordering::Equal,
    }
}

fn author_of(update: &Value) -> String {
    update["revisedBy"]["displayName"].as_str().filter(|name| !name.is_empty()).unwrap_or("Someone").to_owned()
}

/// The comments of a work item: each revision that set `System.History`, oldest first.
pub fn comments_of(updates: &[Value]) -> Vec<Comment> {
    let mut out: Vec<Comment> = updates
        .iter()
        .filter_map(|update| {
            let history = update["fields"]["System.History"]["newValue"].as_str().filter(|history| !history.trim().is_empty())?;
            Some(Comment { id: update["id"].as_u64().unwrap_or_default(), author: author_of(update), at: update_time(update), html: history.to_owned() })
        })
        .collect();
    out.sort_by(|a, b| time_order(&a.at, &b.at));
    out
}

// ---------- fields ----------

/// A number as JavaScript writes it: `5`, `2.5`.
pub fn js_number(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 { format!("{}", n as i64) } else { n.to_string() }
}

/// The last part of an area or iteration path: `Proj\Team\Sprint 5` is `Sprint 5`.
pub fn last_segment(path: Option<&str>) -> String {
    path.and_then(|path| path.split('\\').rfind(|part| !part.is_empty())).unwrap_or_default().to_owned()
}

/// `a; b; c` as `["a", "b", "c"]`.
pub fn tags_of(tags: Option<&str>) -> Vec<String> {
    tags.unwrap_or_default().split(';').map(str::trim).filter(|tag| !tag.is_empty()).map(str::to_owned).collect()
}

#[derive(Debug, Default, PartialEq)]
pub struct Summary {
    pub priority: Option<f64>,
    pub severity: Option<String>,
    /// Story points, effort or size, whichever the process uses.
    pub estimate: Option<(&'static str, f64)>,
    pub remaining: Option<f64>,
    pub completed: Option<f64>,
}

/// `Severity` without the number Azure puts before its name: `2 - High` is `High`.
fn severity_name(severity: &str) -> &str {
    let rest = severity.trim_start_matches(|c: char| c.is_ascii_digit());
    if rest.len() == severity.len() {
        return severity;
    }
    rest.trim_start().strip_prefix('-').map_or(severity, str::trim_start)
}

/// The numbers worth a chip: priority, severity, the estimate and the work left.
pub fn summary_of(fields: &Value) -> Summary {
    let num = |name: &str| fields[name].as_f64().filter(|n| n.is_finite());
    Summary {
        priority: num("Microsoft.VSTS.Common.Priority"),
        severity: fields["Microsoft.VSTS.Common.Severity"].as_str().filter(|s| !s.is_empty()).map(|s| severity_name(s).to_owned()),
        estimate: [("Story points", "Microsoft.VSTS.Scheduling.StoryPoints"), ("Effort", "Microsoft.VSTS.Scheduling.Effort"), ("Size", "Microsoft.VSTS.Scheduling.Size")]
            .into_iter()
            .find_map(|(label, name)| num(name).map(|value| (label, value))),
        remaining: num("Microsoft.VSTS.Scheduling.RemainingWork"),
        completed: num("Microsoft.VSTS.Scheduling.CompletedWork"),
    }
}

// ---------- links ----------

/// The work items a work item links to, by id. Parent, child, related, predecessor and successor are Azure's own link
/// types.
#[derive(Debug, Default, PartialEq)]
pub struct LinkIds {
    pub parent: Option<u64>,
    pub children: Vec<u64>,
    pub related: Vec<u64>,
    pub predecessors: Vec<u64>,
    pub successors: Vec<u64>,
}

/// The most linked work items the tab looks up in one query.
pub const LINKS_CAP: usize = 40;

/// The id a work item link's url ends with: `.../workItems/<id>`.
fn linked_id(url: &str) -> Option<u64> {
    let lower = url.to_ascii_lowercase();
    let digits = &url[lower.rfind("/workitems/")? + "/workitems/".len()..];
    digits.parse().ok().filter(|id| *id > 0 && digits.bytes().all(|b| b.is_ascii_digit()))
}

/// The work item links among `relations`, grouped; the parent is the `System.Parent` field when the relations don't say.
pub fn link_ids(relations: &Value, parent_field: Option<u64>) -> LinkIds {
    let mut out = LinkIds { parent: parent_field, ..LinkIds::default() };
    for relation in relations.as_array().into_iter().flatten() {
        let Some(id) = relation["url"].as_str().and_then(linked_id) else { continue };
        let group = match relation["rel"].as_str() {
            Some("System.LinkTypes.Hierarchy-Reverse") => {
                out.parent = Some(id);
                continue;
            }
            Some("System.LinkTypes.Hierarchy-Forward") => &mut out.children,
            Some("System.LinkTypes.Related") => &mut out.related,
            Some("System.LinkTypes.Dependency-Reverse") => &mut out.predecessors,
            Some("System.LinkTypes.Dependency-Forward") => &mut out.successors,
            _ => continue,
        };
        if !group.contains(&id) {
            group.push(id);
        }
    }
    out
}

/// Every id in `links`, once, at most `LINKS_CAP`: parent first, then children, related, predecessors, successors.
pub fn all_link_ids(links: &LinkIds) -> Vec<u64> {
    let mut out: Vec<u64> = Vec::new();
    for id in links.parent.iter().chain(&links.children).chain(&links.related).chain(&links.predecessors).chain(&links.successors) {
        if !out.contains(id) {
            out.push(*id);
        }
    }
    out.truncate(LINKS_CAP);
    out
}

/// WIQL for the title, state and type of `ids`: one query for all the links, instead of one `show` each.
pub fn ids_query(ids: &[u64]) -> String {
    let list = ids.iter().filter(|id| **id > 0).map(u64::to_string).collect::<Vec<_>>().join(",");
    format!("SELECT [System.Id],[System.Title],[System.State],[System.WorkItemType],[System.AssignedTo] FROM WorkItems WHERE [System.Id] IN ({list})")
}

#[derive(Clone, Debug, PartialEq)]
pub struct LinkedItem {
    pub title: String,
    pub state: String,
    pub kind: String,
    pub assignee: Option<String>,
}

/// The name of whoever a row is assigned to: an identity or a plain string.
pub fn assignee_of(row: &QueryRow) -> Option<String> {
    let who = &row.fields["System.AssignedTo"];
    who.as_str().or_else(|| who["displayName"].as_str()).filter(|name| !name.is_empty()).map(str::to_owned)
}

/// What `ids_query` returns, by id.
pub fn linked_items(rows: &[QueryRow]) -> HashMap<u64, LinkedItem> {
    let text = |row: &QueryRow, name: &str| row.fields[name].as_str().map(str::to_owned);
    rows.iter()
        .map(|row| {
            let item = LinkedItem {
                title: text(row, "System.Title").unwrap_or_else(|| format!("#{}", row.id)),
                state: text(row, "System.State").unwrap_or_default(),
                kind: text(row, "System.WorkItemType").unwrap_or_else(|| "Work item".into()),
                assignee: assignee_of(row),
            };
            (row.id, item)
        })
        .collect()
}

/// A linked pull request's status as Azure names it, as words and a tone for its badge.
pub fn pr_status(status: Option<&str>) -> (String, Option<&'static str>) {
    match status {
        Some("active") => ("Active".into(), Some("info")),
        Some("completed") => ("Completed".into(), Some("success")),
        Some("abandoned") => ("Abandoned".into(), None),
        other => (other.unwrap_or_default().to_owned(), None),
    }
}

// ---------- history ----------

/// One field of a revision that changed. A missing `from` was empty before, a missing `to` was cleared.
#[derive(Clone, Debug, PartialEq)]
pub struct Change {
    pub label: &'static str,
    pub from: Option<String>,
    pub to: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Revision {
    pub id: u64,
    pub by: String,
    pub at: String,
    /// The first revision: what the work item started with.
    pub created: bool,
    pub changes: Vec<Change>,
}

const TRACKED: [(&str, &str); 11] = [
    ("System.State", "State"),
    ("System.AssignedTo", "Assigned to"),
    ("System.IterationPath", "Sprint"),
    ("System.AreaPath", "Area"),
    ("System.Title", "Title"),
    ("Microsoft.VSTS.Common.Priority", "Priority"),
    ("Microsoft.VSTS.Common.Severity", "Severity"),
    ("Microsoft.VSTS.Scheduling.StoryPoints", "Story points"),
    ("Microsoft.VSTS.Scheduling.Effort", "Effort"),
    ("Microsoft.VSTS.Scheduling.RemainingWork", "Remaining work"),
    ("System.Tags", "Tags"),
];

/// A changed value as text: a person by name, a path by its last part, nothing as none.
fn value_text(field: &str, value: &Value) -> Option<String> {
    let text = match value {
        Value::Null => return None,
        Value::String(text) => text.clone(),
        Value::Number(n) => js_number(n.as_f64().unwrap_or_default()),
        Value::Object(_) => return value["displayName"].as_str().filter(|name| !name.is_empty()).map(str::to_owned),
        other => other.to_string(),
    };
    if text.is_empty() {
        return None;
    }
    Some(if field == "System.IterationPath" || field == "System.AreaPath" { last_segment(Some(&text)) } else { text })
}

/// The revisions that changed something we show, oldest first. A revision that only added a comment, or touched fields
/// we don't track, is left out.
pub fn revisions_of(updates: &[Value]) -> Vec<Revision> {
    let mut sorted: Vec<&Value> = updates.iter().collect();
    sorted.sort_by(|a, b| time_order(&update_time(a), &update_time(b)).then(a["id"].as_i64().cmp(&b["id"].as_i64())));
    let mut out = Vec::new();
    for (index, update) in sorted.into_iter().enumerate() {
        let mut changes = Vec::new();
        for (field, label) in TRACKED {
            let change = &update["fields"][field];
            if change.is_null() {
                continue;
            }
            let (from, to) = (value_text(field, &change["oldValue"]), value_text(field, &change["newValue"]));
            if from != to {
                changes.push(Change { label, from, to });
            }
        }
        let created = index == 0;
        if !changes.is_empty() || created {
            out.push(Revision { id: update["id"].as_u64().unwrap_or_default(), by: author_of(update), at: update_time(update), created, changes });
        }
    }
    out
}

// ---------- discussion ----------

#[derive(Clone, Debug, PartialEq)]
pub enum Activity {
    Comment { key: String, author: String, at: String, comment: usize },
    Change { key: String, author: String, at: String, created: bool, changes: Vec<Change> },
}

impl Activity {
    pub fn key(&self) -> &str {
        match self {
            Self::Comment { key, .. } | Self::Change { key, .. } => key,
        }
    }
}

/// The fields whose changes read as part of the conversation; the History tab has all of them.
const CONVERSATION: [&str; 3] = ["State", "Assigned to", "Sprint"];

/// Comments and the changes that matter to a conversation (state, assignee, sprint, and its creation), newest first. In
/// time order a comment comes before a change made in the same revision, so newest first puts it after. A comment is
/// its index among `comments`.
pub fn discussion_of(comments: &[Comment], revisions: &[Revision]) -> Vec<Activity> {
    let mut out: Vec<Activity> = comments.iter().enumerate().map(|(at, c)| Activity::Comment { key: format!("c{}", c.id), author: c.author.clone(), at: c.at.clone(), comment: at }).collect();
    for r in revisions {
        let changes: Vec<Change> = if r.created { Vec::new() } else { r.changes.iter().filter(|c| CONVERSATION.contains(&c.label)).cloned().collect() };
        if !changes.is_empty() || r.created {
            out.push(Activity::Change { key: format!("r{}", r.id), author: r.by.clone(), at: r.at.clone(), created: r.created, changes });
        }
    }
    let at = |a: &Activity| match a {
        Activity::Comment { at, .. } | Activity::Change { at, .. } => at.clone(),
    };
    out.sort_by(|a, b| {
        time_order(&at(a), &at(b)).then(match (a, b) {
            (Activity::Comment { .. }, Activity::Change { .. }) => Ordering::Less,
            (Activity::Change { .. }, Activity::Comment { .. }) => Ordering::Greater,
            _ => Ordering::Equal,
        })
    });
    out.reverse();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rel(rel: &str, id: &str) -> Value {
        json!({ "rel": rel, "url": format!("https://dev.azure.com/org/_apis/wit/workItems/{id}"), "attributes": { "name": rel } })
    }

    fn row(id: u64, fields: Value) -> QueryRow {
        QueryRow { id, fields }
    }

    #[test]
    fn link_ids_group_the_work_item_links_by_azures_link_types_and_ignore_the_rest() {
        let links = link_ids(
            &json!([
                rel("System.LinkTypes.Hierarchy-Reverse", "10"),
                rel("System.LinkTypes.Hierarchy-Forward", "11"),
                rel("System.LinkTypes.Hierarchy-Forward", "12"),
                rel("System.LinkTypes.Hierarchy-Forward", "11"),
                rel("System.LinkTypes.Related", "20"),
                rel("System.LinkTypes.Dependency-Reverse", "30"),
                rel("System.LinkTypes.Dependency-Forward", "31"),
                rel("AttachedFile", "40"),
                { "rel": "Hyperlink", "url": "https://example.com/x/1" },
                { "rel": "System.LinkTypes.Related", "url": "https://dev.azure.com/org/_apis/wit/workItems/abc" },
                { "rel": "ArtifactLink", "url": "vstfs:///Git/PullRequestId/p%2Fr%2F5", "attributes": { "name": "Pull Request" } },
            ]),
            None,
        );
        assert_eq!(links, LinkIds { parent: Some(10), children: vec![11, 12], related: vec![20], predecessors: vec![30], successors: vec![31] });
    }

    #[test]
    fn the_parent_comes_from_the_field_when_the_relations_dont_have_it_and_no_relations_is_fine() {
        assert_eq!(link_ids(&Value::Null, Some(7)).parent, Some(7));
        assert_eq!(link_ids(&json!([rel("System.LinkTypes.Hierarchy-Reverse", "8")]), Some(7)).parent, Some(8));
        assert_eq!(link_ids(&Value::Null, None), LinkIds::default());
    }

    #[test]
    fn all_link_ids_lists_each_id_once_parent_first_and_stops_at_the_cap() {
        assert_eq!(all_link_ids(&LinkIds { parent: Some(1), children: vec![2, 3], related: vec![3, 4], predecessors: vec![], successors: vec![5] }), [1, 2, 3, 4, 5]);
        let many = LinkIds { children: (1..=100).collect(), ..LinkIds::default() };
        assert_eq!(all_link_ids(&many).len(), LINKS_CAP);
    }

    #[test]
    fn ids_query_only_puts_whole_positive_numbers_in_the_query() {
        assert!(ids_query(&[1, 22]).ends_with("[System.Id] IN (1,22)"));
        assert!(ids_query(&[1, 0, 4]).ends_with("IN (1,4)"));
    }

    #[test]
    fn linked_items_read_a_queries_rows_by_id_with_the_assignee_as_a_name_or_a_string() {
        let rows = [
            row(5, json!({ "System.Title": "Fix login", "System.State": "Active", "System.WorkItemType": "Bug", "System.AssignedTo": { "displayName": "Ada Lovelace" } })),
            row(6, json!({ "System.AssignedTo": "Grace Hopper" })),
        ];
        let items = linked_items(&rows);
        assert_eq!(items[&5], LinkedItem { title: "Fix login".into(), state: "Active".into(), kind: "Bug".into(), assignee: Some("Ada Lovelace".into()) });
        assert_eq!(items[&6], LinkedItem { title: "#6".into(), state: String::new(), kind: "Work item".into(), assignee: Some("Grace Hopper".into()) });
    }

    #[test]
    fn pr_status_words_and_tones() {
        assert_eq!(pr_status(Some("active")), ("Active".into(), Some("info")));
        assert_eq!(pr_status(Some("completed")), ("Completed".into(), Some("success")));
        assert_eq!(pr_status(Some("abandoned")), ("Abandoned".into(), None));
        assert_eq!(pr_status(None), (String::new(), None));
    }

    #[test]
    fn the_summary_takes_the_estimate_the_process_uses_and_drops_what_isnt_set() {
        let s = summary_of(&json!({ "Microsoft.VSTS.Common.Priority": 2, "Microsoft.VSTS.Scheduling.StoryPoints": 5, "Microsoft.VSTS.Scheduling.RemainingWork": 3 }));
        assert_eq!(s, Summary { priority: Some(2.0), estimate: Some(("Story points", 5.0)), remaining: Some(3.0), ..Summary::default() });
        let s = summary_of(&json!({ "Microsoft.VSTS.Scheduling.Effort": 8, "Microsoft.VSTS.Common.Severity": "2 - High" }));
        assert_eq!(s, Summary { severity: Some("High".into()), estimate: Some(("Effort", 8.0)), ..Summary::default() });
        assert_eq!(summary_of(&json!({})), Summary::default());
        assert_eq!(summary_of(&json!({ "Microsoft.VSTS.Scheduling.RemainingWork": 0 })).remaining, Some(0.0));
    }

    #[test]
    fn last_segment_and_tags_of() {
        assert_eq!(last_segment(Some("Demo\\Team A\\Sprint 5")), "Sprint 5");
        assert_eq!(last_segment(None), "");
        assert_eq!(tags_of(Some("api; ui ;  ;ops")), ["api", "ui", "ops"]);
        assert!(tags_of(None).is_empty());
    }

    fn upd(id: u64, at: &str, by: &str, fields: Value) -> Value {
        let mut all = json!({ "System.ChangedDate": { "newValue": at } });
        for (name, change) in fields.as_object().unwrap() {
            all[name] = change.clone();
        }
        json!({ "id": id, "revisedBy": { "displayName": by }, "revisedDate": "9999-01-01T00:00:00Z", "fields": all })
    }

    fn updates() -> Vec<Value> {
        vec![
            upd(3, "2026-10-03T10:00:00Z", "Grace Hopper", json!({ "System.State": { "oldValue": "Active", "newValue": "Resolved" }, "System.History": { "newValue": "<div>Fixed it</div>" } })),
            upd(1, "2026-10-01T10:00:00Z", "Ada Lovelace", json!({ "System.State": { "newValue": "New" }, "System.Title": { "newValue": "Login" }, "System.Id": { "newValue": 5 } })),
            upd(
                2,
                "2026-10-02T10:00:00Z",
                "Ada Lovelace",
                json!({
                    "System.State": { "oldValue": "New", "newValue": "Active" },
                    "System.AssignedTo": { "oldValue": { "displayName": "Ada Lovelace" }, "newValue": { "displayName": "Grace Hopper" } },
                    "System.IterationPath": { "oldValue": "Demo\\Sprint 4", "newValue": "Demo\\Sprint 5" },
                    "Microsoft.VSTS.Common.Priority": { "oldValue": 2, "newValue": 1 },
                    "System.Rev": { "oldValue": 1, "newValue": 2 },
                }),
            ),
            upd(4, "2026-10-04T10:00:00Z", "Ada Lovelace", json!({ "System.History": { "newValue": "<div>Thanks</div>" } })),
            upd(5, "2026-10-05T10:00:00Z", "Ada Lovelace", json!({ "System.AssignedTo": { "oldValue": { "displayName": "Grace Hopper" } } })),
        ]
    }

    fn change(label: &'static str, from: Option<&str>, to: Option<&str>) -> Change {
        Change { label, from: from.map(str::to_owned), to: to.map(str::to_owned) }
    }

    #[test]
    fn revisions_list_what_changed_oldest_first_as_text_and_skip_revisions_that_changed_nothing_we_show() {
        let revs = revisions_of(&updates());
        assert_eq!(revs.iter().map(|r| r.id).collect::<Vec<_>>(), [1, 2, 3, 5]);
        assert!(revs[0].created);
        assert_eq!(revs[0].changes, [change("State", None, Some("New")), change("Title", None, Some("Login"))]);
        assert_eq!(
            revs[1].changes,
            [
                change("State", Some("New"), Some("Active")),
                change("Assigned to", Some("Ada Lovelace"), Some("Grace Hopper")),
                change("Sprint", Some("Sprint 4"), Some("Sprint 5")),
                change("Priority", Some("2"), Some("1")),
            ]
        );
        assert_eq!(revs[2].changes, [change("State", Some("Active"), Some("Resolved"))]);
        assert_eq!(revs[3].changes, [change("Assigned to", Some("Grace Hopper"), None)]);
        assert_eq!(revs[1].by, "Ada Lovelace");
        assert!(revisions_of(&[]).is_empty());
    }

    #[test]
    fn the_discussion_merges_comments_with_state_assignee_and_sprint_changes_newest_first() {
        let updates = updates();
        let comments = comments_of(&updates);
        assert_eq!(comments.iter().map(|c| (c.id, c.html.as_str())).collect::<Vec<_>>(), [(3, "<div>Fixed it</div>"), (4, "<div>Thanks</div>")]);
        let feed = discussion_of(&comments, &revisions_of(&updates));
        assert_eq!(feed.iter().map(Activity::key).collect::<Vec<_>>(), ["r5", "c4", "r3", "c3", "r2", "r1"]);
        assert!(matches!(feed.last(), Some(Activity::Change { created: true, .. })));
        let Some(Activity::Change { changes, .. }) = feed.get(feed.len() - 2) else { panic!("a change") };
        assert_eq!(changes.iter().map(|c| c.label).collect::<Vec<_>>(), ["State", "Assigned to", "Sprint"]);
    }

    #[test]
    fn comments_keep_revisions_with_a_history_oldest_first() {
        let u = |id: u64, at: &str, history: Option<&str>| {
            let mut fields = json!({ "System.ChangedDate": { "newValue": at } });
            if let Some(history) = history {
                fields["System.History"] = json!({ "newValue": history });
            }
            json!({ "id": id, "revisedBy": { "displayName": "Ana" }, "fields": fields })
        };
        let out = comments_of(&[u(3, "2026-10-03T00:00:00Z", Some("<p>b</p>")), u(2, "2026-10-02T00:00:00Z", None), u(1, "2026-10-01T00:00:00Z", Some("<p>a</p>"))]);
        assert_eq!(out.iter().map(|c| (c.id, c.html.as_str())).collect::<Vec<_>>(), [(1, "<p>a</p>"), (3, "<p>b</p>")]);
    }

    #[test]
    fn pull_request_links_read_the_vstfs_artifact_links_once_each() {
        let link = |id: u32| json!({ "rel": "ArtifactLink", "attributes": { "name": "Pull Request" }, "url": format!("vstfs:///Git/PullRequestId/aaa%2fbbb%2f{id}") });
        let rels = json!([link(12), link(12), { "rel": "ArtifactLink", "attributes": { "name": "Fixed in Commit" }, "url": "vstfs:///Git/Commit/aaa%2fbbb%2fabc" }, { "rel": "Related" }]);
        assert_eq!(pull_request_links(&rels), [PrLink { repo_id: "bbb".into(), id: 12 }]);
        assert!(pull_request_links(&Value::Null).is_empty());
    }

    #[test]
    fn item_args_accept_only_an_id_and_an_https_org_and_the_tab_args_are_the_same_from_anywhere() {
        let org = "https://dev.azure.com/o";
        assert_eq!(item_args(&json!({ "id": 5, "org": org })), Some(ItemArgs { id: 5, org: org.into() }));
        assert_eq!(item_args(&json!({ "id": 5, "org": org, "project": "P" })), Some(ItemArgs { id: 5, org: org.into() }), "a tab saved with a project still opens");
        for bad in [json!(null), json!({}), json!({ "id": "5", "org": "https://x" }), json!({ "id": 5, "org": "--x" }), json!({ "id": 0, "org": "https://x" })] {
            assert_eq!(item_args(&bad), None);
        }
        assert_eq!(item_args(&item_tab_args(1234, org)), Some(ItemArgs { id: 1234, org: org.into() }));
        assert_eq!(item_tab_args(1234, org).to_string(), r#"{"id":1234,"org":"https://dev.azure.com/o"}"#);
    }
}
