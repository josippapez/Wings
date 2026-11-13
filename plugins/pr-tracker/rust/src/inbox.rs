//! Notifications about comments that mention you or reply in a thread you're in, on every PR the lists show: which
//! comments count and what is remembered. The check itself (`scan`) and the `notify-mentions` setting are the main
//! script's.

use crate::forges::shared::same;
use crate::threads::one_line;
use crate::types::{Comment, Me};
use crate::watch::Notification;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashSet};

/// The storage key of the setting the Settings page saves, next to the new pull request notifications.
pub const MENTIONS_KEY: &str = "notify-mentions";

/// The comment keys kept, newest last. A PR's comments are only looked at once the PR changes, so this only has to span a few checks.
pub const MAX_SEEN: usize = 400;
/// The PRs whose comments one check fetches. The ones checked longest ago go first, so a repo with more is covered over a few checks.
pub const MAX_FETCH: usize = 15;
/// More comments than this at once are one notification, so a burst stays under the host's 20 a minute.
pub const MAX_NOTES: usize = 3;

/// What is kept in `wings.storage`: when each PR was last looked at, and the comments already notified.
#[derive(Clone, Default, PartialEq, Debug, Serialize, Deserialize)]
pub struct Inbox {
    /// PR url → what it was last checked at: its `updatedAt` from the list, or the time of the check where the list has none. Comments before this are old news.
    pub marks: BTreeMap<String, String>,
    pub seen: Vec<String>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HitKind {
    Mention,
    Reply,
}

#[derive(Clone, PartialEq, Debug)]
pub struct Hit {
    pub kind: HitKind,
    pub key: String,
    /// The PR it is on.
    pub url: String,
    pub comment: Comment,
}

/// A PR of a list, as a check looks at it.
pub struct Item {
    pub url: String,
    pub updated_at: Option<String>,
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Whether a comment's text writes one of your handles as `@handle`. GitHub, GitLab and Codeberg write `@login`, Bitbucket may wrap the id in `{}` and Azure DevOps in `<>`.
pub fn mentions(body: &str, handles: &[String]) -> bool {
    let body = body.to_lowercase();
    handles.iter().any(|handle| {
        let handle = handle.to_lowercase();
        body.match_indices('@').any(|(at, _)| {
            if body[..at].chars().next_back().is_some_and(|c| is_word(c) || c == '@') {
                return false;
            }
            let rest = &body[at + 1..];
            let opened: Vec<&str> = if rest.starts_with(['{', '<']) { vec![&rest[1..], rest] } else { vec![rest] };
            opened.into_iter().filter_map(|r| r.strip_prefix(handle.as_str())).any(|after| {
                let closed: Vec<&str> = if after.starts_with(['}', '>']) { vec![&after[1..], after] } else { vec![after] };
                closed.into_iter().any(|r| !r.chars().next().is_some_and(|c| is_word(c) || c == '-'))
            })
        })
    })
}

/// The hits among `comments` that are newer than `after` (ms), skipping yours and the provider's own: a mention, or any other comment in a thread you wrote in.
pub fn hits_in(comments: &[Comment], me: &Me, after: f64, url: &str) -> Vec<Hit> {
    let mine = |c: &Comment| c.by.as_deref().is_some_and(|by| me.ids.iter().any(|id| same(Some(id), by)));
    let threads: HashSet<i64> = comments.iter().filter(|c| mine(c)).map(|c| c.reply_to.unwrap_or(c.id)).collect();
    let mut hits = Vec::new();
    for c in comments {
        if c.system || mine(c) || !crate::checklist::iso_to_ms(&c.created_at).is_some_and(|at| at > after) {
            continue;
        }
        let kind = if mentions(&c.body, &me.handles) {
            Some(HitKind::Mention)
        } else if threads.contains(&c.reply_to.unwrap_or(c.id)) {
            Some(HitKind::Reply)
        } else {
            None
        };
        if let Some(kind) = kind {
            hits.push(Hit { kind, key: key_of(url, c), url: url.to_owned(), comment: c.clone() });
        }
    }
    hits
}

/// A short hash of the text, so the keys stay small.
fn short(text: &str) -> String {
    let mut h: u32 = 5381;
    for unit in text.encode_utf16() {
        h = h.wrapping_mul(33) ^ u32::from(unit);
    }
    to_base36(h)
}

fn to_base36(mut n: u32) -> String {
    if n == 0 {
        return "0".into();
    }
    let mut digits = Vec::new();
    while n > 0 {
        digits.push(char::from_digit(n % 36, 36).unwrap_or('0'));
        n /= 36;
    }
    digits.iter().rev().collect()
}

/// What names a comment across checks. A GitHub conversation comment's id counts its place in the list and moves when one is added, so the id alone can't.
pub fn key_of(url: &str, c: &Comment) -> String {
    format!("{}:{}:{}", short(url), c.created_at, short(&format!("{}{}", c.by.as_deref().unwrap_or(&c.author), c.body.encode_utf16().count())))
}

/// A hit with what the notification says of its PR.
pub struct Note {
    pub hit: Hit,
    pub title: String,
    /// What the "pr" view is opened with.
    pub args: Value,
}

fn word(kind: HitKind) -> &'static str {
    match kind {
        HitKind::Mention => "mentioned you",
        HitKind::Reply => "replied",
    }
}

/// One notification per hit, with buttons to open the PR in a tab or the browser, or one for all of them when there are more than `MAX_NOTES`.
pub fn notes_for(project: &str, notes: &[Note]) -> Vec<Notification> {
    if notes.len() > MAX_NOTES {
        return vec![Notification {
            title: format!("{} new comments for you in {project}", notes.len()),
            body: notes.iter().map(|n| format!("{} {} on {}", n.hit.comment.author, word(n.hit.kind), n.title)).collect::<Vec<_>>().join("\n"),
            actions: vec![json!({ "label": "All pull requests", "open": { "view": "pr" } })],
        }];
    }
    notes
        .iter()
        .map(|n| Notification {
            title: format!("{} {} in {project}", n.hit.comment.author, word(n.hit.kind)),
            body: format!("{}\n{}", n.title, one_line(&n.hit.comment.body, 160)),
            actions: vec![json!({ "label": "Open", "open": { "view": "pr", "args": n.args } }), json!({ "label": "Open in browser", "url": n.hit.url })],
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const URL: &str = "https://host/team/repo/pull/7";

    fn me() -> Me {
        Me { ids: vec!["ana".into()], handles: vec!["ana".into()] }
    }

    fn comment(id: i64, by: &str, body: &str, created_at: &str, reply_to: Option<i64>, system: bool) -> Comment {
        Comment { id, reply_to, path: None, line: None, side: "additions".into(), author: by.into(), body: body.into(), created_at: created_at.into(), url: URL.into(), can_reply: None, by: Some(by.into()), discussion: None, thread: None, range: None, snippet: None, refs: None, system }
    }

    fn handles(names: &[&str]) -> Vec<String> {
        names.iter().map(|n| (*n).to_owned()).collect()
    }

    #[test]
    fn a_handle_is_a_mention_only_as_a_whole_at_name() {
        assert!(mentions("can you look, @Ana?", &handles(&["ana"])));
        assert!(mentions("@{abc-123} hi", &handles(&["abc-123"])));
        assert!(!mentions("@anathema and ana@host.com", &handles(&["ana"])));
    }

    #[test]
    fn a_new_comment_in_a_thread_you_wrote_in_is_a_reply_yours_and_the_providers_are_not() {
        let comments = [
            comment(1, "ana", "why this?", "2026-10-16T08:00:00Z", None, false),
            comment(2, "bob", "because of X", "2026-10-16T09:00:00Z", Some(1), false),
            comment(3, "ana", "thanks", "2026-10-16T09:01:00Z", Some(1), false),
            comment(4, "bob", "Merged main", "2026-10-16T09:02:00Z", Some(1), true),
            comment(5, "carl", "unrelated", "2026-10-16T09:03:00Z", None, false),
            comment(6, "bob", "old news", "2026-10-15T09:03:00Z", Some(1), false),
        ];
        let after = crate::checklist::iso_to_ms("2026-10-16T08:30:00Z").unwrap();
        let hits = hits_in(&comments, &me(), after, URL);
        assert_eq!(hits.iter().map(|h| (h.kind, h.comment.body.as_str())).collect::<Vec<_>>(), [(HitKind::Reply, "because of X")]);
    }

    #[test]
    fn a_mention_is_a_hit_in_any_thread() {
        let comments = [comment(1, "bob", "@ana can you look at this", "2026-10-16T09:59:00Z", None, false)];
        let hits = hits_in(&comments, &me(), crate::checklist::iso_to_ms("2026-10-16T09:00:00Z").unwrap(), URL);
        assert_eq!(hits.iter().map(|h| (h.kind, h.comment.author.as_str())).collect::<Vec<_>>(), [(HitKind::Mention, "bob")]);
        assert_eq!(hits[0].key, key_of(URL, &comments[0]));
    }

    #[test]
    fn a_comments_key_is_the_same_whatever_its_position_and_hashes_like_the_typescript() {
        // What the TypeScript's `short("abc")` gives, measured in node.
        assert_eq!(short("abc"), "375fut");
        let a = comment(1, "bob", "hi", "2026-10-16T09:00:00Z", None, false);
        let b = Comment { id: 99, ..a.clone() };
        assert_eq!(key_of(URL, &a), key_of(URL, &b));
    }

    #[test]
    fn a_burst_of_comments_is_one_notification() {
        let note = |i: i64| Note { hit: Hit { kind: HitKind::Mention, key: format!("k{i}"), url: URL.into(), comment: comment(i, "bob", &format!("hi {i}"), "2026-10-16T09:00:00Z", None, false) }, title: format!("PR {i}"), args: json!({}) };
        assert_eq!(notes_for("web", &[note(1), note(2)]).len(), 2);
        let burst: Vec<Note> = (0..=MAX_NOTES as i64).map(note).collect();
        assert_eq!(notes_for("web", &burst).len(), 1);
        assert_eq!(notes_for("web", &[note(1)])[0].body, "PR 1\nhi 1");
    }
}
