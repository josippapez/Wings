//! The Issues page in Wings Settings: which view the Issues tab opens in, how the sidebar sorts, and whether issue keys are kept
//! out of commits and pull requests.

use crate::{
    parts::{line, stack},
    settings::{self, DEFAULT_VIEWS, SECTION_ORDERS},
};
use wings_plugin::ui::{Node, checkbox, chips};

/// The text under each view, which the app's chips hold in place of a drawing.
fn hint(view: &str) -> &'static str {
    match view {
        "board" => "Status columns",
        "rows" => "Columns, a row per epic",
        _ => "Lines under each status",
    }
}

fn heading(text: &str) -> Node {
    Node::new("Heading").text(text)
}

fn view() -> Vec<Node> {
    let views: Vec<(String, String)> = DEFAULT_VIEWS.iter().map(|(value, label)| ((*value).to_owned(), format!("{label}: {}", hint(value)))).collect();
    let view_options: Vec<(&str, &str)> = views.iter().map(|(value, label)| (value.as_str(), label.as_str())).collect();
    vec![stack("s")
        .child(heading("Issues board"))
        .child(line("The view the Issues tab opens in. Switching views in the tab is remembered until you pick here again.", Some("muted"), None))
        .child(chips("Default view", &view_options, &settings::default_view(), |value| {
            drop(settings::set_default_view(value));
            drop(settings::set_last_layout(None));
        }))
        .child(heading("Sidebar"))
        .child(line("Issues an agent is working on right now come first under each project. The rest are sorted by this.", Some("muted"), None))
        .child(chips("Sort by", &SECTION_ORDERS, &settings::section_order(), |value| drop(settings::set_section_order(value))))
        .child(heading("Commits and pull requests"))
        .child(checkbox("Keep issues local", settings::keep_local(), |on| drop(settings::set_keep_local(on))))
        .child(line("Issues live on this computer. With this on, Claude can't put an issue key like WIN-12 in a commit, tag, pull request or issue comment: the command is stopped and it is asked to take the key out.", Some("muted"), Some("small")))]
}

#[unsafe(no_mangle)]
pub fn start() {
    wings_plugin::run(view);
}
