//! How issues relate: parents and children, related and blocking links. The queries read them, `relate` changes them.

use crate::{
    domain::{Actor, Issue, IssueType, MAX_LINKS, Relations, Status, TYPE_NAMES, check_enum, is_key, key_number, normalize_key},
    log::{append, entry},
};
use std::collections::HashSet;

fn key_list(value: &Option<Vec<String>>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for key in value.iter().flatten().map(|k| normalize_key(k)).filter(|k| !k.is_empty()) {
        if !out.contains(&key) {
            out.push(key);
        }
    }
    out
}

/// The issues whose parent is `key`.
pub fn children_of<'a>(issues: &'a [Issue], key: &str) -> Vec<&'a Issue> {
    issues.iter().filter(|i| i.parent.as_deref() == Some(key)).collect()
}

/// The keys under `key` at any depth: its children, theirs and so on.
fn below(issues: &[Issue], key: &str) -> HashSet<String> {
    let mut below: HashSet<String> = HashSet::from([key.to_owned()]);
    let mut grew = true;
    while grew {
        grew = false;
        for i in issues {
            if i.parent.as_ref().is_some_and(|p| below.contains(p)) && !below.contains(&i.key) {
                below.insert(i.key.clone());
                grew = true;
            }
        }
    }
    below.remove(key);
    below
}

/// The issues under `key` at any depth, in the project's order.
pub fn descendants_of<'a>(issues: &'a [Issue], key: &str) -> Vec<&'a Issue> {
    let below = below(issues, key);
    issues.iter().filter(|i| below.contains(&i.key)).collect()
}

/// The issues `key` is blocking: the ones that list it as blocking them.
pub fn blocks_of<'a>(issues: &'a [Issue], key: &str) -> Vec<&'a Issue> {
    issues.iter().filter(|i| i.blocked_by.as_ref().is_some_and(|b| b.iter().any(|k| k == key))).collect()
}

/// Done out of all, not counting canceled ones: "3 of 7 done".
pub fn progress<'a>(children: impl IntoIterator<Item = &'a Issue>) -> (usize, usize) {
    let live: Vec<&Issue> = children.into_iter().filter(|c| c.status != Status::Canceled).collect();
    (live.iter().filter(|c| c.status == Status::Done).count(), live.len())
}

/// The epics and features `issue` may sit under: not itself, and not one of its own children, grandchildren and so on.
pub fn parent_choices<'a>(issues: &'a [Issue], issue: &Issue) -> Vec<&'a Issue> {
    let mut below = below(issues, &issue.key);
    below.insert(issue.key.clone());
    let mut choices: Vec<&Issue> = issues.iter().filter(|i| i.kind.can_parent() && !below.contains(&i.key)).collect();
    choices.sort_by_key(|i| key_number(&i.key));
    choices
}

fn find(issues: &[Issue], key: &str) -> Option<usize> {
    issues.iter().position(|i| i.key == key)
}

fn without(list: &Option<Vec<String>>, key: &str) -> Option<Vec<String>> {
    let rest: Vec<String> = list.iter().flatten().filter(|k| *k != key).cloned().collect();
    (!rest.is_empty()).then_some(rest)
}

/// Applies the type and link changes of an op to `issues[at]` and to the other issues it touches. Every change writes an
/// entry on both issues. Returns whether anything changed. The caller gives copies: a failure half way leaves them as they were.
pub fn relate(issues: &mut [Issue], at: usize, op: &Relations, by: &Actor, now: f64) -> Result<bool, String> {
    let mut changed = false;
    let log = |issues: &mut [Issue], who: Option<usize>, kind: &str, text: String| -> Result<(), String> {
        let Some(who) = who else {
            return Ok(());
        };
        append(&mut issues[who], vec![entry(now, by, kind, text)])?;
        if who != at {
            issues[who].updated_at = now;
        }
        Ok(())
    };
    let other = |issues: &[Issue], raw: &str| -> Result<usize, String> {
        if !is_key(raw) {
            return Err(format!("{raw} isn't an issue key, like WIN-12."));
        }
        if raw == issues[at].key {
            return Err(format!("{} can't link to itself.", issues[at].key));
        }
        find(issues, raw).ok_or_else(|| format!("No issue {raw} in this project. Use list to see the keys."))
    };

    if let Some(wanted) = &op.kind {
        check_enum(wanted, &TYPE_NAMES, "type")?;
        let wanted = IssueType::parse(wanted).expect("checked");
        if wanted != issues[at].kind {
            let key = issues[at].key.clone();
            if !wanted.can_parent() && issues.iter().any(|i| i.parent.as_deref() == Some(&key)) {
                return Err(format!("{key} has child issues, so it has to stay an epic or a feature. Move them to another parent first."));
            }
            let text = format!("Type: {} → {}", issues[at].kind.label(), wanted.label());
            log(issues, Some(at), "type", text)?;
            issues[at].kind = wanted;
            changed = true;
        }
    }

    if let Some(parent) = &op.parent {
        let wanted = normalize_key(parent);
        let key = issues[at].key.clone();
        if wanted.is_empty() {
            if let Some(old) = issues[at].parent.clone() {
                let old_at = find(issues, &old);
                log(issues, Some(at), "link", format!("Parent removed (was {old})"))?;
                log(issues, old_at, "link", format!("Child removed: {key}"))?;
                issues[at].parent = None;
                changed = true;
            }
        } else if Some(&wanted) != issues[at].parent.as_ref() {
            let parent = other(issues, &wanted)?;
            if !issues[parent].kind.can_parent() {
                return Err(format!("{} is a {}. Only an epic or a feature can be a parent: change its type first.", issues[parent].key, issues[parent].kind.label().to_lowercase()));
            }
            let mut seen: HashSet<String> = HashSet::new();
            let mut up = Some(parent);
            while let Some(i) = up.filter(|i| !seen.contains(&issues[*i].key)) {
                if issues[i].key == key {
                    return Err(format!("{} is under {key} already, so {key} can't go under it.", issues[parent].key));
                }
                seen.insert(issues[i].key.clone());
                up = issues[i].parent.as_deref().and_then(|p| find(issues, p));
            }
            if let Some(old) = issues[at].parent.clone() {
                let old_at = find(issues, &old);
                log(issues, old_at, "link", format!("Child removed: {key}"))?;
            }
            let parent_key = issues[parent].key.clone();
            log(issues, Some(at), "link", format!("Parent: {parent_key}"))?;
            log(issues, Some(parent), "link", format!("Child: {key}"))?;
            issues[at].parent = Some(parent_key);
            changed = true;
        }
    }

    for raw in key_list(&op.related_add) {
        let peer = other(issues, &raw)?;
        if issues[at].related.as_ref().is_some_and(|r| r.contains(&raw)) {
            continue;
        }
        let key = issues[at].key.clone();
        if issues[at].related.as_ref().map_or(0, Vec::len) >= MAX_LINKS {
            return Err(format!("{key} already has {MAX_LINKS} related issues, the most one keeps."));
        }
        if issues[peer].related.as_ref().map_or(0, Vec::len) >= MAX_LINKS {
            return Err(format!("{} already has {MAX_LINKS} related issues, the most one keeps.", issues[peer].key));
        }
        issues[at].related.get_or_insert_with(Vec::new).push(raw.clone());
        issues[peer].related.get_or_insert_with(Vec::new).push(key.clone());
        log(issues, Some(at), "link", format!("Linked to {raw} as related"))?;
        log(issues, Some(peer), "link", format!("Linked to {key} as related"))?;
        changed = true;
    }
    for raw in key_list(&op.related_remove) {
        if !issues[at].related.as_ref().is_some_and(|r| r.contains(&raw)) {
            continue;
        }
        let key = issues[at].key.clone();
        issues[at].related = without(&issues[at].related, &raw);
        let peer = find(issues, &raw);
        if let Some(peer) = peer {
            issues[peer].related = without(&issues[peer].related, &key);
        }
        log(issues, Some(at), "link", format!("Unlinked from {raw} (related)"))?;
        log(issues, peer, "link", format!("Unlinked from {key} (related)"))?;
        changed = true;
    }

    for raw in key_list(&op.blocked_by_add) {
        let blocker = other(issues, &raw)?;
        if issues[at].blocked_by.as_ref().is_some_and(|b| b.contains(&raw)) {
            continue;
        }
        let key = issues[at].key.clone();
        if issues[at].blocked_by.as_ref().map_or(0, Vec::len) >= MAX_LINKS {
            return Err(format!("{key} is already blocked by {MAX_LINKS} issues, the most one keeps."));
        }
        // A blocker that waits on this issue, however far down the chain, would leave both stuck.
        let mut seen: HashSet<usize> = HashSet::new();
        let mut stack = vec![blocker];
        while let Some(next) = stack.pop() {
            if !seen.insert(next) {
                continue;
            }
            if issues[next].key == key {
                return Err(format!("{raw} is waiting on {key} already, so {key} can't wait on it: they would block each other."));
            }
            stack.extend(issues[next].blocked_by.iter().flatten().filter_map(|k| find(issues, k)));
        }
        issues[at].blocked_by.get_or_insert_with(Vec::new).push(raw.clone());
        log(issues, Some(at), "link", format!("Blocked by {raw}"))?;
        log(issues, Some(blocker), "link", format!("Blocks {key}"))?;
        changed = true;
    }
    for raw in key_list(&op.blocked_by_remove) {
        if !issues[at].blocked_by.as_ref().is_some_and(|b| b.contains(&raw)) {
            continue;
        }
        let key = issues[at].key.clone();
        issues[at].blocked_by = without(&issues[at].blocked_by, &raw);
        let blocker = find(issues, &raw);
        log(issues, Some(at), "link", format!("No longer blocked by {raw}"))?;
        log(issues, blocker, "link", format!("No longer blocks {key}"))?;
        changed = true;
    }
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::{Op, Store, Update},
        ops::apply_op,
        testing::{fresh, make},
    };

    fn up(s: &Store, key: &str, change: impl FnOnce(&mut Update)) -> Result<Store, String> {
        let mut update = Op::update(key);
        change(&mut update);
        apply_op(s, &Op::Update(update), &Actor::You, 2.0).map(|applied| applied.store)
    }
    fn changed(s: &Store, key: &str, change: impl FnOnce(&mut Update)) -> bool {
        let mut update = Op::update(key);
        change(&mut update);
        apply_op(s, &Op::Update(update), &Actor::You, 2.0).unwrap().changed
    }
    fn get<'a>(s: &'a Store, key: &str) -> &'a Issue {
        s.issues.iter().find(|i| i.key == key).unwrap()
    }
    fn logs(s: &Store, key: &str) -> Vec<String> {
        get(s, key).log.iter().map(|e| e.text.clone()).collect()
    }
    fn has(s: &Store, key: &str, text: &str) -> bool {
        logs(s, key).iter().any(|t| t == text)
    }
    fn keys(issues: &[&Issue]) -> Vec<String> {
        issues.iter().map(|i| i.key.clone()).collect()
    }
    fn list(items: &[&str]) -> Option<Vec<String>> {
        Some(items.iter().map(|k| (*k).to_owned()).collect())
    }
    /// WEB-1 epic, WEB-2 feature, WEB-3 task, WEB-4 bug.
    fn sample() -> Store {
        let mut s = fresh();
        s = make(&s, "Epic", |c| c.rel.kind = Some("epic".into()));
        s = make(&s, "Feature", |c| c.rel.kind = Some("feature".into()));
        s = make(&s, "Task", |_| {});
        make(&s, "Bug", |c| c.rel.kind = Some("bug".into()))
    }
    fn parent(s: &Store, key: &str, to: &str) -> Result<Store, String> {
        up(s, key, |u| u.rel.parent = Some(to.into()))
    }

    #[test]
    fn type_changes_are_logged_a_parent_cant_stop_being_an_epic_or_feature_while_it_has_children() {
        let mut s = up(&sample(), "WEB-3", |u| u.rel.kind = Some("bug".into())).unwrap();
        assert_eq!(get(&s, "WEB-3").kind, IssueType::Bug);
        assert!(has(&s, "WEB-3", "Type: Task → Bug"));
        s = parent(&s, "WEB-3", "WEB-1").unwrap();
        assert!(up(&s, "WEB-1", |u| u.rel.kind = Some("task".into())).unwrap_err().contains("has child issues"));
        assert!(up(&s, "WEB-1", |u| u.rel.kind = Some("story".into())).unwrap_err().contains("type must be one of"));
    }

    #[test]
    fn parent_only_an_epic_or_feature_in_this_project_never_itself_never_a_cycle() {
        let mut s = sample();
        assert!(parent(&s, "WEB-4", "WEB-3").unwrap_err().contains("Only an epic or a feature"));
        assert!(parent(&s, "WEB-4", "WEB-99").unwrap_err().contains("No issue WEB-99"));
        assert!(parent(&s, "WEB-1", "WEB-1").unwrap_err().contains("itself"));
        s = parent(&s, "WEB-2", "WEB-1").unwrap();
        assert!(parent(&s, "WEB-1", "WEB-2").unwrap_err().contains("can't go under it"));
        s = parent(&s, "WEB-3", "web-2").unwrap();
        assert_eq!(get(&s, "WEB-3").parent.as_deref(), Some("WEB-2"));
        assert_eq!(keys(&children_of(&s.issues, "WEB-2")), ["WEB-3"]);
        // deeper cycle: 1 > 2 > 3 can't make 3 a parent of 1, once 3 is a feature
        s = up(&s, "WEB-3", |u| u.rel.kind = Some("feature".into())).unwrap();
        assert!(parent(&s, "WEB-1", "WEB-3").unwrap_err().contains("can't go under it"));
    }

    #[test]
    fn parent_a_link_is_logged_on_both_issues_moving_and_clearing_log_on_all_three() {
        let mut s = parent(&sample(), "WEB-3", "WEB-1").unwrap();
        assert!(has(&s, "WEB-3", "Parent: WEB-1"));
        assert!(has(&s, "WEB-1", "Child: WEB-3"));
        s = parent(&s, "WEB-3", "WEB-2").unwrap();
        assert!(has(&s, "WEB-1", "Child removed: WEB-3"));
        assert!(has(&s, "WEB-2", "Child: WEB-3"));
        s = parent(&s, "WEB-3", "").unwrap();
        assert_eq!(get(&s, "WEB-3").parent, None);
        assert!(has(&s, "WEB-3", "Parent removed (was WEB-2)"));
        assert!(has(&s, "WEB-2", "Child removed: WEB-3"));
        assert!(!changed(&s, "WEB-3", |u| u.rel.parent = Some(String::new())));
    }

    #[test]
    fn parent_can_be_set_when_creating_and_parent_choices_leaves_out_itself_and_what_is_below_it() {
        let mut s = make(&sample(), "New", |c| {
            c.rel.parent = Some("WEB-1".into());
            c.rel.kind = Some("feature".into());
        });
        assert_eq!(get(&s, "WEB-5").parent.as_deref(), Some("WEB-1"));
        assert!(has(&s, "WEB-1", "Child: WEB-5"));
        assert!(apply_op(&sample(), &Op::Create(crate::domain::Create { title: "x".into(), rel: Relations { parent: Some("WEB-3".into()), ..Relations::default() }, ..Default::default() }), &Actor::You, 1.0).err().unwrap().contains("Only an epic"));
        let choices = |s: &Store, key: &str| keys(&parent_choices(&s.issues, get(s, key)));
        assert_eq!(choices(&s, "WEB-1"), ["WEB-2"]);
        assert_eq!(choices(&s, "WEB-3"), ["WEB-1", "WEB-2", "WEB-5"]);
        assert_eq!(choices(&s, "WEB-2"), ["WEB-1", "WEB-5"]);
        // WEB-5 sits under WEB-1, so WEB-1 can no longer go under it
        s = parent(&s, "WEB-1", "WEB-2").unwrap();
        assert!(choices(&s, "WEB-2").is_empty());
    }

    #[test]
    fn related_is_symmetric_deduped_capped_at_20_logged_on_both_and_removable_from_either_side() {
        let mut s = up(&sample(), "WEB-3", |u| u.rel.related_add = list(&["web-4", "WEB-4"])).unwrap();
        assert_eq!(get(&s, "WEB-3").related, list(&["WEB-4"]));
        assert_eq!(get(&s, "WEB-4").related, list(&["WEB-3"]));
        assert!(has(&s, "WEB-3", "Linked to WEB-4 as related"));
        assert!(has(&s, "WEB-4", "Linked to WEB-3 as related"));
        assert!(!changed(&s, "WEB-4", |u| u.rel.related_add = list(&["WEB-3"])));
        assert!(up(&s, "WEB-3", |u| u.rel.related_add = list(&["WEB-3"])).unwrap_err().contains("itself"));
        s = up(&s, "WEB-4", |u| u.rel.related_remove = list(&["WEB-3"])).unwrap();
        assert_eq!(get(&s, "WEB-3").related, None);
        assert_eq!(get(&s, "WEB-4").related, None);
        assert!(has(&s, "WEB-3", "Unlinked from WEB-4 (related)"));
        // the cap
        let mut big = fresh();
        for i in 0..23 {
            big = make(&big, &format!("t{i}"), |_| {});
        }
        for i in 2..=21 {
            big = up(&big, "WEB-1", |u| u.rel.related_add = Some(vec![format!("WEB-{i}")])).unwrap();
        }
        assert_eq!(get(&big, "WEB-1").related.as_ref().unwrap().len(), 20);
        assert!(up(&big, "WEB-1", |u| u.rel.related_add = list(&["WEB-22"])).unwrap_err().contains("already has 20 related"));
        assert!(up(&big, "WEB-22", |u| u.rel.related_add = list(&["WEB-1"])).unwrap_err().contains("WEB-1 already has 20"));
    }

    #[test]
    fn blocked_by_has_an_inverse_blocks_logged_on_both_and_refuses_a_loop() {
        let blocked = |s: &Store, key: &str, by: &str| up(s, key, |u| u.rel.blocked_by_add = list(&[by]));
        let mut s = blocked(&sample(), "WEB-4", "WEB-3").unwrap();
        assert_eq!(get(&s, "WEB-4").blocked_by, list(&["WEB-3"]));
        assert_eq!(keys(&blocks_of(&s.issues, "WEB-3")), ["WEB-4"]);
        assert!(has(&s, "WEB-4", "Blocked by WEB-3"));
        assert!(has(&s, "WEB-3", "Blocks WEB-4"));
        assert!(blocked(&s, "WEB-3", "WEB-4").unwrap_err().contains("block each other"));
        s = blocked(&s, "WEB-4", "WEB-2").unwrap();
        s = blocked(&s, "WEB-2", "WEB-1").unwrap();
        assert!(blocked(&s, "WEB-1", "WEB-4").unwrap_err().contains("block each other"));
        s = up(&s, "WEB-4", |u| u.rel.blocked_by_remove = list(&["WEB-3"])).unwrap();
        assert_eq!(get(&s, "WEB-4").blocked_by, list(&["WEB-2"]));
        assert!(has(&s, "WEB-3", "No longer blocks WEB-4"));
    }

    #[test]
    fn a_failed_relation_change_leaves_the_store_and_the_other_issues_as_they_were() {
        let s = up(&sample(), "WEB-3", |u| u.rel.related_add = list(&["WEB-4"])).unwrap();
        let before = s.clone();
        assert!(up(&s, "WEB-3", |u| {
            u.rel.related_add = list(&["WEB-1"]);
            u.rel.parent = Some("WEB-4".into());
        })
        .is_err());
        assert_eq!(s, before);
    }

    fn removed(s: &Store, key: &str, children: bool) -> Store {
        apply_op(s, &Op::Remove { key: key.into(), children }, &Actor::You, 9.0).unwrap().store
    }

    #[test]
    fn deleting_an_issue_removes_it_from_the_others_relations_and_says_so_in_their_logs() {
        let mut s = up(&sample(), "WEB-3", |u| {
            u.rel.parent = Some("WEB-1".into());
            u.rel.related_add = list(&["WEB-2"]);
            u.rel.blocked_by_add = list(&["WEB-4"]);
        })
        .unwrap();
        s = parent(&s, "WEB-2", "WEB-1").unwrap();
        let gone = removed(&s, "WEB-1", false);
        assert_eq!(get(&gone, "WEB-3").parent, None);
        assert_eq!(get(&gone, "WEB-2").parent, None);
        assert!(has(&gone, "WEB-3", "Parent WEB-1 was deleted"));
        let gone2 = removed(&s, "WEB-2", false);
        assert_eq!(get(&gone2, "WEB-3").related, None);
        assert!(has(&gone2, "WEB-3", "WEB-2 was deleted: unlinked (related)"));
        let gone3 = removed(&s, "WEB-4", false);
        assert_eq!(get(&gone3, "WEB-3").blocked_by, None);
        assert!(has(&gone3, "WEB-3", "WEB-4 was deleted: no longer blocking"));
        assert_eq!(gone3.next, s.next);
    }

    #[test]
    fn deleting_with_children_takes_the_whole_subtree_the_rest_let_go_of_it_in_their_logs() {
        let mut s = parent(&sample(), "WEB-2", "WEB-1").unwrap();
        s = parent(&s, "WEB-3", "WEB-2").unwrap();
        s = up(&s, "WEB-4", |u| {
            u.rel.related_add = list(&["WEB-3"]);
            u.rel.blocked_by_add = list(&["WEB-2"]);
        })
        .unwrap();
        assert_eq!(keys(&descendants_of(&s.issues, "WEB-1")), ["WEB-2", "WEB-3"]);
        let gone = removed(&s, "WEB-1", true);
        assert_eq!(gone.issues.iter().map(|i| i.key.as_str()).collect::<Vec<_>>(), ["WEB-4"]);
        assert_eq!(get(&gone, "WEB-4").related, None);
        assert_eq!(get(&gone, "WEB-4").blocked_by, None);
        assert!(has(&gone, "WEB-4", "WEB-3 was deleted: unlinked (related)"));
        assert!(has(&gone, "WEB-4", "WEB-2 was deleted: no longer blocking"));
        assert_eq!(gone.next, s.next);
        let kept = removed(&s, "WEB-1", false);
        assert_eq!(kept.issues.iter().map(|i| i.key.as_str()).collect::<Vec<_>>(), ["WEB-2", "WEB-3", "WEB-4"]);
        assert_eq!(get(&kept, "WEB-2").parent, None);
        assert_eq!(get(&kept, "WEB-3").parent.as_deref(), Some("WEB-2"));
    }

    #[test]
    fn progress_counts_done_of_all_children_canceled_ones_left_out() {
        let mut s = fresh();
        for (title, status) in [("a", "done"), ("b", "todo"), ("c", "canceled"), ("d", "done")] {
            s = make(&s, title, |c| c.status = Some(status.into()));
        }
        assert_eq!(progress(&s.issues), (2, 3));
        assert_eq!(progress(&[] as &[Issue]), (0, 0));
    }
}
