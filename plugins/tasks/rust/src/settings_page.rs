//! The Issues page in Wings Settings: which view the Issues tab opens in, how the sidebar sorts, and whether issue keys are kept
//! out of commits and pull requests.

use crate::{
    parts::stack,
    settings::{self, DEFAULT_VIEWS, SECTION_ORDERS},
};
use wings_plugin::ui::{Entry, Node, checkbox, chips, entry, muted, rows_card, section, text};

/// The text under each view, which the app's chips hold in place of a drawing.
fn hint(view: &str) -> &'static str {
    match view {
        "board" => "Status columns",
        "rows" => "Columns, a row per epic",
        _ => "Lines under each status",
    }
}

/// A setting as a row: its name and what it does on the left, its control at the end, as Wings' own Settings rows are.
fn setting(name: &str, detail: &str, control: Node) -> Node {
    entry(Entry { lead: None, title: text(name).prop("weight", "medium"), detail: Some(muted(detail).prop("size", "small")), trailing: Some(control) })
}

fn view() -> Vec<Node> {
    let views: Vec<(String, String)> = DEFAULT_VIEWS.iter().map(|(value, label)| ((*value).to_owned(), format!("{label}: {}", hint(value)))).collect();
    let view_options: Vec<(&str, &str)> = views.iter().map(|(value, label)| (value.as_str(), label.as_str())).collect();
    let track = |node: Node| node.prop("segmented", true);
    vec![stack("l")
        .child(section(
            "Issues board",
            None,
            [rows_card([setting(
                "Default view",
                "The view the Issues tab opens in. Switching views in the tab is remembered until you pick here again.",
                track(chips("Default view", &view_options, &settings::default_view(), |value| {
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

#[unsafe(no_mangle)]
pub fn start() {
    wings_plugin::run(view);
}
