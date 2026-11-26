//! HUD, from a Claude session's transcript.
use super::*;

#[test]
fn hud_draws_a_strip_under_a_claude_session_from_its_transcript() {
    let claude = claude_with_session();
    let rig = Rig::with(load("hud"), false, vec![claude_pane(), pane("2", "/other")]);
    *rig.env.claude.lock().unwrap() = vec![claude.clone()];
    let (on_event, main_errors) = errors();
    rig.host
        .start_main(&module("hud", "main"), on_event)
        .unwrap();

    let seen = rig.env.wait("the strip", |seen| {
        seen.iter()
            .any(|seen| matches!(seen, Seen::Status(_, _, Some(_))))
    });
    let Some(Seen::Status(plugin, pane, Some(status))) = seen
        .iter()
        .find(|seen| matches!(seen, Seen::Status(_, _, Some(_))))
    else {
        unreachable!()
    };
    assert_eq!((plugin.as_str(), pane.as_str()), ("hud", "1"));
    let items: Vec<_> = status
        .items
        .iter()
        .map(|item| (item.label.as_deref().unwrap(), item.value.as_str()))
        .collect();
    // No limits until Claude Code reports them (native hears no status lines), 100k of the 1M window used, and the
    // server that failed to connect.
    assert_eq!(
        items,
        [
            ("5h", "–"),
            ("wk", "–"),
            ("ctx", "10%"),
            ("MCP", "1 failed")
        ]
    );
    assert_eq!(status.items[2].percent, Some(10.0));
    assert_eq!(status.items[2].tone, Some(Tone::Success));
    assert_eq!(status.open.as_deref(), Some("hud"));
    let rows: Vec<_> = status
        .rows
        .iter()
        .map(|row| (row.label.as_str(), row.value.as_str()))
        .collect();
    assert!(
        rows.contains(&("Context", "100k of 1M tokens (10%)")),
        "{rows:?}"
    );
    assert!(rows.contains(&("Failed", "broken: timed out")), "{rows:?}");
    assert!(rows.contains(&("Skills (1)", "simplify")), "{rows:?}");
    assert!(
        main_errors.try_recv().is_err(),
        "main reported {:?}",
        main_errors.try_recv()
    );
    let _ = fs::remove_dir_all(claude);
}

#[test]
fn hud_sidebar_page_shows_what_main_read_and_each_frame_is_checked_against_hud_s_manifest() {
    let claude = claude_with_session();
    let rig = Rig::with(load("hud"), false, vec![claude_pane()]);
    *rig.env.claude.lock().unwrap() = vec![claude.clone()];
    let (on_event, main_errors) = errors();
    rig.host
        .start_main(&module("hud", "main"), on_event)
        .unwrap();

    let (sender, events) = mpsc::channel();
    let page = rig
        .host
        .open_page(&module("hud", "sidebar"), move |event| {
            let _ = sender.send(event);
        })
        .unwrap();
    let mut last = String::new();
    let tree = loop {
        match events
            .recv_timeout(WAIT)
            .unwrap_or_else(|_| panic!("the sidebar last showed {last:?}"))
        {
            Event::Tree(tree) if tree.text().contains("simplify") => break tree,
            Event::Tree(tree) => last = tree.text(),
            Event::Error { message, .. } => panic!("the sidebar failed: {message}"),
        }
    };
    let text = tree.text();
    for shown in [
        "Usage",
        "Context",
        "100k of 1M tokens",
        "broken",
        "timed out",
        "plugin_srv_files",
        "simplify",
    ] {
        assert!(
            text.contains(shown),
            "the sidebar shows {text:?}, not {shown:?}"
        );
    }
    assert!(
        main_errors.try_recv().is_err(),
        "main reported {:?}",
        main_errors.try_recv()
    );
    drop(page);
    let _ = fs::remove_dir_all(claude);
}

#[test]
fn hud_reads_the_transcript_only_for_the_types_its_manifest_names() {
    let claude = claude_with_session();
    let rig = Rig::with(load("hud"), false, vec![claude_pane()]);
    *rig.env.claude.lock().unwrap() = vec![claude.clone()];
    let read = |types: &[&str]| match rig.call(
        "transcript",
        json!({ "sessionId": SESSION, "types": types, "last": null }),
    ) {
        Ok(entries) => json!(entries.as_array().unwrap().len()),
        Err(error) => json!(error),
    };
    let result = json!({
        "attachments": read(&["attachment:model"]),
        "assistant": read(&["assistant"]),
        "system": read(&["system"]),
        "status": rig.allows("statusline", json!({})),
    });
    assert_eq!(result["attachments"], 1);
    assert_eq!(result["assistant"], 1);
    assert_eq!(
        result["system"],
        "hud may not read \"system\" transcript entries"
    );
    assert_eq!(
        result["status"],
        json!({ "rateLimits": [], "sessions": {} })
    );
    let _ = fs::remove_dir_all(claude);
}
