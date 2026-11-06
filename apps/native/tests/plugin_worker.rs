use serde_json::json;
use std::io::Write;
use std::process::{Command, Output, Stdio};
use wings_native::plugin::protocol::{
    self, Action, PLUGIN_ID, Project, Request, Screen, ScreenState, Status, TaskType, VERSION,
};

fn request() -> Request {
    Request {
        version: VERSION,
        plugin_id: PLUGIN_ID.into(),
        project: Project {
            id: "/work/wings".into(),
            name: "Wings".into(),
        },
        storage: vec![],
        action: None,
    }
}

fn run(frame: &[u8]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_wings-plugin-worker"))
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(frame).unwrap();
    child.wait_with_output().unwrap()
}

fn invoke(request: &Request) -> Screen {
    let mut frame = Vec::new();
    protocol::write_frame(&mut frame, &protocol::encode(request).unwrap()).unwrap();
    let output = run(&frame);
    assert!(output.status.success(), "built-in Issues worker failed");
    let payload = protocol::read_frame(&mut output.stdout.as_slice()).unwrap();
    let screen: Screen = serde_json::from_slice(&payload).unwrap();
    screen.validate().unwrap();
    screen
}

#[test]
fn embedded_issues_bundle_runs_without_ui_and_refresh_uses_fresh_snapshot() {
    let mut request = request();
    let screen = invoke(&request);
    assert_eq!(screen.title, "Issues");
    assert_eq!(screen.state, ScreenState::Empty);
    assert_eq!(screen.actions[0].id, Action::Refresh);

    request.storage = vec![(
        "issues:/work/wings".into(),
        json!({
            "v":3,"prefix":"WIN","next":2,"issues":[
                {"key":"WIN-1","title":"<b>Display text, not markup</b>","status":"in_review","type":"bug"}
            ]
        }),
    )];
    request.action = Some(Action::Refresh);
    let before = protocol::encode(&request).unwrap();
    let screen = invoke(&request);
    assert_eq!(screen.state, ScreenState::Ready);
    assert_eq!(screen.rows.len(), 1);
    assert_eq!(screen.rows[0].id, "WIN-1");
    assert_eq!(screen.rows[0].title, "<b>Display text, not markup</b>");
    assert_eq!(screen.rows[0].status, Status::InReview);
    assert_eq!(screen.rows[0].task_type, TaskType::Bug);
    assert_eq!(protocol::encode(&request).unwrap(), before);
    request.storage.clear();
    assert_eq!(invoke(&request).state, ScreenState::Empty);
}

#[test]
fn builtin_storage_errors_are_native_error_screens() {
    let mut request = request();
    request.storage = vec![("issues:/work/wings".into(), json!({"broken":true}))];
    let screen = invoke(&request);
    assert_eq!(screen.state, ScreenState::Error);
    assert!(screen.rows.is_empty());
    assert!(screen.error.is_some());
}

#[test]
fn builtin_preserves_valid_issue_title_whitespace_and_controls() {
    let titles = [
        "First\nsecond",
        "First\tsecond",
        "First\r\nsecond",
        "First\u{000b}second",
        "First\u{000c}second",
        "First\0second",
        "First\u{001b}second",
        "First\u{007f}second",
        "\u{0085}",
    ];
    for title in titles {
        let mut request = request();
        request.storage = vec![(
            "issues:/work/wings".into(),
            json!({"v":3,"prefix":"WIN","next":2,"issues":[
                {"key":"WIN-1","title":title,"status":"todo","type":"task"}
            ]}),
        )];
        let before = protocol::encode(&request).unwrap();
        let mut frame = Vec::new();
        protocol::write_frame(&mut frame, &before).unwrap();
        let output = run(&frame);
        assert!(
            output.status.success(),
            "worker rejected valid title {title:?}"
        );
        let payload = protocol::read_frame(&mut output.stdout.as_slice()).unwrap();
        let screen: Screen = serde_json::from_slice(&payload).unwrap();
        screen.validate().unwrap();
        assert_eq!(screen.state, ScreenState::Ready, "title {title:?}");
        assert_eq!(screen.rows.len(), 1);
        assert_eq!(screen.rows[0].title, title);
        assert_eq!(protocol::encode(&request).unwrap(), before);
    }
}

#[test]
fn worker_refuses_unknown_identity_actions_and_arbitrary_source_fields() {
    let original = serde_json::to_value(request()).unwrap();
    for (field, value) in [
        ("plugin_id", json!("third-party")),
        ("version", json!(2)),
        ("action", json!("delete")),
        ("source", json!("while(true){}")),
    ] {
        let mut request = original.clone();
        request[field] = value;
        let mut frame = Vec::new();
        protocol::write_frame(&mut frame, &protocol::encode(&request).unwrap()).unwrap();
        let output = run(&frame);
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
    }
    let output = Command::new(env!("CARGO_BIN_EXE_wings-plugin-worker"))
        .args(["--source", "arbitrary.js"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
}

#[test]
fn worker_refuses_malformed_oversized_truncated_and_trailing_frames() {
    let mut valid = Vec::new();
    protocol::write_frame(&mut valid, &protocol::encode(&request()).unwrap()).unwrap();
    let mut trailing = valid.clone();
    trailing.push(b'\n');
    let mut malformed = Vec::new();
    protocol::write_frame(&mut malformed, b"not JSON").unwrap();
    for frame in [
        vec![],
        vec![0, 0],
        vec![0; 4],
        ((protocol::MAX_FRAME_BYTES + 1) as u32)
            .to_be_bytes()
            .to_vec(),
        valid[..valid.len() - 1].to_vec(),
        trailing,
        malformed,
    ] {
        let output = run(&frame);
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
    }
}
