//! The append-only log of an issue, the sessions it records, and the files its journal reported.

use crate::domain::{Actor, FileChange, FileSession, Issue, LogEntry, SessionRef, limits};

/// Records that `actor`'s session worked on `issue`: adds it, or refreshes its name and time. Users aren't sessions.
pub fn touch(issue: &mut Issue, actor: &Actor, now: f64) {
    issue.updated_at = now;
    let Some(agent) = actor.agent().filter(|agent| !agent.id.is_empty()) else {
        return;
    };
    match issue.sessions.iter_mut().find(|s| s.id == agent.id) {
        Some(known) => {
            known.seen = now;
            if agent.name.is_some() {
                known.name = agent.name.clone();
            }
            if agent.kind.is_some() {
                known.kind = agent.kind.clone();
            }
        }
        None => issue.sessions.push(SessionRef { id: agent.id.clone(), name: agent.name.clone(), kind: agent.kind.clone(), seen: now }),
    }
}

pub fn append(issue: &mut Issue, entries: Vec<LogEntry>) -> Result<(), String> {
    if issue.log.len() + entries.len() > limits::LOG {
        return Err(format!("{} already has {} log entries, the most one issue keeps.", issue.key, limits::LOG));
    }
    issue.log.extend(entries);
    Ok(())
}

/// Like `append`, but drops what doesn't fit instead of failing: for entries the plugin writes on its own.
pub fn append_loose(issue: &mut Issue, entries: Vec<LogEntry>) {
    let room = limits::LOG.saturating_sub(issue.log.len());
    issue.log.extend(entries.into_iter().take(room));
}

pub fn entry(at: f64, by: &Actor, kind: &str, text: impl Into<String>) -> LogEntry {
    LogEntry { at, by: by.clone(), kind: kind.to_owned(), text: text.into() }
}

fn cut(text: &str, max: usize) -> String {
    if text.chars().count() > max { format!("{}…", text.chars().take(max - 1).collect::<String>()) } else { text.to_owned() }
}

/// `old` with `incoming` upserted by path as `by` at `now`: the newest description, time and session win, and past
/// `limits::FILES` the oldest rows go.
pub fn upsert_files(old: Option<&[FileChange]>, incoming: &[(String, String)], by: &Actor, now: f64) -> Vec<FileChange> {
    let session = by.agent().filter(|agent| !agent.id.is_empty()).map(|agent| FileSession { id: agent.id.clone(), name: agent.name.clone(), kind: agent.kind.clone() });
    let mut fresh: Vec<FileChange> = Vec::new();
    for (path, what) in incoming {
        let path = cut(path.trim(), limits::FILE_PATH);
        if path.is_empty() {
            continue;
        }
        let row = FileChange { path: path.clone(), what: cut(what.trim(), limits::FILE_WHAT), at: now, session: session.clone() };
        // A later row for the same path replaces the earlier one in the same place, as a map keeps its first insertion's order.
        match fresh.iter_mut().find(|f| f.path == path) {
            Some(same) => *same = row,
            None => fresh.push(row),
        }
    }
    let kept = old.unwrap_or_default().iter().filter(|f| !fresh.iter().any(|n| n.path == f.path)).cloned();
    let mut all: Vec<FileChange> = fresh.iter().cloned().chain(kept).collect();
    // A stable sort, newest first.
    all.sort_by(|a, b| b.at.total_cmp(&a.at));
    all.truncate(limits::FILES);
    all
}
