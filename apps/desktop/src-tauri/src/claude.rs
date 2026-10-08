//! Reads Claude Code's on-disk transcripts: past sessions per project.
//! The JSONL format is internal to Claude Code, so parsing is tolerant: unknown lines are skipped.

use std::{
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

use serde::Serialize;
use serde_json::Value;

pub fn claude_dir() -> PathBuf {
    std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::home_dir().unwrap_or_default().join(".claude"))
}

/// Claude Code names a project folder after its cwd with every non-alphanumeric character
/// replaced by `-` (paths over 200 chars get truncated plus a hash; not handled here).
pub fn project_folder_name(cwd: &str) -> String {
    cwd.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect()
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummary {
    pub id: String,
    pub title: Option<String>,
    pub first_prompt: Option<String>,
    pub git_branch: Option<String>,
    pub last_active_ms: u64,
    pub size_bytes: u64,
}

fn transcripts(folder: &Path) -> Vec<(PathBuf, u64, u64)> {
    let Ok(entries) = fs::read_dir(folder) else { return vec![] };
    entries
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "jsonl"))
        .filter_map(|e| {
            let meta = e.metadata().ok()?;
            let mtime = meta.modified().ok()?.duration_since(UNIX_EPOCH).ok()?.as_millis() as u64;
            Some((e.path(), mtime, meta.len()))
        })
        .collect()
}

/// Past sessions for one project, most recent first.
pub fn list_sessions(claude_dir: &Path, cwd: &str) -> Vec<SessionSummary> {
    let folder = claude_dir.join("projects").join(project_folder_name(cwd));
    let mut out: Vec<SessionSummary> = transcripts(&folder)
        .into_iter()
        .filter_map(|(path, mtime, size)| summarize(&path, mtime, size))
        .collect();
    out.sort_by_key(|s| std::cmp::Reverse(s.last_active_ms));
    out
}

fn summarize(path: &Path, mtime: u64, size: u64) -> Option<SessionSummary> {
    let reader = BufReader::new(fs::File::open(path).ok()?);
    let (mut ai_title, mut custom_title, mut first_prompt, mut branch) = (None, None, None, None);
    let mut has_messages = false;
    for line in reader.lines().map_while(Result::ok) {
        // Cheap prefilter: only a few entry types matter and most lines are large tool output.
        let wanted = line.contains("\"ai-title\"")
            || line.contains("\"custom-title\"")
            || (first_prompt.is_none() && line.contains("\"type\":\"user\""));
        if !wanted {
            has_messages |= line.contains("\"type\":\"assistant\"");
            continue;
        }
        let Ok(entry) = serde_json::from_str::<Value>(&line) else { continue };
        match entry.get("type").and_then(Value::as_str) {
            Some("ai-title") => ai_title = entry.get("aiTitle").and_then(Value::as_str).map(String::from),
            Some("custom-title") => custom_title = entry.get("customTitle").and_then(Value::as_str).map(String::from),
            Some("user") => {
                has_messages = true;
                if branch.is_none() {
                    branch = entry.get("gitBranch").and_then(Value::as_str).filter(|b| !b.is_empty()).map(String::from);
                }
                if entry.get("isSidechain").and_then(Value::as_bool) != Some(true) && entry.get("isMeta").is_none() {
                    first_prompt = prompt_text(&entry);
                }
            }
            _ => {}
        }
    }
    let id = path.file_stem()?.to_string_lossy().into_owned();
    has_messages.then_some(SessionSummary {
        id,
        title: custom_title.or(ai_title),
        first_prompt,
        git_branch: branch,
        last_active_ms: mtime,
        size_bytes: size,
    })
}

/// Text the user typed, skipping slash-command wrappers and tool results.
fn prompt_text(entry: &Value) -> Option<String> {
    let content = entry.get("message")?.get("content")?;
    let text = match content {
        Value::String(s) => s.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .find(|b| b.get("type").and_then(Value::as_str) == Some("text"))?
            .get("text")?
            .as_str()?
            .to_string(),
        _ => return None,
    };
    let text = text.trim();
    if text.is_empty() || text.starts_with('<') || text.starts_with("Caveat:") {
        return None;
    }
    Some(text.chars().take(240).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_name_matches_claude_encoding() {
        assert_eq!(project_folder_name("/Users/me/Desktop/wings"), "-Users-me-Desktop-wings");
        assert_eq!(project_folder_name("/a/b.c_d e"), "-a-b-c-d-e");
    }

    #[test]
    fn summarizes_a_transcript() {
        let dir = std::env::temp_dir().join(format!("wings-claude-{}", std::process::id()));
        let folder = dir.join("projects").join(project_folder_name("/tmp/proj"));
        fs::create_dir_all(&folder).unwrap();
        let lines = [
            r#"{"type":"user","cwd":"/tmp/proj","gitBranch":"main","message":{"role":"user","content":"<command-name>/effort</command-name>"}}"#,
            r#"{"type":"user","cwd":"/tmp/proj","gitBranch":"main","message":{"role":"user","content":"Fix the login bug"}}"#,
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"ok"}]}}"#,
            r#"{"type":"ai-title","aiTitle":"Login bug fix","sessionId":"s1"}"#,
            "not json",
        ];
        fs::write(folder.join("s1.jsonl"), lines.join("\n")).unwrap();
        fs::write(folder.join("empty.jsonl"), r#"{"type":"last-prompt","lastPrompt":"x"}"#).unwrap();

        let sessions = list_sessions(&dir, "/tmp/proj");
        assert_eq!(sessions.len(), 1, "sessions without messages are skipped");
        let s = &sessions[0];
        assert_eq!(s.id, "s1");
        assert_eq!(s.title.as_deref(), Some("Login bug fix"));
        assert_eq!(s.first_prompt.as_deref(), Some("Fix the login bug"));
        assert_eq!(s.git_branch.as_deref(), Some("main"));
        let _ = fs::remove_dir_all(&dir);
    }
}
