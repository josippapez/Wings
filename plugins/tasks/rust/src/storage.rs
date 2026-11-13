//! Issues live in `wings.storage`, one entry per issue (`issues/<slot>/<KEY>`) and one for the project's prefix and counter
//! (`issues/<slot>`), where the slot is a short id of the project's path. Only the main script writes them: pages ask it to
//! apply operations and the tools run the same ones, so a page and an agent never overwrite each other. This is the only
//! module that reads and writes the issues.

use crate::{
    domain::{FORMAT_VERSION, ISSUE_BUDGET, Issue, Status, Store, key_number, limits, prefix_for},
    kv,
};
use serde_json::{Value, json};

pub fn empty_store(project_name: &str) -> Store {
    Store { prefix: prefix_for(project_name), next: 1, issues: Vec::new() }
}

/// A project's slot: 64 bits of a hash of its path, as 16 hex digits. A key is at most 128 characters and a path can be
/// longer, so the path itself is only kept once, in the project's entry, where `read_store` checks it. The hash is the one
/// the TypeScript build used, since the slot names where existing issues are.
pub fn slot_of(project_id: &str) -> String {
    let (mut h1, mut h2) = (0xdead_beef_u32, 0x41c6_ce57_u32);
    for unit in project_id.encode_utf16() {
        let c = u32::from(unit);
        h1 = (h1 ^ c).wrapping_mul(2_654_435_761);
        h2 = (h2 ^ c).wrapping_mul(1_597_334_677);
    }
    h1 = (h1 ^ (h1 >> 16)).wrapping_mul(2_246_822_507) ^ (h2 ^ (h2 >> 13)).wrapping_mul(3_266_489_909);
    h2 = (h2 ^ (h2 >> 16)).wrapping_mul(2_246_822_507) ^ (h1 ^ (h1 >> 13)).wrapping_mul(3_266_489_909);
    format!("{h2:08x}{h1:08x}")
}

fn project_key(project_id: &str) -> String {
    format!("issues/{}", slot_of(project_id))
}

fn issue_key(project_id: &str, key: &str) -> String {
    format!("{}/{key}", project_key(project_id))
}

/// Where a project's issues were before one entry per issue: all of them in one entry.
fn old_key(project_id: &str) -> String {
    format!("issues:{project_id}")
}

fn legacy_key(project_id: &str) -> String {
    format!("tasks:{project_id}")
}

/// What an older entry holds. Wings keeps keys of up to 128 characters, so a project whose path makes its older key longer
/// never had one, and asking for it would only be refused.
fn held(key: &str) -> Result<Value, String> {
    if key.len() > 128 { Ok(Value::Null) } else { kv::get(key) }
}

/// Format `version`, with its list of issues.
fn is_shape(value: &Value, version: u64) -> bool {
    value["v"] == version && value["issues"].is_array() && value["prefix"].is_string() && value["next"].is_number()
}

fn is_project(value: &Value) -> bool {
    value["v"] == FORMAT_VERSION && value["path"].is_string() && value["prefix"].is_string() && value["next"].is_number()
}

fn store_of(value: &Value) -> Store {
    Store {
        prefix: value["prefix"].as_str().unwrap_or_default().to_owned(),
        next: value["next"].as_f64().unwrap_or(1.0) as u64,
        issues: value["issues"].as_array().into_iter().flatten().filter_map(Issue::from_value).collect(),
    }
}

/// A store as the entries that hold it, project entry last.
pub fn entries_of(project_id: &str, store: &Store) -> Vec<(String, Value)> {
    let mut entries: Vec<(String, Value)> = store.issues.iter().map(|i| (issue_key(project_id, &i.key), i.to_value())).collect();
    entries.push((project_key(project_id), project_of(project_id, store)));
    entries
}

fn project_of(project_id: &str, store: &Store) -> Value {
    json!({ "v": FORMAT_VERSION, "path": project_id, "prefix": store.prefix, "next": store.next })
}

/// Format 2, before types and relations: every issue is a task.
pub fn migrate_v2(value: &Value) -> Option<Store> {
    is_shape(value, 2).then(|| store_of(value))
}

/// Tasks as a store: oldest first they get the numbers 1, 2, 3, notes become the description, doing becomes in progress,
/// and each issue starts a log with "Imported from Tasks" by you at the time the task was made. Entries that aren't tasks
/// are dropped.
pub fn migrate_tasks(value: &Value, project_name: &str) -> Store {
    let mut store = empty_store(project_name);
    let Some(old) = value.as_array() else {
        return store;
    };
    let mut old: Vec<(usize, f64, &Value)> = old.iter().enumerate().filter(|(_, t)| t["title"].is_string() && t["status"].is_string()).map(|(i, t)| (i, t["createdAt"].as_f64().unwrap_or(0.0), t)).collect();
    old.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
    for (_, at, t) in old {
        let status = match t["status"].as_str() {
            Some("doing") => Status::InProgress,
            Some("done") => Status::Done,
            _ => Status::Todo,
        };
        let mut issue = Issue::from_value(&json!({
            "key": format!("{}-{}", store.prefix, store.next),
            "title": t["title"].as_str().unwrap_or_default().chars().take(limits::TITLE).collect::<String>(),
            "status": status.as_str(),
            "type": "task",
            "sessions": [],
            "log": [{ "at": at, "by": "you", "kind": "created", "text": "Imported from Tasks" }],
            "createdAt": at,
            "updatedAt": t["updatedAt"].as_f64().unwrap_or(at),
        }))
        .expect("a key");
        store.next += 1;
        if let Some(notes) = t["notes"].as_str().filter(|n| !n.trim().is_empty()) {
            issue.description = Some(notes.chars().take(limits::DESCRIPTION).collect());
        }
        if let Some(branch) = t["links"]["branch"].as_str().filter(|b| !b.is_empty()) {
            issue.links = Some(crate::domain::Links { branch: Some(branch.chars().take(limits::BRANCH).collect()), ..Default::default() });
        }
        store.issues.push(issue);
    }
    store
}

/// A project's store. Pages and main both read through here: a project that still has only older entries reads as migrated.
pub fn read_store(project_id: &str, project_name: &str) -> Result<Store, String> {
    let prefix = project_key(project_id);
    let entries = kv::entries(&prefix)?;
    let entries = entries.as_array().map_or(&[][..], Vec::as_slice);
    let Some(project) = entries.iter().find(|e| e[0] == prefix.as_str()).map(|e| &e[1]).filter(|p| is_project(p)) else {
        return read_old(project_id, project_name);
    };
    if project["path"] != project_id {
        return Err(format!("Issues keeps {} where {project_id} would go. Rename one of the two folders.", project["path"].as_str().unwrap_or_default()));
    }
    let under = format!("{prefix}/");
    let mut issues: Vec<Issue> = entries.iter().filter(|e| e[0].as_str().is_some_and(|k| k.starts_with(&under))).filter_map(|e| Issue::from_value(&e[1])).collect();
    issues.sort_by_key(|i| key_number(&i.key));
    Ok(Store { prefix: project["prefix"].as_str().unwrap_or_default().to_owned(), next: project["next"].as_f64().unwrap_or(1.0) as u64, issues })
}

/// What a project held before one entry per issue: all of it in `issues:<path>` (format 3 or 2), or old tasks.
fn read_old(project_id: &str, project_name: &str) -> Result<Store, String> {
    let value = held(&old_key(project_id))?;
    if is_shape(&value, FORMAT_VERSION) {
        return Ok(store_of(&value));
    }
    if let Some(v2) = migrate_v2(&value) {
        return Ok(v2);
    }
    Ok(migrate_tasks(&held(&legacy_key(project_id))?, project_name))
}

/// The size of a value as saved: UTF-8 bytes of its JSON.
pub fn estimate_bytes(value: &Value) -> usize {
    value.to_string().len()
}

/// Saves the change from `before` to `after`: the issues that differ, and the project's entry when its counter moved. The
/// counter goes first, so a crash in between can skip a number but never hand one out twice.
pub fn write_store(project_id: &str, before: &Store, after: &Store) -> Result<(), String> {
    let changed: Vec<&Issue> = after.issues.iter().filter(|i| before.issues.iter().find(|was| was.key == i.key) != Some(*i)).collect();
    let rows: Vec<(&Issue, Value)> = changed.into_iter().map(|i| (i, i.to_value())).collect();
    for (issue, value) in &rows {
        let bytes = estimate_bytes(value);
        if bytes > ISSUE_BUDGET {
            return Err(format!("{} would take {} KB, over the {} KB one issue may use. Carry on in a new issue.", issue.key, (bytes as f64 / 1000.0).round(), ISSUE_BUDGET / 1000));
        }
    }
    if after.next != before.next || after.prefix != before.prefix {
        kv::set(&project_key(project_id), &project_of(project_id, after))?;
    }
    for (issue, value) in &rows {
        kv::set(&issue_key(project_id, &issue.key), value)?;
    }
    for issue in &before.issues {
        if !after.issues.iter().any(|i| i.key == issue.key) {
            kv::delete(&issue_key(project_id, &issue.key))?;
        }
    }
    Ok(())
}

/// Once per project: saves what it held in older entries (format 3 or 2 in `issues:<path>`, or old tasks) as one entry per
/// issue, then drops the older entries. The project's entry is written last, so until then pages still read the older ones,
/// and a migration cut short starts again. A no-op when the project already has its entry.
pub fn migrate_legacy(project_id: &str, project_name: &str) -> Result<bool, String> {
    if is_project(&kv::get(&project_key(project_id))?) {
        return Ok(false);
    }
    let current = held(&old_key(project_id))?;
    let store = if is_shape(&current, FORMAT_VERSION) {
        store_of(&current)
    } else if let Some(v2) = migrate_v2(&current) {
        v2
    } else {
        let old = held(&legacy_key(project_id))?;
        if !old.is_array() {
            return Ok(false);
        }
        migrate_tasks(&old, project_name)
    };
    for (key, value) in entries_of(project_id, &store) {
        kv::set(&key, &value)?;
    }
    kv::delete(&old_key(project_id))?;
    kv::delete(&legacy_key(project_id))?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::{Actor, Agent, Op},
        kv::memory,
        ops::apply_op,
        testing::{create_with, fresh, make},
    };

    const PATH: &str = "/work/web";

    fn at(key: &str) -> String {
        if key.is_empty() { format!("issues/{}", slot_of(PATH)) } else { format!("issues/{}/{key}", slot_of(PATH)) }
    }

    #[test]
    fn the_slot_is_the_one_the_typescript_build_made_so_existing_issues_are_found() {
        // Computed by the TypeScript build's `slotOf`.
        for (path, slot) in [("/work/web", "c10b05ffeab5d23e"), ("/work/other", "79d4fae2c27eb043"), ("proj1", "98edeb2f691efab1"), ("/Users/me/Ünï😀/project", "1e8666917051b16f"), ("", "488bdcb81aee8d83")] {
            assert_eq!(slot_of(path), slot, "{path}");
        }
        let long = format!("/Users/me/{}project", "deep/".repeat(60));
        assert_eq!(slot_of(&long).len(), 16);
        let slots: std::collections::HashSet<String> = (0..2000).map(|i| slot_of(&format!("/work/project-{i}"))).collect();
        assert_eq!(slots.len(), 2000);
        assert!(format!("issues/{}/ABC-999999999", slot_of(&long)).len() <= 128);
    }

    #[test]
    fn a_project_with_a_long_path_has_no_older_entries_to_read_instead_of_a_key_wings_refuses() {
        memory::reset(&[]);
        let path = format!("/work/{}", "deep/".repeat(30));
        assert!(format!("issues:{path}").len() > 128);
        let store = read_store(&path, "web").unwrap();
        assert!(store.issues.is_empty());
        assert_eq!(migrate_legacy(&path, "web"), Ok(false));
    }

    #[test]
    fn migration_tasks_become_issues_oldest_first_notes_and_doing_mapped_branch_kept() {
        let store = migrate_tasks(
            &json!([
                { "id": "b", "title": "Second", "notes": "n", "status": "doing", "links": { "branch": "feat/x" }, "createdAt": 20, "updatedAt": 30 },
                { "id": "a", "title": "First", "status": "todo", "createdAt": 10, "updatedAt": 10 },
                { "id": "c", "title": "Third", "status": "done", "createdAt": 30, "updatedAt": 40 },
                { "junk": true },
            ]),
            "Web Shop",
        );
        assert_eq!((store.prefix.as_str(), store.next), ("WEB", 4));
        assert_eq!(store.issues.iter().map(|i| (i.key.as_str(), i.title.as_str(), i.status)).collect::<Vec<_>>(), [("WEB-1", "First", Status::Todo), ("WEB-2", "Second", Status::InProgress), ("WEB-3", "Third", Status::Done)]);
        let second = &store.issues[1];
        assert_eq!(second.description.as_deref(), Some("n"));
        assert_eq!(second.links.as_ref().unwrap().branch.as_deref(), Some("feat/x"));
        assert_eq!(second.updated_at, 30.0);
        assert_eq!(second.to_value()["log"], json!([{ "at": 20, "by": "you", "kind": "created", "text": "Imported from Tasks" }]));
        assert!(migrate_tasks(&Value::Null, "x").issues.is_empty());
    }

    #[test]
    fn a_format_2_store_reads_as_format_3_with_every_issue_a_task() {
        let v2 = json!({ "v": 2, "prefix": "WEB", "next": 3, "issues": [{ "key": "WEB-1", "title": "a", "status": "todo", "sessions": [], "log": [], "createdAt": 1, "updatedAt": 1 }, { "key": "WEB-2", "title": "b", "status": "done", "sessions": [], "log": [], "createdAt": 1, "updatedAt": 1 }] });
        let migrated = migrate_v2(&v2).unwrap();
        assert_eq!(migrated.issues.iter().map(|i| i.kind.as_str()).collect::<Vec<_>>(), ["task", "task"]);
        assert_eq!(migrated.next, 3);
        let mut v3 = v2.clone();
        v3["v"] = json!(3);
        assert!(migrate_v2(&v3).is_none());
        assert_eq!(migrate_tasks(&json!([{ "title": "x", "status": "todo", "createdAt": 1 }]), "web").issues[0].kind.as_str(), "task");
    }

    fn old_store() -> Store {
        let mut s = fresh();
        for t in ["a", "b", "c"] {
            s = make(&s, t, |_| {});
        }
        s
    }

    fn one_entry(s: &Store, v: u64) -> Value {
        json!({ "v": v, "prefix": s.prefix, "next": s.next, "issues": s.issues.iter().map(Issue::to_value).collect::<Vec<_>>() })
    }

    #[test]
    fn migration_a_project_in_one_entry_or_old_tasks_becomes_one_entry_per_issue_and_the_old_entries_go() {
        let s = old_store();
        let mut v2 = one_entry(&s, 2);
        for issue in v2["issues"].as_array_mut().unwrap() {
            issue.as_object_mut().unwrap().remove("type");
        }
        for (initial, keys) in [
            (vec![(format!("issues:{PATH}"), one_entry(&s, 3))], vec!["WEB-1", "WEB-2", "WEB-3"]),
            (vec![(format!("issues:{PATH}"), v2)], vec!["WEB-1", "WEB-2", "WEB-3"]),
            (vec![(format!("tasks:{PATH}"), json!([{ "title": "x", "status": "todo", "createdAt": 1 }]))], vec!["WEB-1"]),
        ] {
            let mut stored: Vec<(&str, Value)> = initial.iter().map(|(k, v)| (k.as_str(), v.clone())).collect();
            stored.push(("issues:/other", one_entry(&s, 3)));
            memory::reset(&stored);
            let before = read_store(PATH, "web").unwrap();
            assert_eq!(migrate_legacy(PATH, "web"), Ok(true));
            let mut expected: Vec<String> = ["issues:/other".to_owned(), at("")].into_iter().chain(keys.iter().map(|k| at(k))).collect();
            expected.sort();
            assert_eq!(memory::keys(), expected);
            assert_eq!(memory::peek(&at("")), Some(json!({ "v": 3, "path": PATH, "prefix": "WEB", "next": keys.len() + 1 })));
            assert_eq!(read_store(PATH, "web").unwrap(), before);
            assert_eq!(migrate_legacy(PATH, "web"), Ok(false), "once");
        }
        memory::reset(&[]);
        assert_eq!(migrate_legacy(PATH, "web"), Ok(false), "nothing to migrate");
    }

    #[test]
    fn migration_cut_short_pages_read_the_old_entry_until_the_projects_entry_is_written_and_the_next_start_migrates_again() {
        let mut s = fresh();
        for t in ["a", "b"] {
            s = make(&s, t, |_| {});
        }
        let mut half = s.issues[0].to_value();
        half["title"] = json!("half");
        memory::reset(&[(&format!("issues:{PATH}"), one_entry(&s, 3)), (&at("WEB-1"), half)]);
        assert_eq!(read_store(PATH, "web").unwrap().issues[0].title, "a");
        migrate_legacy(PATH, "web").unwrap();
        assert_eq!(memory::peek(&at("WEB-1")).unwrap()["title"], "a");
        assert_eq!(read_store(PATH, "web").unwrap().issues.len(), 2);
    }

    fn save(op: &Op) -> Vec<(String, String)> {
        let before = read_store(PATH, "web").unwrap();
        let after = apply_op(&before, op, &Actor::You, 5.0).unwrap().store;
        memory::clear_writes();
        write_store(PATH, &before, &after).unwrap();
        memory::writes()
    }

    #[test]
    fn writes_a_note_saves_only_its_issue_a_create_saves_the_counter_before_the_issue_a_removal_deletes_its_entry() {
        memory::reset(&[]);
        let set = |key: &str| ("set".to_owned(), at(key));
        let create = |title: &str| Op::Create(Op::create(title));
        assert_eq!(save(&create("a")), [set(""), set("WEB-1")]);
        save(&create("b"));
        save(&create("c"));
        assert_eq!(save(&Op::Note { key: "WEB-2".into(), text: "next".into() }), [set("WEB-2")]);
        let related = Op::Update(Update { key: "WEB-1".into(), rel: crate::domain::Relations { related_add: Some(vec!["WEB-3".into()]), ..Default::default() }, ..Default::default() });
        assert_eq!(save(&related), [set("WEB-1"), set("WEB-3")]);
        assert_eq!(save(&Op::Remove { key: "WEB-3".into(), children: false }), [set("WEB-1"), ("delete".to_owned(), at("WEB-3"))]);
        let read = read_store(PATH, "web").unwrap();
        assert_eq!(read.issues.iter().map(|i| i.key.as_str()).collect::<Vec<_>>(), ["WEB-1", "WEB-2"]);
        assert_eq!(read.next, 4);
        assert_eq!(memory::peek(&at("")).unwrap()["next"], 4);
    }

    #[test]
    fn issues_read_back_in_key_order_past_web_9_and_another_path_in_the_same_slot_is_refused_not_merged() {
        let mut s = fresh();
        for i in 0..11 {
            s = make(&s, &format!("t{i}"), |_| {});
        }
        memory::reset(&[]);
        write_store(PATH, &fresh(), &s).unwrap();
        assert_eq!(read_store(PATH, "web").unwrap().issues.iter().map(|i| i.key.clone()).collect::<Vec<_>>(), s.issues.iter().map(|i| i.key.clone()).collect::<Vec<_>>());
        let mut project = memory::peek(&at("")).unwrap();
        project["path"] = json!("/elsewhere");
        memory::reset(&[(&at(""), project)]);
        assert_eq!(read_store(PATH, "web").unwrap_err(), "Issues keeps /elsewhere where /work/web would go. Rename one of the two folders.");
    }

    #[test]
    fn size_an_issue_with_a_full_description_and_48_full_notes_fits_one_that_would_go_over_is_refused_and_nothing_is_saved() {
        let mut s = make(&fresh(), "big", |c| c.description = Some("d".repeat(limits::DESCRIPTION)));
        let note = |store: &Store| apply_op(store, &Op::Note { key: "WEB-1".into(), text: "n".repeat(limits::NOTE) }, &Actor::You, 2.0).unwrap().store;
        for _ in 0..48 {
            s = note(&s);
        }
        memory::reset(&[]);
        write_store(PATH, &fresh(), &s).unwrap();
        assert!(estimate_bytes(&s.issues[0].to_value()) < ISSUE_BUDGET);
        let full = note(&note(&note(&s)));
        let error = write_store(PATH, &s, &full).unwrap_err();
        assert!(error.starts_with("WEB-1 would take 1") && error.ends_with("KB, over the 1000 KB one issue may use. Carry on in a new issue."), "{error}");
        assert_eq!(memory::peek(&at("WEB-1")).unwrap()["log"].as_array().unwrap().len(), 49);
    }

    #[test]
    fn size_200_issues_with_10_log_entries_each_are_about_2_kb_an_issue_and_the_estimate_counts_utf_8_bytes() {
        let mut s = fresh();
        let session = Actor::Agent(Agent { id: "6f1c2a90-3b7e-4d51-9a02-5c8e1d4b7a11".into(), name: Some("checkout summary".into()), ..Agent::default() });
        for i in 0..200 {
            s = create_with(&s, &format!("Issue number {i} with a title of a normal length"), |c| c.description = Some("Goal: do the thing.\n\nScope: this and that.\n\nDone when: it works and has a test.".into()), &Actor::You, f64::from(i)).store;
            for j in 0..9 {
                s = apply_op(&s, &Op::Note { key: format!("WEB-{}", i + 1), text: "Did the next step of the work and checked that it still passes. Next: the follow-up.".into() }, &session, f64::from(i + j)).unwrap().store;
            }
        }
        let bytes: usize = s.issues.iter().map(|i| estimate_bytes(&i.to_value())).sum();
        assert!(bytes / 200 < 2_500, "{} bytes an issue", bytes / 200);
        assert_eq!(estimate_bytes(&json!({ "prefix": "é" })), "{\"prefix\":\"é\"}".len());
    }

    use crate::domain::Update;
}
