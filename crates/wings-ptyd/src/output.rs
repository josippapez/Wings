//! What a pane keeps of its output: the recent bytes, and the title and modes a program set. Both backends keep it
//! the same way, so a terminal that attaches later catches up the same way wherever the pane runs.

use std::sync::Mutex;
use wings_agents::program_status::{codex_report, CodexReport};

/// Output kept per pane for a UI that loads again (a webview reload). xterm keeps 10,000 lines of scrollback, about
/// 1 MB of plain shell output; output heavy in escape codes, like Claude Code's, fills it faster, but the repaint
/// after attaching redraws whatever is on screen. It grows only as output arrives, so a quiet pane holds little.
pub const REPLAY_BYTES: usize = 1 << 20;

/// The last `cap` bytes written, oldest first from `start` once full.
pub struct Ring {
    buf: Vec<u8>,
    start: usize,
    cap: usize,
    /// Older bytes were dropped, so the oldest one kept can be in the middle of an escape sequence.
    wrapped: bool,
}

impl Ring {
    pub fn new(cap: usize) -> Ring {
        Ring { buf: Vec::new(), start: 0, cap, wrapped: false }
    }

    pub fn push(&mut self, mut data: &[u8]) {
        if data.len() >= self.cap {
            self.wrapped |= !self.buf.is_empty() || data.len() > self.cap;
            self.buf.clear();
            self.buf.extend_from_slice(&data[data.len() - self.cap..]);
            self.start = 0;
            return;
        }
        if self.buf.len() < self.cap {
            let take = data.len().min(self.cap - self.buf.len());
            // Doubling as usual, but never past `cap`, which a plain `extend` could nearly double.
            let want = (self.buf.len() + take).max(self.buf.len() * 2).min(self.cap);
            self.buf.reserve_exact(want - self.buf.len());
            self.buf.extend_from_slice(&data[..take]);
            data = &data[take..];
        }
        if data.is_empty() {
            return;
        }
        self.wrapped = true;
        let first = data.len().min(self.cap - self.start);
        self.buf[self.start..self.start + first].copy_from_slice(&data[..first]);
        self.buf[..data.len() - first].copy_from_slice(&data[first..]);
        self.start = (self.start + data.len()) % self.cap;
    }

    /// Everything kept, oldest first. Once wrapped, it starts after the first newline, so a sequence cut in half
    /// doesn't garble the first line.
    pub fn contents(&self) -> Vec<u8> {
        let (newer, older) = self.buf.split_at(self.start);
        let mut out = [older, newer].concat();
        if self.wrapped {
            let line = out.iter().position(|&b| b == b'\n').map_or(0, |i| i + 1);
            out.drain(..line);
        }
        out
    }

    /// The newest `n` bytes exactly as written, for a terminal that already has everything before them. `None` once
    /// some of them were dropped.
    pub fn tail(&self, n: usize) -> Option<Vec<u8>> {
        if n > self.buf.len() {
            return None;
        }
        let (newer, older) = self.buf.split_at(self.start);
        let mut out = Vec::with_capacity(n);
        if n > newer.len() {
            out.extend_from_slice(&older[older.len() - (n - newer.len())..]);
            out.extend_from_slice(newer);
        } else {
            out.extend_from_slice(&newer[newer.len() - n..]);
        }
        Some(out)
    }
}

/// Output saved before a restart, ended so the new shell starts as in a fresh terminal: the modes it left on go off.
pub fn restored(mut history: Vec<u8>) -> Vec<u8> {
    let mut kept = Kept::default();
    kept.push(&mut vte::Parser::new(), &Mutex::new(String::new()), &history);
    history.extend(kept.history_end());
    history
}

/// DEC private modes a program set long ago, like Claude Code's bracketed paste and mouse reporting, that the replay
/// no longer contains once the ring has wrapped. Without them a reattached pane would paste and click differently.
pub const MODES: [u16; 11] = [1, 25, 47, 1000, 1002, 1003, 1004, 1006, 1047, 1049, 2004];

/// A pane's recent output and the modes in force, without a terminal to send it to.
pub struct Kept {
    pub ring: Ring,
    /// Whether each of `MODES` is on.
    pub modes: [bool; MODES.len()],
    /// Latest root Codex status, kept even when no UI is attached.
    pub codex_status: Option<CodexReport>,
    codex_generation: u64,
}

impl Default for Kept {
    fn default() -> Kept {
        // Only the cursor (25) starts on, as in a fresh xterm.
        Kept { ring: Ring::new(REPLAY_BYTES), modes: MODES.map(|m| m == 25), codex_status: None, codex_generation: 0 }
    }
}

impl Kept {
    pub fn push(&mut self, parser: &mut vte::Parser, title: &Mutex<String>, chunk: &[u8]) {
        parser.advance(&mut Tap { title, modes: &mut self.modes, codex_status: &mut self.codex_status, codex_generation: &mut self.codex_generation }, chunk);
        self.ring.push(chunk);
    }

    /// Ends output from before a restart: the modes it left on go off, so the new shell starts as in a fresh terminal.
    pub fn history_end(&self) -> Vec<u8> {
        let mut out = Vec::new();
        for (mode, on) in MODES.iter().zip(self.modes) {
            if on != (*mode == 25) {
                out.extend_from_slice(format!("\x1b[?{mode}{}", if on { 'l' } else { 'h' }).as_bytes());
            }
        }
        out.extend_from_slice(b"\x1b[0m\r\n");
        out
    }

    /// What a fresh terminal needs to catch up: the modes in force if the ring has dropped where they were set,
    /// then the output kept.
    pub fn replay(&self) -> Vec<u8> {
        let mut out = Vec::new();
        if self.ring.wrapped {
            for (mode, on) in MODES.iter().zip(self.modes) {
                if on != (*mode == 25) {
                    out.extend_from_slice(format!("\x1b[?{mode}{}", if on { 'h' } else { 'l' }).as_bytes());
                }
            }
        }
        out.extend(self.ring.contents());
        out
    }
}

/// The longest title kept. vte hands over an OSC of any length, so a program (or a file someone `cat`s) can set a
/// title of megabytes, which the terminal host couldn't fit in a frame to send.
pub const MAX_TITLE: usize = 4096;

/// Watches the byte stream for OSC 0/2 title changes and the modes in `MODES`, without keeping a screen model.
pub struct Tap<'a> {
    pub title: &'a Mutex<String>,
    pub modes: &'a mut [bool; MODES.len()],
    pub codex_status: &'a mut Option<CodexReport>,
    pub codex_generation: &'a mut u64,
}

impl vte::Perform for Tap<'_> {
    fn osc_dispatch(&mut self, params: &[&[u8]], _bell_terminated: bool) {
        if let [kind, rest @ ..] = params {
            if *kind == b"7501" && rest.len() == 1 {
                if let Some(status) = codex_report(rest[0]) {
                    *self.codex_generation = self.codex_generation.wrapping_add(1);
                    *self.codex_status = status.map(|state| CodexReport { state, generation: *self.codex_generation, pid: None });
                }
            }
            if matches!(*kind, b"0" | b"2") && !rest.is_empty() {
                let mut title = String::from_utf8_lossy(&rest.join(&b';')).into_owned();
                title.truncate(title.floor_char_boundary(MAX_TITLE));
                *self.title.lock().unwrap() = title;
            }
        }
    }

    fn csi_dispatch(&mut self, params: &vte::Params, intermediates: &[u8], _ignore: bool, action: char) {
        if intermediates != b"?" || !matches!(action, 'h' | 'l') {
            return;
        }
        for param in params.iter() {
            if let Some(i) = MODES.iter().position(|m| param.first() == Some(m)) {
                self.modes[i] = action == 'h';
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wings_agents::program_status::CodexStatus;

    fn title_after(bytes: &[u8]) -> String {
        let title = Mutex::new(String::new());
        let mut modes = Kept::default().modes;
        vte::Parser::new().advance(&mut Tap { title: &title, modes: &mut modes, codex_status: &mut None, codex_generation: &mut 0 }, bytes);
        title.into_inner().unwrap()
    }

    #[test]
    fn reads_osc_titles_including_semicolons() {
        assert_eq!(title_after(b"\x1b]0;\xe2\x9c\xb3 Fix login\x07"), "✳ Fix login");
        assert_eq!(title_after(b"\x1b]2;a;b\x1b\\"), "a;b");
        assert_eq!(title_after(b"\x1b]7;file:///tmp\x07"), "");
    }

    #[test]
    fn a_title_longer_than_a_frame_is_cut_on_a_character() {
        let long = |text: &str| [&b"\x1b]0;"[..], text.repeat(crate::proto::MAX_FRAME).as_bytes(), b"\x07"].concat();
        assert_eq!(title_after(&long("x")), "x".repeat(MAX_TITLE));
        // Three bytes each: 4096 isn't a multiple of 3.
        assert_eq!(title_after(&long("✳")), "✳".repeat(MAX_TITLE / 3));
    }

    fn ring_after(cap: usize, pushes: &[&[u8]]) -> (Vec<u8>, bool) {
        let mut ring = Ring::new(cap);
        for p in pushes {
            ring.push(p);
        }
        // A Vec of bytes never allocates fewer than 8.
        assert!(ring.buf.capacity() <= cap.max(8), "grew past its cap");
        (ring.contents(), ring.wrapped)
    }

    #[test]
    fn ring_keeps_the_newest_bytes_from_a_line_start() {
        assert_eq!(ring_after(8, &[b"ab", b"cd"]), (b"abcd".to_vec(), false));
        assert_eq!(ring_after(8, &[b"abcdefgh"]), (b"abcdefgh".to_vec(), false));
        // Wrapped: "\x1b[3" lost its start, so the replay begins after the next newline.
        assert_eq!(ring_after(8, &[b"x\x1b[31m", b"r\nok"]), (b"ok".to_vec(), true));
        assert_eq!(ring_after(8, &[b"12345", b"6789\nab"]), (b"ab".to_vec(), true));
        assert_eq!(ring_after(4, &[b"1\n345678"]), (b"5678".to_vec(), true));
        assert_eq!(ring_after(4, &[b"1", b"2", b"3", b"4", b"\n", b"6"]), (b"6".to_vec(), true));
    }

    #[test]
    fn ring_tail_is_the_newest_bytes_as_written() {
        let mut ring = Ring::new(8);
        ring.push(b"abc");
        assert_eq!(ring.tail(2).as_deref(), Some(&b"bc"[..]));
        assert_eq!(ring.tail(0).as_deref(), Some(&b""[..]));
        assert_eq!(ring.tail(4), None);
        ring.push(b"defghij");
        // Holds "cdefghij" across the wrap point.
        assert_eq!(ring.tail(8).as_deref(), Some(&b"cdefghij"[..]));
        assert_eq!(ring.tail(3).as_deref(), Some(&b"hij"[..]));
        assert_eq!(ring.tail(9), None);
    }

    #[test]
    fn a_wrapped_replay_restores_modes_set_before_it() {
        let title = Mutex::new(String::new());
        let mut parser = vte::Parser::new();
        let mut kept = Kept::default();
        kept.push(&mut parser, &title, b"\x1b[?2004h\x1b[?1000;1006h\x1b[?25l\x1b[?1000l");
        assert!(kept.replay().starts_with(b"\x1b[?2004h"), "not wrapped: the modes are in the replay itself");
        kept.push(&mut parser, &title, &vec![b'y'; REPLAY_BYTES]);
        kept.push(&mut parser, &title, b"\nprompt");
        assert_eq!(kept.replay(), b"\x1b[?25l\x1b[?1006h\x1b[?2004hprompt");
    }
    #[test]
    fn codex_status_survives_split_osc_sequences_and_ring_wraps_and_clear() {
        let title = Mutex::new(String::new());
        let mut kept = Kept::default();
        let mut parser = vte::Parser::new();
        kept.push(&mut parser, &title, b"\x1b]7501;state=work");
        assert_eq!(kept.codex_status.map(|s| s.state), None);
        kept.push(&mut parser, &title, b"ing:app=codex\x1b\\");
        assert_eq!(kept.codex_status.map(|s| s.state), Some(CodexStatus::Working));
        kept.push(&mut parser, &title, &vec![b'x'; REPLAY_BYTES + 1]);
        assert_eq!(kept.codex_status.map(|s| s.state), Some(CodexStatus::Working), "status remains after its bytes leave the replay");
        kept.push(&mut parser, &title, b"\x1b]7501;state=done:app=other\x07");
        assert_eq!(kept.codex_status.map(|s| s.state), Some(CodexStatus::Working));
        kept.push(&mut parser, &title, b"\x1b]7501;state=clear\x07");
        assert_eq!(kept.codex_status.map(|s| s.state), None);
    }

    #[cfg(unix)]
    #[test]
    fn codex_osc_reports_drive_working_blocked_done_and_focus_acknowledgement() {
        use std::{fs, path::PathBuf, process::Command};
        use wings_agents::detect::{AgentKind, AgentState, Detector, PaneProbe};
        let tmp = std::env::temp_dir().join(format!("wings-codex-state-{}", std::process::id()));
        fs::create_dir_all(&tmp).unwrap();
        let fake = tmp.join("codex");
        std::os::unix::fs::symlink("/bin/sleep", &fake).unwrap();
        let mut child = Command::new(&fake).arg("30").spawn().unwrap();
        struct Cleanup<'a>(&'a mut std::process::Child, PathBuf);
        impl Drop for Cleanup<'_> {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
                let _ = fs::remove_dir_all(&self.1);
            }
        }
        let pid = child.id();
        let _cleanup = Cleanup(&mut child, tmp.clone());
        let mut detector = Detector::new(&tmp);
        let mut kept = Kept::default();
        let mut parser = vte::Parser::new();
        let title = std::sync::Mutex::new(String::new());
        let mut scan = |report: &[u8], focused| {
            kept.push(&mut parser, &title, report);
            let probe = PaneProbe {
                pane_id: "p".into(),
                space_id: "s".into(),
                shell_pid: Some(pid),
                foreground_pid: Some(pid),
                title: title.lock().unwrap().clone(),
                codex_status: kept.codex_status,
            };
            detector.scan(&[probe], focused).agents.remove(0)
        };
        assert_eq!(scan(b"\x1b]7501;state=working:app=codex\x1b\\", None).state, AgentState::Working);
        for (kind, reason) in [("permission", "permission prompt"), ("question", "question"), ("auth", "sign in")] {
            let a = scan(format!("\x1b]7501;state=blocked:app=codex:kind={kind}\x07").as_bytes(), None);
            assert_eq!((a.kind, a.state, a.waiting_for.as_deref()), (AgentKind::Codex, AgentState::Blocked, Some(reason)));
        }
        assert_eq!(scan(b"\x1b]7501;state=working:app=codex\x07", None).state, AgentState::Working);
        assert_eq!(scan(b"\x1b]7501;state=done:app=codex\x07", None).state, AgentState::Done);
        assert_eq!(scan(b"", Some("p")).state, AgentState::Idle);
        assert_eq!(scan(b"", None).state, AgentState::Idle, "looking away does not re-arm the done report");
        assert_eq!(
            scan(b"\x1b]7501;state=working:app=codex\x07\x1b]7501;state=done:app=codex\x07", None).state,
            AgentState::Done,
            "a fast complete turn between scans still needs acknowledgement"
        );
        assert_eq!(scan(b"", Some("p")).state, AgentState::Idle);
        assert_eq!(scan(b"\x1b]7501;state=idle:app=codex\x07", None).state, AgentState::Idle);
        assert_eq!(scan(b"\x1b]7501;state=error:app=codex\x07", None).waiting_for.as_deref(), Some("Codex reported an error"));
        let a = scan(b"\x1b]7501;state=clear\x07\x1b]2;\xe2\xa0\x8b Fix session labels\x07", None);
        assert_eq!(a.state, AgentState::Working, "older hosts and versions use the default activity title");
        assert_eq!(scan(b"\x1b]2;[ . ] Action Required | Fix labels\x07", None).state, AgentState::Blocked);
        assert_eq!(scan(b"\x1b]7501;state=idle:app=codex\x07", None).state, AgentState::Idle, "explicit status wins over a stale title");
        drop(scan);
        let stale = PaneProbe {
            pane_id: "p".into(),
            space_id: "s".into(),
            shell_pid: Some(pid),
            foreground_pid: Some(pid),
            title: String::new(),
            codex_status: Some(wings_agents::program_status::CodexReport {
                state: wings_agents::program_status::CodexStatus::Working,
                generation: 100,
                pid: Some(pid + 1),
            }),
        };
        assert_eq!(detector.scan(&[stale], None).agents[0].state, AgentState::Idle, "a new agent cannot inherit another process's status");
    }
}
