//! The PR's pushes and how they sit among its threads.

use crate::checklist::iso_to_ms;
use crate::threads::{Thread, last_activity};
use crate::types::{Push, PushCommit};
use serde::Deserialize;
use std::collections::HashSet;
use wings_plugin::host;

#[derive(Clone, Default, Deserialize)]
pub struct Person {
    #[serde(rename = "displayName")]
    pub display_name: Option<String>,
    pub name: Option<String>,
    pub date: Option<String>,
}

#[derive(Clone, Deserialize)]
pub struct CommitRef {
    #[serde(rename = "commitId")]
    pub commit_id: Option<String>,
}

#[derive(Clone, Deserialize)]
pub struct IterationCommit {
    #[serde(rename = "commitId")]
    pub commit_id: String,
    pub comment: Option<String>,
    pub author: Option<Person>,
}

/// An iteration from `pullRequests/{id}/iterations?includeCommits=true`, the parts read.
#[derive(Clone, Deserialize)]
pub struct AzureIteration {
    pub id: i64,
    pub author: Option<Person>,
    #[serde(rename = "createdDate")]
    pub created_date: String,
    #[serde(rename = "hasMoreCommits", default)]
    pub has_more_commits: bool,
    pub reason: Option<String>,
    #[serde(rename = "sourceRefCommit")]
    pub source_ref_commit: Option<CommitRef>,
    #[serde(rename = "commonRefCommit")]
    pub common_ref_commit: Option<CommitRef>,
    pub commits: Option<Vec<IterationCommit>>,
}

fn time(iso: &str) -> f64 {
    iso_to_ms(iso).unwrap_or(0.0)
}

/// Pushes, oldest first. A commit shows in the first push that has it, so a list that repeats earlier commits (as a
/// cumulative one would) still reads as what each push added. A push with no new commits, like a merge of the target
/// branch's own update, has no card.
pub fn pushes_of(iterations: &[AzureIteration]) -> Vec<Push> {
    let mut seen = HashSet::new();
    let mut pushes = Vec::new();
    let mut sorted: Vec<&AzureIteration> = iterations.iter().collect();
    sorted.sort_by_key(|it| it.id);
    for it in sorted {
        let fresh: Vec<&IterationCommit> = it.commits.iter().flatten().filter(|c| !seen.contains(&c.commit_id)).collect();
        seen.extend(fresh.iter().map(|c| c.commit_id.clone()));
        if fresh.is_empty() {
            continue;
        }
        let mut commits: Vec<PushCommit> = fresh
            .iter()
            .map(|c| PushCommit {
                sha: c.commit_id.clone(),
                message: c.comment.as_deref().unwrap_or("").split('\n').next().unwrap_or("").trim().to_owned(),
                author: c.author.as_ref().and_then(|a| a.name.clone()).unwrap_or_else(|| "unknown".into()),
                at: c.author.as_ref().and_then(|a| a.date.clone()).unwrap_or_else(|| it.created_date.clone()),
            })
            .collect();
        commits.sort_by(|a, b| time(&b.at).total_cmp(&time(&a.at)));
        pushes.push(Push {
            id: it.id,
            author: it.author.as_ref().and_then(|a| a.display_name.clone()).unwrap_or_else(|| commits[0].author.clone()),
            at: it.created_date.clone(),
            commits,
            more: it.has_more_commits,
            head: it.source_ref_commit.as_ref().and_then(|c| c.commit_id.clone()),
            base: it.common_ref_commit.as_ref().and_then(|c| c.commit_id.clone()),
            created: it.reason.as_deref() == Some("create"),
        });
    }
    pushes
}

#[derive(Clone, PartialEq, Debug)]
pub enum Entry {
    Push { push: Push, at: f64 },
    Thread { thread: Thread, at: f64 },
}

impl Entry {
    pub fn at(&self) -> f64 {
        match self {
            Self::Push { at, .. } | Self::Thread { at, .. } => *at,
        }
    }
}

/// Pushes and threads in one list, the newest activity first.
pub fn timeline_of(threads: &[Thread], pushes: &[Push]) -> Vec<Entry> {
    let mut entries: Vec<Entry> = threads.iter().map(|thread| Entry::Thread { thread: thread.clone(), at: last_activity(thread) }).collect();
    entries.extend(pushes.iter().map(|push| Entry::Push { push: push.clone(), at: time(&push.at) }));
    entries.sort_by(|a, b| b.at().total_cmp(&a.at()));
    entries
}

const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// "Today at 10:50 AM", "Yesterday at 9:33 AM", else "12 Oct, 10:50 AM", in this Mac's timezone. `now` is milliseconds since the epoch.
pub fn day_time(iso: &str, now: f64) -> String {
    let Some(ms) = iso_to_ms(iso) else { return String::new() };
    let local = |ms: f64| ms + f64::from(host::utc_offset(ms)) * 60_000.0;
    let (then, today) = (local(ms), local(now));
    let day = |local: f64| (local / 86_400_000.0).floor();
    let minutes = ((then / 60_000.0).rem_euclid(1440.0)) as u32;
    let (hour, minute) = (minutes / 60, minutes % 60);
    let time = format!("{}:{minute:02} {}", if hour % 12 == 0 { 12 } else { hour % 12 }, if hour < 12 { "AM" } else { "PM" });
    match day(today) - day(then) {
        d if d == 0.0 => format!("Today at {time}"),
        d if d == 1.0 => format!("Yesterday at {time}"),
        _ => {
            // Civil date from days since the epoch.
            let z = day(then) as i64 + 719_468;
            let era = z.div_euclid(146_097);
            let doe = z - era * 146_097;
            let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
            let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
            let mp = (5 * doy + 2) / 153;
            let (d, m) = (doy - (153 * mp + 2) / 5 + 1, if mp < 10 { mp + 3 } else { mp - 9 });
            format!("{d} {}, {time}", MONTHS[(m - 1) as usize])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Comment;
    use serde_json::json;

    fn commit(id: &str, at: &str) -> serde_json::Value {
        json!({ "commitId": id, "comment": format!("msg {id}\nbody"), "author": { "name": "Ada", "date": at } })
    }

    fn it(id: i64, created: &str, commits: Vec<serde_json::Value>, extra: serde_json::Value) -> AzureIteration {
        let mut v = json!({ "id": id, "createdDate": created, "author": { "displayName": "Ada" }, "commits": commits, "sourceRefCommit": { "commitId": format!("head{id}") }, "commonRefCommit": { "commitId": "base" } });
        for (k, value) in extra.as_object().into_iter().flatten() {
            v[k] = value.clone();
        }
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn iterations_become_pushes_oldest_first_commits_newest_first_with_the_first_line_of_the_message() {
        let pushes = pushes_of(&[
            it(2, "2026-01-02T10:00:00Z", vec![commit("a", "2026-01-02T09:00:00Z"), commit("b", "2026-01-02T09:30:00Z")], json!({})),
            it(1, "2026-01-01T10:00:00Z", vec![commit("c", "2026-01-01T09:00:00Z")], json!({})),
        ]);
        assert_eq!(pushes.iter().map(|p| p.id).collect::<Vec<_>>(), [1, 2]);
        assert_eq!(pushes[1].commits.iter().map(|c| (c.sha.as_str(), c.message.as_str())).collect::<Vec<_>>(), [("b", "msg b"), ("a", "msg a")]);
        assert_eq!(pushes[1].head.as_deref(), Some("head2"));
        assert_eq!(pushes[1].base.as_deref(), Some("base"));
    }

    #[test]
    fn a_commit_shows_only_in_the_first_push_that_has_it_and_a_push_with_nothing_new_has_no_card() {
        let pushes = pushes_of(&[
            it(1, "2026-01-01T10:00:00Z", vec![commit("a", "2026-01-01T09:00:00Z")], json!({})),
            it(2, "2026-01-02T10:00:00Z", vec![commit("a", "2026-01-01T09:00:00Z"), commit("b", "2026-01-02T09:00:00Z")], json!({})),
            it(3, "2026-01-03T10:00:00Z", vec![commit("a", "2026-01-01T09:00:00Z")], json!({})),
            it(4, "2026-01-04T10:00:00Z", vec![], json!({})),
        ]);
        assert_eq!(pushes.iter().map(|p| (p.id, p.commits.iter().map(|c| c.sha.as_str()).collect::<Vec<_>>())).collect::<Vec<_>>(), [(1, vec!["a"]), (2, vec!["b"])]);
    }

    #[test]
    fn a_cut_commit_list_is_marked() {
        assert!(pushes_of(&[it(1, "2026-01-01T10:00:00Z", vec![commit("a", "2026-01-01T09:00:00Z")], json!({ "hasMoreCommits": true }))])[0].more);
    }

    #[test]
    fn the_push_that_opened_the_pr_is_marked_by_its_reason() {
        let pushes = pushes_of(&[
            it(1, "2026-01-01T10:00:00Z", vec![commit("a", "2026-01-01T09:00:00Z")], json!({ "reason": "create" })),
            it(2, "2026-01-02T10:00:00Z", vec![commit("b", "2026-01-02T09:00:00Z")], json!({ "reason": "push" })),
        ]);
        assert_eq!(pushes.iter().map(|p| p.created).collect::<Vec<_>>(), [true, false]);
    }

    #[test]
    fn pushes_and_threads_share_one_timeline_newest_first_and_pushes_are_not_threads() {
        let pushes = pushes_of(&[it(1, "2026-01-01T10:00:00Z", vec![commit("a", "2026-01-01T09:00:00Z")], json!({})), it(2, "2026-01-03T10:00:00Z", vec![commit("b", "2026-01-03T09:00:00Z")], json!({}))]);
        let comment = |id: i64, at: &str| Comment { id, reply_to: None, path: None, line: None, side: "additions".into(), author: "Ada".into(), body: String::new(), created_at: at.into(), url: String::new(), can_reply: None, by: None, discussion: None, thread: None, range: None, snippet: None, refs: None, system: false };
        let thread = |id: i64, at: &str, reply: Option<&str>| Thread { root: comment(id, at), replies: reply.map(|r| comment(id + 1, r)).into_iter().collect() };
        let entries = timeline_of(&[thread(10, "2026-01-02T00:00:00Z", None), thread(20, "2026-01-01T00:00:00Z", Some("2026-01-04T00:00:00Z"))], &pushes);
        let names: Vec<String> = entries.iter().map(|e| match e { Entry::Push { push, .. } => format!("push{}", push.id), Entry::Thread { thread, .. } => format!("thread{}", thread.root.id) }).collect();
        assert_eq!(names, ["thread20", "push2", "thread10", "push1"]);
    }

    #[test]
    fn commit_times_read_as_today_yesterday_or_a_date() {
        // The test host's timezone is UTC.
        let now = iso_to_ms("2026-10-14T15:00:00Z").unwrap();
        assert!(day_time("2026-10-14T10:50:00Z", now).starts_with("Today at 10:50"));
        assert!(day_time("2026-10-13T09:33:00Z", now).starts_with("Yesterday at 9:33"));
        assert!(day_time("2026-10-01T09:33:00Z", now).starts_with("1 Oct, 9:33"));
        assert_eq!(day_time("nope", now), "");
    }
}
