//! The Claude Code transcript entries a plugin may read, from a session's file and as new lines are appended.

use std::{fs, path::PathBuf};

use serde::Serialize;
use serde_json::Value;

use crate::manifest::Plugin;

/// A line one session appended, from the pane it runs in.
pub struct Line {
    pub pane_id: String,
    pub session_id: String,
    pub bytes: Vec<u8>,
}

/// Whether a transcript entry is of a `permissions.transcript` type. `attachment:<kind>` is one kind of
/// attachment: attachments include whole files and hook output, so most plugins want only one kind.
fn is_type(allowed: &str, entry: &Value) -> bool {
    let kind = entry.get("type").and_then(Value::as_str);
    match allowed.strip_prefix("attachment:") {
        Some(sub) => kind == Some("attachment") && entry.pointer("/attachment/type").and_then(Value::as_str) == Some(sub),
        None => kind == Some(allowed),
    }
}

/// Text every line of that type holds, so the others are skipped before they're parsed.
fn type_marker(allowed: &str) -> String {
    format!("\"type\":\"{}\"", allowed.strip_prefix("attachment:").unwrap_or(allowed))
}

/// Entries of the allowed types from a Claude Code session transcript, oldest first.
/// The most entries `last` can ask for.
pub const TRANSCRIPT_LAST_MAX: usize = 1000;
/// The most with `cut`, whose entries are small: a 2,000-entry session of 65 MB reads as about 8 MB with its strings cut
/// to 4,000 characters (measured), much what 400 whole entries of it take.
pub const TRANSCRIPT_CUT_LAST_MAX: usize = 10_000;
/// The fewest characters `cut` keeps of a string.
pub const TRANSCRIPT_CUT_MIN: usize = 200;

/// `value` with each string over `cut` characters cut to that many, as a long tool output or a whole file is. A picture
/// stays whole, since it can't be shown in part.
fn cut_strings(value: &mut Value, cut: usize) {
    match value {
        Value::String(text) => {
            if let Some((at, _)) = text.char_indices().nth(cut) {
                text.truncate(at);
            }
        }
        Value::Array(items) => items.iter_mut().for_each(|item| cut_strings(item, cut)),
        Value::Object(map) if map.get("type").and_then(Value::as_str) == Some("image") => {}
        Value::Object(map) => map.values_mut().for_each(|item| cut_strings(item, cut)),
        _ => {}
    }
}

/// The transcript of `session_id` in any of the Claude Code account folders `claude_dirs`.
pub fn find_transcript(claude_dirs: &[PathBuf], session_id: &str) -> Option<PathBuf> {
    // The id becomes a file name, so only the UUID shape Claude Code uses.
    if session_id.len() != 36 || !session_id.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-') {
        return None;
    }
    let file = format!("{session_id}.jsonl");
    claude_dirs.iter().find_map(|dir| fs::read_dir(dir.join("projects")).ok()?.flatten().map(|d| d.path().join(&file)).find(|p| p.is_file()))
}

/// The transcript of the subagent `agent` of the session `session_id`, which Claude Code keeps in a folder named after
/// the session, beside the session's own file: `<project>/<session>/subagents/agent-<agent>.jsonl`. Only that folder:
/// the agent id becomes a file name, so it is Claude Code's shape, letters and digits.
pub fn find_subagent_transcript(claude_dirs: &[PathBuf], session_id: &str, agent: &str) -> Option<PathBuf> {
    if agent.is_empty() || agent.len() > 64 || !agent.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return None;
    }
    let session = find_transcript(claude_dirs, session_id)?;
    let file = session.parent()?.join(session_id).join("subagents").join(format!("agent-{agent}.jsonl"));
    file.is_file().then_some(file)
}

/// With `last`, only the newest that many entries, read from the end. A long session's transcript runs to tens
/// of MB, and most callers want only its latest state.
pub fn transcript_entries(plugin: &Plugin, claude_dirs: &[PathBuf], session_id: &str, types: &[String], last: Option<usize>) -> Result<Vec<Value>, String> {
    agent_entries(plugin, claude_dirs, session_id, None, types, last, None)
}

/// `transcript_entries` of the session's subagent `agent`, or of the session itself without one. With `cut`, each string
/// in an entry keeps at most that many characters (`TRANSCRIPT_CUT_MIN` at least), and `last` can ask for more entries.
pub fn agent_entries(
    plugin: &Plugin,
    claude_dirs: &[PathBuf],
    session_id: &str,
    agent: Option<&str>,
    types: &[String],
    last: Option<usize>,
    cut: Option<usize>,
) -> Result<Vec<Value>, String> {
    let allowed = &plugin.manifest.permissions.transcript;
    if let Some(t) = types.iter().find(|t| !allowed.contains(t)) {
        return Err(format!("{} may not read {t:?} transcript entries", plugin.manifest.id));
    }
    if session_id.len() != 36 || !session_id.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-') {
        return Err("not a session id".into());
    }
    let path = match agent {
        None => find_transcript(claude_dirs, session_id).ok_or("no transcript for that session")?,
        Some(agent) => find_subagent_transcript(claude_dirs, session_id, agent).ok_or("no transcript for that subagent of the session")?,
    };
    let text = fs::read_to_string(path).map_err(|e| e.to_string())?;
    let patterns: Vec<String> = types.iter().map(|t| type_marker(t)).collect();
    let mentions = |l: &&str| patterns.iter().any(|p| l.contains(p.as_str()));
    // A nested object can say `"type":"user"` too, so the entry's own type is checked once it's parsed.
    let cut = cut.map(|cut| cut.max(TRANSCRIPT_CUT_MIN));
    let entry = |l: &str| {
        let mut entry = serde_json::from_str::<Value>(l).ok().filter(|v| types.iter().any(|t| is_type(t, v)))?;
        if let Some(cut) = cut {
            cut_strings(&mut entry, cut);
        }
        Some(entry)
    };
    let most = if cut.is_some() { TRANSCRIPT_CUT_LAST_MAX } else { TRANSCRIPT_LAST_MAX };
    Ok(match last {
        Some(n) => {
            let mut newest: Vec<Value> = text.lines().rev().filter(mentions).filter_map(entry).take(n.min(most)).collect();
            newest.reverse();
            newest
        }
        None => text.lines().filter(mentions).filter_map(entry).collect(),
    })
}

/// A transcript entry Claude just wrote, for `wings.onTranscript`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptEvent {
    pub session_id: String,
    pub pane_id: String,
    pub entry: Value,
    /// The running plugins whose `permissions.transcript` has the entry's type. Only they get it.
    pub plugins: Vec<String>,
}

/// The event for a line a transcript appended, if it's a JSON entry of a type one of `running` may read.
pub fn transcript_event(running: &[Plugin], line: Line) -> Option<TranscriptEvent> {
    let text = std::str::from_utf8(&line.bytes).ok()?;
    // Most lines are large tool output of a type no plugin reads, so look before parsing.
    let types = || running.iter().flat_map(|p| &p.manifest.permissions.transcript);
    if !types().any(|t| text.contains(&type_marker(t))) {
        return None;
    }
    let entry: Value = serde_json::from_str(text).ok()?;
    let plugins: Vec<String> =
        running.iter().filter(|p| p.manifest.permissions.transcript.iter().any(|t| is_type(t, &entry))).map(|p| p.manifest.id.clone()).collect();
    (!plugins.is_empty()).then_some(TranscriptEvent { session_id: line.session_id, pane_id: line.pane_id, entry, plugins })
}

/// An OpenCode session did something, for `wings.onAgentActivity`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentActivityEvent {
    pub session_id: String,
    /// The running plugins whose `permissions.agentActivity` is on. Only they get it.
    pub plugins: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::plugin;
    use std::path::Path;

    #[test]
    fn transcript_reads_only_declared_types() {
        let claude = std::env::temp_dir().join(format!("wings-tr-{}", std::process::id()));
        let id = "3c21b0c5-8113-4f55-9493-fa156c3fa369";
        fs::create_dir_all(claude.join("projects/-x")).unwrap();
        fs::write(
            claude.join(format!("projects/-x/{id}.jsonl")),
            "{\"type\":\"user\",\"message\":{}}\n{\"type\":\"pr-link\",\"prNumber\":7,\"prUrl\":\"https://github.com/a/b/pull/7\"}\n",
        )
        .unwrap();
        let p = plugin(&claude, &[], &["pr-link"]);
        let entries = transcript_entries(&p, std::slice::from_ref(&claude), id, &["pr-link".into()], None).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["prNumber"], 7);
        assert!(transcript_entries(&p, std::slice::from_ref(&claude), id, &["user".into()], None).is_err());
        assert!(transcript_entries(&p, std::slice::from_ref(&claude), "../../x", &["pr-link".into()], None).is_err());
        let _ = fs::remove_dir_all(&claude);
    }

    #[test]
    fn transcript_last_gives_the_newest_entries_in_order() {
        let claude = std::env::temp_dir().join(format!("wings-tr-last-{}", std::process::id()));
        let id = "4c21b0c5-8113-4f55-9493-fa156c3fa369";
        fs::create_dir_all(claude.join("projects/-x")).unwrap();
        // The assistant entry mentions `"type":"user"` inside its content, which mustn't make it a user entry.
        let lines = [
            r#"{"type":"user","n":1}"#,
            r#"{"type":"assistant","n":2,"message":{"content":[{"type":"user"}]}}"#,
            r#"{"type":"user","n":3}"#,
            "not json",
            r#"{"type":"user","n":4}"#,
        ];
        fs::write(claude.join(format!("projects/-x/{id}.jsonl")), lines.join("\n")).unwrap();
        let p = plugin(&claude, &[], &["user"]);
        let n = |entries: Vec<Value>| entries.iter().map(|e| e["n"].as_i64().unwrap()).collect::<Vec<_>>();
        assert_eq!(n(transcript_entries(&p, std::slice::from_ref(&claude), id, &["user".into()], Some(2)).unwrap()), [3, 4]);
        assert_eq!(n(transcript_entries(&p, std::slice::from_ref(&claude), id, &["user".into()], Some(10)).unwrap()), [1, 3, 4]);
        assert_eq!(n(transcript_entries(&p, std::slice::from_ref(&claude), id, &["user".into()], None).unwrap()), [1, 3, 4]);
        let _ = fs::remove_dir_all(&claude);
    }

    #[test]
    fn cut_entries_keep_their_shape_with_long_strings_cut_and_pictures_whole() {
        let mut entry = serde_json::json!({
            "type": "user",
            "message": { "content": [
                { "type": "tool_result", "content": "é".repeat(300) },
                { "type": "image", "source": { "data": "A".repeat(500) } },
            ] },
            "toolUseResult": { "structuredPatch": [{ "lines": ["+short"] }], "originalFile": "x".repeat(1000) },
        });
        cut_strings(&mut entry, 200);
        assert_eq!(entry["message"]["content"][0]["content"].as_str().unwrap().chars().count(), 200);
        assert_eq!(entry["message"]["content"][1]["source"]["data"].as_str().unwrap().len(), 500);
        assert_eq!(entry["toolUseResult"]["structuredPatch"][0]["lines"][0], "+short");
        assert_eq!(entry["toolUseResult"]["originalFile"].as_str().unwrap().len(), 200);
    }

    #[test]
    fn a_subagents_transcript_is_found_only_in_its_sessions_folder() {
        let claude = std::env::temp_dir().join(format!("wings-tr-agent-{}", std::process::id()));
        let (id, other) = ("6c21b0c5-8113-4f55-9493-fa156c3fa369", "7c21b0c5-8113-4f55-9493-fa156c3fa369");
        // As Claude Code 2.1.327 keeps them: the session's file, and its subagents in a folder named after it.
        for session in [id, other] {
            fs::create_dir_all(claude.join(format!("projects/-x/{session}/subagents"))).unwrap();
            fs::write(claude.join(format!("projects/-x/{session}.jsonl")), "{\"type\":\"user\",\"n\":0}\n").unwrap();
        }
        let agent = |session: &str, name: &str, n: u32| {
            let line = format!("{{\"type\":\"user\",\"isSidechain\":true,\"agentId\":\"{name}\",\"n\":{n}}}\n");
            fs::write(claude.join(format!("projects/-x/{session}/subagents/agent-{name}.jsonl")), line).unwrap();
        };
        agent(id, "a1b2c3d4e5f60718a", 1);
        agent(other, "a9f8e7d6c5b4a3921", 2);
        fs::write(claude.join("projects/-x/secret.jsonl"), "{\"type\":\"user\",\"n\":9}\n").unwrap();
        let p = plugin(&claude, &[], &["user"]);
        let dirs = std::slice::from_ref(&claude);
        let read = |agent: &str| agent_entries(&p, dirs, id, Some(agent), &["user".into()], None, None);
        assert_eq!(read("a1b2c3d4e5f60718a").unwrap()[0]["n"], 1);
        // Another session's subagent isn't this session's, and a page can't name a path.
        assert!(read("a9f8e7d6c5b4a3921").is_err());
        for name in ["../../secret", "a/b", "", "agent-a1b2c3d4e5f60718a.jsonl", &"a".repeat(65)] {
            assert!(read(name).is_err(), "{name}");
        }
        // The types the manifest allows still hold.
        assert!(agent_entries(&p, dirs, id, Some("a1b2c3d4e5f60718a"), &["assistant".into()], None, None).is_err());
        // Without an agent it is the session's own file, as before.
        assert_eq!(agent_entries(&p, dirs, id, None, &["user".into()], None, None).unwrap()[0]["n"], 0);
        let _ = fs::remove_dir_all(&claude);
    }

    #[test]
    fn transcript_events_go_only_to_plugins_that_read_the_type() {
        let reader = |id: &str, types: &[&str]| {
            let mut p = plugin(Path::new("/tmp"), &[], types);
            p.manifest.id = id.into();
            p
        };
        let running = [reader("timeline", &["user", "assistant"]), reader("cost", &["assistant", "cost-state"]), reader("pr", &["pr-link"])];
        let event = |text: &str| {
            let line = Line { pane_id: "p1".into(), session_id: "s1".into(), bytes: text.as_bytes().to_vec() };
            transcript_event(&running, line)
        };
        let e = event(r#"{"type":"assistant","message":{"content":[{"type":"text","text":"hi"}]}}"#).unwrap();
        assert_eq!((e.plugins.as_slice(), e.pane_id.as_str(), e.session_id.as_str()), (["timeline".to_string(), "cost".to_string()].as_slice(), "p1", "s1"));
        assert_eq!(e.entry["message"]["content"][0]["text"], "hi");
        assert_eq!(event(r#"{"type":"cost-state","costUSD":1.5}"#).unwrap().plugins, ["cost"]);
        assert_eq!(event(r#"{"type":"pr-link","prNumber":7}"#).unwrap().plugins, ["pr"]);
        // A type nobody reads, even when a nested block names a type someone does, and lines that aren't entries.
        assert!(event(r#"{"type":"attachment","content":[{"type":"user"}]}"#).is_none());
        for line in [r#"{"type":"user""#, "not json", r#"["type","user"]"#, r#"{"kind":"user","x":{"type":"user"}}"#, r#"{"type":7}"#] {
            assert!(event(line).is_none(), "{line}");
        }
        let line = |bytes: &[u8]| Line { pane_id: "p1".into(), session_id: "s1".into(), bytes: bytes.to_vec() };
        assert!(transcript_event(&running, line(b"{\"type\":\"user\",\"x\":\"\xff\"}")).is_none());
        assert!(transcript_event(&[], line(b"{\"type\":\"user\"}")).is_none());
    }

    #[test]
    fn attachment_kinds_narrow_what_a_plugin_reads() {
        let claude = std::env::temp_dir().join(format!("wings-tr-att-{}", std::process::id()));
        let id = "5c21b0c5-8113-4f55-9493-fa156c3fa369";
        fs::create_dir_all(claude.join("projects/-x")).unwrap();
        let lines = [
            r#"{"type":"attachment","attachment":{"type":"model","identity":{"modelId":"claude-opus-5-5[1m]"}}}"#,
            r#"{"type":"attachment","attachment":{"type":"file","content":"secret source"}}"#,
            // Says `"type":"model"` inside, but isn't a model attachment.
            r#"{"type":"user","message":{"content":[{"type":"model"}]}}"#,
            r#"{"type":"attachment","attachment":{"type":"deferred_tools_delta","failedMcpServers":[]}}"#,
        ];
        fs::write(claude.join(format!("projects/-x/{id}.jsonl")), lines.join("\n")).unwrap();
        let p = plugin(&claude, &[], &["attachment:model", "attachment:deferred_tools_delta"]);
        let kinds = |entries: Vec<Value>| entries.iter().map(|e| e["attachment"]["type"].as_str().unwrap().to_string()).collect::<Vec<_>>();
        let both = ["attachment:model".into(), "attachment:deferred_tools_delta".into()];
        assert_eq!(kinds(transcript_entries(&p, std::slice::from_ref(&claude), id, &both, None).unwrap()), ["model", "deferred_tools_delta"]);
        assert_eq!(kinds(transcript_entries(&p, std::slice::from_ref(&claude), id, &["attachment:model".into()], Some(5)).unwrap()), ["model"]);
        // One kind doesn't allow every attachment, or another kind.
        assert!(transcript_entries(&p, std::slice::from_ref(&claude), id, &["attachment".into()], None).is_err());
        assert!(transcript_entries(&p, std::slice::from_ref(&claude), id, &["attachment:file".into()], None).is_err());
        // A plain `attachment` permission still reads them all.
        let all = plugin(&claude, &[], &["attachment"]);
        assert_eq!(transcript_entries(&all, std::slice::from_ref(&claude), id, &["attachment".into()], None).unwrap().len(), 3);
        let _ = fs::remove_dir_all(&claude);

        let mut narrow = plugin(Path::new("/tmp"), &[], &["attachment:model"]);
        narrow.manifest.id = "narrow".into();
        let mut wide = plugin(Path::new("/tmp"), &[], &["attachment"]);
        wide.manifest.id = "wide".into();
        let running = [narrow, wide];
        let event = |text: &str| transcript_event(&running, Line { pane_id: "p1".into(), session_id: "s1".into(), bytes: text.as_bytes().to_vec() });
        assert_eq!(event(lines[0]).unwrap().plugins, ["narrow", "wide"]);
        assert_eq!(event(lines[1]).unwrap().plugins, ["wide"]);
        assert!(event(lines[2]).is_none());
    }
}
