use std::io::Write;

use super::*;

/// This Mac's Claude folder, for the ignored benchmarks that time the real transcripts.
fn real_claude_dir() -> PathBuf {
    std::env::var_os("CLAUDE_CONFIG_DIR").map(PathBuf::from).unwrap_or_else(|| dirs::home_dir().unwrap().join(".claude"))
}

fn temp_claude_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("wings-history-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    dir
}

fn transcript(dir: &Path, cwd: &str, id: &str, lines: &[String]) -> PathBuf {
    let folder = dir.join("projects").join(project_folder_name(cwd));
    fs::create_dir_all(&folder).unwrap();
    let path = folder.join(format!("{id}.jsonl"));
    fs::write(&path, lines.iter().map(|l| format!("{l}\n")).collect::<String>()).unwrap();
    path
}

fn user(cwd: &str, branch: &str, text: &str) -> String {
    json!({ "type": "user", "cwd": cwd, "gitBranch": branch, "timestamp": "2026-10-01T10:00:00.000Z", "message": { "role": "user", "content": text } }).to_string()
}

fn assistant(text: &str) -> String {
    json!({ "type": "assistant", "timestamp": "2026-10-01T10:00:05.000Z", "message": { "model": "claude-opus-5-5", "role": "assistant", "content": [{ "type": "text", "text": text }] } }).to_string()
}

fn texts(history: &History, claude_dirs: &[PathBuf], id: &str) -> Vec<(String, String)> {
    let page = history.read_session(claude_dirs, id, 0, 100, None).unwrap();
    page["messages"].as_array().unwrap().iter().map(|m| (m["role"].as_str().unwrap().to_string(), m["text"].as_str().unwrap().to_string())).collect()
}

fn pair(role: &str, text: &str) -> (String, String) {
    (role.to_string(), text.to_string())
}

#[test]
fn parses_transcripts_tolerantly() {
    let dir = temp_claude_dir("parse");
    let cwd = "/tmp/proj";
    let lines = [
        "not json".to_string(),
        r#"{"type":"some-future-kind","whatever":[1,2,3]}"#.to_string(),
        r#"{"type":"user","message":{"role":"user","content":42}}"#.to_string(),
        user(cwd, "main", "<command-name>/effort</command-name>"),
        json!({ "type": "user", "isMeta": true, "message": { "role": "user", "content": "Skill text the user never typed" } }).to_string(),
        json!({ "type": "user", "cwd": cwd, "gitBranch": "main", "message": { "role": "user", "content": [{ "type": "text", "text": "Fix the login bug" }, { "type": "image", "source": {} }] } }).to_string(),
        assistant("Looking."),
        json!({ "type": "assistant", "message": { "model": "claude-opus-5-5", "content": [{ "type": "tool_use", "name": "Edit", "input": { "file_path": "/tmp/proj/src/login.rs", "old_string": "a", "new_string": "b" } }] } }).to_string(),
        json!({ "type": "user", "message": { "role": "user", "content": [{ "type": "tool_result", "tool_use_id": "t1", "content": "huge tool output" }] }, "toolUseResult": { "stdout": "huge tool output" } }).to_string(),
        json!({ "type": "assistant", "isSidechain": true, "message": { "content": [{ "type": "text", "text": "A subagent's reply" }] } }).to_string(),
        assistant("Fixed it."),
        json!({ "type": "assistant", "message": { "model": "<synthetic>", "content": [{ "type": "text", "text": "API error" }] } }).to_string(),
        json!({ "type": "user", "message": { "role": "user", "content": [{ "type": "text", "text": "[Request interrupted by user]" }] } }).to_string(),
        r#"{"type":"ai-title","aiTitle":"Login bug fix"}"#.to_string(),
        r#"{"type":"custom-title","customTitle":"My login fix"}"#.to_string(),
        r#"{"type":"ai-title","aiTitle":"A later AI title"}"#.to_string(),
        r#"{"type":"pr-link","prUrl":"https://github.com/o/r/pull/7"}"#.to_string(),
        user(cwd, "feature", "Now the signup page"),
    ];
    transcript(&dir, cwd, "s1", &lines);
    let history = History::default();
    assert_eq!(
        texts(&history, std::slice::from_ref(&dir), "s1"),
        [pair("user", "Fix the login bug"), pair("assistant", "Looking.\n\nFixed it."), pair("user", "Now the signup page")]
    );
    let page = history.read_session(std::slice::from_ref(&dir), "s1", 0, 100, None).unwrap();
    let s = &page["session"];
    assert_eq!(s["title"], "My login fix", "a custom title beats the AI's");
    assert_eq!(s["filesEdited"], json!(["/tmp/proj/src/login.rs"]));
    assert_eq!(s["branches"], json!(["main", "feature"]));
    assert_eq!(s["models"], json!(["claude-opus-5-5"]));
    assert_eq!(s["pullRequests"], json!(["https://github.com/o/r/pull/7"]));
    assert_eq!(s["project"], cwd);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn reads_only_what_was_appended() {
    let dir = temp_claude_dir("append");
    let path = transcript(&dir, "/tmp/proj", "s1", &[user("/tmp/proj", "main", "first prompt")]);
    let half = assistant("a reply being written");
    let (head, tail) = half.split_at(20);
    fs::OpenOptions::new().append(true).open(&path).unwrap().write_all(head.as_bytes()).unwrap();
    let history = History::default();
    assert_eq!(texts(&history, std::slice::from_ref(&dir), "s1").len(), 1, "a half-written line waits");

    // Garbage in place of the first line proves the next read starts after it.
    let mut bytes = fs::read(&path).unwrap();
    let first_len = bytes.iter().position(|b| *b == b'\n').unwrap();
    bytes[..first_len].fill(b'x');
    bytes.extend_from_slice(format!("{tail}\n{}\n", user("/tmp/proj", "main", "second prompt")).as_bytes());
    fs::write(&path, bytes).unwrap();
    assert_eq!(
        texts(&history, std::slice::from_ref(&dir), "s1"),
        [pair("user", "first prompt"), pair("assistant", "a reply being written"), pair("user", "second prompt")]
    );

    fs::write(&path, format!("{}\n", user("/tmp/proj", "main", "rewritten"))).unwrap();
    assert_eq!(texts(&history, std::slice::from_ref(&dir), "s1"), [pair("user", "rewritten")], "a shrunk file is read again");
    fs::remove_file(&path).unwrap();
    assert!(history.read_session(std::slice::from_ref(&dir), "s1", 0, 10, None).is_err(), "a deleted transcript drops out");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_query_reads_only_the_matching_messages_and_their_neighbours() {
    let dir = temp_claude_dir("read-query");
    let lines = [
        user("/tmp/proj", "main", "set up the project"),
        assistant("Done."),
        user("/tmp/proj", "main", "measure the Baseline method"),
        assistant("Used the build durations."),
        user("/tmp/proj", "main", "now the docs"),
        assistant("Wrote them."),
        user("/tmp/proj", "main", "and the baseline again"),
    ];
    transcript(&dir, "/tmp/proj", "s1", &lines);
    let history = History::default();
    let index = |page: &Value| page["messages"].as_array().unwrap().iter().map(|m| m["index"].as_u64().unwrap()).collect::<Vec<_>>();
    let page = history.read_session(std::slice::from_ref(&dir), "s1", 0, 100, Some("baseline")).unwrap();
    assert_eq!(index(&page), [1, 2, 3, 5, 6]);
    assert_eq!(page["matchingMessages"], 2);
    let first = history.read_session(std::slice::from_ref(&dir), "s1", 0, 2, Some("baseline")).unwrap();
    assert_eq!((index(&first), first["nextOffset"].clone()), (vec![1, 2], json!(3)));
    assert_eq!(index(&history.read_session(std::slice::from_ref(&dir), "s1", 3, 100, Some("baseline")).unwrap()), [3, 5, 6]);
    assert_eq!(index(&history.read_session(std::slice::from_ref(&dir), "s1", 0, 100, Some("\"baseline method\" durations")).unwrap()), Vec::<u64>::new());
    assert_eq!(history.read_session(std::slice::from_ref(&dir), "s1", 0, 100, None).unwrap()["matchingMessages"], Value::Null);
    let _ = fs::remove_dir_all(&dir);
}

fn sorted_ids(results: &Results) -> Vec<String> {
    let mut ids: Vec<String> = results.sessions.iter().map(|h| h.id.clone()).collect();
    ids.sort();
    ids
}

#[test]
fn a_search_lists_a_folder_again_only_when_a_transcript_comes_or_goes() {
    let dir = temp_claude_dir("listings");
    transcript(&dir, "/tmp/proj", "s1", &[user("/tmp/proj", "main", "first prompt")]);
    let history = History::default();
    let search = |q: &str| sorted_ids(&history.search(std::slice::from_ref(&dir), q, &Filter::default(), 10));
    let listed = || LISTED.with(std::cell::Cell::get);
    assert_eq!(search("first"), ["s1"]);
    let once = listed();
    assert_eq!(search("first"), ["s1"]);
    assert_eq!(listed(), once, "nothing came or went, so no folder is listed again");
    std::thread::sleep(std::time::Duration::from_millis(5));
    transcript(&dir, "/tmp/proj", "s2", &[user("/tmp/proj", "main", "second prompt")]);
    assert_eq!(search("prompt"), ["s1", "s2"], "a new session is found");
    assert!(listed() > once, "its folder was listed again");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn search_matches_every_term_and_filters() {
    let dir = temp_claude_dir("search");
    let edit = json!({ "type": "assistant", "message": { "content": [{ "type": "tool_use", "name": "Write", "input": { "file_path": "/w/app/src/history-sheet.tsx" } }] } }).to_string();
    transcript(&dir, "/w/app", "a", &[user("/w/app", "main", "Fix the Login BUG on the Čakovec page"), assistant("Done, the login works."), edit]);
    transcript(&dir, "/w/app", "b", &[user("/w/app", "feature", "Restyle the login screen")]);
    transcript(&dir, "/w/app/.claude/worktrees/x", "c", &[user("/w/app/.claude/worktrees/x", "main", "login bug in a worktree")]);
    transcript(&dir, "/w/other", "d", &[user("/w/other", "main", "login bug elsewhere")]);
    transcript(&dir, "/w/app-old", "e", &[user("/w/app-old", "main", "login bug in a sibling folder")]);
    transcript(&dir, "/w/app", "empty", &[r#"{"type":"last-prompt","lastPrompt":"x"}"#.to_string()]);
    let history = History::default();
    let search = |q: &str, filter: Filter| history.search(std::slice::from_ref(&dir), q, &filter, 10);
    let app = || Filter { project: Some("/w/app/".into()), ..Filter::default() };

    assert_eq!(sorted_ids(&search("", Filter::default())), ["a", "b", "c", "d", "e"], "sessions without messages are left out");
    assert_eq!(sorted_ids(&search("login bug", app())), ["a", "c"], "every term, in the project and folders inside it");
    assert_eq!(sorted_ids(&search("login BUG čakovec", app())), ["a"], "case folds, past ASCII too");
    assert_eq!(sorted_ids(&search("\"the login works\"", app())), ["a"]);
    assert!(search("\"login screen works\"", app()).sessions.is_empty());
    assert_eq!(sorted_ids(&search("history-sheet", app())), ["a"], "edited files are searched");
    assert_eq!(sorted_ids(&search("login", Filter { branch: Some("feature".into()), ..app() })), ["b"]);
    assert!(search("login", Filter { since_ms: Some(u64::MAX), ..app() }).sessions.is_empty());

    let results = search("works", app());
    let hit = &results.sessions[0];
    assert_eq!((hit.matches, hit.snippets.len()), (1, 1));
    assert_eq!(hit.snippets[0].role, Role::Assistant);
    assert_eq!(hit.snippets[0].text, "Done, the login works.");
    assert_eq!(hit.first_prompt.as_deref(), Some("Fix the Login BUG on the Čakovec page"));
    assert_eq!(search("history-sheet", app()).sessions[0].snippets[0].role, Role::File);
    let mut branches = results.branches.clone();
    branches.sort();
    assert_eq!(branches, ["feature", "main"]);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_claude_hit_names_its_transcript_in_whichever_account_it_is() {
    let (default, work) = (temp_claude_dir("account-default"), temp_claude_dir("account-work"));
    let mine = transcript(&default, "/w/app", "a", &[user("/w/app", "main", "Fix the login")]);
    let theirs = transcript(&work, "/w/app", "b", &[user("/w/app", "main", "Fix the logout")]);
    let results = History::default().search(&[default.clone(), work.clone()], "", &Filter::default(), 10);
    let mut found: Vec<(&str, Option<&Path>)> = results.sessions.iter().map(|h| (h.id.as_str(), h.transcript.as_deref())).collect();
    found.sort();
    assert_eq!(found, [("a", Some(mine.as_path())), ("b", Some(theirs.as_path()))]);
    assert!(serde_json::to_value(&results).unwrap()["sessions"][0].get("transcript").is_none(), "the MCP tool's answer stays as it was");
    let _ = (fs::remove_dir_all(&default), fs::remove_dir_all(&work));
}

#[test]
fn a_session_copied_into_another_account_is_listed_once_as_the_copy_that_moved_on_last() {
    let (default, work) = (temp_claude_dir("copied-default"), temp_claude_dir("copied-work"));
    let original = transcript(&work, "/w/app", "a", &[user("/w/app", "main", "Fix the login")]);
    let copy = transcript(&default, "/w/app", "a", &[user("/w/app", "main", "Fix the login"), assistant("Done")]);
    let earlier = std::time::SystemTime::now() - Duration::from_secs(60);
    fs::File::options().write(true).open(&original).unwrap().set_modified(earlier).unwrap();
    let results = History::default().search(&[default.clone(), work.clone()], "", &Filter::default(), 10);
    let found: Vec<Option<&Path>> = results.sessions.iter().map(|h| h.transcript.as_deref()).collect();
    assert_eq!(found, [Some(copy.as_path())]);
    assert_eq!(results.total, 1);
    let _ = (fs::remove_dir_all(&default), fs::remove_dir_all(&work));
}

#[test]
fn a_phrase_never_matches_across_two_messages() {
    let dir = temp_claude_dir("phrase");
    transcript(&dir, "/p", "a", &[user("/p", "main", "ends with foo"), assistant("bar starts this")]);
    let history = History::default();
    assert_eq!(history.search(std::slice::from_ref(&dir), "foo bar", &Filter::default(), 10).total, 1);
    assert_eq!(history.search(std::slice::from_ref(&dir), "\"foo bar\"", &Filter::default(), 10).total, 0);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn quotes_around_the_match() {
    let text = format!("{} needle {}", "a".repeat(200), "b".repeat(300));
    let quoted = quote(&text, 201, 6);
    assert!(quoted.starts_with('…') && quoted.ends_with('…') && quoted.contains("needle"));
    assert!(quoted.chars().count() <= BEFORE + AFTER + 8);
    assert_eq!(quote("ž needle", 3, 6), "ž needle", "cuts only on character boundaries");
}

#[test]
fn mcp_tools_have_valid_shapes() {
    let tools = mcp_tools();
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["search_history", "read_session"]);
    for tool in &tools {
        let name = tool["name"].as_str().unwrap();
        assert!(name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') && !name.contains("__"), "can't clash with <plugin>__<tool>");
        assert!(tool["description"].as_str().unwrap().len() < 2048, "Claude Code cuts descriptions at 2,048 characters");
        let schema = &tool["inputSchema"];
        assert_eq!(schema["type"], "object");
        for required in schema["required"].as_array().unwrap() {
            assert!(schema["properties"].get(required.as_str().unwrap()).is_some());
        }
    }
}

#[test]
fn a_short_first_prompt_is_joined_by_the_next_ones() {
    let dir = temp_claude_dir("opening");
    let long = "Now move the resume button into the session list and keep the old one in the toolbar. ".repeat(4).trim_end().to_string();
    transcript(&dir, "/p", "s1", &[user("/p", "main", "hi"), assistant("Hello."), user("/p", "main", "fix the login redirect"), assistant("Done."), user("/p", "main", &long), user("/p", "main", "never reached")]);
    let e = History::default().excerpt(std::slice::from_ref(&dir), "s1").unwrap();
    assert_eq!(e.first_prompt, format!("hi\nfix the login redirect\n{long}"));
    assert_eq!(e.first_reply.as_deref(), Some("Hello."));
    // A first prompt that says enough stands alone.
    transcript(&dir, "/p", "s2", &[user("/p", "main", &long), user("/p", "main", "second")]);
    assert_eq!(History::default().excerpt(std::slice::from_ref(&dir), "s2").unwrap().first_prompt, long);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn mcp_tools_search_and_read_in_pages() {
    let dir = temp_claude_dir("mcp");
    let long = "word ".repeat(600);
    let mut lines = Vec::new();
    for i in 0..30 {
        lines.push(user("/p", "main", &format!("prompt {i} {long}")));
        lines.push(assistant(&format!("reply {i}")));
    }
    lines.push(user("/p", "main", &format!("huge {}", "x".repeat(20_000))));
    transcript(&dir, "/p", "s1", &lines);
    let history = History::default();
    let call = |name: &str, args: Value| call_tool(&history, std::slice::from_ref(&dir), name, &args).unwrap();

    let found: Value = serde_json::from_str(&call("search_history", json!({ "query": "prompt 7", "project": "/p" })).unwrap()).unwrap();
    assert_eq!(found["total"], 1);
    let hit = &found["sessions"][0];
    assert_eq!((hit["sessionId"].as_str(), hit["project"].as_str(), hit["messages"].as_u64()), (Some("s1"), Some("/p"), Some(61)));
    assert!(hit["snippets"][0]["text"].as_str().unwrap().contains("prompt"));

    let mut offset = Some(0);
    let (mut seen, mut pages) = (0, 0);
    while let Some(at) = offset {
        let text = call("read_session", json!({ "session_id": "s1", "offset": at, "limit": 100 })).unwrap();
        assert!(text.len() < PAGE_BYTES + MESSAGE_BYTES + 4_000, "a page stays near its budget: {}", text.len());
        let page: Value = serde_json::from_str(&text).unwrap();
        let messages = page["messages"].as_array().unwrap();
        assert_eq!(messages[0]["index"].as_u64(), Some(at as u64));
        seen += messages.len();
        pages += 1;
        offset = page["nextOffset"].as_u64().map(|n| n as usize);
    }
    assert_eq!(seen, 61, "the pages cover every message once");
    assert!(pages > 1);
    let last: Value = serde_json::from_str(&call("read_session", json!({ "session_id": "s1", "offset": 60 })).unwrap()).unwrap();
    assert!(last["messages"][0]["text"].as_str().unwrap().ends_with("more bytes not shown]"));
    let small: Value = serde_json::from_str(&call("read_session", json!({ "session_id": "s1", "limit": 2 })).unwrap()).unwrap();
    assert_eq!((small["messages"].as_array().unwrap().len(), small["nextOffset"].as_u64()), (2, Some(2)));

    assert!(call("search_history", json!({})).is_err());
    assert!(call("read_session", json!({ "session_id": "nope" })).is_err());
    assert!(call_tool(&history, std::slice::from_ref(&dir), "cyclops__start_timer", &json!({})).is_none());
    let _ = fs::remove_dir_all(&dir);
}

/// An OpenCode service with fixed sessions. Like the real one, a list's last page still has a `next`.
struct FakeService {
    sessions: Mutex<Vec<Value>>,
    messages: HashMap<&'static str, Vec<Value>>,
    fetched: Mutex<Vec<String>>,
}

impl Api for FakeService {
    fn get(&self, path: &str, query: &[(&str, &str)]) -> Result<Value, opencode::Error> {
        let next = Some("c1");
        if query.iter().any(|(k, _)| *k == "cursor") {
            return Ok(json!({ "data": [], "cursor": { "next": next } }));
        }
        if path == "/api/session" {
            return Ok(json!({ "data": *self.sessions.lock().unwrap(), "cursor": { "next": next } }));
        }
        let id = path.strip_prefix("/api/session/").and_then(|p| p.strip_suffix("/message")).ok_or(opencode::Error::Status(404))?;
        self.fetched.lock().unwrap().push(id.to_string());
        // The real service sends the newest message first.
        let mut data = self.messages.get(id).cloned().ok_or(opencode::Error::Status(404))?;
        data.reverse();
        Ok(json!({ "data": data, "cursor": { "next": next } }))
    }
}

fn oc_session(id: &str, dir: &str, updated: u64, title: Option<&str>, parent: Option<&str>) -> Value {
    let mut s = json!({ "id": id, "location": { "directory": dir }, "time": { "created": 1_790_000_000_000u64, "updated": updated } });
    if let Some(t) = title {
        s["title"] = json!(t);
    }
    if let Some(p) = parent {
        s["parentID"] = json!(p);
    }
    s
}

fn oc_user(text: &str) -> Value {
    json!({ "id": "m", "type": "user", "text": text, "time": { "created": 1_790_000_001_000u64 } })
}

fn oc_assistant(parts: Value) -> Value {
    json!({ "id": "m", "type": "assistant", "model": { "id": "gpt-6", "providerID": "openai" }, "content": parts, "time": { "created": 1_790_000_002_000u64 } })
}

fn fake_opencode() -> FakeService {
    FakeService {
        sessions: Mutex::new(vec![
            oc_session("ses_root1", "/w/app", 1_790_000_900_000, Some("Wire up the MCP config"), None),
            oc_session("ses_child", "/w/app", 1_790_000_950_000, None, Some("ses_root1")),
            oc_session("ses_root2", "/w/other", 1_790_000_100_000, None, None),
            oc_session("ses_empty", "/w/app", 1_790_000_999_000, None, None),
        ]),
        messages: HashMap::from([
            (
                "ses_root1",
                vec![
                    json!({ "id": "s", "type": "system", "text": "instructions about mcp-config.json" }),
                    oc_user("Where is mcp-config.json read?"),
                    oc_assistant(json!([{ "type": "reasoning", "text": "secret thoughts" }, { "type": "text", "text": "It is read in Config::load." }, { "type": "tool", "name": "read" }, { "type": "text", "text": "Done." }])),
                    json!({ "id": "i", "type": "idle", "outcome": "succeeded" }),
                ],
            ),
            ("ses_root2", vec![oc_user("Čakovec release notes")]),
            ("ses_empty", vec![json!({ "id": "i", "type": "idle" })]),
        ]),
        fetched: Mutex::new(Vec::new()),
    }
}

#[test]
fn formats_times() {
    assert_eq!(iso(0), "1970-01-01T00:00:00.000Z");
    assert_eq!(iso(1_790_000_001_234), "2026-09-21T14:13:21.234Z");
    assert_eq!(iso(951_782_400_000), "2000-02-29T00:00:00.000Z");
}

#[test]
fn parses_an_opencode_session() {
    let service = fake_opencode();
    let info = oc_session("ses_root1", "/w/app", 1_790_000_900_000, Some("Wire up the MCP config"), None);
    let session = opencode_session(&info, &opencode_messages(&service, "ses_root1").unwrap());
    assert_eq!((session.id.as_str(), session.agent, session.cwd.as_deref(), session.title()), ("ses_root1", Agent::OpenCode, Some("/w/app"), Some("Wire up the MCP config")));
    assert_eq!(session.models, ["gpt-6"]);
    let texts: Vec<(Role, &str)> = (0..session.messages.len()).map(|i| (session.messages[i].role, &session.text[session.range(i).0..session.range(i).1])).collect();
    assert_eq!(
        texts,
        [(Role::User, "Where is mcp-config.json read?"), (Role::Assistant, "It is read in Config::load.\n\nDone.")],
        "oldest first; system notices, reasoning and tool calls are left out"
    );
    assert_eq!(session.messages[0].time.as_deref(), Some("2026-09-21T14:13:21.000Z"));
}

#[test]
fn searches_opencode_next_to_claude() {
    let dir = temp_claude_dir("opencode");
    transcript(&dir, "/w/app", "a", &[user("/w/app", "main", "Fix the config loader"), assistant("Config::load fixed.")]);
    let service = fake_opencode();
    let history = History::default();
    assert!(!history.search(std::slice::from_ref(&dir), "", &Filter::default(), 10).opencode_loading);
    assert!(history.begin_opencode());
    assert!(history.search(std::slice::from_ref(&dir), "", &Filter::default(), 10).opencode_loading, "a refresh is running");
    assert!(!history.begin_opencode(), "one at a time");
    history.refresh_opencode(&service);

    let all = history.search(std::slice::from_ref(&dir), "", &Filter::default(), 10);
    assert!(!all.opencode_loading);
    let ids: Vec<(&str, Agent)> = all.sessions.iter().map(|h| (h.id.as_str(), h.agent)).collect();
    assert_eq!(ids, [("a", Agent::Claude), ("ses_root1", Agent::OpenCode), ("ses_root2", Agent::OpenCode)], "child and empty sessions are left out");
    assert!(all.sessions[1].transcript.is_none(), "OpenCode keeps its sessions in its service");
    let found = history.search(std::slice::from_ref(&dir), "MCP-CONFIG.json", &Filter::default(), 10);
    assert_eq!(found.sessions.len(), 1, "a system notice is not searched");
    let hit = &found.sessions[0];
    assert_eq!((hit.id.as_str(), hit.agent, hit.last_active_ms), ("ses_root1", Agent::OpenCode, 1_790_000_900_000));
    assert_eq!((hit.snippets[0].role, hit.snippets[0].text.as_str()), (Role::User, "Where is mcp-config.json read?"));
    let reply = history.search(std::slice::from_ref(&dir), "config::load", &Filter::default(), 10);
    assert_eq!(reply.sessions.iter().map(|h| h.id.as_str()).collect::<Vec<_>>(), ["a", "ses_root1"], "a reply is searched, newest first");
    assert_eq!(reply.sessions[1].snippets[0].role, Role::Assistant);
    assert!(history.search(std::slice::from_ref(&dir), "secret thoughts", &Filter::default(), 10).sessions.is_empty(), "reasoning is not searched");
    assert_eq!(history.search(std::slice::from_ref(&dir), "wire up", &Filter::default(), 10).sessions.len(), 1, "titles are searched");

    let app = Filter { project: Some("/w/app".into()), ..Filter::default() };
    assert_eq!(history.search(std::slice::from_ref(&dir), "", &app, 10).sessions.iter().map(|h| h.id.as_str()).collect::<Vec<_>>(), ["a", "ses_root1"], "the project filter uses the session's folder");
    assert!(history.search(std::slice::from_ref(&dir), "", &Filter { branch: Some("main".into()), project: Some("/w/other".into()), ..Filter::default() }, 10).sessions.is_empty());
    assert_eq!(history.search(std::slice::from_ref(&dir), "čakovec", &Filter { since_ms: Some(1_790_000_100_000), ..Filter::default() }, 10).total, 1);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn refetches_only_changed_opencode_sessions() {
    let dir = temp_claude_dir("opencode-delta");
    let service = fake_opencode();
    let history = History::default();
    history.refresh_opencode(&service);
    let mut first = service.fetched.lock().unwrap().clone();
    first.sort();
    assert_eq!(first, ["ses_empty", "ses_root1", "ses_root2"], "root sessions only");
    service.fetched.lock().unwrap().clear();
    history.refresh_opencode(&service);
    assert!(service.fetched.lock().unwrap().is_empty(), "nothing changed, nothing fetched");

    service.sessions.lock().unwrap()[2] = oc_session("ses_root2", "/w/other", 1_790_000_200_000, None, None);
    service.sessions.lock().unwrap().remove(0);
    history.refresh_opencode(&service);
    assert_eq!(*service.fetched.lock().unwrap(), ["ses_root2"]);
    let ids: Vec<String> = history.search(std::slice::from_ref(&dir), "", &Filter::default(), 10).sessions.into_iter().map(|h| h.id).collect();
    assert_eq!(ids, ["ses_root2"], "a session the service no longer lists is dropped");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn reads_an_opencode_session_and_the_mcp_tools_name_the_agent() {
    let dir = temp_claude_dir("opencode-read");
    let service = fake_opencode();
    let history = History::default();
    history.refresh_opencode(&service);
    let page = history.read_session(std::slice::from_ref(&dir), "ses_root1", 0, 1, None).unwrap();
    assert_eq!((page["session"]["agent"].as_str(), page["session"]["project"].as_str(), page["session"]["messages"].as_u64()), (Some("opencode"), Some("/w/app"), Some(2)));
    assert_eq!((page["messages"][0]["role"].as_str(), page["nextOffset"].as_u64()), (Some("user"), Some(1)));
    let next = history.read_session(std::slice::from_ref(&dir), "ses_root1", 1, 10, None).unwrap();
    assert_eq!((next["messages"][0]["text"].as_str(), next["nextOffset"].as_u64()), (Some("It is read in Config::load.\n\nDone."), None));
    assert!(history.read_session(std::slice::from_ref(&dir), "ses_child", 0, 10, None).is_err());
    let found: Value = serde_json::from_str(&call_tool(&history, std::slice::from_ref(&dir), "search_history", &json!({ "query": "Čakovec" })).unwrap().unwrap()).unwrap();
    assert_eq!((found["sessions"][0]["sessionId"].as_str(), found["sessions"][0]["agent"].as_str()), (Some("ses_root2"), Some("opencode")));
    let _ = fs::remove_dir_all(&dir);
}

/// Loads the sessions of the opencode service running on this Mac and prints the timings:
/// `cargo test --release live_opencode -- --ignored --nocapture`. It only reads.
#[test]
#[ignore]
fn live_opencode() {
    let client = opencode::connect().expect("an opencode service is running");
    let history = History::default();
    let start = Instant::now();
    assert!(history.begin_opencode());
    history.refresh_opencode(&client);
    println!("first load: {:?}", start.elapsed());
    let (sessions, messages) = {
        let oc = history.opencode.lock().unwrap();
        (oc.sessions.len(), oc.sessions.values().map(|c| c.session.messages.len()).sum::<usize>())
    };
    println!("{sessions} root sessions loaded, {messages} messages");
    let roots = opencode_roots(&client).unwrap().len();
    println!("{roots} root sessions listed, {} failed to load", roots - sessions);
    let real = real_claude_dir();
    history.search(std::slice::from_ref(&real), "", &Filter::default(), 1);
    for query in ["mcp-config.json", "the", "xyzzy-nowhere"] {
        let start = Instant::now();
        let found = history.search(std::slice::from_ref(&real), query, &Filter::default(), 100);
        let opencode = found.sessions.iter().filter(|h| h.agent == Agent::OpenCode).count();
        println!("search {query:?} over Claude and OpenCode: {:?}, {} sessions ({opencode} OpenCode)", start.elapsed(), found.total);
    }
    let start = Instant::now();
    history.refresh_opencode(&client);
    println!("repeat refresh with nothing new: {:?}", start.elapsed());
}

/// Times this Mac's transcripts: `cargo test --release bench_real -- --ignored --nocapture`. It only reads
/// them; the append it times goes to a copy in a temp folder.
#[cfg(unix)]
#[test]
#[ignore]
fn bench_real_transcripts() {
    use std::time::{Duration, Instant};
    let median = |runs: &mut Vec<Duration>| {
        runs.sort();
        runs[runs.len() / 2]
    };
    let real = real_claude_dir();
    let dir = temp_claude_dir("bench");
    let projects = dir.join("projects");
    fs::create_dir_all(&projects).unwrap();
    let (mut biggest, mut files, mut bytes) = ((0, PathBuf::new()), 0, 0);
    for folder in fs::read_dir(real.join("projects")).unwrap().flatten() {
        std::os::unix::fs::symlink(folder.path(), projects.join(folder.file_name())).unwrap();
        for file in fs::read_dir(folder.path()).into_iter().flatten().flatten() {
            let len = file.metadata().unwrap().len();
            if file.path().extension().is_some_and(|x| x == "jsonl") {
                (files, bytes) = (files + 1, bytes + len);
                if len > biggest.0 {
                    biggest = (len, file.path());
                }
            }
        }
    }
    let live = projects.join("-bench-live");
    fs::create_dir_all(&live).unwrap();
    let copy = live.join("bench-live.jsonl");
    fs::copy(&biggest.1, &copy).unwrap();
    println!("{files} transcripts, {} MB, plus a copy of the biggest ({} MB)", bytes / 1_000_000, biggest.0 / 1_000_000);

    let history = History::default();
    let start = Instant::now();
    let all = history.search(std::slice::from_ref(&dir), "", &Filter::default(), 1);
    println!("cold build: {:?} for {} sessions with messages", start.elapsed(), all.total);
    {
        let index = history.index.lock().unwrap();
        let messages: usize = index.values().map(|t| t.session.messages.len()).sum();
        let held: usize = index.values().map(|t| t.session.text.len() + t.session.folded.len() + t.session.meta.len()).sum();
        println!("{messages} messages, {} MB of text held (original plus folded)", held / 1_000_000);
    }
    let mut runs: Vec<Duration> = (0..7)
        .map(|_| {
            let start = Instant::now();
            history.search(std::slice::from_ref(&dir), "", &Filter::default(), 1);
            start.elapsed()
        })
        .collect();
    println!("refresh with nothing new: {:?} (median of 7)", median(&mut runs));

    let biggest_cwd = all.sessions.first().and_then(|h| h.cwd.clone());
    for (query, project) in [
        ("fork-session", None),
        ("the", None),
        ("login bug", None),
        ("\"resume the session\"", None),
        ("xyzzy-nowhere", None),
        ("plugin", biggest_cwd.as_deref()),
    ] {
        let filter = Filter { project: project.map(String::from), ..Filter::default() };
        let mut total = 0;
        let mut runs: Vec<Duration> = (0..7)
            .map(|_| {
                let start = Instant::now();
                total = history.search(std::slice::from_ref(&dir), query, &filter, 100).total;
                start.elapsed()
            })
            .collect();
        println!("search {query:?}{}: {:?} (median of 7), {total} sessions", project.map(|p| format!(" in {p}")).unwrap_or_default(), median(&mut runs));
    }

    // One turn's worth of appended lines: the copy's last ~200 KB again.
    let data = fs::read(&copy).unwrap();
    let from = data[..data.len() - 200_000].iter().rposition(|b| *b == b'\n').unwrap() + 1;
    let tail = data[from..].to_vec();
    let mut runs = Vec::new();
    for _ in 0..5 {
        fs::OpenOptions::new().append(true).open(&copy).unwrap().write_all(&tail).unwrap();
        let start = Instant::now();
        history.search(std::slice::from_ref(&dir), "", &Filter::default(), 1);
        runs.push(start.elapsed());
    }
    println!("refresh after {} KB appended: {:?} (median of 5)", tail.len() / 1000, median(&mut runs));

    let _ = fs::remove_dir_all(&dir);
}

/// A History with the semantic index of `claude`'s transcripts made by the test model, after one round.
fn with_meaning(claude: &Path, name: &str) -> History {
    let (semantic, _) = crate::semantic::tests::fake(&crate::semantic::tests::temp(name), 1);
    semantic.round(&[claude.to_owned()], &|| {});
    let history = History::default();
    history.set_semantic(semantic);
    history
}

fn ids(results: &Results) -> Vec<&str> {
    results.sessions.iter().map(|h| h.id.as_str()).collect()
}

#[test]
fn a_query_worded_differently_finds_the_session_by_what_it_means() {
    use crate::semantic::tests::{reply, temp, write};
    let claude = temp("meaning-claude");
    write(&claude, "restart", &[user("/p", "main", "the pane history is lost after a restart"), reply("I will keep it on disk")]);
    write(&claude, "sidebar", &[user("/p", "main", "make the sidebar text bigger please"), reply("done, it uses the setting")]);
    let dirs = [claude.clone()];
    let query = "scrollback disappears when relaunching";
    assert!(History::default().search(&dirs, query, &Filter::default(), 10).sessions.is_empty(), "no session has these words");
    let found = with_meaning(&claude, "meaning-dir").search(&dirs, query, &Filter::default(), 10);
    assert_eq!(ids(&found)[0], "restart");
    let snippet = &found.sessions[0].snippets[0];
    assert_eq!((snippet.role, snippet.text.as_str()), (Role::User, "the pane history is lost after a restart"));
    assert_eq!(found.sessions[0].matches, 0, "found by meaning, not by its words");
}

/// Writes a transcript of `lines` for session `id`, last changed `age` seconds ago.
fn aged(claude: &Path, id: &str, age: u64, lines: &[String]) {
    let path = crate::semantic::tests::write(claude, id, lines);
    let at = std::time::SystemTime::now() - Duration::from_secs(age);
    fs::File::options().append(true).open(path).unwrap().set_modified(at).unwrap();
}

#[test]
fn a_long_session_holding_every_word_somewhere_doesnt_outrank_the_one_that_means_the_query() {
    use crate::semantic::tests::{reply, temp};
    let claude = temp("long-claude");
    // The most recent, and every word of the question is in it, each in a different message.
    let long: Vec<String> = ["we added scrollback settings for terminals", "the old icon disappears in dark mode", "ask when the build ends", "relaunching needs a signed app", "fine"]
        .iter()
        .flat_map(|text| [user("/p", "main", text), reply("done, as you asked")])
        .collect();
    aged(&claude, "long", 0, &long);
    aged(&claude, "meant", 600, &[user("/p", "main", "the pane history is lost after a restart"), reply("kept on disk now")]);
    let dirs = [claude.clone()];
    let query = "the scrollback disappears when relaunching";
    let found = with_meaning(&claude, "long-dir").search(&dirs, query, &Filter::default(), 10);
    assert_eq!(ids(&found)[0], "meant", "{:?}", ids(&found));
    assert!(ids(&found).contains(&"long"), "its words still match: it stays in the list");
}

#[test]
fn by_words_the_session_a_rare_word_names_comes_before_one_that_mentions_it_in_passing() {
    use crate::semantic::tests::{reply, temp};
    let claude = temp("named-claude");
    aged(
        &claude,
        "passing",
        0,
        &[user("/p", "main", "compare our plugin list with what herdr and tmux and iterm offer and keep notes on all of the differences"), reply("noted")],
    );
    aged(
        &claude,
        "named",
        600,
        &[
            r#"{"type":"custom-title","customTitle":"Herdr desktop app with plugins"}"#.to_string(),
            user("/p", "main", "build the herdr idea as a desktop app"),
            reply("planned the herdr style app"),
        ],
    );
    let dirs = [claude.clone()];
    let found = History::default().search(&dirs, "herdr", &Filter::default(), 10);
    assert_eq!((ids(&found), found.total), (vec!["named", "passing"], 2), "not most recent first: the session it names");
    let found = History::default().search(&dirs, "herdr tmux", &Filter::default(), 10);
    assert_eq!(ids(&found), ["passing"], "every word still has to match");
}

#[test]
fn common_words_count_for_little_and_a_long_message_doesnt_win_by_its_size() {
    let session = |id: &str, texts: &[&str]| {
        let mut s = Session { id: id.into(), ..Session::default() };
        for text in texts {
            s.push(Role::User, text, None);
        }
        s.finish();
        s
    };
    let short = session("short", &["the build of the dts decoder"]);
    let long = session("long", &[&format!("the build {}", "and the logs and the steps of the run ".repeat(40)), "dts once"]);
    let common = session("common", &["the build", "the build again", "the build passed"]);
    let scores = bm25(&[&short, &long, &common], &["build", "dts"]);
    assert!(scores[0] > scores[1], "a short message with both words beats a long one: {scores:?}");
    assert!(scores[0] > scores[2], "a word in most messages counts for little: {scores:?}");
    assert_eq!(ranking_terms(&terms("the build of the decoder")), ["build", "decoder"]);
    assert_eq!(ranking_terms(&terms("the")), ["the"], "only stop words: all of them");
    assert_eq!(word_starts("a decision on ci", "ci").count(), 1, "inside a word doesn't count");
}

#[test]
fn a_rewritten_transcript_never_shows_a_broken_result() {
    use crate::semantic::tests::{reply, temp, write};
    let claude = temp("rewritten-claude");
    let path = write(
        &claude,
        "restart",
        &[user("/p", "main", "hello there friend"), reply("hi, what shall we do"), user("/p", "main", "the pane history is lost after a restart")],
    );
    let dirs = [claude.clone()];
    let history = with_meaning(&claude, "rewritten-dir");
    assert_eq!(ids(&history.search(&dirs, "scrollback relaunching", &Filter::default(), 10)), ["restart"]);
    // Rewritten shorter, as a compaction might: the index's rows for it say nothing true until it is embedded again.
    fs::write(&path, format!("{}\n", user("/p", "main", "hello there friend"))).unwrap();
    let found = history.search(&dirs, "scrollback relaunching", &Filter::default(), 10);
    assert!(found.sessions.is_empty(), "no result pointing at a message that is gone: {:?}", ids(&found));
    fs::remove_file(&path).unwrap();
    assert!(history.search(&dirs, "scrollback relaunching", &Filter::default(), 10).sessions.is_empty(), "removed");
}
