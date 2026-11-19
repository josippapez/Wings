//! The central plugin diff contract and parsed, browser-free patches.
pub mod comments;
#[cfg(feature = "ui")]
pub mod highlight;
pub mod patch;
pub mod rows;
pub mod stack;
#[cfg(feature = "ui")]
pub mod viewer;
pub mod worktree;

use serde_json::Value;
use std::sync::Arc;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Update {
    pub patch: Option<Arc<patch::Diff>>,
    /// The review comments, all of them. None keeps those shown, as an update after a reply sends only `comments`.
    pub comments: Option<Arc<Vec<comments::Comment>>>,
    pub error: Option<String>,
    pub focus: Option<Focus>,
    /// Which of `scopes` the patch is of. An update for a scope other than the one picked is dropped: it was on its way
    /// when another was picked.
    pub scope: Option<String>,
    /// What else the diff can show, like a repo's uncommitted changes and its branch's. The viewer offers them in a
    /// picker and asks the plugin's refresh handler for the one picked.
    pub scopes: Option<Arc<Vec<Scope>>>,
    /// The patch's new side is the files in the diff's `cwd` as they are now, so the viewer may read the unchanged lines
    /// its folds leave out from there.
    pub worktree: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Scope {
    pub id: String,
    pub label: String,
}

/// The most scopes a diff offers, and the longest id or label, in UTF-16 units.
const MAX_SCOPES: usize = 6;
const MAX_SCOPE_TEXT: usize = 60;

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
    /// The id of the plugin sidebar the diff was opened from: it shows in that sidebar's place while that sidebar is open.
    pub sidebar: Option<String>,
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
    let scope = params
        .get("scope")
        .and_then(Value::as_str)
        .map(|_| text(params.get("scope"), MAX_SCOPE_TEXT))
        .filter(|scope| !scope.is_empty());
    let scopes = match params.get("scopes") {
        None | Some(Value::Null) => None,
        Some(Value::Array(list)) => Some(Arc::new(
            list.iter()
                .map(|scope| Scope {
                    id: text(scope.get("id"), MAX_SCOPE_TEXT),
                    label: text(scope.get("label"), MAX_SCOPE_TEXT),
                })
                .filter(|scope| !scope.id.is_empty() && !scope.label.is_empty())
                .take(MAX_SCOPES)
                .collect(),
        )),
        Some(_) => return Err("scopes must be a list".into()),
    };
    Ok(Update {
        patch,
        comments,
        error,
        focus,
        scope,
        scopes,
        worktree: params.get("worktree").and_then(Value::as_bool),
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

    #[test]
    fn scopes_are_bounded_and_need_an_id_and_a_label() {
        let scope = |id: &str, label: &str| json!({ "id": id, "label": label });
        let mut list = vec![scope("uncommitted", "Uncommitted"), scope("", "No id"), json!("branch")];
        list.extend((0..10).map(|n| scope(&format!("s{n}"), "More")));
        let update = clean_update(&json!({ "scopes": list, "scope": "x".repeat(100), "worktree": true })).unwrap();
        let scopes = update.scopes.unwrap();
        assert_eq!(scopes.len(), MAX_SCOPES);
        assert_eq!(
            scopes[0],
            Scope {
                id: "uncommitted".into(),
                label: "Uncommitted".into()
            }
        );
        assert_eq!(update.scope.unwrap().len(), MAX_SCOPE_TEXT);
        assert_eq!(update.worktree, Some(true));
        assert!(clean_update(&json!({})).unwrap().scopes.is_none());
        assert_eq!(
            clean_update(&json!({ "scopes": "branch" })),
            Err("scopes must be a list".into())
        );
    }
}
