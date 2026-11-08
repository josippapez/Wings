//! The central plugin diff contract and parsed, browser-free patches.
pub mod comments;
#[cfg(feature = "ui")]
pub mod highlight;
pub mod patch;
pub mod rows;
#[cfg(feature = "ui")]
pub mod viewer;

use serde_json::Value;
use std::sync::Arc;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Update {
    pub patch: Option<Arc<patch::Diff>>,
    /// The review comments, all of them. None keeps those shown, as an update after a reply sends only `comments`.
    pub comments: Option<Arc<Vec<comments::Comment>>>,
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
    // `null` clears them, as the desktop's `cleanComments` does with what isn't a list.
    let comments = match params.get("comments") {
        None => None,
        Some(Value::Null) => Some(Arc::default()),
        Some(Value::Array(list)) => Some(Arc::new(comments::clean(list))),
        Some(_) => return Err("comments must be a list".into()),
    };
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
        comments,
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
        // Comments are bounded and kept, or left out when they aren't comments; anything but a list is refused.
        let comments = clean_update(&json!({ "comments": [{ "id": 1, "path": "a", "line": 2, "body": "x".repeat(30_000) }, { "body": "no id" }] }))
            .unwrap()
            .comments
            .unwrap();
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].body.len(), 20_000);
        assert!(clean_update(&json!({})).unwrap().comments.is_none());
        assert!(
            clean_update(&json!({ "comments": null }))
                .unwrap()
                .comments
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            clean_update(&json!({ "comments": "review" })),
            Err("comments must be a list".into())
        );
        assert!(
            clean_update(&json!({ "focus": { "path": "" } }))
                .unwrap()
                .focus
                .is_none()
        );
    }
}
