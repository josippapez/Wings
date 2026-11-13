//! The PR tab's conversation: comments grouped into threads, and what each thread is (a person's, resolved, a bot's).

use crate::checklist::iso_to_ms;
use crate::types::Comment;
use serde_json::{Value, json};
use std::collections::HashMap;
use wings_plugin::format::truncate;

#[derive(Clone, PartialEq, Debug)]
pub struct Thread {
    pub root: Comment,
    pub replies: Vec<Comment>,
}

fn time(iso: &str) -> f64 {
    iso_to_ms(iso).unwrap_or(0.0)
}

/// Written by a bot or by the provider itself: an app's `[bot]` login, a build service, a policy or system note.
pub fn is_automated(c: &Comment) -> bool {
    let author = c.author.trim().to_lowercase();
    let bot_word = author.strip_suffix("bot").is_some_and(|before| !before.chars().next_back().is_some_and(|c| c.is_alphanumeric() || c == '_'));
    c.system || author.ends_with("[bot]") || bot_word || author.contains("build service") || author.starts_with("github-actions") || author.starts_with("azure pipelines")
}

/// When a thread last had a comment: its newest reply, else its first comment.
pub fn last_activity(t: &Thread) -> f64 {
    time(&t.replies.last().unwrap_or(&t.root).created_at)
}

/// Comments grouped under their thread's first comment. The thread with the newest comment comes first; replies read oldest first.
pub fn threads_of(comments: &[Comment]) -> Vec<Thread> {
    let mut replies: HashMap<i64, Vec<Comment>> = HashMap::new();
    for c in comments {
        if let Some(to) = c.reply_to {
            replies.entry(to).or_default().push(c.clone());
        }
    }
    let mut threads: Vec<Thread> = comments
        .iter()
        .filter(|c| c.reply_to.is_none())
        .map(|root| {
            let mut own = replies.get(&root.id).cloned().unwrap_or_default();
            own.sort_by(|a, b| time(&a.created_at).total_cmp(&time(&b.created_at)));
            Thread { root: root.clone(), replies: own }
        })
        .collect();
    threads.sort_by(|a, b| last_activity(b).total_cmp(&last_activity(a)));
    threads
}

/// Where the diff viewer should look for a thread: its file, and its line on the side it was written on (`WingsDiffFocus`).
pub fn focus_of(root: &Comment) -> Option<Value> {
    let path = root.path.as_ref()?;
    Some(match root.line.filter(|line| *line != 0) {
        Some(line) => json!({ "path": path, "line": line, "side": if root.side == "deletions" { "old" } else { "new" } }),
        None => json!({ "path": path }),
    })
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Open,
    Resolved,
    Automated,
}

/// How a thread shows: resolved ones start collapsed; every other thread, a bot's too, reads in full. `Automated` only marks it.
pub fn kind_of(thread: &Thread) -> Kind {
    if thread.root.thread.as_ref().is_some_and(|t| t.state == "resolved") {
        Kind::Resolved
    } else if is_automated(&thread.root) {
        Kind::Automated
    } else {
        Kind::Open
    }
}

/// `text` with every `open`...`close` span (the first close after each open) replaced by `with`.
fn replace_spans(text: &str, open: &str, close: &str, with: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = rest.find(open) {
        let after = &rest[at + open.len()..];
        match after.find(close) {
            Some(end) => {
                out.push_str(&rest[..at]);
                out.push_str(with);
                rest = &after[end + close.len()..];
            }
            None => break,
        }
    }
    out.push_str(rest);
    out
}

/// A link reference definition, `[label]: target`, up to 3 spaces in.
fn is_definition(line: &str) -> bool {
    let rest = line.trim_start_matches(char::is_whitespace);
    if line.len() - rest.len() > 3 {
        return false;
    }
    let Some(inner) = rest.strip_prefix('[') else { return false };
    let Some(end) = inner.find(']') else { return false };
    (1..=200).contains(&inner[..end].chars().count()) && !inner[..end].contains('\n') && inner[end + 1..].starts_with(':')
}

/// `<tag ...>` and `</tag>`, which an HTML comment body can hold.
fn without_tags(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = rest.find('<') {
        out.push_str(&rest[..at]);
        let tag = &rest[at + 1..];
        let name = tag.strip_prefix('/').unwrap_or(tag);
        let end = name.find(['>', '\n']);
        match end {
            Some(end) if name.starts_with(|c: char| c.is_ascii_alphabetic()) && name[end..].starts_with('>') && name[1..end].chars().count() <= 200 => {
                out.push(' ');
                rest = &name[end + 1..];
            }
            _ => {
                out.push('<');
                rest = tag;
            }
        }
    }
    out.push_str(rest);
    out
}

/// `[text](target)` and `![alt](target)` as their text.
fn without_links(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = rest.find('[') {
        let start = if at > 0 && rest[..at].ends_with('!') { at - 1 } else { at };
        let inner = &rest[at + 1..];
        let link = inner.find(']').and_then(|close| inner[close + 1..].strip_prefix('(').and_then(|after| after.find(')').map(|end| (&inner[..close], &after[end + 1..]))));
        match link {
            Some((label, after)) => {
                out.push_str(&rest[..start]);
                out.push_str(label);
                rest = after;
            }
            None => {
                out.push_str(&rest[..at + 1]);
                rest = inner;
            }
        }
    }
    out.push_str(rest);
    out
}

/// A comment's text on one line, without Markdown marks, for a collapsed thread's summary.
pub fn one_line(body: &str, max: usize) -> String {
    let text = replace_spans(body, "```", "```", " code ");
    let text = text.split('\n').map(|line| if is_definition(line) { " " } else { line }).collect::<Vec<_>>().join("\n");
    let text = without_tags(&text);
    let text = text.split('\n').map(|line| if !line.is_empty() && line.chars().all(|c| c.is_whitespace() || matches!(c, '|' | ':' | '-')) { " " } else { line }).collect::<Vec<_>>().join("\n");
    let text: String = without_links(&text).chars().filter(|c| !matches!(c, '`' | '*' | '_' | '~' | '>' | '#' | '|')).collect();
    truncate(&text.split_whitespace().collect::<Vec<_>>().join(" "), max)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ThreadState;

    fn c(id: i64, reply_to: Option<i64>, created_at: &str) -> Comment {
        Comment { id, reply_to, path: None, line: None, side: "additions".into(), author: "Ada".into(), body: "x".into(), created_at: created_at.into(), url: String::new(), can_reply: None, by: None, discussion: None, thread: None, range: None, snippet: None, refs: None, system: false }
    }

    fn thread(root: Comment) -> Thread {
        Thread { root, replies: vec![] }
    }

    #[test]
    fn replies_go_under_their_first_comment_and_read_oldest_first_the_thread_with_the_newest_comment_comes_first() {
        let threads = threads_of(&[
            c(2, None, "2026-01-02T00:00:00Z"),
            c(3, Some(1), "2026-01-03T00:00:00Z"),
            c(1, None, "2026-01-01T00:00:00Z"),
            c(4, Some(1), "2026-01-02T12:00:00Z"),
            c(5, None, "2026-01-02T18:00:00Z"),
        ]);
        // Thread 1 started first but its reply on the 3rd is the newest comment, so a reply brings a thread back to the top.
        let shown: Vec<(i64, Vec<i64>)> = threads.iter().map(|t| (t.root.id, t.replies.iter().map(|r| r.id).collect())).collect();
        assert_eq!(shown, [(1, vec![4, 3]), (5, vec![]), (2, vec![])]);
    }

    #[test]
    fn a_thread_is_resolved_automated_or_open() {
        let with = |f: &dyn Fn(&mut Comment)| {
            let mut root = c(1, None, "2026-01-01T00:00:00Z");
            f(&mut root);
            thread(root)
        };
        let state = |s: &str| ThreadState { state: s.into(), label: s.into() };
        assert_eq!(kind_of(&with(&|_| {})), Kind::Open);
        assert_eq!(kind_of(&with(&|r| r.thread = Some(state("active")))), Kind::Open);
        assert_eq!(kind_of(&with(&|r| r.thread = Some(state("resolved")))), Kind::Resolved);
        assert_eq!(kind_of(&with(&|r| r.system = true)), Kind::Automated);
        assert_eq!(kind_of(&with(&|r| r.author = "release-helper[bot]".into())), Kind::Automated);
        assert_eq!(kind_of(&with(&|r| r.author = "Project Collection Build Service (Acme)".into())), Kind::Automated);
        // Resolved wins over automated, so a resolved bot thread is a resolved thread.
        assert_eq!(kind_of(&with(&|r| { r.author = "x[bot]".into(); r.thread = Some(state("resolved")) })), Kind::Resolved);
        assert!(!is_automated(&Comment { author: "Abbott".into(), ..c(1, None, "") }));
    }

    #[test]
    fn a_collapsed_threads_summary_is_one_plain_line() {
        assert_eq!(one_line("**Bold** and `code`\n\n- [link](https://example.com)", 140), "Bold and code - link");
        assert_eq!(one_line("```js\nlong\n```\nafter", 140), "code after");
        assert_eq!(one_line("## Report\n\n| File | Lines |\n|---|---|\n| a.ts | 9% |", 140), "Report File Lines a.ts 9%");
        assert_eq!(one_line(&"a".repeat(200), 10), "aaaaaaaaa…");
        assert_eq!(one_line("Logs the raw error.\n<details>\n<summary>Context</summary>\nWhy\n</details>\n\n[ISSUE_KEY]: AbC-1", 140), "Logs the raw error. Context Why");
    }

    #[test]
    fn view_in_diff_points_at_the_threads_file_and_the_line_on_its_side() {
        let at = |path: &str, line: Option<i64>, side: &str| Comment { path: Some(path.into()), line, side: side.into(), ..c(1, None, "t") };
        assert_eq!(focus_of(&at("src/a.ts", Some(12), "additions")), Some(json!({ "path": "src/a.ts", "line": 12, "side": "new" })));
        assert_eq!(focus_of(&at("src/a.ts", Some(3), "deletions")), Some(json!({ "path": "src/a.ts", "line": 3, "side": "old" })));
        assert_eq!(focus_of(&at("src/a.ts", None, "additions")), Some(json!({ "path": "src/a.ts" })));
        assert_eq!(focus_of(&c(1, None, "t")), None);
    }
}
