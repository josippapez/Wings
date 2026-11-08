//! The review comments a plugin's diff carries, as the desktop's `DiffComment`: bounded where they come in, as
//! `plugins.ts`'s `cleanComments` bounds them, and grouped into threads as `diff-viewer.tsx`'s `threadsByPath` groups them.

use serde_json::Value;
use std::collections::{HashMap, HashSet};

use super::text;

/// The most comments a diff keeps.
pub const MAX_COMMENTS: usize = 1000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Comment {
    pub id: i64,
    /// The comment this one answers.
    pub reply_to: Option<i64>,
    /// None for a comment on the whole change.
    pub path: Option<String>,
    /// None when the comment no longer matches the diff: the code changed since.
    pub line: Option<usize>,
    /// On the removed side (`deletions`), else on the added one.
    pub old: bool,
    pub author: String,
    /// Markdown.
    pub body: String,
    pub created_at: String,
    /// An https link to the comment, or nothing.
    pub url: String,
    /// On a thread's first comment: the viewer offers a reply box, and sends replies to the plugin's `onReply`.
    pub can_reply: bool,
}

/// What the desktop keeps of a plugin's comments: the first 1000, each with a whole-number id and a path of 1 to 500
/// characters or none, its texts cut to the desktop's lengths and its link only when it is https. Anything else
/// in the list is left out.
pub fn clean(raw: &[Value]) -> Vec<Comment> {
    raw.iter()
        .take(MAX_COMMENTS)
        .filter_map(|c| {
            let id = whole(c.get("id"))?;
            let path = match c.get("path") {
                Some(Value::Null) => None,
                Some(path @ Value::String(s)) if !s.is_empty() => Some(text(Some(path), 500)),
                _ => return None,
            };
            let url = text(c.get("url"), 500);
            Some(Comment {
                id,
                reply_to: whole(c.get("replyTo")),
                path,
                line: whole(c.get("line"))
                    .filter(|line| *line > 0)
                    .and_then(|line| line.try_into().ok()),
                old: c.get("side").and_then(Value::as_str) == Some("deletions"),
                author: c
                    .get("author")
                    .filter(|author| author.is_string())
                    .map_or_else(|| "unknown".into(), |author| text(Some(author), 80)),
                body: text(c.get("body"), 20_000),
                created_at: text(c.get("createdAt"), 40),
                url: if url.starts_with("https://") {
                    url
                } else {
                    String::new()
                },
                can_reply: c.get("canReply") == Some(&Value::Bool(true)),
            })
        })
        .collect()
}

/// A JSON number that is a whole number a JavaScript number holds exactly.
fn whole(value: Option<&Value>) -> Option<i64> {
    const SAFE: f64 = 9_007_199_254_740_991.0;
    let number = value?.as_number()?;
    number.as_i64().or_else(|| {
        number
            .as_f64()
            .filter(|n| n.fract() == 0.0 && n.abs() <= SAFE)
            .map(|n| n as i64)
    })
}

/// A comment and the replies to it, oldest first, by their places in the list of comments.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Thread {
    pub root: usize,
    pub replies: Vec<usize>,
}

/// The desktop's `threadsByPath`, oldest first: a comment answering one of the others goes under it, and one answering a
/// comment that isn't there starts a thread of its own.
pub fn threads(comments: &[Comment]) -> Vec<Thread> {
    let ids: HashSet<i64> = comments.iter().map(|c| c.id).collect();
    let mut order: Vec<usize> = (0..comments.len()).collect();
    order.sort_by(|&a, &b| comments[a].created_at.cmp(&comments[b].created_at));
    let mut threads = Vec::new();
    let mut roots = HashMap::new();
    for &at in &order {
        if comments[at].reply_to.is_some_and(|to| ids.contains(&to)) {
            continue;
        }
        roots.insert(comments[at].id, threads.len());
        threads.push(Thread {
            root: at,
            replies: Vec::new(),
        });
    }
    for &at in &order {
        if let Some(to) = comments[at].reply_to
            && let Some(&thread) = roots.get(&to)
        {
            threads[thread].replies.push(at);
        }
    }
    threads
}

/// The threads with no line to sit on, by their places in `threads`: on the whole change, and on code that changed since.
/// The one with the newest comment first.
pub fn discussion(comments: &[Comment], threads: &[Thread]) -> Vec<usize> {
    let root = |t: &Thread| &comments[t.root];
    let mut list: Vec<usize> = (0..threads.len())
        .filter(|&t| root(&threads[t]).path.is_none())
        .chain((0..threads.len()).filter(|&t| {
            let root = root(&threads[t]);
            root.path.is_some() && root.line.is_none()
        }))
        .collect();
    let newest = |t: usize| {
        let thread = &threads[t];
        &comments[*thread.replies.last().unwrap_or(&thread.root)].created_at
    };
    list.sort_by(|&a, &b| newest(b).cmp(newest(a)));
    list
}

/// The threads on lines of the file at `path`, by side (true for the removed one) and line.
pub fn on_lines(
    comments: &[Comment],
    threads: &[Thread],
    path: &str,
) -> HashMap<(bool, usize), Vec<usize>> {
    let mut lines: HashMap<_, Vec<_>> = HashMap::new();
    for (t, thread) in threads.iter().enumerate() {
        let root = &comments[thread.root];
        if root.path.as_deref() == Some(path)
            && let Some(line) = root.line
        {
            lines.entry((root.old, line)).or_default().push(t);
        }
    }
    lines
}

/// How many comments, replies too, are on the file at `path`, as the file list counts them.
pub fn on_file(comments: &[Comment], threads: &[Thread], path: &str) -> usize {
    threads
        .iter()
        .filter(|t| comments[t.root].path.as_deref() == Some(path))
        .map(|t| 1 + t.replies.len())
        .sum()
}

/// The first comments of threads on code that changed since, which have no line any more.
pub fn outdated(comments: &[Comment]) -> usize {
    comments
        .iter()
        .filter(|c| c.path.is_some() && c.line.is_none() && c.reply_to.is_none())
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn comment(
        id: i64,
        reply_to: Option<i64>,
        path: Option<&str>,
        line: Option<usize>,
        at: &str,
    ) -> Comment {
        Comment {
            id,
            reply_to,
            path: path.map(Into::into),
            line,
            old: false,
            author: "a".into(),
            body: String::new(),
            created_at: at.into(),
            url: String::new(),
            can_reply: true,
        }
    }

    #[test]
    fn bad_comments_are_left_out_and_the_rest_are_bounded() {
        let kept = clean(&[
            json!({ "id": 1, "replyTo": null, "path": "src/a.rs", "line": 3, "side": "deletions", "author": "maria",
                "body": "x".repeat(20_001), "createdAt": "2026-11-01T10:00:00Z", "url": "https://github.com/a/b/pull/1", "canReply": true }),
            // No id, a fractional one, an empty path, a path that isn't text, and not a comment at all.
            json!({ "path": "src/a.rs" }),
            json!({ "id": 1.5, "path": null }),
            json!({ "id": 2, "path": "" }),
            json!({ "id": 3, "path": 7 }),
            json!("comment"),
            // A comment on the whole change, with an author too long, a link that isn't https, and lines that aren't any.
            json!({ "id": -4, "replyTo": 1.5, "path": null, "line": 0, "author": "é".repeat(100), "url": "javascript:alert(1)",
                "canReply": "yes" }),
            json!({ "id": 5.0, "path": "p".repeat(600), "line": -2, "author": 12 }),
        ]);
        assert_eq!(kept.len(), 3);
        let first = &kept[0];
        assert_eq!(
            (
                first.id,
                first.path.as_deref(),
                first.line,
                first.old,
                first.can_reply
            ),
            (1, Some("src/a.rs"), Some(3), true, true)
        );
        assert_eq!(first.body.len(), 20_000);
        assert_eq!(first.url, "https://github.com/a/b/pull/1");
        let whole = &kept[1];
        assert_eq!(
            (whole.id, whole.reply_to, whole.path.as_ref(), whole.line),
            (-4, None, None, None)
        );
        assert_eq!(whole.author.chars().count(), 80);
        assert!(whole.url.is_empty() && !whole.can_reply && !whole.old);
        let long = &kept[2];
        assert_eq!(
            (long.id, long.line, long.author.as_str()),
            (5, None, "unknown")
        );
        assert_eq!(long.path.as_ref().unwrap().len(), 500);
        // The first 1000 are read, valid or not.
        let many: Vec<_> = (0..1500)
            .map(|id| json!({ "id": id, "path": null }))
            .collect();
        assert_eq!(clean(&many).len(), MAX_COMMENTS);
    }

    #[test]
    fn replies_go_under_the_comment_they_answer_oldest_first() {
        let comments = [
            comment(3, Some(1), Some("a"), Some(2), "2026-11-01T12:00:00Z"),
            comment(1, None, Some("a"), Some(2), "2026-11-01T10:00:00Z"),
            comment(2, Some(1), Some("a"), Some(2), "2026-11-01T11:00:00Z"),
            // Answers a comment that isn't there, so it starts a thread.
            comment(4, Some(99), None, None, "2026-11-01T09:00:00Z"),
            // Answers a reply, which only a thread's first comment takes, so it shows nowhere.
            comment(5, Some(2), Some("a"), Some(2), "2026-11-01T13:00:00Z"),
        ];
        assert_eq!(
            threads(&comments),
            [
                Thread {
                    root: 3,
                    replies: vec![]
                },
                Thread {
                    root: 1,
                    replies: vec![2, 0]
                },
            ]
        );
    }

    #[test]
    fn the_discussion_has_the_comments_with_no_line_newest_first_and_files_count_theirs() {
        let comments = [
            comment(1, None, None, None, "2026-11-01T10:00:00Z"),
            comment(2, None, Some("a.rs"), None, "2026-11-01T11:00:00Z"),
            comment(3, None, Some("a.rs"), Some(4), "2026-11-01T12:00:00Z"),
            comment(4, Some(1), None, None, "2026-11-01T13:00:00Z"),
            comment(5, Some(3), Some("a.rs"), Some(4), "2026-11-01T14:00:00Z"),
        ];
        let threads = threads(&comments);
        // The comment on the whole change has the newest reply.
        let roots: Vec<i64> = discussion(&comments, &threads)
            .iter()
            .map(|&t| comments[threads[t].root].id)
            .collect();
        assert_eq!(roots, [1, 2]);
        let lines = on_lines(&comments, &threads, "a.rs");
        assert_eq!(lines.len(), 1);
        assert_eq!(comments[threads[lines[&(false, 4)][0]].root].id, 3);
        assert_eq!(on_file(&comments, &threads, "a.rs"), 3);
        assert_eq!(on_file(&comments, &threads, "b.rs"), 0);
        // Only the one on code that changed: the comment on the whole change was never on code.
        assert_eq!(outdated(&comments), 1);
    }
}
