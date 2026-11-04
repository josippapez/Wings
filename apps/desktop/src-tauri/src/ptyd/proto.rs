//! How Wings and the terminal host talk. Frames on a Unix socket: a u32 LE length (of the kind byte and the body),
//! a kind byte, then the body. Control messages are JSON; output and keystrokes are raw bytes behind a fixed binary
//! header, so a keystroke costs one write and no encoding.
//!
//! A connection opens with the client's `hello` and the host's `welcome`. After that the client sends requests,
//! each with an `id` the host's `reply` carries back, and the host sends events about its panes whenever they
//! happen. Panes are numbered by the host.

use std::{
    io::{self, Read},
    path::PathBuf,
};

use serde::{Deserialize, Serialize};

/// The protocol this build speaks. Bump on any change an older peer couldn't read.
pub const PROTO: u32 = 1;
/// The oldest protocol this build still speaks.
pub const MIN_PROTO: u32 = 1;
/// The largest frame, its kind byte included.
pub const MAX_FRAME: usize = 1 << 20;
/// Replays and blobs go in pieces this big, so each frame stays well under `MAX_FRAME`.
pub const CHUNK: usize = 256 << 10;

const CTRL: u8 = 0;
const OUT: u8 = 1;
const IN: u8 = 2;
const BLOB: u8 = 3;

#[derive(Debug, PartialEq)]
pub enum Frame {
    /// One JSON message: a `Call` from the client, a `HostMsg` from the host.
    Ctrl(Vec<u8>),
    /// A pane's output, host to client. `seq_end` counts every byte the pane has produced, up to the end of `data`.
    Out { pane: u32, seq_end: u64, data: Vec<u8> },
    /// Keystrokes for a pane, client to host.
    In { pane: u32, data: Vec<u8> },
    /// Bytes of request `req`: a spawn's saved output client to host, a read's output host to client.
    Blob { req: u32, data: Vec<u8> },
}

fn frame(kind: u8, header: &[u8], data: &[u8]) -> Vec<u8> {
    let len = 1 + header.len() + data.len();
    debug_assert!(len <= MAX_FRAME, "split it: a {len} byte frame");
    let mut out = Vec::with_capacity(4 + len);
    out.extend_from_slice(&(len as u32).to_le_bytes());
    out.push(kind);
    out.extend_from_slice(header);
    out.extend_from_slice(data);
    out
}

pub fn ctrl(msg: &impl Serialize) -> io::Result<Vec<u8>> {
    let json = serde_json::to_vec(msg)?;
    if json.len() >= MAX_FRAME {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, format!("a {} byte message", json.len())));
    }
    Ok(frame(CTRL, &[], &json))
}

pub fn out(pane: u32, seq_end: u64, data: &[u8]) -> Vec<u8> {
    let mut header = [0u8; 12];
    header[..4].copy_from_slice(&pane.to_le_bytes());
    header[4..].copy_from_slice(&seq_end.to_le_bytes());
    frame(OUT, &header, data)
}

pub fn input(pane: u32, data: &[u8]) -> Vec<u8> {
    frame(IN, &pane.to_le_bytes(), data)
}

pub fn blob(req: u32, data: &[u8]) -> Vec<u8> {
    frame(BLOB, &req.to_le_bytes(), data)
}

/// Reads one frame, or `None` where the stream ends cleanly between frames.
pub fn read(r: &mut impl Read) -> io::Result<Option<Frame>> {
    let mut head = [0u8; 5];
    let mut got = 0;
    while got < head.len() {
        match r.read(&mut head[got..]) {
            Ok(0) if got == 0 => return Ok(None),
            Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
            Ok(n) => got += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    let len = u32::from_le_bytes(head[..4].try_into().unwrap()) as usize;
    if len == 0 || len > MAX_FRAME {
        return Err(invalid(format!("a frame of {len} bytes")));
    }
    let body = len - 1;
    let bytes = |r: &mut dyn Read, n: usize| -> io::Result<Vec<u8>> {
        let mut buf = vec![0u8; n];
        r.read_exact(&mut buf)?;
        Ok(buf)
    };
    let header = |r: &mut dyn Read, n: usize| -> io::Result<Vec<u8>> {
        if body < n {
            return Err(invalid(format!("a frame of kind {} with {body} bytes", head[4])));
        }
        bytes(r, n)
    };
    Ok(Some(match head[4] {
        CTRL => Frame::Ctrl(bytes(r, body)?),
        OUT => {
            let h = header(r, 12)?;
            Frame::Out {
                pane: u32::from_le_bytes(h[..4].try_into().unwrap()),
                seq_end: u64::from_le_bytes(h[4..].try_into().unwrap()),
                data: bytes(r, body - 12)?,
            }
        }
        IN => {
            let h = header(r, 4)?;
            Frame::In { pane: u32::from_le_bytes(h[..].try_into().unwrap()), data: bytes(r, body - 4)? }
        }
        BLOB => {
            let h = header(r, 4)?;
            Frame::Blob { req: u32::from_le_bytes(h[..].try_into().unwrap()), data: bytes(r, body - 4)? }
        }
        kind => return Err(invalid(format!("a frame of unknown kind {kind}"))),
    }))
}

fn invalid(what: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, what)
}

/// A client's message: `hello` first (its `id` is unused), then requests the host replies to by `id`.
#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct Call {
    #[serde(default)]
    pub id: u32,
    #[serde(flatten)]
    pub req: Request,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum Request {
    /// The protocols the client speaks, from `min_proto` to `proto`.
    Hello { proto: u32, min_proto: u32 },
    /// Starts a pane and streams it to this client. The pane's saved output, if any, comes before it as `Blob`
    /// frames with the same `id`. Replies `Spawned`.
    Spawn(Spawn),
    /// Streams a pane to this client: the bytes after `from_seq` if the host still has them, else everything it
    /// kept (`reset`). Replies `Attached`, then the output follows as `Out` frames.
    Attach { pane: u32, from_seq: Option<u64> },
    Resize { pane: u32, cols: u16, rows: u16 },
    /// Ends the pane's process, and forgets the pane once its output has ended. A pane that already exited is
    /// forgotten at once.
    Kill { pane: u32 },
    /// Replies `[PaneInfo]`.
    List,
    /// The output a pane kept, as `Blob` frames before the reply.
    Read { pane: u32 },
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct Spawn {
    pub space_id: String,
    pub cwd: PathBuf,
    pub cols: u16,
    pub rows: u16,
    pub history_id: Option<String>,
    /// The shell's whole environment but its `WINGS_PANE_ID`, which the host adds. The app builds it, so a pane gets
    /// the same one whoever started the host.
    pub env: Vec<(String, String)>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum HostMsg {
    /// The answer to `hello`. With an `error` the host doesn't speak any of the client's protocols, and closes.
    Welcome {
        proto: u32,
        host_version: u32,
        features: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    Reply {
        id: u32,
        #[serde(default)]
        ok: serde_json::Value,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    /// The program changed its title (OSC 0/2) or Codex lifecycle state (OSC 7501).
    Title {
        pane: u32,
        title: String,
        // Optional so an older host can still serve a newer app, using title fallback detection.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        codex_status: Option<crate::program_status::CodexReport>,
    },
    /// The foreground process changed: the shell itself at a prompt, else the command it runs.
    Fg { pane: u32, pid: Option<u32> },
    /// The pane's output ended: its shell exited, or it was killed. The host keeps it until it is killed.
    Exited { pane: u32 },
    /// The client fell too far behind: this pane's queued output was dropped and it no longer streams. Attach
    /// again from the last `seq_end` received.
    Lagged { pane: u32 },
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct Spawned {
    pub pane: u32,
    pub shell_pid: Option<u32>,
    /// Where the pane's output stream starts: the length of the saved output it was given.
    pub seq: u64,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct Attached {
    pub pane: u32,
    /// The output that follows is everything kept, not the bytes after `from_seq`.
    pub reset: bool,
    /// Where the output that follows ends.
    pub seq: u64,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct PaneInfo {
    pub pane: u32,
    pub space_id: String,
    pub cwd: PathBuf,
    pub history_id: Option<String>,
    pub shell_pid: Option<u32>,
    pub fg: Option<u32>,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codex_status: Option<crate::program_status::CodexReport>,
    pub seq: u64,
    pub exited: bool,
}

/// The protocol both sides speak, the newest one, or `None` when they share none.
pub fn negotiate(proto: u32, min_proto: u32) -> Option<u32> {
    let both = proto.min(PROTO);
    (both >= min_proto.max(MIN_PROTO)).then_some(both)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one(bytes: &[u8]) -> io::Result<Option<Frame>> {
        read(&mut &bytes[..])
    }

    #[test]
    fn frames_round_trip() {
        let call = Call { id: 7, req: Request::Attach { pane: 3, from_seq: Some(42) } };
        let mut wire = ctrl(&call).unwrap();
        wire.extend(out(3, 1 << 40, b"\x1b[31mred"));
        wire.extend(input(3, b"ls\r"));
        wire.extend(blob(7, &[0, 255, 10]));
        wire.extend(out(9, 0, b""));
        let mut r = &wire[..];
        let Some(Frame::Ctrl(json)) = read(&mut r).unwrap() else { panic!("not a control frame") };
        assert_eq!(serde_json::from_slice::<Call>(&json).unwrap(), call);
        assert_eq!(String::from_utf8(json).unwrap(), r#"{"id":7,"t":"attach","pane":3,"from_seq":42}"#);
        assert_eq!(read(&mut r).unwrap(), Some(Frame::Out { pane: 3, seq_end: 1 << 40, data: b"\x1b[31mred".to_vec() }));
        assert_eq!(read(&mut r).unwrap(), Some(Frame::In { pane: 3, data: b"ls\r".to_vec() }));
        assert_eq!(read(&mut r).unwrap(), Some(Frame::Blob { req: 7, data: vec![0, 255, 10] }));
        assert_eq!(read(&mut r).unwrap(), Some(Frame::Out { pane: 9, seq_end: 0, data: Vec::new() }));
        assert_eq!(read(&mut r).unwrap(), None, "a clean end between frames");
    }

    #[test]
    fn messages_round_trip_and_ignore_fields_they_do_not_know() {
        let spawn = Call {
            id: 1,
            req: Request::Spawn(Spawn {
                space_id: "s".into(),
                cwd: "/tmp".into(),
                cols: 80,
                rows: 24,
                history_id: None,
                env: vec![("TERM".into(), "xterm-256color".into())],
            }),
        };
        let json = serde_json::to_vec(&spawn).unwrap();
        assert_eq!(serde_json::from_slice::<Call>(&json).unwrap(), spawn);
        let hello: Call = serde_json::from_str(r#"{"t":"hello","proto":2,"min_proto":1,"app":"0.2"}"#).unwrap();
        assert_eq!(hello.req, Request::Hello { proto: 2, min_proto: 1 });
        let reply: HostMsg = serde_json::from_str(r#"{"t":"reply","id":4,"later":true}"#).unwrap();
        assert_eq!(reply, HostMsg::Reply { id: 4, ok: serde_json::Value::Null, error: None });
        let welcome = HostMsg::Welcome { proto: 1, host_version: 3, features: vec![], error: None };
        assert_eq!(serde_json::to_string(&welcome).unwrap(), r#"{"t":"welcome","proto":1,"host_version":3,"features":[]}"#);
    }

    #[test]
    fn a_short_read_is_an_error_but_an_empty_one_is_the_end() {
        let wire = out(1, 5, b"hello");
        for cut in [1, 4, 5, 10, wire.len() - 1] {
            assert_eq!(one(&wire[..cut]).unwrap_err().kind(), io::ErrorKind::UnexpectedEof, "cut at {cut}");
        }
        assert_eq!(one(&[]).unwrap(), None);
    }

    #[test]
    fn an_oversize_or_empty_frame_is_refused_before_its_body_is_read() {
        let mut wire = ((MAX_FRAME + 1) as u32).to_le_bytes().to_vec();
        wire.push(CTRL);
        assert_eq!(one(&wire).unwrap_err().kind(), io::ErrorKind::InvalidData);
        assert_eq!(one(&[0, 0, 0, 0, CTRL]).unwrap_err().kind(), io::ErrorKind::InvalidData);
        let largest = vec![b'x'; MAX_FRAME - 1 - 12];
        assert!(matches!(one(&out(1, 1, &largest)).unwrap(), Some(Frame::Out { .. })), "the largest frame still reads");
        assert!(ctrl(&"x".repeat(MAX_FRAME)).is_err());
    }

    #[test]
    fn an_unknown_kind_or_a_header_cut_short_is_refused() {
        assert_eq!(one(&[3, 0, 0, 0, 9, 1, 2]).unwrap_err().kind(), io::ErrorKind::InvalidData);
        // An `Out` frame needs 12 header bytes.
        assert_eq!(one(&[4, 0, 0, 0, OUT, 1, 0, 0]).unwrap_err().kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn negotiates_the_newest_protocol_both_speak() {
        assert_eq!(negotiate(PROTO, MIN_PROTO), Some(PROTO));
        assert_eq!(negotiate(PROTO + 5, 1), Some(PROTO), "a newer app that still speaks ours");
        assert_eq!(negotiate(PROTO + 5, PROTO + 1), None, "a newer app that dropped ours");
        assert_eq!(negotiate(0, 0), None, "older than the host still speaks");
    }
    #[test]
    fn old_host_titles_have_no_codex_report_and_new_reports_round_trip() {
        let old: HostMsg = serde_json::from_str(r#"{"t":"title","pane":1,"title":"Working"}"#).unwrap();
        assert_eq!(old, HostMsg::Title { pane: 1, title: "Working".into(), codex_status: None });
        let new = HostMsg::Title { pane: 1, title: String::new(), codex_status: Some(crate::program_status::CodexReport { state: crate::program_status::CodexStatus::Permission, generation: 1, pid: Some(10) }) };
        assert_eq!(serde_json::from_slice::<HostMsg>(&serde_json::to_vec(&new).unwrap()).unwrap(), new);
    }

}
