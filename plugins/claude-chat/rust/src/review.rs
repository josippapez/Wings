//! Review comments: the notes you leave on lines in Wings' diff viewer, from the side panel's Review changes. The Changes
//! plugin sends each as the `review-comment` event, which this plugin's main passes on to the chat page (by the key the
//! page put in its `open-changes` event). The chat keeps them above the message box until you send your next message,
//! which carries them, each as `path:line`, the line quoted, and what you said.

use serde_json::{Value, json};
use std::cell::RefCell;

/// The event of the Changes plugin that carries a comment (`events` in the manifest), and what the main tells the chat
/// pages: the event's data.
pub const EVENT: &str = "review-comment";

/// The most comments a chat holds before the next message: more is a review sent in pieces.
pub const MAX: usize = 50;

thread_local! {
    static KEY: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// What names this chat page in the events it asks the Changes plugin for, so a comment finds its chat among the open
/// ones: made the first time it is needed.
pub fn key() -> String {
    KEY.with(|key| {
        key.borrow_mut()
            .get_or_insert_with(|| format!("chat-{:x}", (wings_plugin::host::now() * 1000.0) as u64))
            .clone()
    })
}

/// One comment on a line, as the viewer's author wrote it.
#[derive(Clone, Debug, PartialEq)]
pub struct Comment {
    /// The viewer's draft id with when that viewer opened: it names the comment for taking it back.
    pub id: String,
    pub path: String,
    /// On the line as it was before the change, else as it is after.
    pub old: bool,
    pub line: u64,
    /// The last line, for a comment on a range.
    pub end_line: Option<u64>,
    /// The text of the line, or of the lines.
    pub quote: String,
    pub body: String,
}

impl Comment {
    /// `path:line`, or `path:line-end` for a range, and where it counts from when that's the old side.
    pub fn place(&self) -> String {
        let range = self.end_line.map_or(String::new(), |end| format!("-{end}"));
        format!("{}:{}{range}{}", self.path, self.line, if self.old { " (old side)" } else { "" })
    }

    /// A chip's label: the file's name and the line.
    pub fn chip(&self) -> String {
        format!("{}:{}", self.path.rsplit('/').next().unwrap_or(&self.path), self.line)
    }

    fn json(&self) -> Value {
        let mut saved = json!({ "id": self.id, "path": self.path, "old": self.old, "line": self.line, "quote": self.quote, "body": self.body });
        if let Some(end) = self.end_line {
            saved["endLine"] = end.into();
        }
        saved
    }

    fn of(saved: &Value) -> Option<Self> {
        Some(Self {
            id: saved["id"].as_str()?.to_owned(),
            path: saved["path"].as_str().filter(|path| !path.is_empty())?.to_owned(),
            old: saved["old"].as_bool().unwrap_or(false) || saved["side"] == "old",
            line: saved["line"].as_u64().filter(|line| *line > 0)?,
            end_line: saved["endLine"].as_u64(),
            quote: saved["quote"].as_str().unwrap_or_default().to_owned(),
            body: saved["body"].as_str().filter(|body| !body.trim().is_empty())?.to_owned(),
        })
    }
}

/// What the tab keeps of the comments not sent yet.
pub fn to_json(comments: &[Comment]) -> Value {
    Value::Array(comments.iter().map(Comment::json).collect())
}

/// The comments a tab kept.
pub fn from_json(saved: &Value) -> Vec<Comment> {
    saved.as_array().into_iter().flatten().filter_map(Comment::of).take(MAX).collect()
}

/// A comment, or a comment taken back, that the Changes plugin sent for this chat (`data`), applied to `comments`. Whether
/// they changed. A comment with an id already there replaces it.
pub fn apply(comments: &mut Vec<Comment>, data: &Value) -> bool {
    let Some(id) = data["id"].as_str() else {
        return false;
    };
    if data["delete"] == true {
        let before = comments.len();
        comments.retain(|comment| comment.id != id);
        return comments.len() != before;
    }
    let Some(comment) = Comment::of(data) else {
        return false;
    };
    match comments.iter().position(|had| had.id == id) {
        Some(at) => comments[at] = comment,
        None if comments.len() < MAX => comments.push(comment),
        None => return false,
    }
    true
}

/// "2 comments on 2 files", "1 comment on the plan", or both.
pub fn summary(comments: &[Comment]) -> String {
    let mut files: Vec<&str> = comments.iter().filter(|comment| !crate::plan_doc::is_plan(&comment.path)).map(|comment| comment.path.as_str()).collect();
    files.sort_unstable();
    files.dedup();
    let mut plans: Vec<&str> = comments.iter().filter(|comment| crate::plan_doc::is_plan(&comment.path)).map(|comment| comment.path.as_str()).collect();
    plans.sort_unstable();
    plans.dedup();
    let many = |n: usize, one: &str| format!("{n} {one}{}", if n == 1 { "" } else { "s" });
    let mut on = Vec::new();
    if !files.is_empty() {
        on.push(many(files.len(), "file"));
    }
    match plans.len() {
        0 => {}
        1 => on.push("the plan".to_owned()),
        n => on.push(many(n, "plan")),
    }
    format!("{} on {}", many(comments.len(), "comment"), on.join(" and "))
}

/// A fence of backticks that the quoted text can't end early.
fn fence(quote: &str) -> String {
    let (mut run, mut longest) = (0, 0);
    for c in quote.chars() {
        run = if c == '`' { run + 1 } else { 0 };
        longest = longest.max(run);
    }
    "`".repeat((longest + 1).max(3))
}

/// One comment as the message gives it: its `path:line`, the quoted lines in a fenced block, then what was said.
fn note(comment: &Comment) -> String {
    let quote = comment.quote.trim_end_matches('\n');
    let quoted = if quote.is_empty() {
        String::new()
    } else {
        let fence = fence(quote);
        format!("{fence}\n{quote}\n{fence}\n")
    };
    format!("{}\n{quoted}{}", comment.place(), comment.body.trim())
}

/// `text` with the comments after it, those on the changes and then those on a plan, each under its own heading. Without
/// comments, `text` as it was.
pub fn message(text: &str, comments: &[Comment]) -> String {
    if comments.is_empty() {
        return text.to_owned();
    }
    let (plans, changes): (Vec<&Comment>, Vec<&Comment>) = comments.iter().partition(|comment| crate::plan_doc::is_plan(&comment.path));
    let group = |heading: &str, of: Vec<&Comment>| (!of.is_empty()).then(|| format!("{heading}\n\n{}", of.into_iter().map(note).collect::<Vec<_>>().join("\n\n")));
    let groups: Vec<String> = [group("Review comments on the changes:", changes), group("Comments on the plan (lines count from the plan's first):", plans)].into_iter().flatten().collect();
    let head = if text.trim().is_empty() { String::new() } else { format!("{}\n\n", text.trim()) };
    format!("{head}{}", groups.join("\n\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn comment(id: &str, path: &str, line: u64, body: &str) -> Comment {
        Comment { id: id.into(), path: path.into(), old: false, line, end_line: None, quote: "let a = 1;".into(), body: body.into() }
    }

    #[test]
    fn comments_go_after_the_message_each_with_its_place_the_quoted_line_and_what_was_said() {
        let list = [
            comment("1:-1", "src/a.rs", 12, "Why one?"),
            Comment { old: true, end_line: Some(5), quote: "one\ntwo".into(), ..comment("1:-2", "src/b.rs", 3, "Keep it.") },
        ];
        assert_eq!(
            message("Fix these", &list),
            "Fix these\n\nReview comments on the changes:\n\nsrc/a.rs:12\n```\nlet a = 1;\n```\nWhy one?\n\nsrc/b.rs:3-5 (old side)\n```\none\ntwo\n```\nKeep it."
        );
        // Sent alone, the comments are the message.
        assert!(message("  ", &list).starts_with("Review comments on the changes:\n\nsrc/a.rs:12"));
        // A quote with a fence in it gets a longer one; no quote, no block.
        let fenced = Comment { quote: "a ```b``` c".into(), ..comment("1:-3", "a", 1, "x") };
        assert!(message("", &[fenced]).contains("````\na ```b``` c\n````\n"));
        let bare = Comment { quote: String::new(), ..comment("1:-4", "a", 1, "x") };
        assert!(message("", &[bare]).ends_with("a:1\nx"));
    }

    #[test]
    fn nothing_is_added_when_there_are_no_comments() {
        assert_eq!(message("Just this", &[]), "Just this");
        assert_eq!(message("", &[]), "");
    }

    #[test]
    fn a_comment_event_adds_one_a_repeat_replaces_it_and_a_delete_removes_it() {
        let mut list = Vec::new();
        let event = |id: &str, body: &str| json!({ "chat": "c", "path": "src/a.rs", "side": "old", "line": 4, "quote": "q", "body": body, "id": id });
        assert!(apply(&mut list, &event("7:-1", "First")));
        assert!(apply(&mut list, &event("7:-2", "Second")));
        assert!(apply(&mut list, &event("7:-1", "First, again")));
        assert_eq!(list.iter().map(|c| c.body.as_str()).collect::<Vec<_>>(), ["First, again", "Second"]);
        assert!(list[0].old);
        // The same draft number from another viewer is another comment.
        assert!(apply(&mut list, &event("9:-1", "Elsewhere")));
        assert_eq!(list.len(), 3);
        assert!(apply(&mut list, &json!({ "chat": "c", "delete": true, "id": "7:-2" })));
        assert!(!apply(&mut list, &json!({ "chat": "c", "delete": true, "id": "7:-2" })));
        assert_eq!(list.len(), 2);
        // What isn't a comment changes nothing.
        assert!(!apply(&mut list, &json!({ "path": "a", "line": 1, "body": "no id" })));
        assert!(!apply(&mut list, &json!({ "id": "1:1", "path": "a", "line": 0, "body": "no line" })));
        assert!(!apply(&mut list, &json!({ "id": "1:2", "path": "a", "line": 1, "body": "  " })));
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn the_summary_counts_comments_and_the_files_they_are_on_and_a_tab_keeps_them() {
        let list = [comment("1:1", "a.rs", 1, "x"), comment("1:2", "a.rs", 9, "y"), comment("1:3", "b.rs", 2, "z")];
        assert_eq!(summary(&list), "3 comments on 2 files");
        assert_eq!(summary(&list[..1]), "1 comment on 1 file");
        assert_eq!(list[0].chip(), "a.rs:1");
        assert_eq!(comment("1:1", "src/deep/a.rs", 8, "x").chip(), "a.rs:8");
        let kept = from_json(&to_json(&list));
        assert_eq!(kept, list);
        assert!(from_json(&Value::Null).is_empty());
        assert_eq!(from_json(&json!([{ "id": "1:1" }, 3])).len(), 0);
    }

    #[test]
    fn comments_on_a_plan_go_under_their_own_heading_after_those_on_the_changes() {
        let plan = Comment { quote: "2. Add tests".into(), ..comment("plan-1-5-9", "plan #1", 5, "Which tests?") };
        let list = [plan.clone(), comment("1:-1", "src/a.rs", 12, "Why one?")];
        assert_eq!(
            message("Go on", &list),
            "Go on\n\nReview comments on the changes:\n\nsrc/a.rs:12\n```\nlet a = 1;\n```\nWhy one?\n\nComments on the plan (lines count from the plan's first):\n\nplan #1:5\n```\n2. Add tests\n```\nWhich tests?"
        );
        assert!(message("", &[plan.clone()]).starts_with("Comments on the plan"));
        assert_eq!(summary(&[plan.clone()]), "1 comment on the plan");
        assert_eq!(summary(&list), "2 comments on 1 file and the plan");
        let second = Comment { path: "plan #2".into(), ..plan.clone() };
        assert_eq!(summary(&[plan, second]), "2 comments on 2 plans");
    }
}
