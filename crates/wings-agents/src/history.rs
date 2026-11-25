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
    collections::{HashMap, HashSet},
    fmt, fs,
    io::{BufRead, BufReader, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, Instant, UNIX_EPOCH},
};

use serde::{de, Deserialize, Deserializer, Serialize};
use serde_json::{json, Value};

use crate::opencode::{self, Api};

/// Claude Code names a project folder after its cwd with every non-alphanumeric character
/// replaced by `-` (paths over 200 chars get truncated plus a hash; not handled here).
pub fn project_folder_name(cwd: &str) -> String {
    cwd.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect()
}

/// Tools whose input names a file the session changed.
const EDIT_TOOLS: [&str; 4] = ["Edit", "MultiEdit", "Write", "NotebookEdit"];
/// Sessions the semantic index adds to a search at most.
const SIMILAR: usize = 20;
/// Words too common to tell one session from another, which a search doesn't rank by: repo-docs' keyword search's list,
/// with the pronouns prompts are full of.
const STOP_WORDS: &[&str] = &[
    "a", "an", "the", "is", "are", "was", "were", "be", "been", "being", "have", "has", "had", "do", "does", "did", "will",
    "would", "could", "should", "may", "might", "can", "shall", "to", "of", "in", "for", "on", "with", "at", "by", "from",
    "as", "into", "through", "during", "before", "after", "between", "out", "off", "over", "under", "then", "here",
    "there", "when", "where", "why", "how", "all", "each", "every", "both", "few", "more", "most", "other", "some", "such",
    "no", "nor", "not", "only", "own", "same", "so", "than", "too", "very", "just", "because", "but", "and", "or", "if",
    "while", "that", "this", "it", "its", "i", "my",
];
/// BM25+ as Orama scores repo-docs' chunks: k1, b and δ.
const BM25: (f32, f32, f32) = (1.2, 0.75, 0.5);
/// What a session's title, id, branches and edited files count for against one of its messages, as repo-docs' keyword
/// search counts a doc's title and path above its text.
const META_BOOST: f32 = 2.0;
/// How repo-docs weighs the two halves of its hybrid search: the words a little, what they mean mostly. Each half is
/// divided by its best score first.
const TEXT_WEIGHT: f32 = 0.2;
const MEANING_WEIGHT: f32 = 0.8;
/// Snippets per session, and the characters around a match in each.
const SNIPPETS: usize = 3;
const BEFORE: usize = 60;
const AFTER: usize = 140;
/// A `read_session` page stops near this many bytes of text, about 10k tokens, well under Claude Code's
/// 25k-token cap on a tool result. A single longer message is cut.
const PAGE_BYTES: usize = 40_000;
/// The opening a generated name reads: prompts from the first one on, until this many characters or prompts.
const OPENING_CHARS: usize = 200;
const OPENING_PROMPTS: usize = 4;
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
    /// The transcript folders as last listed (`Listings`).
    listings: Mutex<Listings>,
    opencode: Mutex<OpenCode>,
    /// The index of what the sessions say (`semantic`), whose matches a search fuses with its own.
    semantic: std::sync::OnceLock<std::sync::Arc<crate::semantic::Semantic>>,
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
    /// A subagent's transcript, whose every entry is a sidechain: it is read like a session's own.
    sidechains: bool,
    /// Started by a program through the Agent SDK (Python or TypeScript), like an automated review: not listed.
    by_program: Option<bool>,
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
    entrypoint: Option<String>,
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

pub fn fold(s: &str) -> String {
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
        if let Some(entrypoint) = &entry.entrypoint {
            self.by_program.get_or_insert(matches!(entrypoint.as_str(), "sdk-py" | "sdk-ts"));
        }
        let sidechain = entry.sidechain == Some(true) && !self.sidechains;
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
pub fn terms(query: &str) -> Vec<String> {
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

/// The terms a search ranks by: its words and phrases without the stop words, or all of them when that leaves none.
fn ranking_terms(terms: &[String]) -> Vec<&str> {
    let kept: Vec<&str> = terms.iter().map(String::as_str).filter(|t| !STOP_WORDS.contains(t)).collect();
    if kept.is_empty() { terms.iter().map(String::as_str).collect() } else { kept }
}

/// How often `term` starts a word in `text`: inside a word ("ci" in "decision") doesn't count, its start
/// ("relaunch" in "relaunching") does.
fn word_starts<'a>(text: &'a str, term: &'a str) -> impl Iterator<Item = usize> + 'a {
    text.match_indices(term)
        .map(|(at, _)| at)
        .filter(|&at| !text[..at].chars().next_back().is_some_and(char::is_alphanumeric))
}

/// Each session's BM25+ score for `terms`, as Orama scores repo-docs' chunks: its best message's, so a long session
/// doesn't win by its size, and a word in most messages counts for almost nothing. Its title, id, branches and edited
/// files count as one more message, `META_BOOST` times.
fn bm25(sessions: &[&Session], terms: &[&str]) -> Vec<f32> {
    let (k, b, d) = BM25;
    let docs: usize = sessions.iter().map(|s| s.messages.len()).sum();
    if docs == 0 {
        return vec![0.0; sessions.len()];
    }
    let average = sessions.iter().map(|s| s.text.len()).sum::<usize>() as f32 / docs as f32;
    let score = |idf: f32, tf: f32, length: f32| idf * (d + tf * (k + 1.0)) / (tf + k * (1.0 - b + b * length / average));
    // Each message's score so far, and the meta's last.
    let mut scores: Vec<Vec<f32>> = sessions.iter().map(|s| vec![0.0; s.messages.len() + 1]).collect();
    for term in terms {
        // (session, message, how often) for each message holding the term.
        let mut found: Vec<(usize, usize, f32)> = Vec::new();
        for (i, s) in sessions.iter().enumerate() {
            let mut last: Option<(usize, f32)> = None;
            for at in word_starts(&s.folded, term) {
                let message = s.messages.partition_point(|m| m.start <= at).saturating_sub(1);
                match &mut last {
                    Some((m, tf)) if *m == message => *tf += 1.0,
                    _ => {
                        if let Some((m, tf)) = last.take() {
                            found.push((i, m, tf));
                        }
                        last = Some((message, 1.0));
                    }
                }
            }
            if let Some((m, tf)) = last {
                found.push((i, m, tf));
            }
        }
        let df = found.len() as f32;
        let idf = (1.0 + (docs as f32 - df + 0.5) / (df + 0.5)).ln();
        for (i, message, tf) in found {
            let (start, end) = sessions[i].range(message);
            scores[i][message] += score(idf, tf, (end - start) as f32);
        }
        for (i, s) in sessions.iter().enumerate() {
            let tf = word_starts(&s.meta, term).count() as f32;
            if tf > 0.0 {
                *scores[i].last_mut().unwrap() += META_BOOST * score(idf, tf, average);
            }
        }
    }
    scores.into_iter().map(|s| s.into_iter().fold(0.0, f32::max)).collect()
}

/// The passage of `session` (read from `transcript`) that the semantic index found `hit` in. None when it is gone: the
/// transcript is smaller than when it was indexed, so it was rewritten, or no longer has that message.
fn similar_snippet(transcript: Option<&PathBuf>, session: &Session, hit: &crate::semantic::Ranked) -> Option<Snippet> {
    if fs::metadata(&hit.path).map_or(true, |m| m.len() < hit.len) {
        return None;
    }
    let i = hit.best.message as usize;
    let (role, text) = if transcript == Some(&hit.path) {
        let message = session.messages.get(i)?;
        let (start, end) = session.range(i);
        (message.role, std::borrow::Cow::Borrowed(&session.text[start..end]))
    } else {
        // A subagent's, or the session's copy in another account: read back from that transcript.
        let messages = transcript_messages(&hit.path, crate::semantic::is_subagent(&hit.path));
        (Role::Assistant, std::borrow::Cow::Owned(messages.into_iter().nth(i)?))
    };
    Some(Snippet { role, text: crate::semantic::passage(&text, &hit.best)? })
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

/// Every message of the transcript at `path` as a search reads it, numbered the same way. `sidechains`: a subagent's
/// transcript, all of whose entries are sidechains. For the semantic index.
pub fn transcript_messages(path: &Path, sidechains: bool) -> Vec<String> {
    let mut session = Session { sidechains, ..Session::default() };
    read(path, 0, &mut session);
    (0..session.messages.len())
        .map(|i| {
            let (start, end) = session.range(i);
            session.text[start..end].to_owned()
        })
        .collect()
}

fn modified_ms(meta: &fs::Metadata) -> u64 {
    meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_millis() as u64)
}

#[cfg(test)]
thread_local! {
    /// How many folders `Listings` has read, for a test.
    static LISTED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Each transcript folder as last listed, with its modified time, which an added, removed or renamed file moves: a
/// folder is listed again only then. Listing all of them on every search, as each keystroke in Past sessions makes one,
/// took up to seconds while the disk was busy; a stat of each folder tells that nothing came or went.
#[derive(Default)]
struct Listings(HashMap<PathBuf, (std::time::SystemTime, Vec<PathBuf>)>);

impl Listings {
    fn list(&mut self, folder: &Path) -> Vec<PathBuf> {
        let modified = fs::metadata(folder).and_then(|m| m.modified()).ok();
        if let (Some(at), Some((seen, entries))) = (modified, self.0.get(folder)) {
            if *seen == at {
                return entries.clone();
            }
        }
        #[cfg(test)]
        LISTED.with(|listed| listed.set(listed.get() + 1));
        let entries: Vec<PathBuf> = fs::read_dir(folder).into_iter().flatten().flatten().map(|entry| entry.path()).collect();
        match modified {
            Some(at) => {
                self.0.insert(folder.to_owned(), (at, entries.clone()));
            }
            None => {
                self.0.remove(folder);
            }
        }
        entries
    }
}

/// Brings the index up to date with the transcripts on disk, in every account's folder: new files are parsed, grown
/// ones read from where they ended, and deleted ones dropped. Files are parsed on every core.
fn refresh(index: &mut HashMap<PathBuf, Transcript>, listings: &mut Listings, claude_dirs: &[PathBuf]) {
    let mut kept = HashMap::with_capacity(index.len());
    let mut work = Vec::new();
    let folders: Vec<PathBuf> = claude_dirs.iter().flat_map(|dir| listings.list(&dir.join("projects"))).collect();
    for folder in folders {
        let folder_name = folder.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        for path in listings.list(&folder) {
            if path.extension().is_none_or(|x| x != "jsonl") {
                continue;
            }
            let Ok(meta) = fs::symlink_metadata(&path) else { continue };
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
    if work.is_empty() {
        *index = kept;
        return;
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
        ai_title: crate::opencode::real_title(text("title")),
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

#[derive(Debug, Clone, Serialize)]
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
    /// A name Wings generated, for a session with no `title`. See `ai.rs`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// A Claude session's transcript, `<account>/projects/<folder>/<id>.jsonl`, which says the account it ran under.
    #[serde(skip)]
    pub transcript: Option<PathBuf>,
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
    /// The semantic index's helper is starting, so the next search may find sessions by what they mean.
    pub semantic_loading: bool,
}

/// See `History::excerpt`. The texts are whole messages: the caller cuts them to what its model can take.
#[derive(Debug, Default)]
pub struct Excerpt {
    /// The session's own title, if it has one.
    pub title: Option<String>,
    pub first_prompt: String,
    pub first_reply: Option<String>,
    pub last_prompt: String,
    pub last_reply: Option<String>,
}

impl History {
    /// Ranks the sessions whose words match by BM25, and once it has the semantic index (`set_semantic`) adds the
    /// sessions it ranks best and weighs both halves as repo-docs does. Without a query, the most recent first. It
    /// doesn't wait for the index's helper to start: `semantic_loading` says to ask again.
    pub fn search(&self, claude_dirs: &[PathBuf], query: &str, filter: &Filter, limit: usize) -> Results {
        self.search_with(claude_dirs, query, filter, limit, false)
    }

    /// `search`, waiting for the semantic index's helper when it isn't running: for a caller that can't ask again.
    fn search_with(&self, claude_dirs: &[PathBuf], query: &str, filter: &Filter, limit: usize, wait: bool) -> Results {
        let terms = terms(query);
        let semantic = self.semantic.get().filter(|_| !terms.is_empty());
        // Before the lock: the query's vector can wait for the helper.
        let ranked = semantic.and_then(|s| s.rank(query, wait));
        let semantic_loading = semantic.is_some_and(|s| s.warming());
        let mut index = self.index.lock().unwrap();
        refresh(&mut index, &mut self.listings.lock().unwrap(), claude_dirs);
        let opencode = self.opencode.lock().unwrap();
        let project = filter.project.as_deref().map(|p| p.trim_end_matches('/')).filter(|p| !p.is_empty());
        // Each session with the time it was last active, and its transcript if it has one.
        let mut scope: Vec<(u64, Option<&PathBuf>, &Session)> = index
            .iter()
            .map(|(path, t)| (t.modified, Some(path), &t.session))
            .chain(opencode.sessions.values().map(|c| (c.updated, None, &c.session)))
            .filter(|(_, _, s)| !s.messages.is_empty() && s.by_program != Some(true) && project.is_none_or(|p| s.in_project(p)))
            .collect();
        scope.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.2.id.cmp(&b.2.id)));
        // Resuming a session in another account copies its transcript there, so one session can have two: list the one
        // that moved on last.
        let mut seen = HashSet::new();
        scope.retain(|(_, _, s)| seen.insert(s.id.as_str()));
        let mut branches = Vec::new();
        for (_, _, s) in &scope {
            for branch in s.branches.iter().rev() {
                push_unique(&mut branches, branch.clone());
            }
        }
        branches.truncate(50);
        let shown: Vec<(u64, Option<&PathBuf>, &Session)> = scope
            .into_iter()
            .filter(|(_, _, s)| filter.branch.as_ref().is_none_or(|b| s.branches.contains(b)))
            .filter(|(modified, _, _)| filter.since_ms.is_none_or(|since| *modified >= since))
            .collect();
        let matching: Vec<usize> = (0..shown.len()).filter(|&i| shown[i].2.matches(&terms)).collect();
        let at: HashMap<&str, usize> = shown.iter().enumerate().map(|(i, (_, _, s))| (s.id.as_str(), i)).collect();
        // The sessions that say what the query means, best first, each with the passage that does, read back from its
        // transcript: one whose passage is gone (its transcript was rewritten or removed) is left out.
        let mut similar: Vec<(usize, Snippet)> = Vec::new();
        for (id, hit) in ranked.iter().flatten() {
            if similar.len() == SIMILAR {
                break;
            }
            let Some(&i) = at.get(id.as_str()) else { continue };
            if let Some(snippet) = similar_snippet(shown[i].1, shown[i].2, hit) {
                similar.push((i, snippet));
            }
        }
        let order: Vec<usize> = if terms.is_empty() {
            matching.clone()
        } else {
            // The sessions whose words match and those the index found, by repo-docs' hybrid score: each half divided by
            // its best, weighed 0.2 for the words (BM25) and 0.8 for the meaning; the most recent first between equals.
            // BM25 counts its words in these sessions alone: in all of them, a search took up to 114 ms instead of 21.
            let mut candidates = matching.clone();
            candidates.extend(similar.iter().map(|(i, _)| *i).filter(|i| matching.binary_search(i).is_err()));
            let sessions: Vec<&Session> = candidates.iter().map(|&i| shown[i].2).collect();
            let text = bm25(&sessions, &ranking_terms(&terms));
            let meaning: HashMap<&str, f32> =
                ranked.iter().flatten().map(|(id, hit)| (id.as_str(), hit.best.score.max(0.0))).collect();
            let meaning: Vec<f32> =
                sessions.iter().map(|s| meaning.get(s.id.as_str()).copied().unwrap_or(0.0)).collect();
            let best = |of: &[f32]| of.iter().copied().fold(0.0, f32::max);
            let part = |value: f32, best: f32| if best > 0.0 { value / best } else { 0.0 };
            let (best_text, best_meaning) = (best(&text), best(&meaning));
            let hybrid: Vec<f32> = (0..candidates.len())
                .map(|c| TEXT_WEIGHT * part(text[c], best_text) + MEANING_WEIGHT * part(meaning[c], best_meaning))
                .collect();
            let mut order: Vec<usize> = (0..candidates.len()).collect();
            order.sort_by(|&a, &b| hybrid[b].total_cmp(&hybrid[a]).then(candidates[a].cmp(&candidates[b])));
            order.into_iter().map(|c| candidates[c]).collect()
        };
        let sessions = order
            .iter()
            .take(limit)
            .map(|&i| {
                let (modified, transcript, s) = &shown[i];
                let (matches, mut snippets) =
                    if terms.is_empty() || matching.binary_search(&i).is_err() { (0, Vec::new()) } else { s.snippets(&terms) };
                if snippets.is_empty() {
                    snippets.extend(similar.iter().find(|(j, _)| *j == i).map(|(_, snippet)| snippet.clone()));
                }
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
                    name: None,
                    transcript: transcript.cloned(),
                }
            })
            .collect();
        Results { sessions, total: order.len(), branches, opencode_loading: opencode.loading, semantic_loading }
    }

    /// Fuses the semantic index's matches with each search's (`search`). Only the first call counts.
    pub fn set_semantic(&self, semantic: std::sync::Arc<crate::semantic::Semantic>) {
        let _ = self.semantic.set(semantic);
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

    /// What a session says at its two ends, for the generated names and summaries: its opening prompts and the
    /// latest one, each with the reply that follows the first. A short first prompt ("exit", "hi") says little, so
    /// the next prompts join it up to `OPENING_CHARS`. None when the session isn't loaded or has no prompt yet.
    pub fn excerpt(&self, claude_dirs: &[PathBuf], id: &str) -> Option<Excerpt> {
        let mut index = self.index.lock().unwrap();
        refresh(&mut index, &mut self.listings.lock().unwrap(), claude_dirs);
        let opencode = self.opencode.lock().unwrap();
        let s = index.values().map(|t| &t.session).find(|s| s.id == id).or_else(|| opencode.sessions.get(id).map(|c| &c.session))?;
        let text = |i: usize| {
            let (start, end) = s.range(i);
            s.text[start..end].to_string()
        };
        let reply_after = |i: usize| s.messages.get(i + 1).filter(|m| m.role == Role::Assistant).map(|_| text(i + 1));
        let first = s.messages.iter().position(|m| m.role == Role::User)?;
        let last = s.messages.iter().rposition(|m| m.role == Role::User)?;
        let mut opening = text(first);
        for i in (first + 1..s.messages.len()).filter(|&i| s.messages[i].role == Role::User).take(OPENING_PROMPTS - 1) {
            if opening.trim().chars().count() >= OPENING_CHARS {
                break;
            }
            opening.push('\n');
            opening.push_str(&text(i));
        }
        Some(Excerpt {
            title: s.title().map(String::from),
            first_prompt: opening,
            first_reply: reply_after(first),
            last_prompt: text(last),
            last_reply: reply_after(last),
        })
    }

    /// One session's summary and a page of its messages, for `read_session`. With a `query`, only the messages that
    /// match all of it and the one before and after each.
    pub fn read_session(&self, claude_dirs: &[PathBuf], id: &str, offset: usize, limit: usize, query: Option<&str>) -> Result<Value, String> {
        let mut index = self.index.lock().unwrap();
        refresh(&mut index, &mut self.listings.lock().unwrap(), claude_dirs);
        let opencode = self.opencode.lock().unwrap();
        let s = index
            .values()
            .map(|t| &t.session)
            .find(|s| s.id == id)
            .or_else(|| opencode.sessions.get(id).map(|c| &c.session))
            .ok_or_else(|| format!("No session {id}"))?;
        let terms = query.map(terms).filter(|t| !t.is_empty());
        let hits: Vec<usize> = match &terms {
            None => vec![],
            Some(terms) => (0..s.messages.len()).filter(|&i| {
                let (start, end) = s.range(i);
                terms.iter().all(|t| s.folded[start..end].contains(t.as_str()))
            }).collect(),
        };
        let shown: Vec<usize> = match &terms {
            None => (0..s.messages.len()).collect(),
            Some(_) => {
                let mut around: Vec<usize> = hits.iter().flat_map(|&i| i.saturating_sub(1)..=(i + 1).min(s.messages.len() - 1)).collect();
                around.sort_unstable();
                around.dedup();
                around
            }
        };
        let mut messages = Vec::new();
        let mut used = 0;
        let mut rest = shown.iter().copied().skip_while(|&i| i < offset).peekable();
        while let Some(&i) = rest.peek() {
            let (start, end) = s.range(i);
            let text = clip(&s.text[start..end]);
            if messages.len() == limit || (!messages.is_empty() && used + text.len() > PAGE_BYTES) {
                break;
            }
            used += text.len();
            let m = &s.messages[i];
            messages.push(json!({ "index": i, "role": m.role, "time": m.time, "text": text }));
            rest.next();
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
            "matchingMessages": terms.is_some().then_some(hits.len()),
            "messages": messages,
            "nextOffset": rest.peek(),
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

/// The built-in Wings MCP tools, listed before the plugins' tools.
pub fn mcp_tools() -> Vec<Value> {
    vec![
        json!({
            "name": "search_history",
            "description": "Search past Claude Code and OpenCode sessions on this computer: the prompts, the replies, session titles and the files each Claude Code session edited. Every word or \"quoted phrase\" has to match, ignoring case, and sessions that say what the query means in other words are found too. Returns the best matches first, with snippets. Read one with read_session.",
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
            "description": "Read a past Claude Code or OpenCode session found with search_history: a summary, then the prompts and replies, oldest first. Looking for one thing in it? Pass query to get only the messages that match, each with the one before and after. Long sessions come in pages; pass nextOffset back as offset for the next one.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "session_id": { "type": "string", "description": "The session's id, from search_history." },
                    "query": { "type": "string", "description": "Words or \"quoted phrases\" a message has to hold, all of them, ignoring case. Leave out to read every message." },
                    "offset": { "type": "integer", "minimum": 0, "default": 0, "description": "Index of the first message to return." },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 100, "default": 30, "description": "Most messages to return. A page also ends at about 10k tokens." }
                },
                "required": ["session_id"]
            }
        }),
    ]
}

/// Runs a built-in tool. `None` when `name` isn't one.
pub fn call_tool(history: &History, claude_dirs: &[PathBuf], name: &str, args: &Value) -> Option<Result<String, String>> {
    let number = |key: &str, default: usize, max: usize| args.get(key).and_then(Value::as_u64).map_or(default, |n| (n as usize).min(max));
    Some(match name {
        "search_history" => match args.get("query").and_then(Value::as_str) {
            Some(query) => {
                let filter = Filter { project: args.get("project").and_then(Value::as_str).map(String::from), ..Filter::default() };
                let results = history.search_with(claude_dirs, query, &filter, number("limit", 10, 50).max(1), true);
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
            Some(id) => history.read_session(claude_dirs, id, number("offset", 0, usize::MAX), number("limit", 30, 100).max(1), args.get("query").and_then(Value::as_str)).map(|v| v.to_string()),
            None => Err("read_session needs a session_id".into()),
        },
        _ => return None,
    })
}

#[cfg(test)]
mod tests;
