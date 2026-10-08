//! Follows the transcripts of the Claude sessions running in panes, so plugins get each entry as Claude
//! writes it. A transcript is followed from where it ended when its session was bound to the pane, and only
//! complete lines are read. Claude Code only ever appends to it.

use std::{
    collections::HashMap,
    fs,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

/// Longer lines are skipped, so no single entry sent to the webview is bigger. In 43,606 lines from 200 real
/// transcripts it kept every assistant entry and skipped 51 (0.12%), all tool results and attachments.
pub const MAX_ENTRY: usize = 256 * 1024;
/// The most read from one transcript per tick, so a burst can't stall the detection loop. It's more than
/// `MAX_ENTRY`, so a line that fits is always found whole.
const MAX_READ: u64 = 1024 * 1024;
/// A transcript lookup scans every project folder, ~0.9 ms for 327 of them, so a missing one is looked for
/// again only every few ticks.
const LOOKUP_EVERY: u8 = 4;

/// One transcript, read from `offset` on.
pub struct Tail {
    path: PathBuf,
    offset: u64,
    /// In the middle of a line longer than `MAX_ENTRY`, reading on to its end.
    skipping: bool,
}

impl Tail {
    /// Follows `path` from its current end, so nothing already in it is read.
    pub fn at_end(path: PathBuf) -> Self {
        let offset = fs::metadata(&path).map_or(0, |m| m.len());
        Self { path, offset, skipping: false }
    }

    /// Follows `path` from its start, for a transcript Claude created after the session was bound.
    pub fn at_start(path: PathBuf) -> Self {
        Self { path, offset: 0, skipping: false }
    }

    /// The complete lines appended since the last read, each at most `MAX_ENTRY` bytes, without their newline.
    /// A line still being written waits for the next read.
    pub fn read(&mut self) -> Vec<Vec<u8>> {
        let Ok(len) = fs::metadata(&self.path).map(|m| m.len()) else { return Vec::new() };
        if len < self.offset {
            // Replaced or truncated, so what's there now wasn't appended: follow it from its new end.
            self.offset = len;
            self.skipping = false;
            return Vec::new();
        }
        if len == self.offset {
            return Vec::new();
        }
        let mut chunk = Vec::new();
        let read = fs::File::open(&self.path).and_then(|mut f| {
            f.seek(SeekFrom::Start(self.offset))?;
            f.take(MAX_READ.min(len - self.offset)).read_to_end(&mut chunk)
        });
        if read.is_err() || chunk.is_empty() {
            return Vec::new();
        }
        let mut start = 0;
        if self.skipping {
            let Some(end) = chunk.iter().position(|&b| b == b'\n') else {
                self.offset += chunk.len() as u64;
                return Vec::new();
            };
            start = end + 1;
            self.skipping = false;
        }
        let Some(last) = chunk[start..].iter().rposition(|&b| b == b'\n').map(|i| start + i) else {
            if start == 0 && chunk.len() as u64 == MAX_READ {
                // No line ends in a full read, so this one is over `MAX_ENTRY`.
                self.offset += chunk.len() as u64;
                self.skipping = true;
            } else {
                // The line from `start` is still being written; wait for the rest of it.
                self.offset += start as u64;
            }
            return Vec::new();
        };
        self.offset += last as u64 + 1;
        chunk[start..last].split(|&b| b == b'\n').filter(|l| !l.is_empty() && l.len() <= MAX_ENTRY).map(<[u8]>::to_vec).collect()
    }
}

/// The transcript of a session, `<claude_dir>/projects/<project>/<session id>.jsonl`.
pub fn find_transcript(claude_dir: &Path, session_id: &str) -> Option<PathBuf> {
    // The id becomes a file name, so only the UUID shape Claude Code uses.
    if session_id.len() != 36 || !session_id.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-') {
        return None;
    }
    let file = format!("{session_id}.jsonl");
    fs::read_dir(claude_dir.join("projects")).ok()?.flatten().map(|d| d.path().join(&file)).find(|p| p.is_file())
}

/// A line one session appended, from the pane it runs in.
pub struct Line {
    pub pane_id: String,
    pub session_id: String,
    pub bytes: Vec<u8>,
}

struct Bound {
    session_id: String,
    /// `None` until Claude creates the transcript.
    tail: Option<Tail>,
    /// Ticks until the next lookup while `tail` is `None`.
    lookup_in: u8,
}

/// The transcripts of the sessions running in panes, by pane id.
pub struct Tailer {
    claude_dir: PathBuf,
    panes: HashMap<String, Bound>,
}

impl Tailer {
    pub fn new(claude_dir: &Path) -> Self {
        Self { claude_dir: claude_dir.to_path_buf(), panes: HashMap::new() }
    }

    /// Follows the transcripts of `sessions`, `(pane id, session id)`, and returns what they appended since the
    /// last call. A session new to its pane is followed from the transcript's current end.
    pub fn poll(&mut self, sessions: &[(String, String)]) -> Vec<Line> {
        self.panes.retain(|pane, bound| sessions.iter().any(|(p, s)| p == pane && *s == bound.session_id));
        let mut lines = Vec::new();
        for (pane_id, session_id) in sessions {
            let bound = self.panes.entry(pane_id.clone()).or_insert_with(|| Bound {
                session_id: session_id.clone(),
                tail: find_transcript(&self.claude_dir, session_id).map(Tail::at_end),
                lookup_in: LOOKUP_EVERY,
            });
            if bound.tail.is_none() {
                bound.lookup_in -= 1;
                if bound.lookup_in > 0 {
                    continue;
                }
                bound.lookup_in = LOOKUP_EVERY;
                // Created since the session was bound, so all of it is new.
                bound.tail = find_transcript(&self.claude_dir, session_id).map(Tail::at_start);
            }
            let Some(tail) = bound.tail.as_mut() else { continue };
            lines.extend(tail.read().into_iter().map(|bytes| Line { pane_id: pane_id.clone(), session_id: session_id.clone(), bytes }));
        }
        lines
    }

    /// Stops following everything, so the next `poll` starts again from the transcripts' current ends.
    pub fn clear(&mut self) {
        self.panes.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("wings-tail-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("projects/-tmp-proj")).unwrap();
        dir
    }

    fn append(path: &Path, text: &str) {
        fs::OpenOptions::new().create(true).append(true).open(path).unwrap().write_all(text.as_bytes()).unwrap();
    }

    fn text(lines: Vec<Vec<u8>>) -> Vec<String> {
        lines.into_iter().map(|l| String::from_utf8(l).unwrap()).collect()
    }

    #[test]
    fn reads_only_complete_lines_appended_since_it_started() {
        let dir = scratch("lines");
        let path = dir.join("projects/-tmp-proj/t.jsonl");
        append(&path, "{\"old\":1}\n");
        let mut tail = Tail::at_end(path.clone());
        assert!(tail.read().is_empty());
        append(&path, "{\"a\":1}\n{\"b\":");
        assert_eq!(text(tail.read()), ["{\"a\":1}"]);
        // The half-written line comes once it ends.
        assert!(tail.read().is_empty());
        append(&path, "2}\n\n{\"c\":3}\n");
        assert_eq!(text(tail.read()), ["{\"b\":2}", "{\"c\":3}"]);
        assert_eq!(tail.offset, fs::metadata(&path).unwrap().len());
        // A truncated file is followed from its new end.
        fs::write(&path, "{\"x\":1}\n").unwrap();
        assert!(tail.read().is_empty());
        append(&path, "{\"y\":2}\n");
        assert_eq!(text(tail.read()), ["{\"y\":2}"]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn skips_lines_over_the_cap_and_keeps_going() {
        let dir = scratch("cap");
        let path = dir.join("projects/-tmp-proj/t.jsonl");
        fs::write(&path, "").unwrap();
        let mut tail = Tail::at_start(path.clone());
        let big = "x".repeat(MAX_ENTRY + 1);
        append(&path, &format!("{{\"a\":1}}\n{big}\n{{\"b\":2}}\n"));
        assert_eq!(text(tail.read()), ["{\"a\":1}", "{\"b\":2}"]);
        // Longer than one read: it takes a few reads to get past it, and nothing of it comes out.
        let huge = "y".repeat(MAX_READ as usize * 2 + 10);
        append(&path, &format!("{huge}\n{{\"c\":3}}\n"));
        let mut got = Vec::new();
        for _ in 0..4 {
            got.extend(text(tail.read()));
        }
        assert_eq!(got, ["{\"c\":3}"]);
        assert_eq!(tail.offset, fs::metadata(&path).unwrap().len());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn follows_the_sessions_bound_to_panes() {
        let dir = scratch("tailer");
        let (a, b) = ("3c21b0c5-8113-4f55-9493-fa156c3fa369", "9d0e6a8e-1d1f-4c3e-8a43-5f3c1b2a7e10");
        let file = |id: &str| dir.join(format!("projects/-tmp-proj/{id}.jsonl"));
        append(&file(a), "{\"before\":1}\n");
        let mut tailer = Tailer::new(&dir);
        let sessions = vec![("p1".to_string(), a.to_string()), ("p2".to_string(), b.to_string())];
        assert!(tailer.poll(&sessions).is_empty());
        append(&file(a), "{\"after\":1}\n");
        let lines = tailer.poll(&sessions);
        assert_eq!(lines.len(), 1);
        assert_eq!((lines[0].pane_id.as_str(), lines[0].session_id.as_str(), lines[0].bytes.as_slice()), ("p1", a, b"{\"after\":1}".as_slice()));

        // b had no transcript when it was bound, so once Claude creates one all of it is new.
        append(&file(b), "{\"first\":1}\n");
        let got: Vec<_> = (0..LOOKUP_EVERY).flat_map(|_| tailer.poll(&sessions)).map(|l| (l.pane_id, l.bytes)).collect();
        assert_eq!(got, [("p2".to_string(), b"{\"first\":1}".to_vec())]);

        // A pane whose session ends is let go; bound again, it starts from the end.
        assert!(tailer.poll(&sessions[1..]).is_empty());
        append(&file(a), "{\"unbound\":1}\n");
        assert!(tailer.poll(&sessions).is_empty());
        append(&file(a), "{\"again\":1}\n");
        assert_eq!(tailer.poll(&sessions)[0].bytes, b"{\"again\":1}");

        // Cleared, it starts from the end too.
        tailer.clear();
        append(&file(a), "{\"while-cleared\":1}\n");
        assert!(tailer.poll(&sessions).is_empty());
        assert!(find_transcript(&dir, "../../etc/passwd").is_none());
        let _ = fs::remove_dir_all(&dir);
    }

    /// The whole Rust path: plugins installed on disk, a session's transcript in a scratch Claude folder, and
    /// what Claude appends to it turned into events for only the plugins allowed each entry's type.
    #[test]
    fn appended_entries_reach_only_the_plugins_allowed_their_type() {
        let dir = scratch("live");
        let id = "5e1f2a3b-4c5d-4e6f-8a9b-0c1d2e3f4a5b";
        let path = dir.join(format!("projects/-tmp-proj/{id}.jsonl"));
        append(&path, "{\"type\":\"assistant\",\"marker\":\"backlog\"}\n");
        let install = |plugin_id: &str, types: &[&str]| {
            let folder = dir.join("plugins").join(plugin_id);
            fs::create_dir_all(&folder).unwrap();
            fs::write(folder.join("main.js"), "").unwrap();
            let manifest = serde_json::json!({ "id": plugin_id, "name": plugin_id, "version": "1", "api": 1, "main": "main.js", "permissions": { "transcript": types } });
            fs::write(folder.join("wings-plugin.json"), manifest.to_string()).unwrap();
            crate::plugins::load(&folder).unwrap()
        };
        let running = [install("timeline", &["assistant", "cost-state"]), install("prs", &["pr-link"])];
        let sessions = [("p1".to_string(), id.to_string())];
        let mut tailer = Tailer::new(&dir);
        let events = |tailer: &mut Tailer| -> Vec<(String, Vec<String>)> {
            let lines = tailer.poll(&sessions);
            lines
                .into_iter()
                .filter_map(|line| crate::plugins::transcript_event(&running, line))
                .map(|e| {
                    assert_eq!((e.pane_id.as_str(), e.session_id.as_str()), ("p1", id));
                    (e.entry["marker"].as_str().unwrap().to_string(), e.plugins)
                })
                .collect()
        };
        assert!(events(&mut tailer).is_empty(), "what was there before isn't sent");

        let big = "x".repeat(MAX_ENTRY);
        append(
            &path,
            &[
                r#"{"type":"user","marker":"u1"}"#.to_string(),
                r#"{"type":"assistant","marker":"a1","message":{"content":[{"type":"tool_use","name":"Edit"}]}}"#.into(),
                r#"{"type":"attachment","marker":"x1","content":[{"type":"assistant"}]}"#.into(),
                "not json".into(),
                r#"{"type":"cost-state","marker":"c1","costUSD":0.42}"#.into(),
                format!(r#"{{"type":"assistant","marker":"too-big","pad":"{big}"}}"#),
                r#"{"type":"pr-link","marker":"pr1","prNumber":7}"#.into(),
                r#"{"type":"assistant","marker":"half"#.into(),
            ]
            .join("\n"),
        );
        let owners = |ids: &[&str]| ids.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            events(&mut tailer),
            [("a1".to_string(), owners(&["timeline"])), ("c1".into(), owners(&["timeline"])), ("pr1".into(), owners(&["prs"]))]
        );
        // The entry Claude was still writing arrives once it ends.
        append(&path, "\"}\n");
        assert_eq!(events(&mut tailer), [("half".to_string(), owners(&["timeline"]))]);
        let _ = fs::remove_dir_all(&dir);
    }
}
