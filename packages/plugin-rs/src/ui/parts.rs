//! Pieces plugins kept writing again, built from the nodes of `ui`: a page's header with its context, an icon button, a
//! settings row, a line of facts, and the props a button takes by name. They only compose nodes the app already draws, so
//! they work in any Wings that has the nodes.

use super::{Entry, Node, button, card, checkbox, entry, muted, notice, row, slot, stack, text, title};

impl Node {
    /// On a `Button`: its size, one of `sm`, `icon-sm`, `icon-xs`...
    pub fn size(self, size: &str) -> Self {
        self.prop("size", size)
    }

    /// On a `Button` or an `Svg`: the name a screen reader reads out, which a button with only an icon needs.
    pub fn aria_label(self, label: &str) -> Self {
        self.prop("label", label)
    }

    /// The hover text of a `Button` or `Text`.
    pub fn tooltip(self, text: &str) -> Self {
        self.prop("title", text)
    }

    /// Greys a `Button` and takes its clicks while `disabled`. False sets nothing.
    pub fn disabled(self, disabled: bool) -> Self {
        if disabled { self.prop("disabled", true) } else { self }
    }

    /// On a `Chips`: drawn as a segmented track, as the app's Settings draws a choice of a few.
    pub fn segmented(self) -> Self {
        self.prop("segmented", true)
    }
}

/// A ghost button with only an icon of `paths` (Lucide's, on a 24-point grid; see `icons`), 16 points wide. `label` is
/// what a screen reader reads out and what the tooltip says. Chain `.tooltip(..)` for another tooltip and `.disabled(..)`.
pub fn icon_button(label: &str, paths: &[&str], on_click: impl Fn() + 'static) -> Node {
    Node::new("Button")
        .prop("variant", "ghost")
        .prop("size", "icon-sm")
        .prop("label", label)
        .prop("title", label)
        .on("onClick", move |_| on_click())
        .child(super::icon(24, 2.0, paths).prop("size", 16))
}

/// A page whose `top` (its header and the bars under it) stays put over its `body`.
pub fn page_with(top: impl IntoIterator<Item = Node>, body: impl IntoIterator<Item = Node>) -> Node {
    Node::new("Page").child(Node::new("Slot").prop("name", "toolbar").children(top)).child(Node::new("Slot").prop("name", "body").children(body))
}

/// A header's title as the design system has it: `name` strong and its `context` muted and small after it, each cut
/// short with its full text as the tooltip when there is no room. Nothing is drawn for an empty `context`. Give it to
/// `toolbar_titled`.
pub fn title_with_context(name: &str, context: &str) -> Node {
    row()
        .prop("gap", "m")
        .prop("align", "center")
        .child(text(name).prop("weight", "strong").prop("truncate", true).prop("title", name))
        .maybe((!context.is_empty()).then(|| muted(context).prop("size", "small").prop("truncate", true).prop("title", context)))
}

/// A page's top row with any node as its title (`title_with_context`) and `actions` on the right.
pub fn toolbar_titled(title: Node, actions: Option<Node>) -> Node {
    Node::new("Toolbar").child(slot("title", title)).maybe(actions.map(|actions| slot("actions", actions)))
}

/// A small muted label over a section of a page, with its count: "Conversation · 3".
pub fn section_label(label: impl Into<String>) -> Node {
    muted(label).prop("size", "small").prop("weight", "medium")
}

/// `parts` as one line split by a muted " · ", each part keeping its own tone.
pub fn joined(parts: Vec<Node>) -> Node {
    let mut line = row().prop("gap", "s").prop("wrap", true).prop("align", "center");
    for (at, part) in parts.into_iter().enumerate() {
        if at > 0 {
            line = line.child(muted("·").prop("size", "small").key(format!("dot{at}")));
        }
        line = line.child(part.key(format!("part{at}")));
    }
    line
}

/// A setting as a row of a `rows_card`: its `name` and what it does under it on the left, its `control` at the end, as
/// Wings' own Settings rows are.
pub fn setting(name: &str, detail: &str, control: Node) -> Node {
    entry(Entry { lead: None, title: text(name).prop("weight", "medium"), detail: Some(muted(detail).prop("size", "small")), trailing: Some(control) })
}

/// How wide the control column of a settings row is: Wings' own Settings draws its menus 176 points wide.
pub const SETTING_COLUMN: u32 = 176;

/// A `setting` whose `control` (a `select` or a `text_input`) fills a column of one width at the end of the row, as Wings'
/// own Settings rows do, so the menus of a page line up whatever their choices say.
pub fn setting_field(name: &str, detail: &str, control: Node) -> Node {
    setting(name, detail, stack().prop("width", SETTING_COLUMN).child(control))
}

/// A setting that is on or off, as a row of a `rows_card`: the name and what it does on the left and a switch at the
/// end, as Wings' own Settings draws it. `on_change` hears the next state. Needs Wings 0.2.112 for the switch; an older
/// Wings draws a checkbox at the end instead.
pub fn setting_toggle(name: &str, detail: &str, on: bool, on_change: impl Fn(bool) + 'static) -> Node {
    setting(name, detail, checkbox("", on, on_change).prop("switch", true).aria_label(name))
}

/// What a page shows in place of what it could not load: a card with a `title` line, the `message` in a danger notice
/// and, with `retry` as `(label, handler)`, a primary button that tries again.
pub fn error_state(title_line: &str, message: &str, retry: Option<(&str, Box<dyn Fn()>)>) -> Node {
    let again = retry.map(|(label, on_click)| row().prop("gap", "s").child(button(label, "default", false, on_click)));
    card().child(stack().prop("gap", "m").child(title(title_line)).child(notice("danger", message)).maybe(again))
}

/// A setting that is a tick, as a row of a `rows_card`: the `tick` (a `checkbox`) is labelled with the setting's name,
/// and `detail` says what it does under it.
pub fn setting_tick(tick: Node, detail: &str) -> Node {
    entry(Entry { lead: None, title: tick, detail: Some(muted(detail).prop("size", "small")), trailing: None })
}

/// Lucide's icons as the paths `icon` and `icon_button` draw on a 24-point grid, by the icon's name.
pub mod icons {
    pub const PLUS: [&str; 2] = ["M5 12h14", "M12 5v14"];
    pub const CLOSE: [&str; 2] = ["M18 6 6 18", "m6 6 12 12"];
    pub const CHECK: [&str; 1] = ["M20 6 9 17l-5-5"];
    /// `refresh-cw`: two arrows round, as Reload.
    pub const REFRESH_CW: [&str; 4] = ["M3 12a9 9 0 0 1 9-9 9.75 9.75 0 0 1 6.74 2.74L21 8", "M21 3v5h-5", "M21 12a9 9 0 0 1-9 9 9.75 9.75 0 0 1-6.74-2.74L3 16", "M8 16H3v5"];
    /// `rotate-cw`: one arrow round, as Rescan and Repeat.
    pub const ROTATE_CW: [&str; 2] = ["M21 12a9 9 0 1 1-9-9c2.52 0 4.93 1 6.74 2.74L21 8", "M21 3v5h-5"];
    pub const EXTERNAL_LINK: [&str; 3] = ["M15 3h6v6", "M10 14 21 3", "M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6"];
    pub const TRASH: [&str; 3] = ["M3 6h18", "M19 6v14c0 1-1 2-2 2H7c-1 0-2-1-2-2V6", "M8 6V4c0-1 1-2 2-2h4c1 0 2 1 2 2v2"];
    /// `trash-2`: the bin with its two lines.
    pub const TRASH_2: [&str; 5] = ["M3 6h18", "M19 6v14c0 1-1 2-2 2H7c-1 0-2-1-2-2V6", "M8 6V4c0-1 1-2 2-2h4c1 0 2 1 2 2v2", "M10 11v6", "M14 11v6"];
    pub const PENCIL: [&str; 2] = ["M21.174 6.812a1 1 0 0 0-3.986-3.987L3.842 16.174a2 2 0 0 0-.5.83l-1.321 4.352a.5.5 0 0 0 .623.622l4.353-1.32a2 2 0 0 0 .83-.497z", "m15 5 4 4"];
    pub const COPY: [&str; 2] = ["M10 8h10a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H10a2 2 0 0 1-2-2V10a2 2 0 0 1 2-2z", "M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2"];
    pub const SEARCH: [&str; 2] = ["M3 11a8 8 0 1 0 16 0a8 8 0 1 0-16 0", "m21 21-4.3-4.3"];
    pub const PLAY: [&str; 1] = ["m6 3 14 9-14 9z"];
    pub const PAUSE: [&str; 2] = ["M8 5v14", "M16 5v14"];
    pub const CHEVRON_RIGHT: [&str; 1] = ["m9 18 6-6-6-6"];
    pub const CHEVRON_LEFT: [&str; 1] = ["m15 18-6-6 6-6"];
    pub const CHEVRON_UP: [&str; 1] = ["m18 15-6-6-6 6"];
    pub const CHEVRON_DOWN: [&str; 1] = ["m6 9 6 6 6-6"];
    pub const FOLDER_OPEN: [&str; 1] = ["m6 14 1.5-2.9A2 2 0 0 1 9.24 10H20a2 2 0 0 1 1.94 2.5l-1.54 6a2 2 0 0 1-1.95 1.5H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h3.9a2 2 0 0 1 1.69.9l.81 1.2a2 2 0 0 0 1.67.9H18a2 2 0 0 1 2 2v2"];
    pub const SQUARE_TERMINAL: [&str; 3] = ["M5 3h14a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2z", "m7 11 2-2-2-2", "M11 13h4"];
    /// `settings`: the cog.
    pub const SETTINGS: [&str; 2] = [
        "M9.671 4.136a2.34 2.34 0 0 1 4.659 0 2.34 2.34 0 0 0 3.319 1.915 2.34 2.34 0 0 1 2.33 4.033 2.34 2.34 0 0 0 0 3.831 2.34 2.34 0 0 1-2.33 4.033 2.34 2.34 0 0 0-3.319 1.915 2.34 2.34 0 0 1-4.659 0 2.34 2.34 0 0 0-3.32-1.915 2.34 2.34 0 0 1-2.33-4.033 2.34 2.34 0 0 0 0-3.831A2.34 2.34 0 0 1 6.35 6.051a2.34 2.34 0 0 0 3.319-1.915",
        "M9 12a3 3 0 1 0 6 0a3 3 0 1 0-6 0",
    ];
    pub const PANEL_RIGHT: [&str; 2] = ["M5 3h14a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2z", "M15 3v18"];
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::{checkbox, chips, tree_json};
    use serde_json::json;

    fn props(node: Node) -> serde_json::Value {
        tree_json(node)["props"].clone()
    }

    #[test]
    fn the_button_props_by_name_set_the_props_the_app_reads() {
        let mut shown = props(crate::ui::button("Go", "ghost", false, || {}).size("sm").aria_label("Go on").tooltip("Go now").disabled(true));
        assert!(shown.as_object_mut().unwrap().remove("onClick").is_some());
        assert_eq!(shown, json!({ "variant": "ghost", "size": "sm", "label": "Go on", "title": "Go now", "disabled": true }));
        assert!(props(crate::ui::button("Go", "ghost", false, || {}).disabled(false)).get("disabled").is_none());
        assert_eq!(props(chips("Show", &[("a", "A")], "a", |_| {}).segmented())["segmented"], json!(true));
    }

    #[test]
    fn an_icon_button_is_a_ghost_button_with_one_16_point_icon_named_and_explained_by_its_label() {
        let tree = tree_json(icon_button("Refresh", &icons::PLUS, || {}));
        assert_eq!(tree["type"], "Button");
        let mut shown = tree["props"].as_object().unwrap().clone();
        assert!(shown.remove("onClick").is_some());
        assert_eq!(serde_json::Value::Object(shown), json!({ "variant": "ghost", "size": "icon-sm", "label": "Refresh", "title": "Refresh" }));
        assert_eq!(tree["children"].as_array().unwrap().len(), 1);
        let svg = &tree["children"][0];
        assert_eq!((&svg["type"], &svg["props"]["size"], &svg["props"]["viewBox"], &svg["props"]["strokeWidth"]), (&json!("Svg"), &json!(16), &json!("0 0 24 24"), &json!(2.0)));
        assert_eq!(svg["props"]["shapes"], json!([["path", { "d": "M5 12h14" }], ["path", { "d": "M12 5v14" }]]));
        let tipped = props(icon_button("Rescan", &icons::ROTATE_CW, || {}).tooltip("Scan again").disabled(true));
        assert_eq!((&tipped["label"], &tipped["title"], &tipped["disabled"]), (&json!("Rescan"), &json!("Scan again"), &json!(true)));
    }

    #[test]
    fn a_page_with_a_stacked_top_has_a_toolbar_slot_of_all_of_it_over_the_body_slot() {
        let tree = tree_json(page_with([text("a"), text("b")], [text("c")]));
        assert_eq!(tree["type"], "Page");
        assert_eq!(tree["children"][0]["props"]["name"], "toolbar");
        assert_eq!(tree["children"][0]["children"].as_array().unwrap().len(), 2);
        assert_eq!((&tree["children"][1]["props"]["name"], tree["children"][1]["children"].as_array().unwrap().len()), (&json!("body"), 1));
    }

    #[test]
    fn a_title_with_context_is_the_name_strong_and_the_context_small_and_muted() {
        let tree = tree_json(toolbar_titled(title_with_context("Pull requests", "wings"), Some(text("act"))));
        assert_eq!(tree["type"], "Toolbar");
        let title = &tree["children"][0];
        assert_eq!(title["props"]["name"], "title");
        let line = &title["children"][0];
        assert_eq!(line["props"], json!({ "gap": "m", "align": "center" }));
        assert_eq!(line["children"][0]["props"], json!({ "weight": "strong", "truncate": true, "title": "Pull requests" }));
        assert_eq!(line["children"][1]["props"], json!({ "tone": "muted", "size": "small", "truncate": true, "title": "wings" }));
        assert_eq!(tree["children"][1]["props"]["name"], "actions");
        let bare = tree_json(toolbar_titled(title_with_context("Pull requests", ""), None));
        assert_eq!(bare["children"].as_array().unwrap().len(), 1);
        assert_eq!(bare["children"][0]["children"][0]["children"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn facts_are_joined_by_muted_dots_and_a_section_label_is_small_and_medium() {
        let line = tree_json(joined(vec![text("a"), text("b"), text("c")]));
        assert_eq!(line["props"], json!({ "gap": "s", "wrap": true, "align": "center" }));
        let kinds: Vec<_> = line["children"].as_array().unwrap().iter().map(|child| child["children"][0].get("text").cloned().unwrap_or_else(|| child["children"][0]["text"].clone())).collect();
        assert_eq!(kinds, [json!("a"), json!("·"), json!("b"), json!("·"), json!("c")]);
        assert_eq!(props(section_label("Conversation · 3")), json!({ "tone": "muted", "size": "small", "weight": "medium" }));
    }

    #[test]
    fn an_error_state_is_a_card_of_a_title_a_danger_notice_and_a_primary_retry() {
        let tree = tree_json(error_state("Couldn't scan", "No access", Some(("Try again", Box::new(|| {})))));
        assert_eq!(tree["type"], "Card");
        let body = &tree["children"][0];
        let kinds: Vec<_> = body["children"].as_array().unwrap().iter().map(|child| child["type"].clone()).collect();
        assert_eq!(kinds, [json!("Text"), json!("Notice"), json!("Row")]);
        assert_eq!(body["children"][0]["props"], json!({ "weight": "strong" }));
        assert_eq!(body["children"][1]["props"], json!({ "tone": "danger" }));
        let retry = &body["children"][2]["children"][0];
        assert_eq!((&retry["type"], &retry["props"]["variant"], &retry["children"][0]["text"]), (&json!("Button"), &json!("default"), &json!("Try again")));
        let without = tree_json(error_state("Couldn't read", "Gone", None));
        assert_eq!(without["children"][0]["children"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn a_field_setting_puts_its_control_in_a_176_point_column_and_a_toggle_ends_in_a_named_switch() {
        let field = tree_json(setting_field("View", "Which.", text("menu")));
        let column = &field["children"][2]["children"][0];
        assert_eq!((&column["type"], &column["props"]["width"]), (&json!("Stack"), &json!(176)));
        let toggle = tree_json(setting_toggle("Keep local", "Why.", true, |_| {}));
        let switch = &toggle["children"][2]["children"][0];
        assert_eq!(switch["type"], "Checkbox");
        assert_eq!((&switch["props"]["switch"], &switch["props"]["label"], &switch["props"]["checked"]), (&json!(true), &json!("Keep local"), &json!(true)));
    }

    #[test]
    fn a_setting_is_a_list_row_with_its_control_in_the_trailing_slot() {
        let tree = tree_json(setting("Name", "What it does.", text("control")));
        assert_eq!(tree["type"], "ListRow");
        let names: Vec<_> = tree["children"].as_array().unwrap().iter().map(|slot| slot["props"]["name"].clone()).collect();
        assert_eq!(names, [json!("title"), json!("detail"), json!("trailing")]);
        assert_eq!(tree["children"][0]["children"][0]["props"], json!({ "weight": "medium" }));
        assert_eq!(tree["children"][1]["children"][0]["props"], json!({ "tone": "muted", "size": "small" }));
        let tick = tree_json(setting_tick(checkbox("Keep", true, |_| {}), "Why."));
        let names: Vec<_> = tick["children"].as_array().unwrap().iter().map(|slot| slot["props"]["name"].clone()).collect();
        assert_eq!(names, [json!("title"), json!("detail")]);
        assert_eq!(tick["children"][0]["children"][0]["type"], "Checkbox");
    }
}
