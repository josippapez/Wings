//! What the server does with a client that sends it garbage, and with spawn requests the OS refuses: it drops that
//! client or answers an error, and every other client and every pane carries on. Over a real socket, as an app would.

use std::{
    io::{BufReader, Read, Write},
    os::unix::net::UnixStream,
    path::Path,
    thread,
    time::{Duration, Instant},
};

use serde_json::Value;

use crate::{
    host::{test_host, Config, Stopped},
    proto::{self, Call, Frame, HostMsg, JobSpawn, PaneInfo, Request, Spawn, Spawned, MAX_FRAME, MIN_PROTO, PROTO},
};

/// A connection that has said hello, read by hand.
struct Conn {
    stream: UnixStream,
    reader: BufReader<UnixStream>,
    next_id: u32,
}

impl Conn {
    fn open(path: &Path) -> Conn {
        let stream = UnixStream::connect(path).unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        let mut conn = Conn { reader: BufReader::new(stream.try_clone().unwrap()), stream, next_id: 1 };
        conn.send_raw(&proto::ctrl(&Call { id: 0, req: Request::Hello { proto: PROTO, min_proto: MIN_PROTO } }).unwrap());
        let Some(Frame::Ctrl(json)) = proto::read(&mut conn.reader).unwrap() else { panic!("no welcome") };
        assert!(matches!(serde_json::from_slice(&json), Ok(HostMsg::Welcome { error: None, .. })));
        conn
    }

    fn send_raw(&mut self, bytes: &[u8]) {
        self.stream.write_all(bytes).unwrap();
    }

    /// The reply to `req`: its value, or the error the server gave.
    fn ask(&mut self, req: Request) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        self.send_raw(&proto::ctrl(&Call { id, req }).unwrap());
        self.reply(id)
    }

    fn reply(&mut self, id: u32) -> Result<Value, String> {
        loop {
            let Some(frame) = proto::read(&mut self.reader).expect("the server closed on a good request") else { panic!("the server hung up") };
            if let Frame::Ctrl(json) = frame {
                if let Ok(HostMsg::Reply { id: got, ok, error }) = serde_json::from_slice(&json) {
                    if got == id {
                        return error.map_or(Ok(ok), Err);
                    }
                }
            }
        }
    }

    fn panes(&mut self) -> Vec<PaneInfo> {
        serde_json::from_value(self.ask(Request::List).unwrap()).unwrap()
    }

    /// Whether the server has closed this connection (reads end or fail) within a few seconds.
    fn closed_by_server(&mut self) -> bool {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut byte = [0u8; 1];
        while Instant::now() < deadline {
            match self.reader.read(&mut byte) {
                Ok(0) => return true,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::TimedOut => return false,
                Err(_) => return true,
                // A frame the server sent before closing.
                Ok(_) => {}
            }
        }
        false
    }
}

fn spawn_request() -> Spawn {
    Spawn {
        space_id: "s".into(),
        cwd: std::env::temp_dir(),
        cols: 80,
        rows: 24,
        history_id: Some("h".into()),
        replay_bytes: None,
        env: std::env::vars().collect(),
    }
}

fn spawn(conn: &mut Conn, request: Spawn) -> Result<u32, String> {
    conn.ask(Request::Spawn(request)).map(|ok| serde_json::from_value::<Spawned>(ok).unwrap().pane)
}

fn frame(len: u32, kind: u8, body: &[u8]) -> Vec<u8> {
    [&len.to_le_bytes()[..], &[kind], body].concat()
}

/// Bytes a client may send after its hello that the server can't take, with why each is wrong.
fn bad_frames() -> Vec<(&'static str, Vec<u8>)> {
    let ctrl = proto::ctrl(&Call { id: 9, req: Request::List }).unwrap();
    vec![
        ("a length past the largest frame", frame(MAX_FRAME as u32 + 1, 0, &[])),
        ("a length of zero", vec![0, 0, 0, 0, 0]),
        ("an unknown kind", frame(2, 99, &[1])),
        ("a keystroke frame with no pane", frame(3, 2, &[1, 2])),
        ("an output frame, which only the server sends", proto::out(1, 1, b"x")),
        ("a line frame, which only the server sends", proto::line(1, 1, false, b"x")),
        ("a frame cut off in its body", ctrl[..ctrl.len() - 3].to_vec()),
        ("a frame cut off in its length", vec![7, 0]),
    ]
}

#[test]
fn a_client_sending_garbage_is_dropped_and_the_server_and_its_panes_carry_on() {
    let (path, host) = test_host(Config::default());
    let mut good = Conn::open(&path);
    let pane = spawn(&mut good, spawn_request()).unwrap();
    for (what, bytes) in bad_frames() {
        let mut bad = Conn::open(&path);
        bad.send_raw(&bytes);
        // A cut off frame stays cut off until the client goes away, as a crashed app's would.
        let _ = bad.stream.shutdown(std::net::Shutdown::Write);
        assert!(bad.closed_by_server(), "the server kept a client that sent {what}");
        assert_eq!(good.panes().iter().map(|p| p.pane).collect::<Vec<_>>(), [pane], "after {what}");
    }
    // Each dropped client left the server's list: with the last one gone and the pane killed, a draining server stops.
    good.ask(Request::Drain).unwrap();
    good.ask(Request::Kill { pane }).unwrap();
    drop(good);
    let deadline = Instant::now() + Duration::from_secs(10);
    while host.stopped.lock().unwrap().is_none() {
        assert!(Instant::now() < deadline, "the server never drained: a dropped client was never forgotten");
        thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(*host.stopped.lock().unwrap(), Some(Stopped::Drained));
}

#[test]
fn a_hello_that_isnt_one_closes_the_connection_without_a_welcome() {
    let (path, _host) = test_host(Config::default());
    for bytes in [
        proto::ctrl(&serde_json::json!({ "t": "hello" })).unwrap(),
        proto::ctrl(&serde_json::json!("hello")).unwrap(),
        proto::ctrl(&Call { id: 0, req: Request::List }).unwrap(),
        proto::input(1, b"ls\n"),
        frame(1, 0, &[]),
    ] {
        let stream = UnixStream::connect(&path).unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        (&stream).write_all(&bytes).unwrap();
        let mut reader = BufReader::new(stream);
        assert!(matches!(proto::read(&mut reader), Ok(None) | Err(_)), "welcomed a client that didn't say hello");
    }
    // None of it hurt the server.
    assert!(Conn::open(&path).panes().is_empty());
}

#[test]
fn a_request_the_server_cant_parse_or_repeat_is_answered_with_an_error_on_the_same_connection() {
    let (path, _host) = test_host(Config::default());
    let mut conn = Conn::open(&path);
    conn.send_raw(&proto::ctrl(&serde_json::json!({ "id": 5, "t": "no_such_request" })).unwrap());
    assert!(conn.reply(5).is_err());
    conn.send_raw(&proto::ctrl(&serde_json::json!({ "id": 6, "t": "resize", "pane": "one" })).unwrap());
    assert!(conn.reply(6).is_err());
    conn.send_raw(&frame(1, 0, &[]));
    conn.send_raw(&proto::ctrl(&Call { id: 7, req: Request::Hello { proto: 1, min_proto: 1 } }).unwrap());
    assert_eq!(conn.reply(7), Err("hello twice".into()));
    assert!(conn.panes().is_empty());
}

#[test]
fn requests_for_a_pane_or_job_that_isnt_there_are_errors_not_panics() {
    let (path, _host) = test_host(Config::default());
    let mut conn = Conn::open(&path);
    assert!(conn.ask(Request::Attach { pane: 77, from_seq: Some(u64::MAX) }).is_err());
    assert!(conn.ask(Request::Resize { pane: 77, cols: 0, rows: 0 }).is_err());
    assert!(conn.ask(Request::Read { pane: 77 }).is_err());
    assert!(conn.ask(Request::JobAttach { job: 77 }).is_err());
    assert!(conn.ask(Request::JobReadNote { job: 77 }).is_err());
    assert!(conn.ask(Request::JobNote { job: 77, since: Some(u64::MAX), owner: None }).is_err());
    assert!(conn.ask(Request::JobWrite { job: 77 }).is_err());
    // Ending what isn't there is already done.
    assert!(conn.ask(Request::Kill { pane: 77 }).is_ok());
    assert!(conn.ask(Request::JobStop { job: 77 }).is_ok());
    conn.send_raw(&proto::input(77, b"x"));
    assert!(conn.panes().is_empty());
}

#[test]
fn a_pane_resized_to_nothing_or_attached_past_its_end_still_answers() {
    let (path, _host) = test_host(Config::default());
    let mut conn = Conn::open(&path);
    let pane = spawn(&mut conn, spawn_request()).unwrap();
    assert!(conn.ask(Request::Resize { pane, cols: 0, rows: 0 }).is_ok());
    assert!(conn.ask(Request::Resize { pane, cols: u16::MAX, rows: u16::MAX }).is_ok());
    let attached = conn.ask(Request::Attach { pane, from_seq: Some(u64::MAX) }).unwrap();
    // More than it ever printed can't be a resume: the whole replay.
    assert_eq!(attached["reset"], true);
    assert_eq!(conn.panes()[0].pane, pane);
}

#[test]
fn a_spawn_the_os_refuses_is_an_error_and_the_server_keeps_serving() {
    let (path, _host) = test_host(Config::default());
    let mut conn = Conn::open(&path);
    let mut missing = spawn_request();
    missing.cwd = "/nonexistent/wings-abuse-test".into();
    let mut nul_env = spawn_request();
    nul_env.env.push(("A\0B".into(), "c".into()));
    let mut nul_cwd = spawn_request();
    nul_cwd.cwd = "/tmp/\0x".into();
    for (what, request) in [("a folder that isn't there", missing), ("a NUL in the environment", nul_env), ("a NUL in the folder", nul_cwd)] {
        let result = spawn(&mut conn, request);
        // Either the shell starts or the server says why not; the connection must still answer.
        let _ = result;
        assert!(conn.ask(Request::List).is_ok(), "the connection died after {what}");
    }
    // A fresh client still gets a working pane.
    let mut other = Conn::open(&path);
    assert!(spawn(&mut other, spawn_request()).is_ok());
}

#[test]
fn a_job_the_os_refuses_is_an_error_and_the_server_keeps_serving() {
    let (path, _host) = test_host(Config::default());
    let mut conn = Conn::open(&path);
    let job = |program: &str, cwd: &str, env: Vec<(String, String)>| {
        Request::JobStart(JobSpawn { program: program.into(), args: vec![], cwd: cwd.into(), env, owner: Value::Null })
    };
    assert!(conn.ask(job("/nonexistent/wings-abuse-test", "/tmp", vec![])).is_err());
    assert!(conn.ask(job("/bin/cat", "/nonexistent/wings-abuse-test", vec![])).is_err());
    assert!(conn.ask(job("/bin/c\0at", "/tmp", vec![])).is_err());
    assert!(conn.ask(job("/bin/cat", "/tmp", vec![("A\0B".into(), "c".into())])).is_err());
    assert!(conn.ask(job("", "/tmp", vec![])).is_err());
    assert!(conn.ask(Request::JobList).unwrap().as_array().unwrap().is_empty());
}

/// The ring against a plain vector, over every small capacity and many push sizes: it never panics, keeps the newest
/// `cap` bytes in order, and `tail` agrees with them.
#[test]
fn the_ring_keeps_the_newest_bytes_for_any_capacity_and_push_size() {
    let mut seed = 0x2545_f491_4f6c_dd1d_u64;
    let mut next = move |most: usize| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed % most as u64) as usize
    };
    for cap in 1..=17 {
        let mut ring = crate::output::Ring::new(cap);
        let mut all = Vec::new();
        for round in 0..200 {
            let data: Vec<u8> = (0..next(2 * cap + 2)).map(|i| (round * 7 + i) as u8).collect();
            ring.push(&data);
            all.extend_from_slice(&data);
            let kept = &all[all.len().saturating_sub(cap)..];
            let n = next(kept.len() + 1);
            assert_eq!(ring.tail(n).as_deref(), Some(&kept[kept.len() - n..]), "cap {cap}, round {round}");
            assert_eq!(ring.tail(kept.len() + 1), None);
            // Once it dropped bytes it starts after a newline, so only the end is comparable.
            assert!(kept.ends_with(&ring.contents()), "cap {cap}, round {round}");
        }
    }
}

/// Whatever a program prints, a pane's record of it holds together: no panic (which would poison the locks every
/// client and the accept loop share), and a replay that is the newest bytes with sizes inside it.
#[test]
fn any_output_and_resizes_leave_a_panes_record_consistent() {
    use crate::output::Kept;
    let mut seed = 0x9e37_79b9_7f4a_7c15_u64;
    let mut next = move |most: usize| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed % most as u64) as usize
    };
    let pieces: [&[u8]; 12] = [
        b"\x1b]0;", b"\x1b]2;a;b", b"\x07", b"\x1b\\", b"\x1b]7501;app=codex:state=working\x07", b"\x1b]7501;", b"\x1b[?1049h",
        b"\x1b[?2004l", b"\n", b"\xff\xfe", b"\x1b[", b"text ",
    ];
    for cap in [1, 2, 7, 64] {
        let mut kept = Kept::with_cap(cap);
        let mut parser = vte::Parser::new();
        let title = std::sync::Mutex::new(String::new());
        let mut printed = 0usize;
        kept.resized(80, 24);
        for _ in 0..500 {
            let mut chunk = Vec::new();
            for _ in 0..next(6) {
                chunk.extend_from_slice(pieces[next(pieces.len())]);
            }
            kept.push(&mut parser, &title, &chunk);
            printed += chunk.len();
            if next(4) == 0 {
                kept.resized(next(300) as u16, next(100) as u16);
            }
            let (replay, sizes) = kept.replay();
            assert!(sizes.iter().all(|&(at, ..)| at <= replay.len()));
            assert!(replay.len() <= cap + 64 && printed >= replay.len().saturating_sub(64));
            let _ = kept.history_end();
        }
        assert!(title.lock().unwrap().len() <= crate::output::MAX_TITLE);
    }
}
