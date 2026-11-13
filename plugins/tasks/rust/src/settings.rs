//! The plugin's settings, kept in `wings.storage` under the keys the TypeScript build used, and told to the other frames
//! when they change (`setting:<key>`, which they read again).

use crate::kv;
use serde_json::{Value, json};
use wings_plugin::messages;

fn read(key: &str) -> Value {
    kv::get(key).unwrap_or(Value::Null)
}

fn save(key: &str, value: &Value) -> Result<(), String> {
    kv::set(key, value)?;
    messages::tell(&format!("setting:{key}"), &Value::Null);
    Ok(())
}

/// Hears a setting changed by another frame, and when it has, reads it again.
pub fn on_change(key: &str, listener: impl Fn() + 'static) {
    messages::on(&format!("setting:{key}"), move |_| listener());
}

// ---------- Keep issues local: on by default. Saved by the settings page, read by the commit hook. ----------

pub const KEEP_LOCAL: &str = "keepLocal";

pub fn keep_local() -> bool {
    read(KEEP_LOCAL).as_bool().unwrap_or(true)
}

pub fn set_keep_local(on: bool) -> Result<(), String> {
    save(KEEP_LOCAL, &json!(on))
}

// ---------- Sidebar order: how the project section sorts the issues no agent is working on. ----------

pub const SECTION_ORDER: &str = "sectionOrder";
pub const SECTION_ORDERS: [(&str, &str); 2] = [("status", "Status"), ("date", "Latest change")];

pub fn section_order() -> String {
    read(SECTION_ORDER).as_str().filter(|saved| SECTION_ORDERS.iter().any(|(value, _)| value == saved)).unwrap_or("status").to_owned()
}

pub fn set_section_order(order: &str) -> Result<(), String> {
    save(SECTION_ORDER, &json!(order))
}

// ---------- Default view: which layout the Issues tab opens in. ----------

pub const DEFAULT_VIEW: &str = "defaultView";
pub const DEFAULT_VIEWS: [(&str, &str); 3] = [("board", "Board"), ("rows", "Board by epic"), ("list", "List")];

pub fn default_view() -> String {
    read(DEFAULT_VIEW).as_str().filter(|saved| DEFAULT_VIEWS.iter().any(|(value, _)| value == saved)).unwrap_or("board").to_owned()
}

pub fn set_default_view(view: &str) -> Result<(), String> {
    save(DEFAULT_VIEW, &json!(view))
}

/// How the tab shows its issues: columns of cards or a list, and whether the board has a row per epic or feature.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Layout {
    pub list: bool,
    pub by_parent: bool,
}

/// How the tab starts out: rows are the board grouped by epic or feature, the list is grouped by status.
pub fn start_layout(view: &str) -> Layout {
    Layout { list: view == "list", by_parent: view == "rows" }
}

pub const LAST_LAYOUT: &str = "lastLayout";

/// The layout and rows choice last picked in the tab, so a reopened tab starts as it was left. Cleared when the Default
/// view changes.
pub fn last_layout() -> Option<Layout> {
    let saved = read(LAST_LAYOUT);
    let list = match saved["layout"].as_str()? {
        "board" => false,
        "list" => true,
        _ => return None,
    };
    Some(Layout { list, by_parent: saved["byParent"].as_bool()? })
}

pub fn set_last_layout(layout: Option<Layout>) -> Result<(), String> {
    save(LAST_LAYOUT, &layout.map_or(Value::Null, |l| json!({ "layout": if l.list { "list" } else { "board" }, "byParent": l.by_parent })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kv::memory;

    #[test]
    fn a_setting_reads_as_its_default_when_unset_or_not_a_value_it_takes_and_the_view_starts_the_layout() {
        memory::reset(&[]);
        assert!(keep_local());
        assert_eq!((section_order().as_str(), default_view().as_str(), last_layout()), ("status", "board", None));
        memory::reset(&[("keepLocal", json!("yes")), ("sectionOrder", json!("bogus")), ("defaultView", json!("grid")), ("lastLayout", json!({ "layout": "wide", "byParent": true }))]);
        assert!(keep_local());
        assert_eq!((section_order().as_str(), default_view().as_str(), last_layout()), ("status", "board", None));
        set_keep_local(false).unwrap();
        set_section_order("date").unwrap();
        assert!(!keep_local());
        assert_eq!(section_order(), "date");
        assert_eq!(memory::peek("sectionOrder"), Some(json!("date")));
        assert_eq!((start_layout("board"), start_layout("rows"), start_layout("list")), (Layout { list: false, by_parent: false }, Layout { list: false, by_parent: true }, Layout { list: true, by_parent: false }));
    }

    #[test]
    fn the_last_layout_is_kept_as_the_typescript_build_kept_it_and_cleared_with_null() {
        memory::reset(&[]);
        set_last_layout(Some(Layout { list: true, by_parent: false })).unwrap();
        assert_eq!(memory::peek("lastLayout"), Some(json!({ "layout": "list", "byParent": false })));
        assert_eq!(last_layout(), Some(Layout { list: true, by_parent: false }));
        set_last_layout(None).unwrap();
        assert_eq!(last_layout(), None);
    }
}
