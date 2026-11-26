//! Dev servers and Share, as the app runs them.
use super::*;

#[test]
fn dev_servers_badges_the_pane_with_a_listening_port_and_its_button_opens_the_browser() {
    let rig = Rig::with(
        load("dev-servers"),
        false,
        vec![pane("1", "/work"), pane("2", "/other")],
    );
    *rig.env.ports.lock().unwrap() = Ok(vec![node("1", 5173), node("1", 3000)]);
    let (on_event, main_errors) = errors();
    rig.host
        .start_main(&module("dev-servers", "main"), on_event)
        .unwrap();

    // main looks as soon as it hears the panes: pane 1 gets a badge, and pane 2, which listens to nothing, none.
    let seen = rig.env.wait("the badges", |seen| seen.len() >= 2);
    let badge = seen
        .iter()
        .find_map(|seen| match seen {
            Seen::Badge(plugin, pane, Some(badge)) if plugin == "dev-servers" && pane == "1" => {
                Some(badge.clone())
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("{seen:?}"));
    assert_eq!(badge.label, "localhost:3000 +1");
    assert_eq!(badge.tone, Tone::Neutral);
    assert_eq!(badge.title.as_deref(), Some("Listening on 2 ports"));
    let rows: Vec<_> = badge
        .rows
        .iter()
        .map(|row| (row.label.as_str(), row.value.as_str()))
        .collect();
    assert_eq!(rows, [("node", ":3000"), ("node", ":5173")]);
    let buttons: Vec<_> = badge
        .actions
        .iter()
        .map(|action| (action.id.as_str(), action.primary))
        .collect();
    assert_eq!(
        buttons,
        [
            ("browser:3000", true),
            ("browser:5173", false),
            ("refresh", false)
        ]
    );
    assert!(
        seen.contains(&Seen::Badge("dev-servers".into(), "2".into(), None)),
        "{seen:?}"
    );

    // The button opens the page in the browser, through the permission the manifest declares.
    assert_eq!(action(&rig, "1", "browser:5173"), Ok(()));
    assert_eq!(
        rig.env.seen().last(),
        Some(&Seen::Url("http://localhost:5173/".into()))
    );
    // A click on a badge a few seconds old is for a port that closed.
    *rig.env.ports.lock().unwrap() = Ok(vec![node("1", 3000)]);
    assert_eq!(
        action(&rig, "1", "browser:9999"),
        Err("That port isn't open any more.".into())
    );
    assert_eq!(
        rig.env
            .seen()
            .iter()
            .filter(|seen| matches!(seen, Seen::Url(_)))
            .count(),
        1
    );

    // The badge goes when the port closes, which Refresh looks for.
    *rig.env.ports.lock().unwrap() = Ok(Vec::new());
    assert_eq!(action(&rig, "1", "refresh"), Ok(()));
    let seen = rig.env.wait("the badge to go", |seen| {
        seen.iter()
            .filter(|seen| matches!(seen, Seen::Badge(_, pane, None) if pane == "1"))
            .count()
            == 1
    });
    assert!(!seen.is_empty());
    assert!(
        main_errors.try_recv().is_err(),
        "main reported {:?}",
        main_errors.try_recv()
    );
}

#[test]
fn dev_servers_cannot_reach_what_its_manifest_does_not_grant() {
    // The same main without its `ports` and `openUrl` grants: every call it makes is refused, not run.
    let mut plugin = load("dev-servers");
    plugin.manifest.permissions.ports = false;
    plugin.manifest.permissions.open_url.clear();
    let rig = Rig::with(plugin, false, vec![pane("1", "/work")]);
    *rig.env.ports.lock().unwrap() = Ok(vec![node("1", 5173)]);
    let results = [
        rig.refuses("ports", json!({})),
        rig.refuses("openUrl", json!({ "url": "http://localhost:5173/" })),
    ];
    assert_eq!(
        results,
        [
            "dev-servers may not read listening ports",
            "dev-servers may not open http://localhost:5173/"
        ]
    );
    assert!(rig.env.seen().is_empty());
}

#[test]
fn share_badges_the_pane_of_a_shared_port_and_its_buttons_open_the_link_and_list_what_is_shared() {
    let rig = Rig::with(load("share"), true, vec![pane("1", "/work"), pane("2", "/other")]);
    rig.root.save(
        "share",
        "shares",
        json!([{ "name": "demo", "port": 3000, "url": "https://demo.abc123.opentunnel.xyz", "seen": true }]),
    );
    *rig.env.ports.lock().unwrap() = Ok(vec![node("1", 3000), node("2", 5173)]);
    let (on_event, main_errors) = errors();
    rig.host.start_main(&module("share", "main"), on_event).unwrap();

    // Pane 1 runs the shared program and gets the badge. Pane 2 listens to something nobody shared, and gets none.
    let seen = rig.env.wait("the badges", |seen| seen.len() >= 2);
    let badge = seen
        .iter()
        .find_map(|seen| match seen {
            Seen::Badge(plugin, pane, Some(badge)) if plugin == "share" && pane == "1" => Some(badge.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("{seen:?}"));
    assert_eq!((badge.label.as_str(), badge.tone), ("Shared :3000", Tone::Warning));
    let rows: Vec<_> = badge.rows.iter().map(|row| (row.label.as_str(), row.value.as_str())).collect();
    assert_eq!(rows, [(":3000", "https://demo.abc123.opentunnel.xyz")]);
    let buttons: Vec<_> = badge.actions.iter().map(|action| (action.id.as_str(), action.label.as_str(), action.primary)).collect();
    assert_eq!(buttons, [("open:demo", "Open", true), ("links", "Copy link", false), ("stop:demo", "Stop sharing", false)]);
    assert!(seen.contains(&Seen::Badge("share".into(), "2".into(), None)), "{seen:?}");

    // Open goes to the browser through the `https://` the manifest declares; a link that is gone is refused.
    assert_eq!(action(&rig, "1", "open:demo"), Ok(()));
    assert_eq!(rig.env.seen().last(), Some(&Seen::Url("https://demo.abc123.opentunnel.xyz".into())));
    assert_eq!(action(&rig, "1", "open:other"), Err("That link is already gone.".into()));
    // Copy link opens the modal, which has the button that copies.
    assert_eq!(action(&rig, "1", "links"), Ok(()));
    assert!(rig.env.seen().iter().any(|seen| matches!(seen, Seen::OpenView(open) if open.id == "share")));
    assert!(main_errors.try_recv().is_err());
}

#[test]
fn share_tools_refuse_a_bad_port_or_name_before_anything_runs() {
    let rig = Rig::with(load("share"), true, vec![pane("1", "/work")]);
    let (on_event, main_errors) = errors();
    rig.host.start_main(&module("share", "main"), on_event).unwrap();
    for (tool, input, said) in [
        ("share_port", json!({ "port": 0 }), "port must be at least 1"),
        ("share_port", json!({ "port": 70000 }), "port must be at most 65,535"),
        ("share_port", json!({ "port": "127.0.0.1:3000" }), "port must be a whole number"),
        ("share_port", json!({ "port": 3000, "name": "My App" }), "name must be 1 to 63 lowercase"),
        ("share_port", json!({ "port": 3000, "host": "10.0.0.5" }), "Unknown field: host"),
        ("stop_share", json!({}), "Give name or port"),
    ] {
        let started = (0..40).find_map(|_| {
            let (text, was_error) = tool_result(run_tool(&rig, tool, input.clone()));
            (text != "The plugin has no handler for share_port").then_some((text, was_error))
        });
        let (text, was_error) = started.expect("main never answered");
        assert!(was_error && text.contains(said), "{tool} {input}: {text}");
    }
    assert!(rig.env.seen().iter().all(|seen| !matches!(seen, Seen::OpenView(_))));
    assert!(main_errors.try_recv().is_err());
}
