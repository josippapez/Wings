//! Search over past Claude Code sessions, for the history sheet and the `search_history` and `read_session`
//! MCP tools. Each transcript is parsed once into its prompts, Claude's replies, title and edited files, which
//! stay in memory. Transcripts only grow, so every search first re-reads just the bytes appended since.
//! The format is internal to Claude Code: a line that doesn't parse, or an entry of a type not used here, is
//! skipped.
//!
//! debt: a scan, not an index. On 1,510 transcripts (1.2 GB) the first search parses them all in ~0.5 s on 8
//! cores, and the app grows by ~85 MB. Then a search takes ~15 ms, ~12 ms of it the refresh's stat of every
//! file, and a turn appended to a transcript adds ~1 ms. That meets the plan's 100 ms search and 500 ms update
//! targets. If the text outgrows memory or a search passes 100 ms, move it to SQLite FTS5 with the trigram
//! tokenizer, which keeps substring matching.

use std::{
    collections::HashMap,
    fmt, fs,
    io::{BufRead, BufReader, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::Mutex,
    time::UNIX_EPOCH,
};

use serde::{de, Deserialize, Deserializer, Serialize};
use serde_json::{json, Value};
use tauri::State;

use crate::{claude::project_folder_name, AppState};

/// Tools whose input names a file the session changed.
const EDIT_TOOLS: [&str; 4] = ["Edit", "MultiEdit", "Write", "NotebookEdit"];
/// Snippets per session, and the characters around a match in each.
const SNIPPETS: usize = 3;
const BEFORE: usize = 60;
const AFTER: usize = 140;
/// A `read_session` page stops near this many bytes of text, about 10k tokens, well under Claude Code's
/// 25k-token cap on a tool result. A single longer message is cut.
const PAGE_BYTES: usize = 40_000;
const MESSAGE_BYTES: usize = 8_000;

#[derive(Default)]
pub struct History {
    index: Mutex<HashMap<PathBuf, Transcript>>,
}

/// One transcript, parsed up to `offset`. `modified` and `len` are the file's when it was last read.
struct Transcript {
    modified: u64,
    len: u64,
    offset: u64,
    session: Session,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
    /// A snippet from the list of files the session edited.
    File,
}

struct Message {
    role: Role,
    start: usize,
    time: Option<String>,
}

#[derive(Default)]
struct Session {
    id: String,
    /// The folder under `projects/`, named after the cwd the session started in.
    folder: String,
    cwd: Option<String>,
    ai_title: Option<String>,
    custom_title: Option<String>,
    /// Every branch the session ran on, the current one last.
    branches: Vec<String>,
    models: Vec<String>,
    pr_links: Vec<String>,
    files: Vec<String>,
    first_time: Option<String>,
    last_time: Option<String>,
    /// The messages, joined by `\n`.
    text: String,
    /// `text` folded for matching, at the same byte offsets. Its only `\n`s are the joins, and search terms
    /// never contain one, so a match can't span two messages.
    folded: String,
    messages: Vec<Message>,
    /// The id, title, branches and edited files, folded and matched like the messages.
    meta: String,
    /// The last message is Claude's, so its next text block continues it.
    replying: bool,
}

#[derive(Deserialize)]
struct Entry {
    #[serde(rename = "type")]
    kind: Option<String>,
    #[serde(rename = "isSidechain")]
    sidechain: Option<bool>,
    #[serde(rename = "isMeta")]
    meta: Option<bool>,
    #[serde(rename = "isCompactSummary")]
    compact_summary: Option<bool>,
    cwd: Option<String>,
    #[serde(rename = "gitBranch")]
    git_branch: Option<String>,
    timestamp: Option<String>,
    message: Option<Body>,
    #[serde(rename = "aiTitle")]
    ai_title: Option<String>,
    #[serde(rename = "customTitle")]
    custom_title: Option<String>,
    #[serde(rename = "prUrl")]
    pr_url: Option<String>,
}

#[derive(Deserialize)]
struct Body {
    model: Option<String>,
    content: Option<Content>,
}

/// A message's content: a string, or blocks. Fields not declared here, like tool output, are skipped
/// without being copied, which is most of a transcript's bytes.
enum Content {
    Text(String),
    Blocks(Vec<Block>),
}

#[derive(Deserialize)]
struct Block {
    #[serde(rename = "type")]
    kind: Option<String>,
    text: Option<String>,
    name: Option<String>,
    input: Option<ToolInput>,
}

#[derive(Deserialize)]
struct ToolInput {
    file_path: Option<String>,
    notebook_path: Option<String>,
}

impl<'de> Deserialize<'de> for Content {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> de::Visitor<'de> for Visitor {
            type Value = Content;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a string or a list of blocks")
            }
            fn visit_str<E: de::Error>(self, s: &str) -> Result<Content, E> {
                Ok(Content::Text(s.to_owned()))
            }
            fn visit_string<E: de::Error>(self, s: String) -> Result<Content, E> {
                Ok(Content::Text(s))
            }
            fn visit_seq<A: de::SeqAccess<'de>>(self, mut seq: A) -> Result<Content, A::Error> {
                let mut blocks = Vec::new();
                while let Some(block) = seq.next_element()? {
                    blocks.push(block);
                }
                Ok(Content::Blocks(blocks))
            }
        }
        d.deserialize_any(Visitor)
    }
}

impl Content {
    fn blocks(self) -> Vec<Block> {
        match self {
            Content::Text(text) => vec![Block { kind: Some("text".into()), text: Some(text), name: None, input: None }],
            Content::Blocks(blocks) => blocks,
        }
    }
}

/// Lowercases without changing any character's byte length, so an offset in the folded text is the same
/// offset in the original. Whitespace becomes a space.
fn fold_into(out: &mut String, s: &str) {
    for c in s.chars() {
        if c.is_ascii() {
            out.push(if c.is_ascii_whitespace() { ' ' } else { c.to_ascii_lowercase() });
        } else {
            let mut lower = c.to_lowercase();
            match (lower.next(), lower.next()) {
                (Some(l), None) if l.len_utf8() == c.len_utf8() => out.push(l),
                _ => out.push(c),
            }
        }
    }
}

fn fold(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    fold_into(&mut out, s);
    out
}

fn push_unique(list: &mut Vec<String>, value: String) {
    if !list.contains(&value) {
        list.push(value);
    }
}

/// What you typed, not a slash command's wrapper, a background task's notice or an interruption marker.
fn typed(text: &str) -> bool {
    !(text.is_empty() || text.starts_with('<') || text.starts_with("Caveat:") || text.starts_with("[Request interrupted"))
}

impl Session {
    /// Reads one transcript line. False when it isn't a JSON entry.
    fn ingest(&mut self, line: &[u8]) -> bool {
        let Ok(entry) = serde_json::from_slice::<Entry>(line) else { return false };
        if let Some(time) = &entry.timestamp {
            self.first_time.get_or_insert_with(|| time.clone());
            self.last_time = Some(time.clone());
        }
        if self.cwd.is_none() {
            self.cwd = entry.cwd.filter(|c| !c.is_empty());
        }
        let sidechain = entry.sidechain == Some(true);
        match entry.kind.as_deref() {
            Some("ai-title") => self.ai_title = entry.ai_title.or(self.ai_title.take()),
            Some("custom-title") => self.custom_title = entry.custom_title.or(self.custom_title.take()),
            Some("pr-link") => {
                if let Some(url) = entry.pr_url {
                    push_unique(&mut self.pr_links, url);
                }
            }
            Some("user") if !sidechain && entry.meta != Some(true) && entry.compact_summary != Some(true) => {
                if let Some(branch) = entry.git_branch.filter(|b| !b.is_empty()) {
                    if self.branches.last() != Some(&branch) {
                        self.branches.retain(|b| *b != branch);
                        self.branches.push(branch);
                    }
                }
                let blocks = entry.message.and_then(|m| m.content).map(Content::blocks).unwrap_or_default();
                let text: Vec<String> = blocks
                    .into_iter()
                    .filter(|b| b.kind.as_deref() == Some("text"))
                    .filter_map(|b| b.text)
                    .collect();
                let text = text.join("\n");
                let text = text.trim();
                if typed(text) {
                    self.push(Role::User, text, entry.timestamp);
                }
            }
            Some("assistant") if !sidechain => {
                let Some(body) = entry.message else { return true };
                // `<synthetic>` replies are Claude Code's own notices, like API errors.
                match body.model {
                    Some(model) if model.starts_with('<') => return true,
                    Some(model) => push_unique(&mut self.models, model),
                    None => {}
                }
                for block in body.content.map(Content::blocks).unwrap_or_default() {
                    match block.kind.as_deref() {
                        Some("text") => {
                            let text = block.text.unwrap_or_default();
                            let text = text.trim();
                            if !text.is_empty() {
                                self.reply(text, entry.timestamp.clone());
                            }
                        }
                        Some("tool_use") if block.name.as_deref().is_some_and(|n| EDIT_TOOLS.contains(&n)) => {
                            if let Some(path) = block.input.and_then(|i| i.file_path.or(i.notebook_path)) {
                                push_unique(&mut self.files, path);
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
        true
    }

    fn push(&mut self, role: Role, text: &str, time: Option<String>) {
        if !self.messages.is_empty() {
            self.text.push('\n');
            self.folded.push('\n');
        }
        self.messages.push(Message { role, start: self.text.len(), time });
        self.text.push_str(text);
        fold_into(&mut self.folded, text);
        self.replying = role == Role::Assistant;
    }

    /// Claude's reply to a prompt arrives as one text block per entry, with tool calls between them.
    fn reply(&mut self, text: &str, time: Option<String>) {
        if !self.replying {
            return self.push(Role::Assistant, text, time);
        }
        self.text.push_str("\n\n");
        self.folded.push_str("  ");
        self.text.push_str(text);
        fold_into(&mut self.folded, text);
    }

    fn finish(&mut self) {
        let mut meta = format!("{}\n{}", self.id, self.title().unwrap_or_default());
        for value in self.branches.iter().chain(&self.files) {
            meta.push('\n');
            meta.push_str(value);
        }
        self.meta = fold(&meta);
    }

    fn title(&self) -> Option<&str> {
        self.custom_title.as_deref().or(self.ai_title.as_deref())
    }

    fn range(&self, i: usize) -> (usize, usize) {
        let end = self.messages.get(i + 1).map_or(self.text.len(), |next| next.start - 1);
        (self.messages[i].start, end)
    }

    fn first_prompt(&self) -> Option<String> {
        let i = self.messages.iter().position(|m| m.role == Role::User)?;
        let (start, end) = self.range(i);
        Some(self.text[start..end].chars().take(240).collect())
    }

    /// Sessions in `project` or a folder inside it, like a worktree.
    fn in_project(&self, project: &str) -> bool {
        self.folder == project_folder_name(project)
            || self.cwd.as_deref().is_some_and(|cwd| cwd == project || cwd.strip_prefix(project).is_some_and(|rest| rest.starts_with('/')))
    }

    fn matches(&self, terms: &[String]) -> bool {
        terms.iter().all(|t| self.folded.contains(t.as_str()) || self.meta.contains(t.as_str()))
    }

    /// The messages that match any term, counted, and the first few quoted around the match.
    fn snippets(&self, terms: &[String]) -> (usize, Vec<Snippet>) {
        let mut count = 0;
        let mut snippets = Vec::new();
        for i in 0..self.messages.len() {
            let (start, end) = self.range(i);
            let folded = &self.folded[start..end];
            let Some((at, len)) = terms.iter().filter_map(|t| folded.find(t.as_str()).map(|at| (at, t.len()))).min() else { continue };
            count += 1;
            if snippets.len() < SNIPPETS {
                snippets.push(Snippet { role: self.messages[i].role, text: quote(&self.text[start..end], at, len) });
            }
        }
        if snippets.is_empty() {
            if let Some(file) = self.files.iter().find(|f| terms.iter().any(|t| fold(f).contains(t.as_str()))) {
                snippets.push(Snippet { role: Role::File, text: file.clone() });
            }
        }
        (count, snippets)
    }
}

/// The text around a match, on one line.
fn quote(text: &str, at: usize, len: usize) -> String {
    let start = text.floor_char_boundary(at.saturating_sub(BEFORE));
    let end = text.ceil_char_boundary((at + len + AFTER).min(text.len()));
    let body = text[start..end].split_whitespace().collect::<Vec<_>>().join(" ");
    format!("{}{body}{}", if start > 0 { "…" } else { "" }, if end < text.len() { "…" } else { "" })
}

/// Words and "quoted phrases", folded. Every one has to match.
fn terms(query: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (i, part) in query.split('"').enumerate() {
        let words: Vec<&str> = part.split_whitespace().collect();
        let found = if i % 2 == 1 { vec![words.join(" ")] } else { words.into_iter().map(String::from).collect() };
        for term in found.iter().filter(|t| !t.is_empty()).map(|t| fold(t)) {
            push_unique(&mut out, term);
        }
    }
    out
}

/// Reads a transcript from `offset` on. A last line without its newline is kept only if it's complete JSON;
/// otherwise Claude Code is still writing it, and the next read starts there.
fn read(path: &Path, offset: u64, session: &mut Session) -> u64 {
    let Ok(mut file) = fs::File::open(path) else { return offset };
    if file.seek(SeekFrom::Start(offset)).is_err() {
        return offset;
    }
    let mut reader = BufReader::with_capacity(1 << 16, file);
    let mut offset = offset;
    let mut line = Vec::new();
    loop {
        line.clear();
        let n = match reader.read_until(b'\n', &mut line) {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        let parsed = session.ingest(&line);
        if line.last() != Some(&b'\n') && !parsed {
            break;
        }
        offset += n as u64;
    }
    session.finish();
    offset
}

fn modified_ms(meta: &fs::Metadata) -> u64 {
    meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_millis() as u64)
}

/// Brings the index up to date with the transcripts on disk: new files are parsed, grown ones read from
/// where they ended, and deleted ones dropped. Files are parsed on every core.
fn refresh(index: &mut HashMap<PathBuf, Transcript>, claude_dir: &Path) {
    let mut kept = HashMap::with_capacity(index.len());
    let mut work = Vec::new();
    for folder in fs::read_dir(claude_dir.join("projects")).into_iter().flatten().flatten() {
        let folder_name = folder.file_name().to_string_lossy().into_owned();
        for file in fs::read_dir(folder.path()).into_iter().flatten().flatten() {
            let path = file.path();
            if path.extension().is_none_or(|x| x != "jsonl") {
                continue;
            }
            let Ok(meta) = file.metadata() else { continue };
            let (modified, len) = (modified_ms(&meta), meta.len());
            match index.remove(&path) {
                Some(old) if old.modified == modified && old.len == len => {
                    kept.insert(path, old);
                }
                Some(old) if len > old.len => work.push((path, Transcript { modified, len, ..old })),
                _ => {
                    let id = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                    let session = Session { id, folder: folder_name.clone(), ..Session::default() };
                    work.push((path, Transcript { modified, len, offset: 0, session }));
                }
            }
        }
    }
    // Biggest last, since workers pop from the end: the longest files start first.
    work.sort_by_key(|(_, t)| t.len.saturating_sub(t.offset));
    let queue = Mutex::new(work);
    let done = Mutex::new(Vec::new());
    let workers = std::thread::available_parallelism().map_or(4, |n| n.get());
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| loop {
                let Some((path, mut transcript)) = queue.lock().unwrap().pop() else { break };
                transcript.offset = read(&path, transcript.offset, &mut transcript.session);
                done.lock().unwrap().push((path, transcript));
            });
        }
    });
    kept.extend(done.into_inner().unwrap());
    *index = kept;
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Filter {
    /// A project's path. Its sessions, and those in folders inside it, match.
    pub project: Option<String>,
    pub branch: Option<String>,
    /// Last active at or after this time, in ms since the epoch.
    pub since_ms: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct Snippet {
    pub role: Role,
    pub text: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Hit {
    pub id: String,
    pub cwd: Option<String>,
    pub title: Option<String>,
    pub first_prompt: Option<String>,
    pub git_branch: Option<String>,
    pub last_active_ms: u64,
    pub last_active: Option<String>,
    pub messages: usize,
    /// Messages that match the search.
    pub matches: usize,
    pub snippets: Vec<Snippet>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Results {
    /// Most recent first.
    pub sessions: Vec<Hit>,
    /// Sessions that match, of which `sessions` are the first.
    pub total: usize,
    /// The branches of the sessions in the project, most recently used first.
    pub branches: Vec<String>,
}

impl History {
    pub fn search(&self, claude_dir: &Path, query: &str, filter: &Filter, limit: usize) -> Results {
        let mut index = self.index.lock().unwrap();
        refresh(&mut index, claude_dir);
        let terms = terms(query);
        let project = filter.project.as_deref().map(|p| p.trim_end_matches('/')).filter(|p| !p.is_empty());
        let mut scope: Vec<&Transcript> =
            index.values().filter(|t| !t.session.messages.is_empty() && project.is_none_or(|p| t.session.in_project(p))).collect();
        scope.sort_by(|a, b| b.modified.cmp(&a.modified).then_with(|| a.session.id.cmp(&b.session.id)));
        let mut branches = Vec::new();
        for t in &scope {
            for branch in t.session.branches.iter().rev() {
                push_unique(&mut branches, branch.clone());
            }
        }
        branches.truncate(50);
        let found: Vec<&Transcript> = scope
            .into_iter()
            .filter(|t| filter.branch.as_ref().is_none_or(|b| t.session.branches.contains(b)))
            .filter(|t| filter.since_ms.is_none_or(|since| t.modified >= since))
            .filter(|t| t.session.matches(&terms))
            .collect();
        let sessions = found
            .iter()
            .take(limit)
            .map(|t| {
                let s = &t.session;
                let (matches, snippets) = if terms.is_empty() { (0, Vec::new()) } else { s.snippets(&terms) };
                Hit {
                    id: s.id.clone(),
                    cwd: s.cwd.clone(),
                    title: s.title().map(String::from),
                    first_prompt: s.first_prompt(),
                    git_branch: s.branches.last().cloned(),
                    last_active_ms: t.modified,
                    last_active: s.last_time.clone(),
                    messages: s.messages.len(),
                    matches,
                    snippets,
                }
            })
            .collect();
        Results { sessions, total: found.len(), branches }
    }

    /// One session's summary and a page of its messages, for `read_session`.
    pub fn read_session(&self, claude_dir: &Path, id: &str, offset: usize, limit: usize) -> Result<Value, String> {
        let mut index = self.index.lock().unwrap();
        refresh(&mut index, claude_dir);
        let s = index.values().map(|t| &t.session).find(|s| s.id == id).ok_or_else(|| format!("No session {id}"))?;
        let mut messages = Vec::new();
        let mut used = 0;
        let mut next = offset.min(s.messages.len());
        while next < s.messages.len() && messages.len() < limit {
            let (start, end) = s.range(next);
            let text = clip(&s.text[start..end]);
            if !messages.is_empty() && used + text.len() > PAGE_BYTES {
                break;
            }
            used += text.len();
            let m = &s.messages[next];
            messages.push(json!({ "index": next, "role": m.role, "time": m.time, "text": text }));
            next += 1;
        }
        let files = s.files.len();
        Ok(json!({
            "session": {
                "sessionId": s.id,
                "project": s.cwd,
                "title": s.title(),
                "branches": s.branches,
                "models": s.models,
                "pullRequests": s.pr_links,
                "filesEdited": s.files.iter().take(40).collect::<Vec<_>>(),
                "moreFilesEdited": files.saturating_sub(40),
                "started": s.first_time,
                "lastActive": s.last_time,
                "messages": s.messages.len(),
            },
            "messages": messages,
            "nextOffset": (next < s.messages.len()).then_some(next),
        }))
    }
}

/// A message cut to `MESSAGE_BYTES`, saying how much is left out.
fn clip(text: &str) -> String {
    if text.len() <= MESSAGE_BYTES {
        return text.to_string();
    }
    let end = text.floor_char_boundary(MESSAGE_BYTES);
    format!("{}… [{} more bytes not shown]", &text[..end], text.len() - end)
}

#[tauri::command(async)]
pub fn history_search(state: State<AppState>, query: String, filter: Filter, limit: usize) -> Results {
    state.history.search(&state.claude_dir, &query, &filter, limit)
}

/// The built-in Wings MCP tools, listed before the plugins' tools.
pub fn mcp_tools() -> Vec<Value> {
    vec![
        json!({
            "name": "search_history",
            "description": "Search past Claude Code sessions on this computer: the prompts, Claude's replies, session titles and the files each session edited. Every word or \"quoted phrase\" has to match, ignoring case. Returns the most recent matches first, with snippets. Read one with read_session.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Words to find. Empty lists the most recent sessions." },
                    "project": { "type": "string", "description": "Absolute path of a project, such as your working directory, to search only its sessions. Leave out to search every project." },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 50, "default": 10, "description": "Most sessions to return." }
                },
                "required": ["query"]
            }
        }),
        json!({
            "name": "read_session",
            "description": "Read a past Claude Code session found with search_history: a summary, then the prompts and Claude's replies, oldest first. Long sessions come in pages; pass nextOffset back as offset for the next one.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "session_id": { "type": "string", "description": "The session's id, from search_history." },
                    "offset": { "type": "integer", "minimum": 0, "default": 0, "description": "Index of the first message to return." },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 100, "default": 30, "description": "Most messages to return. A page also ends at about 10k tokens." }
                },
                "required": ["session_id"]
            }
        }),
    ]
}

/// Runs a built-in tool. `None` when `name` isn't one.
pub fn call_tool(history: &History, claude_dir: &Path, name: &str, args: &Value) -> Option<Result<String, String>> {
    let number = |key: &str, default: usize, max: usize| args.get(key).and_then(Value::as_u64).map_or(default, |n| (n as usize).min(max));
    Some(match name {
        "search_history" => match args.get("query").and_then(Value::as_str) {
            Some(query) => {
                let filter = Filter { project: args.get("project").and_then(Value::as_str).map(String::from), ..Filter::default() };
                let results = history.search(claude_dir, query, &filter, number("limit", 10, 50).max(1));
                let sessions: Vec<Value> = results
                    .sessions
                    .into_iter()
                    .map(|h| {
                        json!({
                            "sessionId": h.id,
                            "project": h.cwd,
                            "title": h.title.or(h.first_prompt),
                            "branch": h.git_branch,
                            "lastActive": h.last_active,
                            "messages": h.messages,
                            "matchingMessages": h.matches,
                            "snippets": h.snippets,
                        })
                    })
                    .collect();
                Ok(json!({ "sessions": sessions, "total": results.total }).to_string())
            }
            None => Err("search_history needs a query".into()),
        },
        "read_session" => match args.get("session_id").and_then(Value::as_str) {
            Some(id) => history.read_session(claude_dir, id, number("offset", 0, usize::MAX), number("limit", 30, 100).max(1)).map(|v| v.to_string()),
            None => Err("read_session needs a session_id".into()),
        },
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

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

    fn texts(history: &History, claude_dir: &Path, id: &str) -> Vec<(String, String)> {
        let page = history.read_session(claude_dir, id, 0, 100).unwrap();
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
            texts(&history, &dir, "s1"),
            [pair("user", "Fix the login bug"), pair("assistant", "Looking.\n\nFixed it."), pair("user", "Now the signup page")]
        );
        let page = history.read_session(&dir, "s1", 0, 100).unwrap();
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
        assert_eq!(texts(&history, &dir, "s1").len(), 1, "a half-written line waits");

        // Garbage in place of the first line proves the next read starts after it.
        let mut bytes = fs::read(&path).unwrap();
        let first_len = bytes.iter().position(|b| *b == b'\n').unwrap();
        bytes[..first_len].fill(b'x');
        bytes.extend_from_slice(format!("{tail}\n{}\n", user("/tmp/proj", "main", "second prompt")).as_bytes());
        fs::write(&path, bytes).unwrap();
        assert_eq!(
            texts(&history, &dir, "s1"),
            [pair("user", "first prompt"), pair("assistant", "a reply being written"), pair("user", "second prompt")]
        );

        fs::write(&path, format!("{}\n", user("/tmp/proj", "main", "rewritten"))).unwrap();
        assert_eq!(texts(&history, &dir, "s1"), [pair("user", "rewritten")], "a shrunk file is read again");
        fs::remove_file(&path).unwrap();
        assert!(history.read_session(&dir, "s1", 0, 10).is_err(), "a deleted transcript drops out");
        let _ = fs::remove_dir_all(&dir);
    }

    fn sorted_ids(results: &Results) -> Vec<String> {
        let mut ids: Vec<String> = results.sessions.iter().map(|h| h.id.clone()).collect();
        ids.sort();
        ids
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
        let search = |q: &str, filter: Filter| history.search(&dir, q, &filter, 10);
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
    fn a_phrase_never_matches_across_two_messages() {
        let dir = temp_claude_dir("phrase");
        transcript(&dir, "/p", "a", &[user("/p", "main", "ends with foo"), assistant("bar starts this")]);
        let history = History::default();
        assert_eq!(history.search(&dir, "foo bar", &Filter::default(), 10).total, 1);
        assert_eq!(history.search(&dir, "\"foo bar\"", &Filter::default(), 10).total, 0);
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
        let call = |name: &str, args: Value| call_tool(&history, &dir, name, &args).unwrap();

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
        assert!(call_tool(&history, &dir, "cyclops__start_timer", &json!({})).is_none());
        let _ = fs::remove_dir_all(&dir);
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
        let real = crate::claude::claude_dir();
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
        let all = history.search(&dir, "", &Filter::default(), 1);
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
                history.search(&dir, "", &Filter::default(), 1);
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
                    total = history.search(&dir, query, &filter, 100).total;
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
            history.search(&dir, "", &Filter::default(), 1);
            runs.push(start.elapsed());
        }
        println!("refresh after {} KB appended: {:?} (median of 5)", tail.len() / 1000, median(&mut runs));

        if let Some(cwd) = biggest_cwd {
            let start = Instant::now();
            let listed = crate::claude::list_sessions(&real, &cwd).len();
            println!("before, for comparison: claude::list_sessions({cwd}) took {:?} for {listed} sessions", start.elapsed());
        }
        let _ = fs::remove_dir_all(&dir);
    }
}
