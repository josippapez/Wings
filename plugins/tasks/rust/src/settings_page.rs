//! The Issues page in Wings Settings: which view the Issues tab opens in, how the sidebar sorts, and whether issue keys are kept
//! out of commits and pull requests.

use crate::{
    parts::stack,
    settings::{self, DEFAULT_VIEWS, SECTION_ORDERS},
};
use wings_plugin::ui::{Entry, Node, checkbox, chips, entry, muted, rows_card, section, select, setting};

/// A menu's width, as Wings' own Settings and PR tracker's have it.
const MENU_WIDTH: u32 = 220;

fn view() -> Vec<Node> {
    let views: Vec<(String, String)> = DEFAULT_VIEWS.iter().map(|(value, label)| ((*value).to_owned(), (*label).to_owned())).collect();
    let track = |node: Node| node.segmented();
    vec![stack("l")
        .child(section(
            "Issues board",
            None,
            [rows_card([setting(
                "Default view",
                "The view the Issues tab opens in: Board has a column per status, Board by epic adds a row per epic, and List has lines under each status. Switching views in the tab is remembered until you pick here again.",
                // A menu: the three names don't fit on a track beside the text in a narrow page.
                stack("none").prop("width", MENU_WIDTH).child(select("Default view", &views, &settings::default_view(), |value| {
                    drop(settings::set_default_view(value));
                    drop(settings::set_last_layout(None));
                })),
            )])],
        ))
        .child(section(
            "Sidebar",
            None,
            [rows_card([setting(
                "Sort by",
                "Issues an agent is working on right now come first under each project. The rest are sorted by this.",
                track(chips("Sort by", &SECTION_ORDERS, &settings::section_order(), |value| drop(settings::set_section_order(value)))),
            )])],
        ))
        .child(section(
            "Commits and pull requests",
            None,
            [rows_card([entry(Entry {
                lead: None,
                title: checkbox("Keep issues local", settings::keep_local(), |on| drop(settings::set_keep_local(on))),
                detail: Some(muted("Issues live on this computer. With this on, Claude can't put an issue key like WIN-12 in a commit, tag, pull request or issue comment: the command is stopped and it is asked to take the key out.").prop("size", "small")),
                trailing: None,
            })])],
        ))]
}

#[cfg_attr(not(test), unsafe(no_mangle))]
pub fn start() {
    wings_plugin::run(view);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_view_is_a_menu_of_the_three_views_by_name() {
        fn find<'a>(node: &'a serde_json::Value, kind: &str) -> Vec<&'a serde_json::Value> {
            let mut found: Vec<_> = node["children"].as_array().into_iter().flatten().flat_map(|child| find(child, kind)).collect();
            if node["type"] == kind {
                found.push(node);
            }
            found
        }
        let page = wings_plugin::ui::tree_json(view().remove(0));
        let select = find(&page, "Select").into_iter().find(|select| select["props"]["label"] == "Default view").expect("a menu");
        let labels: Vec<&str> = select["props"]["options"].as_array().unwrap().iter().map(|option| option["label"].as_str().unwrap()).collect();
        assert_eq!(labels, ["Board", "Board by epic", "List"]);
    }
}
