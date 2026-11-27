//! The small scenes: Changes' pages, tooltips, a prompt rail, the UI gallery and a stopped page.
use super::*;

/// Actual built pages in a fixture host. This does not exercise workspace openView/window routing.
pub(super) fn changes(out: PathBuf) {
    let dev = out.join("fixture-changes");
    let _ = std::fs::remove_dir_all(&dev);
    let root = dev.join("acme-web");
    std::fs::create_dir_all(root.join("src/cart")).unwrap();
    let root = root.canonicalize().unwrap();
    let git = |days_ago: u64, args: &[&str]| {
        let date = format!("{} +0000", now_ms() / 1000 - days_ago * 86_400);
        let done = std::process::Command::new("git")
            .args(["-c", "user.email=ana@example.com", "-c", "user.name=Ana Kovač", "-c", "commit.gpgsign=false"])
            .args(args)
            .current_dir(&root)
            .envs([("GIT_AUTHOR_DATE", &date), ("GIT_COMMITTER_DATE", &date)])
            .output()
            .unwrap();
        assert!(done.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&done.stderr));
    };
    let commit = |days_ago: u64, file: &str, text: &str, subject: &str| {
        std::fs::write(root.join(file), text).unwrap();
        git(days_ago, &["add", file]);
        git(days_ago, &["commit", "-qm", subject]);
    };
    git(0, &["init", "-q", "-b", "main"]);
    commit(30, "README.md", "acme-web\n", "Start the web shop");
    commit(12, "src/cart/total.ts", "export const total = 0;\n", "Add the cart total");
    git(0, &["tag", "v1.0"]);
    git(0, &["checkout", "-q", "-b", "feature/coupons"]);
    commit(3, "src/cart/coupon.ts", "export const coupon = null;\n", "Add coupon validation");
    // Branch names as long as a ticket's, local and remote, on one commit: the graph's pills hold them, and a pill wider
    // than the row used to run past the sidebar's edge.
    git(0, &["branch", "feature/196116-be-prescription-tracker-list-add-the-patient-filter"]);
    git(0, &["update-ref", "refs/remotes/origin/feature/195903-be-prescription-detail-open-the-record-from-the-list", "HEAD"]);
    git(0, &["checkout", "-q", "main"]);
    commit(2, "src/cart/total.ts", "export const total = 1;\n", "Count the first item");
    git(1, &["merge", "-q", "--no-ff", "-m", "Merge feature/coupons", "feature/coupons"]);
    // What isn't committed: a changed file, a deleted one, a new one and a binary.
    std::fs::write(root.join("src/cart/total.ts"), "export const total = 2;\nexport const tax = 0.25;\n").unwrap();
    std::fs::remove_file(root.join("README.md")).unwrap();
    std::fs::write(root.join("src/cart/shipping.ts"), "export const shipping = 5;\n").unwrap();
    std::fs::write(root.join("logo.png"), [0u8, 159, 146, 150, 0, 1, 2]).unwrap();

    let manifest = serde_json::from_str(include_str!("../../../../plugins/changes/wings-plugin.json")).unwrap();
    let storage = out.join("fixture-storage-changes");
    let _ = std::fs::remove_dir_all(&storage);
    let host = PluginHost::with_access(
        PluginConfig { id: "changes".into(), root: storage, scope: "shot".into() },
        Access {
            plugin: wings_plugins::manifest::Plugin { manifest, dir: root.clone(), icon_svg: None },
            builtin: false,
            env: std::sync::Arc::new(FixtureEnv(root.clone())),
        },
    );
    host.publish_panes(vec![PaneEntry {
        pane_id: "1".into(),
        cwd: root.to_string_lossy().into(),
        command: "zsh".into(),
        project: "acme-web".into(),
        project_id: root.to_string_lossy().into(),
        session: None,
        focused: true,
    }]);
    let module = |frame: &str| {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../plugins/changes/dist/native/{frame}.wasm"));
        format!("{}{}", wings_native::page::runtime::wasm::MARKER, path.canonicalize().expect("sh build-rust.sh changes, in plugins/").display())
    };
    host.start_main(&module("main"), |event| {
        if let wings_native::page::runtime::Event::Error { message, .. } = event {
            panic!("{message}");
        }
    })
    .unwrap();
    let mut cx = app();
    for (frame, width, height) in [("sidebar", 1000.0, 760.0), ("sidebar", 640.0, 760.0), ("sidebar", 470.0, 760.0), ("sidebar", 360.0, 760.0), ("sidebar", 250.0, 760.0), ("graph", 1100.0, 420.0)] {
        let source = module(frame);
        let handle = cx
            .open_offscreen_window(size(px(width), px(height)), |_, cx| cx.new(|cx| PageView::in_plugin(&host, &source, cx)))
            .unwrap();
        let window = handle.into();
        handle.read_with(&cx, |view, _| view.set_visible(true)).unwrap();
        let shown = |cx: &mut VisualTestAppContext, expected: &str, name: &str| {
            let deadline = Instant::now() + Duration::from_secs(10);
            while !handle.read_with(cx, |view, _| view.text().contains(expected)).unwrap() {
                assert!(Instant::now() < deadline, "page did not show {expected:?}: {}", handle.read_with(cx, |view, _| view.text()).unwrap());
                wait(cx, window, Duration::from_millis(20));
            }
            wait(cx, window, Duration::from_millis(300));
            save(cx, window, out.join(format!("changes-{name}-{width}.png")));
        };
        if frame == "graph" {
            shown(&mut cx, "5 commits on every branch", "graph");
        } else {
            shown(&mut cx, "shipping.ts", "files");
            assert!(handle.read_with(&cx, |view, _| view.press_chip("History")).unwrap());
            shown(&mut cx, "Add coupon validation", "history");
            assert!(handle.read_with(&cx, |view, _| view.press_chip("Graph")).unwrap());
            // The graph's rows aren't text, so this waits for its git log to be read.
            wait(&mut cx, window, Duration::from_millis(700));
            shown(&mut cx, "acme-web", "sidebar-graph");
            // The pick is kept per repo, and the next width starts on Files again.
            assert!(handle.read_with(&cx, |view, _| view.press_chip("Files")).unwrap());
            wait(&mut cx, window, Duration::from_millis(300));
        }
        let _ = cx.update_window(window, |_, window, _| window.remove_window());
    }
    // A branch of thousands of files against main, as a big feature branch is: the Branch list stops at its cap and
    // ends in "Showing N of M files" with View all.
    git(0, &["add", "-A"]);
    git(0, &["commit", "-qm", "Save the shop's changes"]);
    git(0, &["checkout", "-q", "-b", "feature/196116-be-prescription-tracker-list-add-the-patient-referral-and-prescription-columns"]);
    for (area, folders) in [("BoundedContext", 5), ("DHP", 3), ("Framework", 2)] {
        for folder in 0..folders {
            let dir = root.join(format!("src/{area}/Module{folder}/Application/Handlers"));
            std::fs::create_dir_all(&dir).unwrap();
            for file in 0..(if area == "BoundedContext" { 300 } else { 200 }) {
                std::fs::write(dir.join(format!("Prescription{file:04}Handler.cs")), format!("class Handler{file} {{}}\n")).unwrap();
            }
        }
    }
    git(0, &["add", "-A"]);
    git(0, &["commit", "-qm", "Add the prescription handlers"]);
    for (width, height) in [(1000.0, 760.0), (640.0, 760.0), (250.0, 760.0)] {
        let source = module("sidebar");
        let handle = cx
            .open_offscreen_window(size(px(width), px(height)), |_, cx| cx.new(|cx| PageView::in_plugin(&host, &source, cx)))
            .unwrap();
        let window = handle.into();
        handle.read_with(&cx, |view, _| view.set_visible(true)).unwrap();
        wait(&mut cx, window, Duration::from_millis(500));
        assert!(handle.read_with(&cx, |view, _| view.press_chip("Branch")).unwrap());
        let deadline = Instant::now() + Duration::from_secs(10);
        while !handle.read_with(&cx, |view, _| view.text().contains("Showing")).unwrap() {
            assert!(Instant::now() < deadline, "page did not show the cap: {}", handle.read_with(&cx, |view, _| view.text()).unwrap());
            wait(&mut cx, window, Duration::from_millis(20));
        }
        wait(&mut cx, window, Duration::from_millis(300));
        save(&mut cx, window, out.join(format!("changes-big-branch-{width}-{height}.png")));
        let _ = cx.update_window(window, |_, window, _| window.remove_window());
    }
}

/// A page with a titled Button and a titled Text, each pointed at until its tooltip shows.
pub(super) fn tooltips(out: PathBuf) {
    let words = |id: u64, text: &str| json!([{ "id": id, "type": "#text", "text": text }]);
    let source = fake::page("page", move |_| {
        json!({ "id": 0, "type": "#root", "children": [{ "id": 1, "type": "Page", "children": [
            { "id": 2, "type": "Slot", "props": { "name": "body" }, "children": [
                { "id": 3, "type": "Button", "props": { "variant": "outline", "title": "Opens a terminal as this account running claude auth login" }, "children": words(4, "Sign in") },
                { "id": 5, "type": "Text", "props": { "tone": "muted", "title": "/Users/ada/.claude-work" }, "children": words(6, "ada@example.com") }] }] }] })
    });
    let host = PluginHost::new(PluginConfig { id: "tooltips".into(), root: out.join("fixture-storage-tooltips"), scope: "shot".into() });
    let mut cx = app();
    for width in [1000.0, 640.0] {
        let handle = cx.open_offscreen_window(size(px(width), px(240.0)), |_, cx| cx.new(|cx| PageView::in_plugin(&host, &source, cx))).unwrap();
        let window = handle.into();
        wait(&mut cx, window, Duration::from_millis(800));
        for (name, at) in [("button", point(px(40.0), px(44.0))), ("text", point(px(40.0), px(80.0)))] {
            cx.simulate_mouse_move(window, at, None, Modifiers::default());
            wait(&mut cx, window, Duration::from_millis(100));
            cx.advance_clock(Duration::from_millis(600));
            wait(&mut cx, window, Duration::from_millis(100));
            save(&mut cx, window, out.join(format!("tooltip-{name}-{width}.png")));
            cx.simulate_mouse_move(window, point(px(width - 5.0), px(230.0)), None, Modifiers::default());
            wait(&mut cx, window, Duration::from_millis(100));
        }
        let _ = cx.update_window(window, |_, window, _| window.remove_window());
    }
}

/// A `PromptRail` in a slim split beside some text: 40 prompts with the 13th in view and the 23rd pointed at, so its
/// card shows, and then 300 prompts with the 250th in view, so the rail has scrolled to keep it there.
pub(super) fn prompt_rail(out: PathBuf) {
    use wings_native::page::tree::prompt_rail as rail;
    let host = PluginHost::new(PluginConfig { id: "prompt-rail".into(), root: out.join("fixture-storage-prompt-rail"), scope: "shot".into() });
    let height = 560.0;
    let mut cx = app();
    for (name, count, current, hover) in [("hover", 40_usize, 12_usize, Some(22_usize)), ("scrolled", 300, 249, None)] {
        let source = fake::page("page", move |_| {
            let marks: Vec<_> = (0..count)
                .map(|n| json!({
                    "title": format!("Prompt {} about the checkout flow", n + 1),
                    "preview": "I read the cart module first. The total is computed twice, once when a coupon applies and once on submit, and the two disagree by the tax. I would fix it in one place."
                }))
                .collect();
            json!({ "id": 0, "type": "#root", "props": { "fill": true }, "children": [{ "id": 1, "type": "Split", "props": { "defaultSize": 64 }, "children": [
                { "id": 2, "type": "Slot", "props": { "name": "start" }, "children": [
                    { "id": 3, "type": "PromptRail", "props": { "marks": marks, "current": current, "onSelect": "3:onSelect" } }] },
                { "id": 4, "type": "Slot", "props": { "name": "end" }, "children": [
                    { "id": 5, "type": "Text", "children": [{ "id": 6, "type": "#text", "text": "The conversation sits here, to the right of the rail." }] }] }] }] })
        });
        let handle = cx.open_offscreen_window(size(px(640.0), px(height)), |_, cx| cx.new(|cx| PageView::in_plugin(&host, &source, cx))).unwrap();
        let window = handle.into();
        wait(&mut cx, window, Duration::from_millis(800));
        if let Some(at) = hover {
            // The rail fills the window's height, so its ticks are where the layout puts them.
            let pitch = rail::pitch(count, height);
            let y = rail::row_top(at, pitch) + pitch / 2.0;
            cx.simulate_mouse_move(window, point(px(rail::PAD_X + 4.0), px(y)), None, Modifiers::default());
            wait(&mut cx, window, Duration::from_millis(300));
        }
        save(&mut cx, window, out.join(format!("prompt-rail-{name}.png")));
        let _ = cx.update_window(window, |_, window, _| window.remove_window());
    }
}

/// The pages of `shot_plugins/ui_gallery.json`, which the Rust SDK's `ui` builders draw, at 1000 and 640 points.
pub(super) fn ui_gallery(out: PathBuf) {
    let gallery: serde_json::Value = serde_json::from_str(include_str!("ui_gallery.json")).expect("the gallery is JSON");
    let host = PluginHost::new(PluginConfig { id: "ui-gallery".into(), root: out.join("fixture-storage-ui-gallery"), scope: "shot".into() });
    let mut cx = app();
    for (name, height) in [("widgets", 1500.0), ("panes", 560.0), ("tabs", 300.0)] {
        let tree = gallery[name].clone();
        let source = fake::page("page", move |_| tree.clone());
        for width in [1000.0, 640.0] {
            let handle = cx.open_offscreen_window(size(px(width), px(height)), |_, cx| cx.new(|cx| PageView::in_plugin(&host, &source, cx))).unwrap();
            let window = handle.into();
            wait(&mut cx, window, Duration::from_millis(800));
            save(&mut cx, window, out.join(format!("ui-gallery-{name}-{width}.png")));
            let _ = cx.update_window(window, |_, window, _| window.remove_window());
        }
    }
}

/// A page that stopped, with its error and Reload, at a tab's widths and a popover's.
pub(super) fn stopped(out: PathBuf) {
    // Shows "Up", then stops a moment later as a page that never gives the turn back is stopped.
    struct Hangs;
    impl Guest for Hangs {
        fn start(&mut self, cx: &mut Cx<'_>) {
            cx.render(&json!({ "id": 0, "type": "#root", "children": [{ "id": 1, "type": "Text", "children": [{ "id": 2, "type": "#text", "text": "Up" }] }] }));
            cx.later(100);
        }
        fn timer(&mut self, cx: &mut Cx<'_>, _id: u32) {
            cx.stop("the page ran for too long and was stopped");
        }
    }
    let source = fake::source("page", || Box::new(Hangs));
    let host = PluginHost::new(PluginConfig { id: "stopped".into(), root: out.join("fixture-storage-stopped"), scope: "shot".into() });
    let mut cx = app();
    for width in [1000.0, 640.0, 320.0] {
        let handle = cx.open_offscreen_window(size(px(width), px(240.0)), |_, cx| cx.new(|cx| PageView::in_plugin(&host, &source, cx))).unwrap();
        let window = handle.into();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !handle.read_with(&cx, |view, _| view.stopped()).unwrap() {
            assert!(Instant::now() < deadline, "the page never stopped");
            wait(&mut cx, window, Duration::from_millis(20));
        }
        save(&mut cx, window, out.join(format!("stopped-{width}.png")));
        let _ = cx.update_window(window, |_, window, _| window.remove_window());
    }
}
