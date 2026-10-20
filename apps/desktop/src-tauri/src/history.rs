//! Search over past Claude Code and OpenCode sessions, for the history sheet and the `search_history` and
//! `read_session` MCP tools. Each Claude transcript is parsed once into its prompts, Claude's replies, title
//! and edited files, which stay in memory. Transcripts only grow, so every search first re-reads just the
//! bytes appended since. The format is internal to Claude Code: a line that doesn't parse, or an entry of a
//! type not used here, is skipped.
//!
//! OpenCode keeps its sessions in its background service, which `opencode.rs` already finds (it never
//! starts it, so without one there are no OpenCode sessions). A refresh lists the root sessions and fetches
//! the messages of those new or changed since (`time.updated`), a few at a time. It runs in the background:
//! a search never waits for it, it reports `opencodeLoading` while one runs, and the sheet asks again. The
//! MCP tools do wait, since their caller can't ask again.
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
    time::{Duration, Instant, UNIX_EPOCH},
};

use serde::{de, Deserialize, Deserializer, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Manager, State};

use crate::{
    claude::project_folder_name,
    opencode::{self, Api},
    AppState,
};

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

/// OpenCode's service is asked for changes at most this often.
const OPENCODE_EVERY: Duration = Duration::from_secs(5);
/// How long an MCP call waits for a refresh already running.
const OPENCODE_WAIT: Duration = Duration::from_secs(30);
/// Sessions fetched at once, and the page size of both lists.
const OPENCODE_WORKERS: usize = 6;
const OPENCODE_PAGE: &str = "200";
/// Stops a list whose cursor never ends.
const OPENCODE_MAX_PAGES: usize = 200;

#[derive(Default)]
pub struct History {
    index: Mutex<HashMap<PathBuf, Transcript>>,
    opencode: Mutex<OpenCode>,
}

/// OpenCode's root sessions, each fetched once and again when its `time.updated` changes.
#[derive(Default)]
struct OpenCode {
    sessions: HashMap<String, Cached>,
    loading: bool,
    last: Option<Instant>,
}

struct Cached {
    updated: u64,
    session: Session,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Agent {
    #[default]
    Claude,
    OpenCode,
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
    agent: Agent,
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

// ---- OpenCode ---------------------------------------------------------------------------------------------

/// `2026-10-01T10:00:05.000Z` for a time in ms since the epoch.
fn iso(ms: u64) -> String {
    let (days, rest) = ((ms / 86_400_000) as i64, ms % 86_400_000);
    // Civil date from a day count (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z", rest / 3_600_000, rest / 60_000 % 60, rest / 1000 % 60, rest % 1000)
}

fn created(item: &Value) -> Option<String> {
    item.pointer("/time/created").and_then(Value::as_u64).map(iso)
}

/// Every page of a list endpoint, in the order the service sends it. The last page's cursor still has a
/// `next`, so the end is an empty page.
fn pages(api: &impl Api, path: &str) -> Result<Vec<Value>, opencode::Error> {
    let mut items = Vec::new();
    let mut cursor: Option<String> = None;
    for _ in 0..OPENCODE_MAX_PAGES {
        // A cursor goes without `order`: the service takes it from the cursor.
        let mut query = vec![("limit", OPENCODE_PAGE)];
        if let Some(c) = &cursor {
            query.push(("cursor", c));
        }
        let page = api.get(path, &query)?;
        let data = page.get("data").and_then(Value::as_array).ok_or(opencode::Error::BadBody)?;
        if data.is_empty() {
            break;
        }
        items.extend(data.iter().cloned());
        match page.pointer("/cursor/next").and_then(Value::as_str) {
            Some(next) if cursor.as_deref() != Some(next) => cursor = Some(next.to_string()),
            _ => break,
        }
    }
    Ok(items)
}

/// The root sessions: `(id, time.updated, the session's JSON)`. Child sessions belong to a subagent.
fn opencode_roots(api: &impl Api) -> Result<Vec<(String, u64, Value)>, opencode::Error> {
    Ok(pages(api, "/api/session")?
        .into_iter()
        .filter(|s| s.get("parentID").is_none_or(Value::is_null))
        .filter_map(|s| {
            let id = s.get("id")?.as_str()?.to_string();
            let updated = s.pointer("/time/updated").and_then(Value::as_u64).unwrap_or(0);
            Some((id, updated, s))
        })
        .collect())
}

/// What a session holds, from the session's JSON and its messages oldest first. Only what you typed and
/// the assistant's text count; tool calls, reasoning and notices are left out.
fn opencode_session(info: &Value, messages: &[Value]) -> Session {
    let text = |key: &str| info.get(key).and_then(Value::as_str).filter(|t| !t.is_empty()).map(String::from);
    let mut session = Session {
        id: text("id").unwrap_or_default(),
        agent: Agent::OpenCode,
        cwd: info.pointer("/location/directory").and_then(Value::as_str).filter(|d| !d.is_empty()).map(String::from),
        ai_title: text("title"),
        first_time: info.pointer("/time/created").and_then(Value::as_u64).map(iso),
        last_time: info.pointer("/time/updated").and_then(Value::as_u64).map(iso),
        ..Session::default()
    };
    for item in messages {
        match item.get("type").and_then(Value::as_str) {
            Some("user") => {
                let prompt = item.get("text").and_then(Value::as_str).unwrap_or_default().trim();
                if !prompt.is_empty() {
                    session.push(Role::User, prompt, created(item));
                }
            }
            Some("assistant") => {
                if let Some(model) = item.pointer("/model/id").and_then(Value::as_str) {
                    push_unique(&mut session.models, model.to_string());
                }
                for part in item.get("content").and_then(Value::as_array).into_iter().flatten() {
                    if part.get("type").and_then(Value::as_str) != Some("text") {
                        continue;
                    }
                    let reply = part.get("text").and_then(Value::as_str).unwrap_or_default().trim();
                    if !reply.is_empty() {
                        session.reply(reply, created(item));
                    }
                }
            }
            _ => {}
        }
    }
    session.finish();
    session
}

fn opencode_messages(api: &impl Api, id: &str) -> Result<Vec<Value>, opencode::Error> {
    // Newest first, so the history reads it backwards.
    let mut messages = pages(api, &format!("/api/session/{id}/message"))?;
    messages.reverse();
    Ok(messages)
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
    pub agent: Agent,
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
    /// OpenCode's sessions are still being fetched, so more may show up on the next search.
    pub opencode_loading: bool,
}

impl History {
    pub fn search(&self, claude_dir: &Path, query: &str, filter: &Filter, limit: usize) -> Results {
        let mut index = self.index.lock().unwrap();
        refresh(&mut index, claude_dir);
        let opencode = self.opencode.lock().unwrap();
        let terms = terms(query);
        let project = filter.project.as_deref().map(|p| p.trim_end_matches('/')).filter(|p| !p.is_empty());
        // Each session with the time it was last active.
        let mut scope: Vec<(u64, &Session)> = index
            .values()
            .map(|t| (t.modified, &t.session))
            .chain(opencode.sessions.values().map(|c| (c.updated, &c.session)))
            .filter(|(_, s)| !s.messages.is_empty() && project.is_none_or(|p| s.in_project(p)))
            .collect();
        scope.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.id.cmp(&b.1.id)));
        let mut branches = Vec::new();
        for (_, s) in &scope {
            for branch in s.branches.iter().rev() {
                push_unique(&mut branches, branch.clone());
            }
        }
        branches.truncate(50);
        let found: Vec<(u64, &Session)> = scope
            .into_iter()
            .filter(|(_, s)| filter.branch.as_ref().is_none_or(|b| s.branches.contains(b)))
            .filter(|(modified, _)| filter.since_ms.is_none_or(|since| *modified >= since))
            .filter(|(_, s)| s.matches(&terms))
            .collect();
        let sessions = found
            .iter()
            .take(limit)
            .map(|(modified, s)| {
                let (matches, snippets) = if terms.is_empty() { (0, Vec::new()) } else { s.snippets(&terms) };
                Hit {
                    id: s.id.clone(),
                    agent: s.agent,
                    cwd: s.cwd.clone(),
                    title: s.title().map(String::from),
                    first_prompt: s.first_prompt(),
                    git_branch: s.branches.last().cloned(),
                    last_active_ms: *modified,
                    last_active: s.last_time.clone(),
                    messages: s.messages.len(),
                    matches,
                    snippets,
                }
            })
            .collect();
        Results { sessions, total: found.len(), branches, opencode_loading: opencode.loading }
    }

    /// Claims the next OpenCode refresh: false while one runs or one started within `OPENCODE_EVERY`.
    pub fn begin_opencode(&self) -> bool {
        let mut opencode = self.opencode.lock().unwrap();
        if opencode.loading || opencode.last.is_some_and(|t| t.elapsed() < OPENCODE_EVERY) {
            return false;
        }
        (opencode.loading, opencode.last) = (true, Some(Instant::now()));
        true
    }

    /// The refresh `begin_opencode` claimed. Without a running service there is nothing to load.
    pub fn run_opencode(&self) {
        match opencode::connect() {
            Ok(client) => self.refresh_opencode(&client),
            Err(_) => self.opencode.lock().unwrap().loading = false,
        }
    }

    /// A refresh that returns when it's done, for the MCP tools. One already running is waited for.
    pub fn sync_opencode(&self) {
        if self.begin_opencode() {
            return self.run_opencode();
        }
        let start = Instant::now();
        while self.opencode.lock().unwrap().loading && start.elapsed() < OPENCODE_WAIT {
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// Lists the root sessions, drops the ones that are gone and fetches the new or changed ones, each one
    /// in the cache as soon as it arrives. A session that fails to load is tried again next time.
    fn refresh_opencode<A: Api + Sync>(&self, api: &A) {
        if let Ok(roots) = opencode_roots(api) {
            let ids: Vec<&str> = roots.iter().map(|(id, _, _)| id.as_str()).collect();
            let mut work = {
                let mut opencode = self.opencode.lock().unwrap();
                opencode.sessions.retain(|id, _| ids.contains(&id.as_str()));
                roots.iter().filter(|(id, updated, _)| opencode.sessions.get(id).is_none_or(|c| c.updated != *updated)).collect::<Vec<_>>()
            };
            // Newest last, since workers pop from the end: what you most likely look for loads first.
            work.sort_by_key(|(_, updated, _)| *updated);
            let queue = Mutex::new(work);
            std::thread::scope(|scope| {
                for _ in 0..OPENCODE_WORKERS {
                    scope.spawn(|| loop {
                        let Some((id, updated, info)) = queue.lock().unwrap().pop() else { break };
                        let Ok(messages) = opencode_messages(api, id) else { continue };
                        let session = opencode_session(info, &messages);
                        self.opencode.lock().unwrap().sessions.insert(id.clone(), Cached { updated: *updated, session });
                    });
                }
            });
        }
        self.opencode.lock().unwrap().loading = false;
    }

    /// One session's summary and a page of its messages, for `read_session`.
    pub fn read_session(&self, claude_dir: &Path, id: &str, offset: usize, limit: usize) -> Result<Value, String> {
        let mut index = self.index.lock().unwrap();
        refresh(&mut index, claude_dir);
        let opencode = self.opencode.lock().unwrap();
        let s = index
            .values()
            .map(|t| &t.session)
            .find(|s| s.id == id)
            .or_else(|| opencode.sessions.get(id).map(|c| &c.session))
            .ok_or_else(|| format!("No session {id}"))?;
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
                "agent": s.agent,
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
pub fn history_search(app: AppHandle, state: State<AppState>, query: String, filter: Filter, limit: usize) -> Results {
    // OpenCode's sessions load in the background, so this search answers with what is there.
    if state.history.begin_opencode() {
        std::thread::spawn(move || app.state::<AppState>().history.run_opencode());
    }
    state.history.search(&state.claude_dir, &query, &filter, limit)
}

/// The built-in Wings MCP tools, listed before the plugins' tools.
pub fn mcp_tools() -> Vec<Value> {
    vec![
        json!({
            "name": "search_history",
            "description": "Search past Claude Code and OpenCode sessions on this computer: the prompts, the replies, session titles and the files each Claude Code session edited. Every word or \"quoted phrase\" has to match, ignoring case. Returns the most recent matches first, with snippets. Read one with read_session.",
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
            "description": "Read a past Claude Code or OpenCode session found with search_history: a summary, then the prompts and replies, oldest first. Long sessions come in pages; pass nextOffset back as offset for the next one.",
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
                            "agent": h.agent,
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
        assert!(!history.search(&dir, "", &Filter::default(), 10).opencode_loading);
        assert!(history.begin_opencode());
        assert!(history.search(&dir, "", &Filter::default(), 10).opencode_loading, "a refresh is running");
        assert!(!history.begin_opencode(), "one at a time");
        history.refresh_opencode(&service);

        let all = history.search(&dir, "", &Filter::default(), 10);
        assert!(!all.opencode_loading);
        let ids: Vec<(&str, Agent)> = all.sessions.iter().map(|h| (h.id.as_str(), h.agent)).collect();
        assert_eq!(ids, [("a", Agent::Claude), ("ses_root1", Agent::OpenCode), ("ses_root2", Agent::OpenCode)], "child and empty sessions are left out");
        let found = history.search(&dir, "MCP-CONFIG.json", &Filter::default(), 10);
        assert_eq!(found.sessions.len(), 1, "a system notice is not searched");
        let hit = &found.sessions[0];
        assert_eq!((hit.id.as_str(), hit.agent, hit.last_active_ms), ("ses_root1", Agent::OpenCode, 1_790_000_900_000));
        assert_eq!((hit.snippets[0].role, hit.snippets[0].text.as_str()), (Role::User, "Where is mcp-config.json read?"));
        let reply = history.search(&dir, "config::load", &Filter::default(), 10);
        assert_eq!(reply.sessions.iter().map(|h| h.id.as_str()).collect::<Vec<_>>(), ["a", "ses_root1"], "a reply is searched, newest first");
        assert_eq!(reply.sessions[1].snippets[0].role, Role::Assistant);
        assert!(history.search(&dir, "secret thoughts", &Filter::default(), 10).sessions.is_empty(), "reasoning is not searched");
        assert_eq!(history.search(&dir, "wire up", &Filter::default(), 10).sessions.len(), 1, "titles are searched");

        let app = Filter { project: Some("/w/app".into()), ..Filter::default() };
        assert_eq!(history.search(&dir, "", &app, 10).sessions.iter().map(|h| h.id.as_str()).collect::<Vec<_>>(), ["a", "ses_root1"], "the project filter uses the session's folder");
        assert!(history.search(&dir, "", &Filter { branch: Some("main".into()), project: Some("/w/other".into()), ..Filter::default() }, 10).sessions.is_empty());
        assert_eq!(history.search(&dir, "čakovec", &Filter { since_ms: Some(1_790_000_100_000), ..Filter::default() }, 10).total, 1);
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
        let ids: Vec<String> = history.search(&dir, "", &Filter::default(), 10).sessions.into_iter().map(|h| h.id).collect();
        assert_eq!(ids, ["ses_root2"], "a session the service no longer lists is dropped");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn reads_an_opencode_session_and_the_mcp_tools_name_the_agent() {
        let dir = temp_claude_dir("opencode-read");
        let service = fake_opencode();
        let history = History::default();
        history.refresh_opencode(&service);
        let page = history.read_session(&dir, "ses_root1", 0, 1).unwrap();
        assert_eq!((page["session"]["agent"].as_str(), page["session"]["project"].as_str(), page["session"]["messages"].as_u64()), (Some("opencode"), Some("/w/app"), Some(2)));
        assert_eq!((page["messages"][0]["role"].as_str(), page["nextOffset"].as_u64()), (Some("user"), Some(1)));
        let next = history.read_session(&dir, "ses_root1", 1, 10).unwrap();
        assert_eq!((next["messages"][0]["text"].as_str(), next["nextOffset"].as_u64()), (Some("It is read in Config::load.\n\nDone."), None));
        assert!(history.read_session(&dir, "ses_child", 0, 10).is_err());
        let found: Value = serde_json::from_str(&call_tool(&history, &dir, "search_history", &json!({ "query": "Čakovec" })).unwrap().unwrap()).unwrap();
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
        let real = crate::claude::claude_dir();
        history.search(&real, "", &Filter::default(), 1);
        for query in ["mcp-config.json", "the", "xyzzy-nowhere"] {
            let start = Instant::now();
            let found = history.search(&real, query, &Filter::default(), 100);
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
