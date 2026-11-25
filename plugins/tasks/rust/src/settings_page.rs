//! The Issues page in Wings Settings: which view the Issues tab opens in, how the sidebar sorts, and whether issue keys are kept
//! out of commits and pull requests.

use crate::{
    parts::stack,
    settings::{self, DEFAULT_VIEWS, SECTION_ORDERS},
};
use wings_plugin::ui::{Node, rows_card, section, select, setting_field, setting_toggle};

fn view() -> Vec<Node> {
    let views: Vec<(String, String)> = DEFAULT_VIEWS.iter().map(|(value, label)| ((*value).to_owned(), (*label).to_owned())).collect();
    let orders: Vec<(String, String)> = SECTION_ORDERS.iter().map(|(value, label)| ((*value).to_owned(), (*label).to_owned())).collect();
    vec![stack("l")
        .child(section(
            "Issues board",
            None,
            [rows_card([setting_field(
                "Default view",
                "The view the Issues tab opens in: Board has a column per status, Board by epic adds a row per epic, and List has lines under each status. Switching views in the tab is remembered until you pick here again.",
                select("Default view", &views, &settings::default_view(), |value| {
                    drop(settings::set_default_view(value));
                    drop(settings::set_last_layout(None));
                }),
            )])],
        ))
        .child(section(
            "Sidebar",
            None,
            [rows_card([setting_field(
                "Sort by",
                "Issues an agent is working on right now come first under each project. The rest are sorted by this.",
                select("Sort by", &orders, &settings::section_order(), |value| drop(settings::set_section_order(value))),
            )])],
        ))
        .child(section(
            "Commits and pull requests",
            None,
            [rows_card([setting_toggle(
                "Keep issues local",
                "Issues live on this computer. With this on, Claude can't put an issue key like WIN-12 in a commit, tag, pull request or issue comment: the command is stopped and it is asked to take the key out.",
                settings::keep_local(),
                |on| drop(settings::set_keep_local(on)),
            )])],
        ))]
}

#[cfg_attr(not(test), unsafe(no_mangle))]
pub fn start() {
    wings_plugin::run(view);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find_row<'a>(node: &'a serde_json::Value, name: &str) -> Option<&'a serde_json::Value> {
        if node["type"] == "ListRow" && node["children"][0]["children"][0]["children"][0]["text"] == name {
            return Some(node);
        }
        node["children"].as_array().into_iter().flatten().find_map(|child| find_row(child, name))
    }

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

    #[test]
    fn every_control_sits_at_the_end_of_its_row_in_one_width_and_the_boolean_is_a_switch() {
        let page = wings_plugin::ui::tree_json(view().remove(0));
        let rows: Vec<&serde_json::Value> = ["Default view", "Sort by", "Keep issues local"]
            .iter()
            .map(|name| find_row(&page, name).unwrap_or_else(|| panic!("a row for {name}")))
            .collect();
        for (row, kind) in rows.iter().zip(["Stack", "Stack", "Checkbox"]) {
            let control = &row["children"][2]["children"][0];
            assert_eq!(control["type"], kind);
        }
        assert_eq!(rows[0]["children"][2]["children"][0]["props"]["width"], rows[1]["children"][2]["children"][0]["props"]["width"]);
        assert_eq!(rows[2]["children"][2]["children"][0]["props"]["switch"], true);
    }
}
