//! An extension the person is asked about before it is added, which the browser tab (from a store page) and the
//! settings page (from a folder, a ZIP or another browser) both show.

use serde_json::Value;
use wings_plugin::ui::{Node, actions, button, dialog, image, muted, notice, row, section_label, small, stack, text, title};

/// What `webExtensions` `stage` answers: the extension unpacked but not installed, and what it asks for.
#[derive(Clone, Debug, PartialEq)]
pub struct Staged {
    pub stage: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub icon: Option<String>,
    pub permissions: Vec<String>,
    pub hosts: Vec<String>,
}

pub fn strings(value: &Value) -> Vec<String> {
    value.as_array().map(|list| list.iter().filter_map(|item| item.as_str().map(str::to_owned)).collect()).unwrap_or_default()
}

impl Staged {
    /// None for a cancelled pick (null) and for an answer with no `stage` to add or discard.
    pub fn from_json(answer: &Value) -> Option<Self> {
        Some(Self {
            stage: answer["stage"].as_str()?.to_owned(),
            name: answer["name"].as_str().unwrap_or("Extension").to_owned(),
            version: answer["version"].as_str().unwrap_or_default().to_owned(),
            description: answer["description"].as_str().unwrap_or_default().to_owned(),
            icon: answer["icon"].as_str().map(str::to_owned),
            permissions: strings(&answer["permissions"]),
            hosts: strings(&answer["hosts"]),
        })
    }
}

/// The dialog that lists what the extension asks for, with Cancel and Add. `error` is a failed add, shown in it.
pub fn dialog_for(staged: &Staged, busy: bool, error: Option<&str>, on_cancel: impl Fn() + Clone + 'static, on_add: impl Fn() + 'static) -> Node {
    let lines = |lines: &[String]| stack().prop("gap", "xs").children(lines.iter().enumerate().map(|(at, line)| small(line.as_str()).prop("mono", true).key(format!("line{at}"))));
    let header = row()
        .prop("gap", "m")
        .prop("align", "center")
        .maybe(staged.icon.as_deref().map(|icon| image(icon, &staged.name, 32, 32)))
        .child(stack().prop("gap", "xs").child(title(staged.name.as_str())).maybe((!staged.version.is_empty()).then(|| muted(format!("Version {}", staged.version)).prop("size", "small"))));
    let asks = stack()
        .prop("gap", "s")
        .maybe((!staged.permissions.is_empty()).then(|| stack().prop("gap", "xs").child(section_label("Permissions")).child(lines(&staged.permissions))))
        .maybe((!staged.hosts.is_empty()).then(|| stack().prop("gap", "xs").child(text("Can read and change your data on:")).child(lines(&staged.hosts))))
        .maybe((staged.permissions.is_empty() && staged.hosts.is_empty()).then(|| muted("It doesn't ask for any permissions.").prop("size", "small")));
    let cancel = on_cancel.clone();
    dialog(&format!("Add {}?", staged.name), 480, on_cancel)
        .child(stack().prop("gap", "l").child(header).maybe((!staged.description.is_empty()).then(|| muted(staged.description.as_str()).prop("size", "small"))).child(asks).maybe(error.map(|error| notice("danger", error))))
        .child(actions(Some(button("Cancel", "ghost", false, cancel).prop("disabled", busy)), [button("Add", "default", busy, on_add)]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_staged_extension_is_read_from_the_answer_and_a_cancelled_pick_is_none() {
        let answer = json!({ "stage": "s1", "name": "Dark", "version": "2.0", "description": "Dims pages", "icon": "img1", "permissions": ["tabs", 5], "hosts": ["*://*.a.com/*"] });
        assert_eq!(
            Staged::from_json(&answer),
            Some(Staged { stage: "s1".into(), name: "Dark".into(), version: "2.0".into(), description: "Dims pages".into(), icon: Some("img1".into()), permissions: vec!["tabs".into()], hosts: vec!["*://*.a.com/*".into()] })
        );
        assert_eq!(Staged::from_json(&Value::Null), None);
        assert_eq!(Staged::from_json(&json!({ "stage": "s2" })).map(|s| (s.name, s.icon)), Some(("Extension".to_owned(), None)));
    }
}
