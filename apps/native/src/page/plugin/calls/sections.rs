//! A plugin's lists under each project in the sidebar: `setProjectSection` and `setSectionSort`, with the desktop's
//! checks and words (`cleanSectionRows` in `apps/desktop/src/lib/project-sections.ts` and the two cases in
//! `apps/desktop/src/lib/plugins.ts`). A plugin's rows are untrusted: each keeps only what the window shows, cut to length.

use serde_json::Value;

use super::{Access, Shared, js_string};
use crate::page::plugin::ui::{Tone, slice, units};

/// The most rows a section keeps.
pub const SECTION_ROWS_MAX: usize = 50;
/// The most JSON a row's args may hold, in JS string units, as `openView`'s.
const ROW_ARGS_MAX: usize = 16 * 1024;

/// A row's icon in place of the plain dot: a pull request's, or a work status drawn like Linear's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SectionIcon {
    PrOpen,
    PrMerged,
    PrClosed,
    PrDraft,
    Todo,
    InProgress,
    InReview,
    Done,
    Canceled,
}

impl SectionIcon {
    fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "pr-open" => Self::PrOpen,
            "pr-merged" => Self::PrMerged,
            "pr-closed" => Self::PrClosed,
            "pr-draft" => Self::PrDraft,
            "todo" => Self::Todo,
            "in-progress" => Self::InProgress,
            "in-review" => Self::InReview,
            "done" => Self::Done,
            "canceled" => Self::Canceled,
            _ => return None,
        })
    }
}

/// One line of a plugin's section under a project. A click opens `view`, one of the plugin's views that open as a tab,
/// in that project.
#[derive(Clone, Debug, PartialEq)]
pub struct SectionRow {
    pub id: String,
    pub label: String,
    pub detail: Option<String>,
    pub badge: Option<(String, Tone)>,
    pub icon: Option<SectionIcon>,
    /// The id of another row of the section that this one sits under, indented.
    pub parent: Option<String>,
    pub view: String,
    pub args: Option<Value>,
}

fn tone(name: Option<&str>) -> Tone {
    match name {
        Some("info") => Tone::Info,
        Some("success") => Tone::Success,
        Some("warning") => Tone::Warning,
        Some("danger") => Tone::Danger,
        Some("merged") => Tone::Merged,
        _ => Tone::Neutral,
    }
}

/// `text(v, max)`: a non-empty string cut to `max`, or nothing for anything else.
fn text(value: Option<&Value>, max: usize) -> Option<String> {
    value
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
        .map(|text| slice(text, max))
}

/// Up to 50 rows with known fields and bounded text. A row with no usable id or label is left out, and so is a second
/// row with an id already seen. Fails when `raw` isn't a list, or a row opens a view that isn't in `tab_views`, or its
/// args are over 16 KB.
pub fn clean_rows(raw: Option<&Value>, tab_views: &[String]) -> Result<Vec<SectionRow>, String> {
    let Some(rows) = raw.and_then(Value::as_array) else {
        return Err("setProjectSection rows must be a list, or null to remove the section".into());
    };
    let mut seen = std::collections::HashSet::new();
    let mut clean = Vec::new();
    for row in rows.iter().take(SECTION_ROWS_MAX) {
        let Some(row) = row.as_object() else {
            continue;
        };
        let id = row
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty() && units(id) <= 64);
        let (Some(id), Some(label)) = (id, text(row.get("label"), 120)) else {
            continue;
        };
        if seen.contains(id) {
            continue;
        }
        let parent = row
            .get("parent")
            .and_then(Value::as_str)
            .filter(|parent| !parent.is_empty() && units(parent) <= 64 && *parent != id);
        let open = row.get("open").and_then(Value::as_object);
        let view = open
            .and_then(|open| open.get("view"))
            .and_then(Value::as_str)
            .filter(|view| tab_views.iter().any(|known| known == view));
        let Some(view) = view else {
            return Err(format!(
                "Row {id} opens {}, which isn't one of the plugin's views that open as a tab",
                js_string(open.and_then(|open| open.get("view")))
            ));
        };
        let args = open
            .and_then(|open| open.get("args"))
            .filter(|args| !args.is_null());
        if args.is_some_and(|args| units(&args.to_string()) > ROW_ARGS_MAX) {
            return Err(format!("Row args is limited to {} KB", ROW_ARGS_MAX / 1024));
        }
        let badge = row.get("badge").and_then(Value::as_object);
        let badge_text = text(badge.and_then(|badge| badge.get("text")), 24);
        seen.insert(id);
        clean.push(SectionRow {
            id: id.to_owned(),
            label,
            detail: text(row.get("detail"), 120),
            badge: badge_text.map(|text| {
                (
                    text,
                    tone(
                        badge
                            .and_then(|badge| badge.get("tone"))
                            .and_then(Value::as_str),
                    ),
                )
            }),
            icon: row
                .get("icon")
                .and_then(Value::as_str)
                .and_then(SectionIcon::parse),
            parent: parent.map(str::to_owned),
            view: view.to_owned(),
            args: args.cloned(),
        });
    }
    Ok(clean)
}

impl Shared {
    /// `setProjectSection`: the plugin's section under a project in the sidebar, or with `rows: null` taken away.
    pub(super) fn set_project_section(
        &self,
        access: &Access,
        params: &Value,
    ) -> Result<String, String> {
        let plugin = &access.plugin.manifest;
        let section = js_string(params.get("sectionId"));
        if !plugin
            .contributes
            .project_sections
            .iter()
            .any(|s| s.id == section)
        {
            return Err(format!(
                "{} has no project section {section}. Declare it in contributes.projectSections",
                plugin.id
            ));
        }
        let project = js_string(params.get("projectId"));
        if !self
            .projects
            .lock()
            .unwrap()
            .iter()
            .any(|p| p.id == project)
        {
            return Err(format!("no project {project}"));
        }
        let tab_views: Vec<String> = plugin
            .contributes
            .views
            .iter()
            .filter(|view| view.opens.iter().any(|place| place == "tab"))
            .map(|view| view.id.clone())
            .collect();
        let rows = match params.get("rows") {
            Some(Value::Null) => None,
            rows => Some(clean_rows(rows, &tab_views)?),
        };
        access
            .env
            .set_project_section(&plugin.id, &project, &section, rows)?;
        Ok("null".into())
    }

    /// `setSectionSort`: the plugin says which of its section's sorts its rows are in.
    pub(super) fn set_section_sort(
        &self,
        access: &Access,
        params: &Value,
    ) -> Result<String, String> {
        let plugin = &access.plugin.manifest;
        let section = js_string(params.get("sectionId"));
        let sort = js_string(params.get("sort"));
        let known = plugin
            .contributes
            .project_sections
            .iter()
            .find(|s| s.id == section)
            .is_some_and(|s| s.sorts.iter().any(|option| option.id == sort));
        if !known {
            return Err(format!(
                "{}'s project section {section} has no sort {sort}. Declare it in its sorts",
                plugin.id
            ));
        }
        access.env.set_section_sort(&plugin.id, &section, &sort);
        Ok("null".into())
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::page::plugin::{
        Project,
        testing::{Rig, Seen},
    };

    fn views() -> Vec<String> {
        vec!["board".into()]
    }

    #[test]
    fn rows_keep_what_the_sidebar_shows_cut_to_length_and_drop_what_it_cannot() {
        let long = "x".repeat(200);
        let rows = json!([
            { "id": "a", "label": long, "detail": "#12 · open", "badge": { "text": "needs review", "tone": "warning" },
              "icon": "pr-open", "open": { "view": "board", "args": { "id": 1 } } },
            { "id": "b", "label": "Child", "parent": "a", "badge": { "text": "x", "tone": "loud" }, "icon": "nope",
              "open": { "view": "board", "args": null } },
            { "id": "a", "label": "Same id again", "open": { "view": "board" } },
            { "id": "c", "label": "", "open": { "view": "board" } },
            { "label": "No id", "open": { "view": "board" } },
            { "id": "d", "label": "Own parent", "parent": "d", "open": { "view": "board" } },
            "not a row",
        ]);
        let rows = clean_rows(Some(&rows), &views()).unwrap();
        let ids: Vec<_> = rows.iter().map(|row| row.id.as_str()).collect();
        assert_eq!(ids, ["a", "b", "d"]);
        assert_eq!(rows[0].label.len(), 120);
        assert_eq!(rows[0].detail.as_deref(), Some("#12 · open"));
        assert_eq!(rows[0].badge, Some(("needs review".into(), Tone::Warning)));
        assert_eq!(rows[0].icon, Some(SectionIcon::PrOpen));
        assert_eq!(rows[0].args, Some(json!({ "id": 1 })));
        // An unknown tone is neutral, an unknown icon is none, and null args are no args.
        assert_eq!(rows[1].badge, Some(("x".into(), Tone::Neutral)));
        assert_eq!(rows[1].icon, None);
        assert_eq!(rows[1].parent.as_deref(), Some("a"));
        assert_eq!(rows[1].args, None);
        assert_eq!(rows[2].parent, None);
    }

    #[test]
    fn a_section_keeps_at_most_fifty_rows() {
        let rows: Vec<_> = (0..80)
            .map(|n| json!({ "id": format!("r{n}"), "label": "Row", "open": { "view": "board" } }))
            .collect();
        let rows = clean_rows(Some(&json!(rows)), &views()).unwrap();
        assert_eq!(rows.len(), SECTION_ROWS_MAX);
    }

    fn plugin_with_sections() -> Rig {
        let rig = Rig::new(
            json!({ "contributes": {
                "views": [
                    { "id": "board", "title": "Board", "page": "dist/board.html", "opens": ["tab"] },
                    { "id": "side", "title": "Side", "page": "dist/side.html", "opens": ["sidebar"] }],
                "projectSections": [
                    { "id": "issues", "title": "Issues", "all": "board",
                      "sorts": [{ "id": "status", "label": "Status" }, { "id": "date", "label": "Latest" }] },
                    { "id": "plain", "title": "Plain" }],
            } }),
            vec![],
        );
        rig.host.publish_projects(vec![Project {
            id: "/work".into(),
            path: "/work".into(),
            name: "Work".into(),
            parent_id: None,
        }]);
        rig
    }

    /// What `wings.setProjectSection(projectId, sectionId, rows)` sends: no `rows` when none was given.
    fn section(project: &str, section: &str, rows: Option<Value>) -> Value {
        let mut params = json!({ "projectId": project, "sectionId": section });
        if let Some(rows) = rows {
            params["rows"] = rows;
        }
        params
    }

    #[test]
    fn set_project_section_hands_the_app_the_cleaned_rows_and_null_takes_the_section_away() {
        let rig = plugin_with_sections();
        assert_eq!(
            rig.allows(
                "setProjectSection",
                section("/work", "issues", Some(json!([{ "id": "r1", "label": "First", "open": { "view": "board", "args": { "n": 1 } } }])))
            ),
            json!(null)
        );
        let Some(Seen::ProjectSection(plugin, project, section_id, Some(rows))) = rig.env.seen().pop()
        else {
            panic!("the app was not given the section");
        };
        assert_eq!(
            (plugin.as_str(), project.as_str(), section_id.as_str()),
            ("probe", "/work", "issues")
        );
        assert_eq!(
            (rows[0].id.as_str(), rows[0].view.as_str()),
            ("r1", "board")
        );
        assert_eq!(rows[0].args, Some(json!({ "n": 1 })));

        rig.allows("setProjectSection", section("/work", "issues", Some(json!(null))));
        assert_eq!(
            rig.env.seen().pop(),
            Some(Seen::ProjectSection(
                "probe".into(),
                "/work".into(),
                "issues".into(),
                None
            ))
        );
    }

    #[test]
    fn set_project_section_is_refused_for_an_undeclared_section_an_unknown_project_and_bad_rows() {
        let rig = plugin_with_sections();
        let refused = |project: &str, id: &str, rows: Option<Value>| {
            rig.refuses("setProjectSection", section(project, id, rows))
        };
        assert_eq!(
            refused("/work", "nope", Some(json!(null))),
            "probe has no project section nope. Declare it in contributes.projectSections"
        );
        assert_eq!(
            refused("/elsewhere", "issues", Some(json!(null))),
            "no project /elsewhere"
        );
        for rows in [Some(json!("text")), Some(json!({})), None] {
            assert_eq!(
                refused("/work", "issues", rows.clone()),
                "setProjectSection rows must be a list, or null to remove the section",
                "{rows:?}"
            );
        }
        // A row opens only a view that opens as a tab.
        assert_eq!(
            refused("/work", "issues", Some(json!([{ "id": "r1", "label": "L", "open": { "view": "side" } }]))),
            "Row r1 opens side, which isn't one of the plugin's views that open as a tab"
        );
        assert_eq!(
            refused("/work", "issues", Some(json!([{ "id": "r1", "label": "L" }]))),
            "Row r1 opens undefined, which isn't one of the plugin's views that open as a tab"
        );
        assert_eq!(
            refused("/work", "issues", Some(json!([{ "id": "r1", "label": "L", "open": { "view": "board", "args": "x".repeat(17000) } }]))),
            "Row args is limited to 16 KB"
        );
        assert!(
            !rig.env
                .seen()
                .iter()
                .any(|seen| matches!(seen, Seen::ProjectSection(..))),
            "a refused call must reach the app as nothing"
        );
    }

    #[test]
    fn set_section_sort_accepts_a_declared_sort_and_refuses_the_rest() {
        let rig = plugin_with_sections();
        let sort = |id: &str, sort: &str| json!({ "sectionId": id, "sort": sort });
        rig.allows("setSectionSort", sort("issues", "date"));
        assert_eq!(
            rig.env.seen().pop(),
            Some(Seen::SectionSort(
                "probe".into(),
                "issues".into(),
                "date".into()
            ))
        );
        assert_eq!(
            rig.refuses("setSectionSort", sort("issues", "nope")),
            "probe's project section issues has no sort nope. Declare it in its sorts"
        );
        // A section that declares no sorts has none to confirm, and neither has one that isn't declared.
        assert_eq!(
            rig.refuses("setSectionSort", sort("plain", "date")),
            "probe's project section plain has no sort date. Declare it in its sorts"
        );
        assert_eq!(
            rig.refuses("setSectionSort", sort("nope", "date")),
            "probe's project section nope has no sort date. Declare it in its sorts"
        );
    }
}
