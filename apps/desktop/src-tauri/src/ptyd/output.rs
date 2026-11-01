//! What a pane keeps of its output: the recent bytes, and the title and modes a program set. Both backends keep it
//! the same way, so a terminal that attaches later catches up the same way wherever the pane runs.

use std::sync::Mutex;

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
}

impl Default for Kept {
    fn default() -> Kept {
        // Only the cursor (25) starts on, as in a fresh xterm.
        Kept { ring: Ring::new(REPLAY_BYTES), modes: MODES.map(|m| m == 25) }
    }
}

impl Kept {
    pub fn push(&mut self, parser: &mut vte::Parser, title: &Mutex<String>, chunk: &[u8]) {
        parser.advance(&mut Tap { title, modes: &mut self.modes }, chunk);
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

/// Watches the byte stream for OSC 0/2 title changes and the modes in `MODES`, without keeping a screen model.
pub struct Tap<'a> {
    pub title: &'a Mutex<String>,
    pub modes: &'a mut [bool; MODES.len()],
}

impl vte::Perform for Tap<'_> {
    fn osc_dispatch(&mut self, params: &[&[u8]], _bell_terminated: bool) {
        if let [kind, rest @ ..] = params {
            if matches!(*kind, b"0" | b"2") && !rest.is_empty() {
                *self.title.lock().unwrap() = String::from_utf8_lossy(&rest.join(&b';')).into_owned();
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

    fn title_after(bytes: &[u8]) -> String {
        let title = Mutex::new(String::new());
        let mut modes = Kept::default().modes;
        vte::Parser::new().advance(&mut Tap { title: &title, modes: &mut modes }, bytes);
        title.into_inner().unwrap()
    }

    #[test]
    fn reads_osc_titles_including_semicolons() {
        assert_eq!(title_after(b"\x1b]0;\xe2\x9c\xb3 Fix login\x07"), "✳ Fix login");
        assert_eq!(title_after(b"\x1b]2;a;b\x1b\\"), "a;b");
        assert_eq!(title_after(b"\x1b]7;file:///tmp\x07"), "");
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
}
