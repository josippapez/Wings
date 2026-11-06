//! Follows the transcripts of the Claude sessions running in panes, so plugins get each entry as Claude writes it
//! (`wings.onTranscript`). A transcript is followed from where it ended when its session was bound to the pane, and
//! only complete lines are read. Claude Code only ever appends to it.
//!
//! This is the desktop's `apps/desktop/src-tauri/src/tail.rs`, ported: the lines go to `wings_plugins::transcript`,
//! which says which plugins may read each entry. It runs on a thread of its own, so reading files never holds up
//! drawing.

use std::{
    collections::HashMap,
    fs,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    thread,
};

use async_channel::{Receiver, Sender};
pub use wings_plugins::transcript::Line;
use wings_plugins::transcript::find_transcript;
use wings_spaces::claude_accounts;

/// Longer lines are skipped, so no single entry sent to a plugin is bigger. In 43,606 lines from 200 real
/// transcripts it kept every assistant entry and skipped 51 (0.12%), all tool results and attachments.
pub const MAX_ENTRY: usize = 256 * 1024;
/// The most read from one transcript per tick, so a burst can't stall the loop. It's more than `MAX_ENTRY`, so a line
/// that fits is always found whole.
const MAX_READ: u64 = 1024 * 1024;
/// A transcript lookup scans every project folder, ~0.9 ms for 327 of them, so a missing one is looked for again only
/// every few ticks.
const LOOKUP_EVERY: u8 = 4;

/// One transcript, read from `offset` on.
struct Tail {
    path: PathBuf,
    offset: u64,
    /// In the middle of a line longer than `MAX_ENTRY`, reading on to its end.
    skipping: bool,
}

impl Tail {
    /// Follows `path` from its current end, so nothing already in it is read.
    fn at_end(path: PathBuf) -> Self {
        let offset = fs::metadata(&path).map_or(0, |meta| meta.len());
        Self {
            path,
            offset,
            skipping: false,
        }
    }

    /// Follows `path` from its start, for a transcript Claude created after the session was bound.
    fn at_start(path: PathBuf) -> Self {
        Self {
            path,
            offset: 0,
            skipping: false,
        }
    }

    /// The complete lines appended since the last read, each at most `MAX_ENTRY` bytes, without their newline. A line
    /// still being written waits for the next read.
    fn read(&mut self) -> Vec<Vec<u8>> {
        let Ok(len) = fs::metadata(&self.path).map(|meta| meta.len()) else {
            return Vec::new();
        };
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
        let read = fs::File::open(&self.path).and_then(|mut file| {
            file.seek(SeekFrom::Start(self.offset))?;
            file.take(MAX_READ.min(len - self.offset))
                .read_to_end(&mut chunk)
        });
        if read.is_err() || chunk.is_empty() {
            return Vec::new();
        }
        let mut start = 0;
        if self.skipping {
            let Some(end) = chunk.iter().position(|&byte| byte == b'\n') else {
                self.offset += chunk.len() as u64;
                return Vec::new();
            };
            start = end + 1;
            self.skipping = false;
        }
        let Some(last) = chunk[start..]
            .iter()
            .rposition(|&byte| byte == b'\n')
            .map(|at| start + at)
        else {
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
        chunk[start..last]
            .split(|&byte| byte == b'\n')
            .filter(|line| !line.is_empty() && line.len() <= MAX_ENTRY)
            .map(<[u8]>::to_vec)
            .collect()
    }
}

struct Bound {
    session_id: String,
    /// `None` until Claude creates the transcript.
    tail: Option<Tail>,
    /// Ticks until the next lookup while `tail` is `None`.
    lookup_in: u8,
}

/// The transcripts of the sessions running in panes, by pane id.
struct Tailer {
    claude_dirs: Vec<PathBuf>,
    panes: HashMap<String, Bound>,
}

impl Tailer {
    fn new(claude_dir: &Path) -> Self {
        Self {
            claude_dirs: vec![claude_dir.to_path_buf()],
            panes: HashMap::new(),
        }
    }

    /// Looks for transcripts in these folders from now on: one per Claude Code account.
    fn set_claude_dirs(&mut self, dirs: Vec<PathBuf>) {
        self.claude_dirs = dirs;
    }

    /// Follows the transcripts of `sessions`, `(pane id, session id)`, and returns what they appended since the last
    /// call. A session new to its pane is followed from the transcript's current end.
    fn poll(&mut self, sessions: &[(String, String)]) -> Vec<Line> {
        self.panes.retain(|pane, bound| {
            sessions
                .iter()
                .any(|(p, session)| p == pane && *session == bound.session_id)
        });
        let mut lines = Vec::new();
        for (pane_id, session_id) in sessions {
            let bound = self.panes.entry(pane_id.clone()).or_insert_with(|| Bound {
                session_id: session_id.clone(),
                tail: find_transcript(&self.claude_dirs, session_id).map(Tail::at_end),
                lookup_in: LOOKUP_EVERY,
            });
            if bound.tail.is_none() {
                bound.lookup_in -= 1;
                if bound.lookup_in > 0 {
                    continue;
                }
                bound.lookup_in = LOOKUP_EVERY;
                // Created since the session was bound, so all of it is new.
                bound.tail = find_transcript(&self.claude_dirs, session_id).map(Tail::at_start);
            }
            let Some(tail) = bound.tail.as_mut() else {
                continue;
            };
            lines.extend(tail.read().into_iter().map(|bytes| Line {
                pane_id: pane_id.clone(),
                session_id: session_id.clone(),
                bytes,
            }));
        }
        lines
    }
}

type Sessions = Vec<(String, String)>;

/// Runs the tailer on its own thread.
pub struct Watcher {
    requests: Sender<Sessions>,
    lines: Receiver<Vec<Line>>,
}

impl Watcher {
    pub fn start(claude_dir: PathBuf) -> Self {
        let (requests, requests_rx) = async_channel::bounded::<Sessions>(1);
        let (lines_tx, lines) = async_channel::bounded(1);
        thread::Builder::new()
            .name("transcript-tail".into())
            .spawn(move || {
                let mut tailer = Tailer::new(&claude_dir);
                while let Ok(sessions) = requests_rx.recv_blocking() {
                    // An account can appear while Wings runs, and each has its own transcripts.
                    tailer.set_claude_dirs(claude_accounts::dirs_with_default(claude_dir.clone()));
                    if lines_tx.send_blocking(tailer.poll(&sessions)).is_err() {
                        break;
                    }
                }
            })
            .expect("Could not start transcript following");
        Self { requests, lines }
    }

    /// What the transcripts of `sessions`, `(pane id, session id)`, appended since the last call.
    pub async fn poll(&self, sessions: Sessions) -> Option<Vec<Line>> {
        self.requests.send(sessions).await.ok()?;
        self.lines.recv().await.ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("wings-native-tail-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("projects/-tmp-proj")).unwrap();
        dir
    }

    fn append(path: &Path, text: &str) {
        fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .unwrap()
            .write_all(text.as_bytes())
            .unwrap();
    }

    fn text(lines: Vec<Vec<u8>>) -> Vec<String> {
        lines
            .into_iter()
            .map(|line| String::from_utf8(line).unwrap())
            .collect()
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
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn follows_the_sessions_bound_to_panes() {
        let dir = scratch("tailer");
        let (a, b) = (
            "3c21b0c5-8113-4f55-9493-fa156c3fa369",
            "9d0e6a8e-1d1f-4c3e-8a43-5f3c1b2a7e10",
        );
        let file = |id: &str| dir.join(format!("projects/-tmp-proj/{id}.jsonl"));
        append(&file(a), "{\"before\":1}\n");
        let mut tailer = Tailer::new(&dir);
        let sessions = vec![
            ("p1".to_string(), a.to_string()),
            ("p2".to_string(), b.to_string()),
        ];
        assert!(tailer.poll(&sessions).is_empty());
        append(&file(a), "{\"after\":1}\n");
        let lines = tailer.poll(&sessions);
        assert_eq!(lines.len(), 1);
        assert_eq!(
            (
                lines[0].pane_id.as_str(),
                lines[0].session_id.as_str(),
                lines[0].bytes.as_slice()
            ),
            ("p1", a, b"{\"after\":1}".as_slice())
        );

        // b had no transcript when it was bound, so once Claude creates one all of it is new.
        append(&file(b), "{\"first\":1}\n");
        let got: Vec<_> = (0..LOOKUP_EVERY)
            .flat_map(|_| tailer.poll(&sessions))
            .map(|line| (line.pane_id, line.bytes))
            .collect();
        assert_eq!(got, [("p2".to_string(), b"{\"first\":1}".to_vec())]);

        // A pane whose session ends is let go; bound again, it starts from the end.
        assert!(tailer.poll(&sessions[1..]).is_empty());
        append(&file(a), "{\"unbound\":1}\n");
        assert!(tailer.poll(&sessions).is_empty());
        append(&file(a), "{\"again\":1}\n");
        assert_eq!(tailer.poll(&sessions)[0].bytes, b"{\"again\":1}");
        assert!(find_transcript(std::slice::from_ref(&dir), "../../etc/passwd").is_none());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn follows_a_session_whose_transcript_is_in_another_accounts_folder() {
        let (default, work) = (scratch("account-default"), scratch("account-work"));
        let id = "3c21b0c5-8113-4f55-9493-fa156c3fa369";
        let file = work.join(format!("projects/-tmp-proj/{id}.jsonl"));
        append(&file, "{\"before\":1}\n");
        let mut tailer = Tailer::new(&default);
        tailer.set_claude_dirs(vec![default.clone(), work.clone()]);
        let sessions = vec![("p1".to_string(), id.to_string())];
        assert!(tailer.poll(&sessions).is_empty());
        append(&file, "{\"after\":1}\n");
        let lines = tailer.poll(&sessions);
        assert_eq!(
            lines
                .iter()
                .map(|line| line.bytes.as_slice())
                .collect::<Vec<_>>(),
            [b"{\"after\":1}".as_slice()]
        );
        let _ = (fs::remove_dir_all(&default), fs::remove_dir_all(&work));
    }
}
