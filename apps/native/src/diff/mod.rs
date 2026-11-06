//! The central plugin diff contract and parsed, browser-free patches.
pub mod patch;
#[cfg(feature = "ui")]
pub mod viewer;

use serde_json::Value;
use std::sync::Arc;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Update {
    pub patch: Option<Arc<patch::Diff>>,
    pub error: Option<String>,
    pub focus: Option<Focus>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Focus {
    pub path: String,
    pub line: Option<usize>,
    pub old: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct View {
    pub id: String,
    pub plugin_id: String,
    pub title: String,
    pub subtitle: String,
    pub cwd: String,
    pub refresh: bool,
    pub update: Update,
}

pub fn text(value: Option<&Value>, max: usize) -> String {
    let mut units = 0;
    value
        .and_then(Value::as_str)
        .unwrap_or("")
        .chars()
        .take_while(|c| {
            units += c.len_utf16();
            units <= max
        })
        .collect()
}

pub fn clean_update(params: &Value) -> Result<Update, String> {
    if params
        .get("comments")
        .and_then(Value::as_array)
        .is_some_and(|comments| !comments.is_empty())
    {
        return Err("Native diffs don't support review comments yet".into());
    }
    let patch = params
        .get("patch")
        .and_then(Value::as_str)
        .map(patch::parse_patch)
        .transpose()
        .map_err(|e| e.to_string())?
        .map(Arc::new);
    let error = params
        .get("error")
        .and_then(Value::as_str)
        .map(|_| text(params.get("error"), 500));
    let focus = params.get("focus").and_then(|focus| {
        let path = focus.get("path")?.as_str()?;
        if path.is_empty() || path.encode_utf16().count() > 500 {
            return None;
        }
        Some(Focus {
            path: path.into(),
            line: focus
                .get("line")
                .and_then(Value::as_u64)
                .filter(|line| *line > 0)
                .and_then(|line| line.try_into().ok()),
            old: focus.get("side").and_then(Value::as_str) == Some("old"),
        })
    });
    Ok(Update {
        patch,
        error,
        focus,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn updates_are_bounded_parsed_and_keep_only_supported_fields() {
        let update = clean_update(&json!({ "error": "x".repeat(600), "focus": { "path": "a", "line": 2, "side": "old" }, "comments": [] })).unwrap();
        assert_eq!(update.error.unwrap().len(), 500);
        let focus = update.focus.unwrap();
        assert_eq!(
            (focus.path.as_str(), focus.line, focus.old),
            ("a", Some(2), true)
        );
        assert!(clean_update(&json!({ "patch": "not a patch" })).is_err());
        assert!(clean_update(&json!({ "patch": "x".repeat(patch::MAX_PATCH_BYTES + 1) })).is_err());
        assert!(clean_update(&json!({ "comments": [{ "body": "review" }] })).is_err());
        assert!(
            clean_update(&json!({ "focus": { "path": "" } }))
                .unwrap()
                .focus
                .is_none()
        );
    }
}
