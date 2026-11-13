//! The operations a page or a tool asks for, applied to a store: `apply_op` gives the changed copy, or an error that says
//! what was wrong. Nothing is saved here.

use crate::{
    domain::{
        Actor, Create, DESCRIPTION_TEMPLATE, Issue, IssueType, LogEntry, Op, Priority, STATUS_NAMES, Status, Store, TYPE_NAMES, Update, check_enum, has_content, limits, normalize_key, priority_label,
    },
    log::{append, append_loose, entry, touch, upsert_files},
    relations::{descendants_of, relate},
};
use std::collections::HashSet;
use wings_plugin::format::grouped;

/// `value` trimmed, or an error when it is longer than `max` characters.
fn text(value: &str, field: &str, max: usize) -> Result<String, String> {
    if value.chars().count() > max {
        return Err(format!("{field} is limited to {} characters.", grouped(max as u64)));
    }
    Ok(value.trim().to_owned())
}

fn is_sha(text: &str) -> bool {
    (7..=40).contains(&text.len()) && text.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn check_pr(value: &str) -> Result<String, String> {
    let pr = text(value, "pr", limits::PR)?;
    if !pr.is_empty() && !(pr.starts_with("https://") && pr.len() > "https://".len() && !pr.contains(char::is_whitespace)) {
        return Err("pr must be an https URL.".into());
    }
    Ok(pr)
}

/// Applies the `link` changes of an update to `next`, and returns the log entries they make.
fn apply_links(next: &mut Issue, op: &Update, by: &Actor, now: f64) -> Result<Vec<LogEntry>, String> {
    let mut entries = Vec::new();
    let mut links = next.links.clone().unwrap_or_default();
    /// Sets the branch or the PR to `value`, or clears it for "", and logs it as `label` when that changes it.
    fn set_link(slot: &mut Option<String>, value: String, label: &str, entries: &mut Vec<LogEntry>, by: &Actor, now: f64) {
        if value == slot.as_deref().unwrap_or_default() {
            return;
        }
        let line = if value.is_empty() { format!("{label} removed") } else { format!("{label}: {value}") };
        *slot = (!value.is_empty()).then_some(value);
        entries.push(entry(now, by, "link", line));
    }
    if let Some(branch) = &op.branch {
        let branch = text(branch, "branch", limits::BRANCH)?;
        if branch.contains(char::is_whitespace) {
            return Err("branch can't contain spaces.".into());
        }
        set_link(&mut links.branch, branch, "Branch", &mut entries, by, now);
    }
    if let Some(pr) = &op.pr {
        set_link(&mut links.pr, check_pr(pr)?, "PR", &mut entries, by, now);
    }
    if let Some(commit) = &op.commit {
        let commit = text(commit, "commit", limits::COMMIT)?.to_lowercase();
        if !is_sha(&commit) {
            return Err("commit must be a git commit hash of 7 to 40 hex digits.".into());
        }
        let short = commit[..7].to_owned();
        let known = links.commits.iter().flatten().any(|c| c.starts_with(&short) || short.starts_with(c.as_str()));
        if !known {
            links.commits.get_or_insert_with(Vec::new).push(short.clone());
            entries.push(entry(now, by, "link", format!("Commit: {short}")));
        }
    }
    next.links = (!links.is_empty()).then_some(links);
    Ok(entries)
}

pub struct Applied {
    pub store: Store,
    /// The issue the op made or changed: for a removal, the one that is gone.
    pub issue: Issue,
    /// False when the op asked for what was already so. Nothing is saved then.
    pub changed: bool,
}

/// `store` with `op` applied by `by`, as a new store. Errors say what was wrong.
pub fn apply_op(store: &Store, op: &Op, by: &Actor, now: f64) -> Result<Applied, String> {
    let key = match op {
        Op::Create(create) => return create_issue(store, create, by, now),
        Op::Update(Update { key, .. }) | Op::Note { key, .. } | Op::Files { key, .. } | Op::Remove { key, .. } => normalize_key(key),
    };
    let index = store.issues.iter().position(|i| i.key == key).ok_or_else(|| format!("No issue {key} in this project. Use list to see the keys."))?;
    match op {
        Op::Remove { children, .. } => Ok(remove_issue(store, index, *children, by, now)),
        _ => update_issue(store, index, op, by, now),
    }
}

fn create_issue(store: &Store, op: &Create, by: &Actor, now: f64) -> Result<Applied, String> {
    let title = text(&op.title, "title", limits::TITLE)?;
    if title.is_empty() {
        return Err("title is required.".into());
    }
    let description = match &op.description {
        Some(description) => text(description, "description", limits::DESCRIPTION)?,
        None => String::new(),
    };
    if let Some(status) = &op.status {
        check_enum(status, &STATUS_NAMES, "status")?;
    }
    if let Some(kind) = &op.rel.kind {
        check_enum(kind, &TYPE_NAMES, "type")?;
    }
    if let Some(priority) = &op.priority {
        check_enum(priority, &Priority::NAMES, "priority")?;
    }
    if store.issues.len() >= limits::ISSUES {
        return Err(format!("A project holds up to {} issues. Remove one first.", limits::ISSUES));
    }
    let mut issue = Issue {
        key: format!("{}-{}", store.prefix, store.next),
        title,
        description: None,
        status: op.status.as_deref().and_then(Status::parse).unwrap_or(Status::Todo),
        kind: op.rel.kind.as_deref().and_then(IssueType::parse).unwrap_or(IssueType::Task),
        priority: op.priority.as_deref().and_then(Priority::parse),
        parent: None,
        related: None,
        blocked_by: None,
        links: None,
        files: None,
        sessions: Vec::new(),
        log: vec![entry(now, by, "created", "Created")],
        created_at: now,
        updated_at: now,
    };
    if has_content(Some(&description)) {
        issue.description = Some(description);
    }
    touch(&mut issue, by, now);
    if let Some(branch) = &op.branch {
        let entries = apply_links(&mut issue, &Update { branch: Some(branch.clone()), ..Update::default() }, by, now)?;
        append(&mut issue, entries)?;
    }
    let mut issues = store.issues.clone();
    issues.push(issue);
    let at = issues.len() - 1;
    // Its type is set already: only its links go through `relate`.
    let mut rel = op.rel.clone();
    rel.kind = None;
    if !rel.is_empty() {
        relate(&mut issues, at, &rel, by, now)?;
    }
    let issue = issues[at].clone();
    Ok(Applied { store: Store { prefix: store.prefix.clone(), next: store.next + 1, issues }, issue, changed: true })
}

fn remove_issue(store: &Store, index: usize, children: bool, by: &Actor, now: f64) -> Applied {
    let current = store.issues[index].clone();
    // Whatever pointed at it lets go, and says why in its log. The counter stays where it is, so the key is never handed out again.
    let mut gone: HashSet<String> = HashSet::from([current.key.clone()]);
    if children {
        gone.extend(descendants_of(&store.issues, &current.key).into_iter().map(|i| i.key.clone()));
    }
    let issues = store
        .issues
        .iter()
        .filter(|i| !gone.contains(&i.key))
        .map(|i| {
            let mut notes: Vec<String> = Vec::new();
            let mut next = i.clone();
            if next.parent.as_ref().is_some_and(|p| gone.contains(p)) {
                notes.push(format!("Parent {} was deleted", next.parent.take().unwrap_or_default()));
            }
            let mut drop_from = |list: &mut Option<Vec<String>>, note: &str| {
                let Some(items) = list.as_ref() else {
                    return;
                };
                let hit: Vec<String> = items.iter().filter(|k| gone.contains(*k)).cloned().collect();
                if hit.is_empty() {
                    return;
                }
                let rest: Vec<String> = items.iter().filter(|k| !gone.contains(*k)).cloned().collect();
                *list = (!rest.is_empty()).then_some(rest);
                notes.extend(hit.into_iter().map(|k| format!("{k} was deleted: {note}")));
            };
            drop_from(&mut next.related, "unlinked (related)");
            drop_from(&mut next.blocked_by, "no longer blocking");
            if next == *i {
                return next;
            }
            append_loose(&mut next, notes.into_iter().map(|t| entry(now, by, "link", t)).collect());
            next.updated_at = now;
            next
        })
        .collect();
    Applied { store: Store { prefix: store.prefix.clone(), next: store.next, issues }, issue: current, changed: true }
}

/// `store` with a note, the files a journal reported, or an update's changes made to the issue at `index`.
fn update_issue(store: &Store, index: usize, op: &Op, by: &Actor, now: f64) -> Result<Applied, String> {
    let current = &store.issues[index];
    let mut next = current.clone();
    let mut entries: Vec<LogEntry> = Vec::new();
    let mut changed = false;
    let mut issues = store.issues.clone();
    match op {
        Op::Files { files, .. } => {
            // No log entry: the log is for people and decisions, and this can't fail on its limit.
            let rows = upsert_files(next.files.as_deref(), files, by, now);
            if Some(&rows) != next.files.as_ref() && !(rows.is_empty() && next.files.is_none()) {
                next.files = Some(rows);
                changed = true;
            }
        }
        Op::Note { text: note, .. } => {
            let note = text(note, "text", limits::NOTE)?;
            if note.is_empty() {
                return Err("text is required.".into());
            }
            entries = vec![entry(now, by, "note", note)];
            changed = true;
        }
        Op::Update(op) => {
            if let Some(title) = &op.title {
                let title = text(title, "title", limits::TITLE)?;
                if title.is_empty() {
                    return Err("title can't be empty.".into());
                }
                if title != next.title {
                    next.title = title;
                    changed = true;
                }
            }
            if let Some(typed) = &op.description {
                let typed = text(typed, "description", limits::DESCRIPTION)?;
                let description = if has_content(Some(&typed)) { typed } else { String::new() };
                if description != next.description.as_deref().unwrap_or_default() {
                    next.description = (!description.is_empty()).then_some(description);
                    changed = true;
                }
            }
            if let Some(status) = &op.status {
                check_enum(status, &STATUS_NAMES, "status")?;
                let status = Status::parse(status).expect("checked");
                if status != next.status {
                    entries.push(entry(now, by, "status", format!("{} → {}", next.status.label(), status.label())));
                    next.status = status;
                    changed = true;
                }
            }
            if let Some(priority) = &op.priority {
                check_enum(priority, &Priority::NAMES, "priority")?;
                let priority = Priority::parse(priority);
                if priority != next.priority {
                    entries.push(entry(now, by, "priority", format!("Priority: {} → {}", priority_label(next.priority), priority_label(priority))));
                    next.priority = priority;
                    changed = true;
                }
            }
            let links = apply_links(&mut next, op, by, now)?;
            if !links.is_empty() {
                entries.extend(links);
                changed = true;
            }
            if !op.rel.is_empty() {
                issues[index] = next;
                // The change to the other issues logs on them and on this one first, like a relation always did.
                let related = relate(&mut issues, index, &op.rel, by, now)?;
                next = issues[index].clone();
                changed |= related;
            }
        }
        Op::Create(_) | Op::Remove { .. } => unreachable!("handled by apply_op"),
    }
    if !changed {
        return Ok(Applied { store: store.clone(), issue: current.clone(), changed: false });
    }
    append(&mut next, entries)?;
    touch(&mut next, by, now);
    issues[index] = next.clone();
    Ok(Applied { store: Store { prefix: store.prefix.clone(), next: store.next, issues }, issue: next, changed: true })
}

/// What an issue's description is when it is first edited.
pub fn description_to_edit(issue: &Issue) -> String {
    match issue.description.as_deref() {
        Some(description) if !description.is_empty() => description.to_owned(),
        _ => DESCRIPTION_TEMPLATE.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::Agent,
        format::format_issue,
        orderings::board_columns,
        testing::{create_with, fresh, make},
    };

    fn update(store: &Store, key: &str, change: impl FnOnce(&mut Update), by: &Actor, now: f64) -> Result<Applied, String> {
        let mut update = Op::update(key);
        change(&mut update);
        apply_op(store, &Op::Update(update), by, now)
    }
    fn note(store: &Store, key: &str, text: &str, by: &Actor, now: f64) -> Store {
        apply_op(store, &Op::Note { key: key.into(), text: text.into() }, by, now).unwrap().store
    }
    fn agent(id: &str, name: Option<&str>, kind: Option<&str>) -> Actor {
        Actor::Agent(Agent { id: id.into(), name: name.map(Into::into), kind: kind.map(Into::into), via: None })
    }
    fn err(store: &Store, op: Op) -> String {
        apply_op(store, &op, &Actor::You, 1.0).err().expect("an error")
    }
    fn up(key: &str, change: impl FnOnce(&mut Update)) -> Op {
        let mut update = Op::update(key);
        change(&mut update);
        Op::Update(update)
    }

    #[test]
    fn keys_count_up_per_project_and_are_never_reused_after_a_delete() {
        let mut s = make(&fresh(), "a", |_| {});
        s = make(&s, "b", |_| {});
        let keys = |s: &Store| s.issues.iter().map(|i| i.key.clone()).collect::<Vec<_>>();
        assert_eq!(keys(&s), ["WEB-1", "WEB-2"]);
        s = apply_op(&s, &Op::Remove { key: "web-2".into(), children: false }, &Actor::You, 1.0).unwrap().store;
        s = make(&s, "c", |_| {});
        assert_eq!(keys(&s), ["WEB-1", "WEB-3"]);
        s = apply_op(&s, &Op::Remove { key: "WEB-3".into(), children: false }, &Actor::You, 1.0).unwrap().store;
        assert_eq!(create_with(&s, "d", |_| {}, &Actor::You, 1.0).issue.key, "WEB-4");
    }

    #[test]
    fn the_log_is_append_only_status_and_link_changes_log_themselves_and_earlier_entries_stay() {
        let mut s = make(&fresh(), "A", |_| {});
        let before = s.issues[0].log.clone();
        let change = |u: &mut Update| {
            u.status = Some("in_progress".into());
            u.branch = Some("feat/a".into());
            u.pr = Some("https://example.com/p/1".into());
            u.commit = Some("ABCDEF1234".into());
        };
        s = update(&s, "WEB-1", change, &Actor::You, 2.0).unwrap().store;
        s = note(&s, "WEB-1", " did it ", &Actor::You, 3.0);
        let log = s.issues[0].log.clone();
        let kinds: Vec<(&str, &str)> = log.iter().map(|e| (e.kind.as_str(), e.text.as_str())).collect();
        assert_eq!(kinds, [("created", "Created"), ("status", "To do → In progress"), ("link", "Branch: feat/a"), ("link", "PR: https://example.com/p/1"), ("link", "Commit: abcdef1"), ("note", "did it")]);
        assert_eq!(log[..before.len()], before[..]);
        let again = update(&s, "WEB-1", |u| { u.status = Some("in_progress".into()); u.branch = Some("feat/a".into()); u.commit = Some("abcdef1".into()); }, &Actor::You, 4.0).unwrap();
        assert!(!again.changed);
        assert_eq!(again.store, s);
        let cleared = update(&s, "WEB-1", |u| u.branch = Some(String::new()), &Actor::You, 5.0).unwrap().store.issues[0].clone();
        assert_eq!(cleared.log.last().unwrap().text, "Branch removed");
        assert_eq!(cleared.links.as_ref().and_then(|l| l.branch.clone()), None);
        assert_eq!(cleared.log.len(), log.len() + 1);
    }

    #[test]
    fn a_full_log_refuses_more_entries_instead_of_dropping_old_ones() {
        let mut s = make(&fresh(), "A", |_| {});
        for i in 1..limits::LOG {
            s = note(&s, "WEB-1", &format!("n{i}"), &Actor::You, 1.0);
        }
        assert_eq!(s.issues[0].log.len(), limits::LOG);
        assert!(err(&s, Op::Note { key: "WEB-1".into(), text: "more".into() }).contains("already has"));
    }

    #[test]
    fn agent_entries_are_stamped_with_the_session_and_the_issue_records_it_once_refreshed_on_later_calls() {
        let id = "6f1c2a90-3b7e-4d51-9a02-5c8e1d4b7a11";
        let first = agent(id, Some("first name"), None);
        let mut s = create_with(&fresh(), "A", |_| {}, &first, 10.0).store;
        assert_eq!(s.issues[0].log[0].by, first);
        assert_eq!(s.issues[0].sessions.iter().map(|x| (x.id.as_str(), x.name.as_deref(), x.seen)).collect::<Vec<_>>(), [(id, Some("first name"), 10.0)]);
        let before = s.clone();
        s = note(&s, "WEB-1", "x", &agent(id, Some("renamed"), None), 20.0);
        assert_eq!(s.issues[0].sessions[0].name.as_deref(), Some("renamed"));
        assert_eq!(s.issues[0].sessions[0].seen, 20.0);
        assert_eq!(before.issues[0].sessions[0].seen, 10.0, "the old store is left alone");
        s = note(&s, "WEB-1", "y", &agent("other", None, None), 30.0);
        assert_eq!(s.issues[0].sessions.iter().map(|x| x.id.as_str()).collect::<Vec<_>>(), [id, "other"]);
        assert_eq!(crate::actor::last_session(&s.issues[0]).unwrap().id, "other");
        // The user and an agent without a session id don't become sessions.
        s = note(&s, "WEB-1", "z", &Actor::You, 40.0);
        s = note(&s, "WEB-1", "w", &agent("", None, None), 50.0);
        assert_eq!(s.issues[0].sessions.len(), 2);
    }

    #[test]
    fn an_issue_keeps_sessions_of_different_agents_side_by_side_each_with_its_kind() {
        let mut s = create_with(&fresh(), "A", |_| {}, &agent("6f1c2a90-3b7e-4d51-9a02-5c8e1d4b7a11", Some("claude one"), Some("claude")), 10.0).store;
        s = note(&s, "WEB-1", "x", &agent("ses_abc12345", Some("oc one"), Some("opencode")), 20.0);
        assert_eq!(s.issues[0].sessions.iter().map(|x| (x.kind.clone().unwrap(), x.seen)).collect::<Vec<_>>(), [("claude".to_owned(), 10.0), ("opencode".to_owned(), 20.0)]);
        assert_eq!(crate::actor::last_session(&s.issues[0]).unwrap().kind.as_deref(), Some("opencode"));
    }

    #[test]
    fn unknown_keys_bad_status_and_bad_links_are_errors_that_say_what_is_wrong() {
        let s = make(&fresh(), "A", |_| {});
        assert!(err(&s, up("WEB-9", |u| u.status = Some("done".into()))).contains("No issue WEB-9"));
        assert!(err(&s, up("WEB-1", |u| u.status = Some("later".into()))).contains("status must be one of"));
        assert!(err(&s, up("WEB-1", |u| u.pr = Some("http://x.example".into()))).contains("https URL"));
        assert!(err(&s, up("WEB-1", |u| u.commit = Some("xyz".into()))).contains("commit must be"));
        assert!(err(&s, up("WEB-1", |u| u.branch = Some("a b".into()))).contains("spaces"));
        assert!(err(&s, Op::Create(Op::create("   "))).contains("required"));
    }

    #[test]
    fn priority_set_on_create_changed_and_cleared_with_a_log_entry_refused_when_unknown_and_sorts_urgent_first() {
        let mut s = make(&fresh(), "Fix it", |c| c.priority = Some("high".into()));
        assert_eq!(s.issues[0].priority, Some(Priority::High));
        assert_eq!(make(&fresh(), "Plain", |_| {}).issues[0].priority, None);
        assert_eq!(make(&fresh(), "None", |c| c.priority = Some("none".into())).issues[0].priority, None);
        s = update(&s, "WEB-1", |u| u.priority = Some("urgent".into()), &Actor::You, 2.0).unwrap().store;
        assert_eq!(s.issues[0].priority, Some(Priority::Urgent));
        assert_eq!(s.issues[0].log.last().unwrap().text, "Priority: High → Urgent");
        s = update(&s, "WEB-1", |u| u.priority = Some("none".into()), &Actor::You, 3.0).unwrap().store;
        assert_eq!(s.issues[0].priority, None);
        assert!(!update(&s, "WEB-1", |u| u.priority = Some("none".into()), &Actor::You, 1.0).unwrap().changed);
        assert!(err(&s, up("WEB-1", |u| u.priority = Some("p1".into()))).contains("priority must be one of"));
        let mk = |n: u32, priority: Option<Priority>| Issue { key: format!("WEB-{n}"), title: format!("t{n}"), description: None, status: Status::Todo, kind: IssueType::Task, priority, parent: None, related: None, blocked_by: None, links: None, files: None, sessions: vec![], log: vec![], created_at: 0.0, updated_at: 0.0 };
        let issues = [mk(1, None), mk(2, Some(Priority::Low)), mk(3, Some(Priority::Urgent)), mk(4, Some(Priority::High))];
        let column = &board_columns(&issues, false)[0];
        assert_eq!(column.issues.iter().map(|i| i.key.as_str()).collect::<Vec<_>>(), ["WEB-3", "WEB-4", "WEB-2", "WEB-1"]);
    }

    #[test]
    fn an_untouched_template_is_stored_as_no_description_and_get_says_to_fill_it() {
        let describe = |s: &Store, d: &str| update(s, "WEB-1", |u| u.description = Some(d.into()), &Actor::You, 1.0).unwrap().store;
        let mut s = make(&fresh(), "a", |c| c.description = Some(DESCRIPTION_TEMPLATE.into()));
        assert_eq!(s.issues[0].description, None);
        s = describe(&s, "## Goal\nx");
        assert_eq!(s.issues[0].description.as_deref(), Some("## Goal\nx"));
        s = describe(&s, DESCRIPTION_TEMPLATE);
        assert_eq!(s.issues[0].description, None);
        assert!(format_issue(&s.issues[0], &[]).contains("No description yet; fill Goal, Scope, Done when, Validation"));
    }

    #[test]
    fn files_one_row_per_path_the_newest_win_past_100_the_oldest_go_texts_are_cut_no_log_entry() {
        let a = agent("s-a", Some("one"), Some("claude"));
        let b = agent("s-b", None, None);
        let files = |list: &[(&str, &str)]| Op::Files { key: "WEB-1".into(), files: list.iter().map(|(p, w)| ((*p).into(), (*w).into())).collect() };
        let mut s = make(&fresh(), "A", |_| {});
        s = apply_op(&s, &files(&[("x.ts", "first"), ("y.ts", "why")]), &a, 10.0).unwrap().store;
        s = apply_op(&s, &files(&[("x.ts", "second"), ("x.ts", "third")]), &b, 20.0).unwrap().store;
        let issue = &s.issues[0];
        let rows: Vec<(&str, &str, f64, &str)> = issue.files.as_ref().unwrap().iter().map(|f| (f.path.as_str(), f.what.as_str(), f.at, f.session.as_ref().unwrap().id.as_str())).collect();
        assert_eq!(rows, [("x.ts", "third", 20.0, "s-b"), ("y.ts", "why", 10.0, "s-a")]);
        assert_eq!(issue.log.len(), 1, "no log entry");
        assert_eq!(issue.sessions.iter().map(|x| (x.id.as_str(), x.seen)).collect::<Vec<_>>(), [("s-a", 10.0), ("s-b", 20.0)], "the session is recorded and refreshed like any write");
        assert!(!apply_op(&s, &files(&[]), &a, 30.0).unwrap().changed);

        let many: Vec<(String, String)> = (0..105).map(|i| (format!("f{i}.ts"), "w".to_owned())).collect();
        let op = |rows: &[(String, String)]| Op::Files { key: "WEB-1".into(), files: rows.to_vec() };
        let mut t = apply_op(&make(&fresh(), "A", |_| {}), &op(&many[..60]), &a, 1.0).unwrap().store;
        t = apply_op(&t, &op(&many[60..]), &a, 2.0).unwrap().store;
        let rows = t.issues[0].files.as_ref().unwrap();
        assert_eq!(rows.len(), limits::FILES);
        assert!(rows[..45].iter().all(|f| f.at == 2.0));
        assert_eq!(rows.iter().filter(|f| f.at == 1.0).count(), 55);

        let long = apply_op(&make(&fresh(), "A", |_| {}), &Op::Files { key: "WEB-1".into(), files: vec![("p".repeat(500), "w".repeat(500))] }, &a, 1.0).unwrap().store.issues[0].files.as_ref().unwrap()[0].clone();
        assert_eq!((long.path.chars().count(), long.what.chars().count()), (limits::FILE_PATH, limits::FILE_WHAT));
    }

    #[test]
    fn an_issue_with_a_full_log_still_takes_its_files_and_an_old_issue_without_files_formats_fine() {
        let mut s = make(&fresh(), "A", |_| {});
        for i in 1..limits::LOG {
            s = note(&s, "WEB-1", &format!("n{i}"), &Actor::You, 1.0);
        }
        let out = apply_op(&s, &Op::Files { key: "WEB-1".into(), files: vec![("a.ts".into(), "x".into())] }, &agent("s", None, None), 5.0).unwrap();
        assert!(out.changed);
        assert_eq!(out.issue.files.as_ref().unwrap().len(), 1);
        assert_eq!(out.issue.log.len(), limits::LOG);
        let old = make(&fresh(), "Old", |_| {}).issues[0].clone();
        assert!(old.files.is_none());
        assert!(!format_issue(&old, &[]).contains("Files changed"));
    }
}
