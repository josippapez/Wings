//! Managing plugins from the window, as the desktop's Plugins section does, and the agent features that press Enter or
//! ask before a setup command is saved: real packages, real plugin mains, real shells, in temp folders only.

use std::{
    io::Write,
    sync::{Arc, Mutex},
    time::Duration,
};

use gpui::{TestApp, TestAppWindow};
use serde_json::{Value, json};
use wings_plugins::store::{Grant, Source};

use super::{
    Workspace,
    plugin_tests::{open_plugins, until},
};
use crate::installed::fixture::Data;

/// A `.wings-plugin` of plugin `id` whose native main saves `version` under `started`, asking to run `exec`.
fn package(id: &str, version: &str, exec: &[&str]) -> Vec<u8> {
    let manifest = json!({ "id": id, "name": format!("Plugin {id}"), "version": version, "api": 1,
        "main": "dist/main.js", "permissions": { "exec": exec } });
    let main = format!("wings.storage.set('started', '{version}');");
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, body) in [
        ("wings-plugin.json", manifest.to_string()),
        ("dist/main.js", String::new()),
        ("dist/native/main.js", main),
    ] {
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(body.as_bytes()).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

fn github() -> Source {
    Source::Github {
        repo: "owner/demo".into(),
        folder: None,
    }
}

#[derive(Default)]
struct Updates {
    latest: Mutex<String>,
    catalog: Mutex<Vec<wings_plugins::store::Available>>,
    failed: std::sync::atomic::AtomicBool,
    calls: std::sync::atomic::AtomicUsize,
    queried: Mutex<Vec<String>>,
}

impl super::plugin_manage::UpdateSource for Updates {
    fn latest(&self, repo: &str, _: Option<&str>) -> Result<Option<String>, String> {
        self.queried.lock().unwrap().push(repo.into());
        if self.failed.load(std::sync::atomic::Ordering::SeqCst) {
            return Err("offline".into());
        }
        Ok(Some(self.latest.lock().unwrap().clone()))
    }

    fn available(&self) -> Result<Vec<wings_plugins::store::Available>, String> {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if self.failed.load(std::sync::atomic::Ordering::SeqCst) {
            return Err("offline".into());
        }
        Ok(self.catalog.lock().unwrap().clone())
    }
}

#[test]
fn plugin_update_announcements_replace_the_previous_version() {
    let data = Data::temp();
    let (app, mut window) = open_plugins(&data, &Arc::default(), |_| {});
    install(&mut window, package("demo", "1.0.0", &[]), github(), None).unwrap();
    let source = Arc::new(Updates::default());
    window.update(|w, _, _| w.manage.update_source = Some(source.clone()));
    for version in ["1.1.0", "1.2.0"] {
        *source.latest.lock().unwrap() = version.into();
        window.update(|w, _, cx| w.check_plugin_updates(false, cx));
        until(&app, &mut window, "check never completed", |w| {
            !w.manage.checking
        });
    }
    assert_eq!(window.read(|w, _| w.manage.announcements.len()), 1);
    assert!(window.read(|w, _| w.manage.announcements[0].title.contains("1.2.0")));
}

fn offered(id: &str) -> wings_plugins::store::Available {
    wings_plugins::store::Available {
        id: id.into(),
        name: format!("Plugin {id}"),
        description: "Official plugin".into(),
        version: "1.0.0".into(),
        link: format!("https://github.com/owner/{id}"),
    }
}

/// Answers only once `expected` lookups are in flight together, or gives up after a while: asked one by one, every
/// lookup fails.
struct Together {
    expected: usize,
    waiting: Mutex<usize>,
    arrived: std::sync::Condvar,
}

impl Together {
    fn wait(&self) -> Result<(), String> {
        let mut waiting = self.waiting.lock().unwrap();
        *waiting += 1;
        self.arrived.notify_all();
        let (waiting, timeout) = self
            .arrived
            .wait_timeout_while(waiting, Duration::from_secs(2), |waiting| {
                *waiting < self.expected
            })
            .unwrap();
        drop(waiting);
        if timeout.timed_out() {
            return Err("asked alone".into());
        }
        Ok(())
    }
}

impl super::plugin_manage::UpdateSource for Together {
    fn latest(&self, _: &str, _: Option<&str>) -> Result<Option<String>, String> {
        self.wait()?;
        Ok(Some("2.0.0".into()))
    }

    fn available(&self) -> Result<Vec<wings_plugins::store::Available>, String> {
        self.wait()?;
        Ok(vec![offered("new")])
    }
}

/// Like the desktop's `Promise.all`, each plugin's latest version and the catalogue are asked for at once.
#[test]
fn plugin_update_lookups_and_the_catalogue_are_asked_for_at_once() {
    let data = Data::temp();
    let (app, mut window) = open_plugins(&data, &Arc::default(), |_| {});
    for id in ["one", "two", "three"] {
        let source = Source::Github {
            repo: format!("owner/{id}"),
            folder: None,
        };
        install(&mut window, package(id, "1.0.0", &[]), source, None).unwrap();
    }
    let source = Arc::new(Together {
        expected: 4,
        waiting: Mutex::new(0),
        arrived: std::sync::Condvar::new(),
    });
    window.update(|w, _, cx| {
        w.manage.update_source = Some(source.clone());
        w.check_plugin_updates(true, cx);
    });
    until(&app, &mut window, "check never completed", |w| {
        !w.manage.checking
    });
    window.read(|w, _| {
        assert!(w.manage.row_error.is_empty(), "{:?}", w.manage.row_error);
        assert_eq!(w.manage.updates.len(), 3);
        assert_eq!(w.manage.available.len(), 1);
    });
}

#[test]
fn plugin_updates_manual_results_catalog_persistence_and_no_automatic_install() {
    let data = Data::temp();
    let saved = data.path().join("window.json");
    let (app, mut window) = open_plugins(&data, &Arc::default(), |c| c.state = Some(saved.clone()));
    install(&mut window, package("demo", "v1.0.0", &[]), github(), None).unwrap();
    install(
        &mut window,
        package("local", "1.0.0", &[]),
        Source::File,
        None,
    )
    .unwrap();
    let source = Arc::new(Updates::default());
    *source.latest.lock().unwrap() = "1.0.0".into();
    *source.catalog.lock().unwrap() = vec![offered("demo")];
    window.update(|w, _, _| w.manage.update_source = Some(source.clone()));
    window.update(|w, _, cx| {
        w.check_plugin_updates(true, cx);
        w.check_plugin_updates(false, cx);
    });
    until(&app, &mut window, "check never completed", |w| {
        !w.manage.checking
    });
    assert_eq!(source.calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert!(window.read(|w, _| w.manage.updates.is_empty() && w.manage.announcements.is_empty()));
    let disk: Value = serde_json::from_slice(&std::fs::read(&saved).unwrap()).unwrap();
    assert!(disk.to_string().contains("demo"));
    assert_eq!(disk["seen_plugins"], json!(["demo"]));
    assert_eq!(*source.queried.lock().unwrap(), vec!["owner/demo"]);
    *source.latest.lock().unwrap() = "1.1.0".into();
    *source.catalog.lock().unwrap() = vec![offered("demo"), offered("new")];
    window.update(|w, _, cx| {
        w.plugin_ui.notices.clear();
        w.check_plugin_updates(true, cx);
    });
    until(&app, &mut window, "manual check never completed", |w| {
        !w.manage.checking
    });
    assert!(window.read(|w, _| w.plugin_ui.notices.is_empty()));
    assert_eq!(window.read(|w, _| w.manage.announcements.len()), 1);
    assert_eq!(window.read(|w, _| w.plugin_views().len()), 2);
    assert!(!runs(&window, "demo"));
    assert!(window.read(|w, _| w.manage.reviews.is_empty()));
    window.update(|w, _, cx| w.check_plugin_updates(false, cx));
    until(&app, &mut window, "auto check never completed", |w| {
        !w.manage.checking
    });
    assert_eq!(window.read(|w, _| w.manage.announcements.len()), 2);
    window.update(|w, _, cx| w.check_plugin_updates(false, cx));
    until(&app, &mut window, "repeat check never completed", |w| {
        !w.manage.checking
    });
    assert_eq!(window.read(|w, _| w.manage.announcements.len()), 2);
    source
        .failed
        .store(true, std::sync::atomic::Ordering::SeqCst);
    window.update(|w, _, cx| w.check_plugin_updates(true, cx));
    until(&app, &mut window, "failed check never completed", |w| {
        !w.manage.checking
    });
    assert!(window.read(|w, _| {
        w.plugin_ui
            .notices
            .iter()
            .any(|n| n.title == "Couldn't check for plugin updates")
    }));
    assert_eq!(window.read(|w, _| w.manage.available.len()), 2);
    assert_eq!(window.read(|w, _| w.prefs.seen_plugins.len()), 2);
    let (_app2, mut reopened) = open_plugins(&data, &Arc::default(), |c| c.state = Some(saved));
    source
        .failed
        .store(false, std::sync::atomic::Ordering::SeqCst);
    reopened.update(|w, _, _| w.manage.update_source = Some(source.clone()));
    reopened.update(|w, _, cx| w.check_plugin_updates(true, cx));
    until(
        &_app2,
        &mut reopened,
        "reopened check never completed",
        |w| !w.manage.checking,
    );
    assert!(reopened.read(|w, _| w.manage.announcements.is_empty()));
}

#[test]
fn plugin_updates_timer_is_launch_anchored_defers_until_focus_and_stops_on_drop() {
    let data = Data::temp();
    let (mut app, mut window) = open_plugins(&data, &Arc::default(), |_| {});
    let source = Arc::new(Updates::default());
    window.update(|w, window, cx| {
        w.manage.update_source = Some(source.clone());
        w.set_look(|look| look.cursor_blink = false, cx);
        window.activate_window();
    });
    app.run_until_parked();
    app.advance_clock(Duration::from_secs(19));
    app.run_until_parked();
    assert_eq!(source.calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    app.advance_clock(Duration::from_secs(1));
    app.run_until_parked();
    until(&app, &mut window, "first check missing", |w| {
        !w.manage.checking
    });
    assert_eq!(source.calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    app.advance_clock(Duration::from_secs(6 * 60 * 60 - 21));
    app.run_until_parked();
    assert_eq!(source.calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    app.advance_clock(Duration::from_secs(1));
    app.run_until_parked();
    until(&app, &mut window, "six hour check missing", |w| {
        !w.manage.checking
    });
    assert_eq!(source.calls.load(std::sync::atomic::Ordering::SeqCst), 2);
    struct Elsewhere;
    impl gpui::Render for Elsewhere {
        fn render(
            &mut self,
            _: &mut gpui::Window,
            _: &mut gpui::Context<Self>,
        ) -> impl gpui::IntoElement {
            gpui::div()
        }
    }
    let mut elsewhere = app.open_window(|_, _| Elsewhere);
    elsewhere.update(|_, window, _| window.activate_window());
    app.run_until_parked();
    app.advance_clock(Duration::from_secs(6 * 60 * 60));
    app.run_until_parked();
    assert!(window.read(|w, _| w.manage.check_due));
    assert_eq!(source.calls.load(std::sync::atomic::Ordering::SeqCst), 2);
    window.update(|_, window, _| window.activate_window());
    app.run_until_parked();
    until(&app, &mut window, "focus check missing", |w| {
        !w.manage.checking
    });
    assert_eq!(source.calls.load(std::sync::atomic::Ordering::SeqCst), 3);
    window.update(|_, window, _| window.remove_window());
    drop(window);
    app.run_until_parked();
    app.advance_clock(Duration::from_secs(6 * 60 * 60));
    app.run_until_parked();
    assert_eq!(source.calls.load(std::sync::atomic::Ordering::SeqCst), 3);
}

#[test]
fn plugin_updates_review_opens_the_plugin_or_available_without_approving() {
    let data = Data::temp();
    let (_app, mut window) = open_plugins(&data, &Arc::default(), |_| {});
    install(&mut window, package("demo", "1.0.0", &[]), github(), None).unwrap();
    window.update(|w, window, cx| w.sample_plugin_updates("stack", window, cx));
    window.draw();
    window.simulate_click(
        crate::ui::spot_at("plugin-update-review-0")
            .unwrap()
            .center(),
        gpui::MouseButton::Left,
    );
    assert!(window.read(|w, _| w.settings.as_ref().unwrap().section
        == super::settings::Section::Plugins
        && w.manage.selected.as_deref() == Some("demo")));
    window.update(|w, window, cx| w.sample_plugin_updates("stack", window, cx));
    window.draw();
    window.simulate_click(
        crate::ui::spot_at("plugin-update-review-1")
            .unwrap()
            .center(),
        gpui::MouseButton::Left,
    );
    assert!(window.read(|w, _| w.manage.browsing_available
        && w.manage.selected.is_none()
        && w.manage.reviews.is_empty()));
    assert!(!runs(&window, "demo"));
    window.update(|w, window, cx| w.sample_plugin_updates("stack", window, cx));
    window.draw();
    window.simulate_click(
        crate::ui::spot_at("plugin-update-dismiss-0")
            .unwrap()
            .center(),
        gpui::MouseButton::Left,
    );
    window.draw();
    window.simulate_click(
        crate::ui::spot_at("plugin-update-dismiss-1")
            .unwrap()
            .center(),
        gpui::MouseButton::Left,
    );
    assert!(window.read(|w, _| w.manage.announcements.is_empty()));
}

#[test]
fn plugin_updates_skip_file_linked_and_development_plugins() {
    let data = Data::temp();
    let (app, mut window) = open_plugins(&data, &Arc::default(), |_| {});
    for id in ["remote", "development"] {
        install(&mut window, package(id, "1.0.0", &[]), github(), None).unwrap();
    }
    install(
        &mut window,
        package("local", "1.0.0", &[]),
        Source::File,
        None,
    )
    .unwrap();
    let linked = data.path().join("linked");
    std::fs::create_dir_all(&linked).unwrap();
    std::fs::write(linked.join("main.js"), "").unwrap();
    std::fs::write(
        linked.join("wings-plugin.json"),
        json!({"id":"folder", "name":"Folder", "version":"1.0.0", "api":1, "main":"main.js"})
            .to_string(),
    )
    .unwrap();
    window.update(|w, window, cx| {
        w.link_plugin(&linked, window, cx).unwrap();
    });
    let dev = data.path().join("dev/development");
    std::fs::create_dir_all(&dev).unwrap();
    std::fs::write(dev.join("main.js"), "").unwrap();
    std::fs::write(
        dev.join("wings-plugin.json"),
        json!({"id":"development", "name":"Dev", "version":"1.0.0", "api":1, "main":"main.js"})
            .to_string(),
    )
    .unwrap();
    window.update(|_, window, _| window.remove_window());
    drop(window);
    drop(app);
    let (app, mut window) = open_plugins(&data, &Arc::default(), |c| {
        c.plugins.as_mut().unwrap().dev = Some(data.path().join("dev"))
    });
    let source = Arc::new(Updates::default());
    *source.latest.lock().unwrap() = "1.1.0".into();
    window.update(|w, _, cx| {
        w.manage.update_source = Some(source.clone());
        w.check_plugin_updates(false, cx);
    });
    until(&app, &mut window, "filter check never completed", |w| {
        !w.manage.checking
    });
    assert_eq!(source.queried.lock().unwrap().len(), 1);
    assert!(
        window.read(|w, _| !w.manage.updates.contains_key("development")
            && !w.manage.updates.contains_key("local")
            && !w.manage.updates.contains_key("folder"))
    );
}

#[test]
fn plugin_updates_manual_catalog_check_works_without_github_plugins() {
    let data = Data::temp();
    let (app, mut window) = open_plugins(&data, &Arc::default(), |_| {});
    let source = Arc::new(Updates::default());
    *source.catalog.lock().unwrap() = vec![offered("new")];
    window.update(|w, window, cx| {
        w.manage.update_source = Some(source.clone());
        w.show_plugins("", &[], window, cx);
    });
    window.draw();
    window.simulate_click(
        crate::ui::spot_at("plugin-check-updates").unwrap().center(),
        gpui::MouseButton::Left,
    );
    app.run_until_parked();
    until(&app, &mut window, "catalog check never completed", |w| {
        !w.manage.checking
    });
    assert_eq!(source.calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert!(source.queried.lock().unwrap().is_empty());
    assert_eq!(window.read(|w, _| w.manage.available.len()), 1);
    assert!(window.read(|w, _| w.manage.reviews.is_empty() && w.plugin_views().is_empty()));
}

fn runs(window: &TestAppWindow<Workspace>, id: &str) -> bool {
    window.read(|w, _| w.plugin_runs(id) && w.plugin_env.running.builtin(id).is_some())
}

/// What the plugin's main saved, read where the desktop reads it.
fn started(data: &Data, w: &Workspace, id: &str) -> Option<Value> {
    let scope = w
        .plugin_env
        .store
        .as_ref()?
        .lock()
        .unwrap()
        .known_storage_scope(id)?;
    data.saved(id, &scope, "started")
}

fn state(data: &Data) -> Value {
    serde_json::from_slice(&std::fs::read(data.paths().state).unwrap()).unwrap()
}

/// Installs `bytes` from `source` as the window would after a download.
fn install(
    window: &mut TestAppWindow<Workspace>,
    bytes: Vec<u8>,
    source: Source,
    expected: Option<&str>,
) -> Result<wings_plugins::store::PluginView, String> {
    window.update(|w, window, cx| w.install_package(&bytes, source, expected, window, cx))
}

/// Approves what the plugin's manifest asks for now, as the review's Turn on does.
fn approve(window: &mut TestAppWindow<Workspace>, id: &str) {
    window.update(|w, window, cx| {
        let view = w
            .plugin_views()
            .into_iter()
            .find(|v| v.manifest.id == id)
            .unwrap();
        w.set_plugin_enabled(id, true, Some(Grant::of(&view.manifest)), window, cx)
            .unwrap();
    });
}

#[test]
fn a_package_installs_off_asks_for_review_and_runs_once_approved() {
    let data = Data::temp();
    let file = data.path().join("demo.wings-plugin");
    std::fs::write(&file, package("demo", "1.0.0", &["git status"])).unwrap();
    let (app, mut window) = open_plugins(&data, &Arc::default(), |_| {});
    window.update(|w, window, cx| w.install_file(file.clone(), window, cx));
    until(&app, &mut window, "the review never opened", |w| {
        !w.manage.reviews.is_empty()
    });
    // Installed, but off and unapproved: nothing of it runs until you turn it on.
    assert!(!runs(&window, "demo"));
    assert_eq!(state(&data)["demo"]["enabled"], false);
    assert_eq!(state(&data)["demo"]["approved"], Value::Null);
    app.advance_clock(Duration::from_secs(1));
    app.run_until_parked();
    window.update(|w, window, cx| w.approve_review(window, cx));
    assert!(window.read(|w, _| w.manage.reviews.is_empty()));
    assert!(runs(&window, "demo"));
    until(&app, &mut window, "the main never ran", |w| {
        started(&data, w, "demo").is_some()
    });
    assert_eq!(
        window.read(|w, _| started(&data, w, "demo")),
        Some(json!("1.0.0"))
    );
    assert_eq!(state(&data)["demo"]["enabled"], true);
    assert_eq!(
        state(&data)["demo"]["approved"]["permissions"]["exec"],
        json!(["git status"])
    );
}

#[test]
fn turn_on_does_nothing_until_each_review_has_been_up_a_moment() {
    use crate::installed::{WINGS_VERSION, fixture::Approval::Never};
    let data = Data::temp();
    data.install("one", WINGS_VERSION, true, false, Never, "s1");
    data.install("two", WINGS_VERSION, true, false, Never, "s2");
    let (app, mut window) = open_plugins(&data, &Arc::default(), |_| {});
    window.update(|w, window, cx| {
        w.review_plugin("one", window, cx);
        w.review_plugin("two", window, cx);
        // A click that lands as the review opens approves nothing.
        w.approve_review(window, cx);
    });
    assert_eq!(window.read(|w, _| w.manage.reviews.len()), 2);
    assert!(!runs(&window, "one"));
    app.advance_clock(Duration::from_secs(1));
    app.run_until_parked();
    window.update(|w, window, cx| {
        w.approve_review(window, cx);
        // The next review shows, and a second click meant for the first lands on nothing.
        w.approve_review(window, cx);
    });
    assert!(runs(&window, "one") && !runs(&window, "two"));
    assert_eq!(window.read(|w, _| w.manage.reviews.len()), 1);
    app.advance_clock(Duration::from_secs(1));
    app.run_until_parked();
    window.update(|w, window, cx| w.approve_review(window, cx));
    assert!(runs(&window, "two"));
}

#[test]
fn an_update_restarts_the_plugin_with_its_new_version_and_keeps_its_approval() {
    let data = Data::temp();
    let (app, mut window) = open_plugins(&data, &Arc::default(), |_| {});
    install(
        &mut window,
        package("demo", "1.0.0", &["git status"]),
        github(),
        None,
    )
    .unwrap();
    approve(&mut window, "demo");
    until(&app, &mut window, "v1 never ran", |w| {
        started(&data, w, "demo").is_some()
    });
    let view = install(
        &mut window,
        package("demo", "1.1.0", &["git status"]),
        github(),
        Some("demo"),
    )
    .unwrap();
    assert!(view.enabled && view.approved && runs(&window, "demo"));
    until(&app, &mut window, "v1.1 never ran", |w| {
        started(&data, w, "demo") == Some(json!("1.1.0"))
    });
    // An update that turns out to be another plugin is refused.
    let other = install(
        &mut window,
        package("other", "1.0.0", &[]),
        github(),
        Some("demo"),
    );
    assert!(other.unwrap_err().contains("different plugin"));
}

#[test]
fn an_update_that_asks_for_more_access_stops_the_plugin_until_it_is_approved_again() {
    let data = Data::temp();
    let (app, mut window) = open_plugins(&data, &Arc::default(), |_| {});
    install(
        &mut window,
        package("demo", "1.0.0", &["git status"]),
        github(),
        None,
    )
    .unwrap();
    approve(&mut window, "demo");
    assert!(runs(&window, "demo"));
    let shown = window.read(|w, _| {
        w.plugin_views()
            .into_iter()
            .find(|v| v.manifest.id == "demo")
            .unwrap()
    });
    let grown = install(
        &mut window,
        package("demo", "1.1.0", &["git status", "gh api"]),
        github(),
        Some("demo"),
    )
    .unwrap();
    assert!(!grown.approved && !runs(&window, "demo"));
    // What the dialog showed before the update doesn't approve the new access.
    let stale = window.update(|w, window, cx| {
        w.set_plugin_enabled("demo", true, Some(Grant::of(&shown.manifest)), window, cx)
    });
    assert!(stale.unwrap_err().contains("changed since"));
    assert!(!runs(&window, "demo"));
    // The switch asks for a review instead of turning it on.
    window.update(|w, window, cx| w.toggle_plugin(grown.clone(), true, window, cx));
    assert_eq!(window.read(|w, _| w.manage.reviews.len()), 1);
    app.advance_clock(Duration::from_secs(1));
    app.run_until_parked();
    window.update(|w, window, cx| w.approve_review(window, cx));
    assert!(runs(&window, "demo"));
}

#[test]
fn turning_a_plugin_off_or_removing_it_stops_its_main_and_closes_its_tabs() {
    let data = Data::temp();
    data.install(
        "demo",
        crate::installed::WINGS_VERSION,
        true,
        true,
        crate::installed::fixture::Approval::Current,
        "scope",
    );
    let (_app, mut window) = open_plugins(&data, &Arc::default(), |_| {});
    assert!(runs(&window, "demo"));
    window.update(|w, window, cx| assert!(w.open_plugin_view("demo", "fixture", None, window, cx)));
    let host = Arc::downgrade(&window.read(|w, _| w.plugins[0].host.clone()));
    window.update(|w, window, cx| {
        w.set_plugin_enabled("demo", false, None, window, cx)
            .unwrap()
    });
    assert!(!runs(&window, "demo"));
    assert!(window.read(|w, _| w.plugin_tabs.is_empty()));
    assert!(
        host.upgrade().is_none(),
        "the stopped plugin's host is still alive"
    );
    assert_eq!(state(&data)["demo"]["enabled"], false);
    // On again, with what it was approved for: it runs again.
    window.update(|w, window, cx| {
        let grant = w
            .plugin_env
            .store
            .as_ref()
            .unwrap()
            .lock()
            .unwrap()
            .current_grant("demo")
            .unwrap();
        w.set_plugin_enabled("demo", true, grant, window, cx)
            .unwrap();
    });
    assert!(runs(&window, "demo"));
    window.update(|w, window, cx| w.remove_plugin("demo", window, cx).unwrap());
    assert!(!runs(&window, "demo"));
    assert!(window.read(|w, _| w.plugin_views().is_empty()));
    assert!(!data.paths().dir.join("demo").exists());
    assert!(state(&data).get("demo").is_none());
}

fn agent(pane: usize, state: crate::agents::AgentState) -> crate::agents::Agent {
    crate::agents::Agent {
        pane_id: pane.to_string(),
        space_id: String::new(),
        pid: 1,
        kind: crate::agents::AgentKind::Claude,
        session_id: None,
        name: None,
        state,
        waiting_for: Some("permission to run Bash".into()),
        args: Vec::new(),
        started_at: 0,
        state_since: None,
        direct: true,
        summary: None,
        weak_name: false,
    }
}

/// Sends `text` to the agent in pane 0 as a plugin's `sendToAgent` does, and the answer.
fn send(app: &TestApp, window: &mut TestAppWindow<Workspace>, text: &str) -> Result<Value, String> {
    let answer = paste(window, text);
    answered(app, &answer)
}

/// Starts sending `text` to the agent in pane 0; the answer comes once Enter was pressed or skipped.
fn paste(window: &mut TestAppWindow<Workspace>, text: &str) -> crate::page::runtime::Answer {
    let (reply, answer) = crate::page::runtime::Reply::channel();
    window.update(|w, _, cx| w.send_to_agent("0", text, reply, cx));
    answer
}

/// Runs the clock past the moment Enter waits for, until the answer is there.
fn answered(app: &TestApp, answer: &crate::page::runtime::Answer) -> Result<Value, String> {
    for _ in 0..50 {
        if let Some(answer) = answer.wait(Duration::ZERO) {
            return Ok(serde_json::from_str(&answer?).unwrap());
        }
        app.advance_clock(Duration::from_millis(50));
        app.run_until_parked();
    }
    panic!("never answered");
}

fn output(window: &TestAppWindow<Workspace>) -> String {
    window.read(|w, cx| {
        String::from_utf8_lossy(&w.terminal(0).unwrap().read(cx).recent_output()).into_owned()
    })
}

#[test]
fn enter_is_pressed_for_an_agent_only_with_the_setting_on_and_never_over_a_prompt_or_your_typing() {
    use crate::agents::AgentState;
    let (app, mut window) = super::tests::open_config(|_| {});
    super::tests::press(&mut window, "d");
    window.update(|w, _, _| {
        w.agents.insert(0, agent(0, AgentState::Idle));
    });
    // Off, as on the desktop by default: the message is pasted and waits for you.
    assert_eq!(
        send(&app, &mut window, "echo off-$((1+1))").unwrap()["submitted"],
        false
    );
    // On: Enter follows a moment later, so the shell standing in for the agent runs it.
    window.update(|w, _, cx| w.set_prefs(|prefs| prefs.agents_message = true, cx));
    assert_eq!(
        send(&app, &mut window, "; echo on-$((2+2))").unwrap()["submitted"],
        true
    );
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while !output(&window).contains("on-4") {
        assert!(
            std::time::Instant::now() < deadline,
            "Enter was never pressed: {}",
            output(&window)
        );
        app.advance_clock(Duration::from_millis(200));
        app.run_until_parked();
        std::thread::sleep(Duration::from_millis(20));
    }
    // An agent waiting on a permission prompt would take the text for its answer.
    window.update(|w, _, _| {
        w.agents.insert(0, agent(0, AgentState::Blocked));
    });
    assert!(
        send(&app, &mut window, "hi")
            .unwrap_err()
            .contains("waiting for permission to run Bash")
    );
    // Text you typed and haven't sent stays yours.
    window.update(|w, window, cx| {
        w.agents.insert(0, agent(0, AgentState::Idle));
        w.terminal(0).unwrap().update(cx, |view, cx| {
            gpui::EntityInputHandler::replace_text_in_range(view, None, "draft", window, cx)
        });
    });
    assert!(
        send(&app, &mut window, "hi")
            .unwrap_err()
            .contains("hasn't sent it")
    );
}

#[test]
fn enter_is_skipped_when_you_type_or_the_agent_comes_to_a_prompt_before_it_is_pressed() {
    use crate::agents::AgentState;
    let (app, mut window) = super::tests::open_config(|_| {});
    super::tests::press(&mut window, "d");
    window.update(|w, _, cx| {
        w.agents.insert(0, agent(0, AgentState::Idle));
        w.set_prefs(|prefs| prefs.agents_message = true, cx);
    });
    // You type in the moment between the paste and Enter.
    let answer = paste(&mut window, "echo typed-$((3+3))");
    window.update(|w, window, cx| {
        w.terminal(0).unwrap().update(cx, |view, cx| {
            gpui::EntityInputHandler::replace_text_in_range(view, None, "x", window, cx)
        });
    });
    assert_eq!(answered(&app, &answer).unwrap()["submitted"], false);
    // The agent comes to a permission prompt in that moment. Ctrl-C clears the prompt and your draft first.
    window.update(|w, window, cx| {
        w.terminal(0).unwrap().update(cx, |view, cx| {
            gpui::EntityInputHandler::replace_text_in_range(view, None, "\u{3}", window, cx)
        });
    });
    let answer = paste(&mut window, "echo blocked-$((4+4))");
    window.update(|w, _, _| {
        w.agents.insert(0, agent(0, AgentState::Blocked));
    });
    assert_eq!(answered(&app, &answer).unwrap()["submitted"], false);
    // Neither was run: no Enter reached the shell.
    for _ in 0..20 {
        app.advance_clock(Duration::from_millis(100));
        app.run_until_parked();
        std::thread::sleep(Duration::from_millis(20));
    }
    let shown = output(&window);
    assert!(
        !shown.contains("typed-6") && !shown.contains("blocked-8"),
        "{shown}"
    );
}

#[test]
fn open_pane_runs_the_command_only_with_the_setting_on() {
    let (_app, mut window) = super::tests::open_config(|_| {});
    let plan = |run| wings_connect::panes::OpenPlan {
        space_id: std::env::temp_dir().to_string_lossy().into_owned(),
        cwd: None,
        input: Some("echo hi".into()),
        run,
        next_to: None,
    };
    let ran = |window: &mut TestAppWindow<Workspace>, run| {
        window.update(|w, window, cx| w.claude_open_pane(plan(run), window, cx).unwrap().ran)
    };
    assert!(
        !ran(&mut window, true),
        "pressed Enter with the setting off"
    );
    window.update(|w, _, cx| w.set_prefs(|prefs| prefs.agents_run_commands = true, cx));
    assert!(ran(&mut window, true));
    assert!(
        !ran(&mut window, false),
        "pressed Enter without being asked to run"
    );
}

/// Calls `worktree_setup` on the app's socket from another thread, and the place its answer lands.
fn propose(socket: &std::path::Path, setup: &str) -> Arc<Mutex<Option<Value>>> {
    use std::io::{BufRead, BufReader};
    let answer = Arc::new(Mutex::new(None));
    let (found, socket) = (answer.clone(), socket.to_owned());
    let request = json!({ "id": 1, "op": "call", "name": "worktree_setup", "arguments": { "project": "alpha", "setup": setup } });
    std::thread::spawn(move || {
        let stream = std::os::unix::net::UnixStream::connect(socket).unwrap();
        writeln!(&stream, "{request}").unwrap();
        for line in BufReader::new(&stream).lines() {
            let message: Value = serde_json::from_str(&line.unwrap()).unwrap_or(Value::Null);
            if message.get("id") == Some(&json!(1)) {
                *found.lock().unwrap() = Some(message);
                return;
            }
        }
    });
    answer
}

/// Waits for the prompt, checks Allow does nothing before it is armed, answers, and gives the tool's reply.
fn answer(
    app: &TestApp,
    window: &mut TestAppWindow<Workspace>,
    asked: &Arc<Mutex<Option<Value>>>,
    allow: bool,
) -> Value {
    until(app, window, "the prompt never showed", |w| {
        !w.setup_asks.queue.is_empty()
    });
    let shown = window.read(|w, _| {
        let ask = &w.setup_asks.queue[0];
        (ask.project.clone(), ask.proposed.clone())
    });
    assert_eq!(shown.0, "alpha");
    if allow {
        window.update(|w, _, cx| w.answer_setup(true, cx));
        assert_eq!(
            window.read(|w, _| w.setup_asks.queue.len()),
            1,
            "Allow answered before the prompt was armed"
        );
        app.advance_clock(Duration::from_secs(1));
        app.run_until_parked();
    }
    window.update(|w, _, cx| w.answer_setup(allow, cx));
    until(app, window, "the tool never answered", |_| {
        asked.lock().unwrap().is_some()
    });
    asked.lock().unwrap().take().unwrap()
}

#[test]
fn a_setup_command_an_agent_proposes_is_saved_only_once_you_allow_it() {
    let data = Data::temp();
    let alpha = data.path().canonicalize().unwrap().join("alpha");
    std::fs::create_dir_all(&alpha).unwrap();
    let list = data.path().join("spaces.json");
    std::fs::write(
        &list,
        json!([{ "id": "a", "name": "alpha", "path": alpha }]).to_string(),
    )
    .unwrap();
    let socket = data.path().join("mcp.sock");
    let (app, mut window) = open_plugins(&data, &Arc::default(), |config| {
        config.project = alpha.clone();
        config.projects = Some(list.clone());
        config.socket = Some(socket.clone());
    });
    let setup = |window: &TestAppWindow<Workspace>| {
        window.read(|w, _| {
            w.spaces
                .worktree_setup(&alpha.to_string_lossy())
                .unwrap()
                .setup
        })
    };

    let asked = propose(&socket, "make bootstrap");
    let reply = answer(&app, &mut window, &asked, false);
    assert_eq!(reply["result"]["isError"], true, "{reply}");
    assert!(
        reply["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("declined")
    );
    assert_eq!(setup(&window), None);

    let asked = propose(&socket, "pnpm install");
    let reply = answer(&app, &mut window, &asked, true);
    assert_eq!(reply["result"]["isError"], false, "{reply}");
    assert_eq!(setup(&window).as_deref(), Some("pnpm install"));
}

/// A zip of `files`, as a hand-made or hostile `.wings-plugin` would be, with `links` as symlink entries.
fn zip_of(files: &[(&str, &str)], links: &[(&str, &str)]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, body) in files {
        zip.start_file(*name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(body.as_bytes()).unwrap();
    }
    for (name, target) in links {
        zip.add_symlink(*name, *target, zip::write::SimpleFileOptions::default())
            .unwrap();
    }
    zip.finish().unwrap().into_inner()
}

fn manifest(id: &str) -> String {
    json!({ "id": id, "name": format!("Plugin {id}"), "version": "1.0.0", "api": 1, "main": "dist/main.js" })
        .to_string()
}

/// Drops `paths` from Finder on the middle of the window, as the platform reports it.
fn drop_on(window: &mut TestAppWindow<Workspace>, paths: &[std::path::PathBuf]) {
    let position = gpui::point(gpui::px(500.0), gpui::px(320.0));
    window.draw();
    window.simulate_event(gpui::FileDropEvent::Entered {
        position,
        paths: gpui::ExternalPaths(paths.iter().cloned().collect()),
    });
    window.draw();
    window.simulate_event(gpui::FileDropEvent::Submit { position });
    window.simulate_event(gpui::FileDropEvent::Ended);
}

fn settings_on_plugins(window: &TestAppWindow<Workspace>) -> bool {
    window.read(|w, _| {
        w.settings
            .as_ref()
            .is_some_and(|dialog| dialog.section == super::settings::Section::Plugins)
    })
}

/// What the plugins folder holds, by name, for checking a refused package wrote nothing.
fn installed_folders(data: &Data) -> Vec<String> {
    std::fs::read_dir(&data.paths().dir)
        .map(|dir| {
            dir.flatten()
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn a_dropped_package_opens_plugins_installs_off_and_runs_once_its_review_is_approved() {
    let data = Data::temp();
    let file = data.path().join("dropped.wings-plugin");
    std::fs::write(&file, package("dropped", "1.0.0", &["git status"])).unwrap();
    let (app, mut window) = open_plugins(&data, &Arc::default(), |_| {});
    // Finder hands over every file dragged; the package is picked out of them.
    let other = data.path().join("notes.txt");
    std::fs::write(&other, "hi").unwrap();
    drop_on(&mut window, &[other, file]);
    assert!(settings_on_plugins(&window));
    until(&app, &mut window, "the review never opened", |w| {
        w.manage
            .reviews
            .first()
            .is_some_and(|v| v.manifest.id == "dropped")
    });
    // As on the desktop: installed off and unapproved, and nothing of it runs until the review is approved.
    assert!(!runs(&window, "dropped"));
    assert_eq!(state(&data)["dropped"]["enabled"], false);
    assert_eq!(state(&data)["dropped"]["approved"], Value::Null);
    app.advance_clock(Duration::from_secs(1));
    app.run_until_parked();
    window.update(|w, window, cx| w.approve_review(window, cx));
    assert!(runs(&window, "dropped"));
    assert_eq!(state(&data)["dropped"]["enabled"], true);
}

#[test]
fn cancelling_a_dropped_packages_review_leaves_it_off_and_unapproved() {
    let data = Data::temp();
    let file = data.path().join("dropped.wings-plugin");
    std::fs::write(&file, package("dropped", "1.0.0", &["git status"])).unwrap();
    let (app, mut window) = open_plugins(&data, &Arc::default(), |_| {});
    drop_on(&mut window, &[file]);
    until(&app, &mut window, "the review never opened", |w| {
        !w.manage.reviews.is_empty()
    });
    window.update(|w, _, cx| w.skip_review(cx));
    app.run_until_parked();
    assert!(window.read(|w, _| w.manage.reviews.is_empty()));
    assert!(!runs(&window, "dropped"));
    assert_eq!(state(&data)["dropped"]["enabled"], false);
    assert_eq!(state(&data)["dropped"]["approved"], Value::Null);
}

#[test]
fn a_dropped_package_that_escapes_its_folder_or_has_a_bad_manifest_is_refused_and_writes_nothing() {
    let data = Data::temp();
    let outside = data.path().join("escaped.js");
    let hostile = [
        (
            "parent",
            zip_of(
                &[
                    ("wings-plugin.json", &manifest("parent")),
                    ("dist/main.js", ""),
                    ("../escaped.js", "pwned"),
                ],
                &[],
            ),
            "Unsafe path",
        ),
        (
            "nested-parent",
            zip_of(
                &[
                    ("wings-plugin.json", &manifest("nested")),
                    ("dist/main.js", ""),
                    ("dist/../../escaped.js", "pwned"),
                ],
                &[],
            ),
            "Unsafe path",
        ),
        (
            "bad-id",
            zip_of(
                &[
                    ("wings-plugin.json", &manifest("../escaped")),
                    ("dist/main.js", ""),
                ],
                &[],
            ),
            "invalid id",
        ),
        (
            "not-json",
            zip_of(
                &[("wings-plugin.json", "{ nope"), ("dist/main.js", "")],
                &[],
            ),
            "",
        ),
        (
            "not-a-zip",
            b"just text".to_vec(),
            "Not a .wings-plugin file",
        ),
    ];
    let (app, mut window) = open_plugins(&data, &Arc::default(), |_| {});
    for (name, bytes, expected) in hostile {
        let file = data.path().join(format!("{name}.wings-plugin"));
        std::fs::write(&file, bytes).unwrap();
        window.update(|w, _, cx| {
            w.manage.error = None;
            cx.notify();
        });
        drop_on(&mut window, &[file]);
        until(&app, &mut window, "the install never ended", |w| {
            w.manage.installing.is_none() && w.manage.error.is_some()
        });
        let error = window.read(|w, _| w.manage.error.clone().unwrap());
        assert!(error.contains(expected), "{name}: {error}");
        assert!(window.read(|w, _| w.manage.reviews.is_empty()), "{name}");
        assert!(
            installed_folders(&data).is_empty(),
            "{name}: {:?}",
            installed_folders(&data)
        );
        assert!(!outside.exists(), "{name} wrote outside the plugins folder");
        assert!(window.read(|w, _| w.plugin_views().is_empty()), "{name}");
    }
}

#[test]
fn absolute_paths_and_symlinks_in_a_dropped_package_stay_inside_its_folder() {
    let data = Data::temp();
    let target = data.path().join("secret.txt");
    std::fs::write(&target, "secret").unwrap();
    let absolute = data.path().join("absolute.js");
    let bytes = zip_of(
        &[
            ("wings-plugin.json", &manifest("contained")),
            ("dist/main.js", ""),
            (absolute.to_str().unwrap(), "inside"),
        ],
        &[("dist/link", target.to_str().unwrap())],
    );
    let file = data.path().join("contained.wings-plugin");
    std::fs::write(&file, bytes).unwrap();
    let (app, mut window) = open_plugins(&data, &Arc::default(), |_| {});
    drop_on(&mut window, &[file]);
    until(&app, &mut window, "the review never opened", |w| {
        !w.manage.reviews.is_empty()
    });
    let folder = data.paths().dir.join("contained");
    // An absolute entry lands under the plugin's own folder, never where it points.
    assert!(!absolute.exists());
    let relative = absolute.strip_prefix("/").unwrap();
    assert_eq!(
        std::fs::read_to_string(folder.join(relative)).unwrap(),
        "inside"
    );
    // A symlink entry is written as a plain file holding its target, so nothing links out of the folder.
    let link = std::fs::symlink_metadata(folder.join("dist/link")).unwrap();
    assert!(link.is_file() && !link.file_type().is_symlink());
    assert_ne!(
        std::fs::read_to_string(folder.join("dist/link")).unwrap(),
        "secret"
    );
}

#[test]
fn dropping_files_that_arent_plugins_or_a_folder_does_nothing() {
    let data = Data::temp();
    let text = data.path().join("notes.txt");
    std::fs::write(&text, "hi").unwrap();
    let folder = data.path().join("folder-plugin");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("wings-plugin.json"), manifest("folder")).unwrap();
    std::fs::write(folder.join("main.js"), "").unwrap();
    let (app, mut window) = open_plugins(&data, &Arc::default(), |_| {});
    drop_on(&mut window, &[text, folder]);
    app.run_until_parked();
    window.draw();
    assert!(window.read(|w, _| w.settings.is_none()
        && w.manage.installing.is_none()
        && w.manage.error.is_none()
        && w.manage.reviews.is_empty()
        && w.plugin_views().is_empty()));
    assert!(installed_folders(&data).is_empty());
}

#[test]
fn a_plugins_page_shows_its_readme_as_markdown_but_not_one_too_big_or_linked_out() {
    let data = Data::temp();
    let readme =
        "# Demo\n\nSee the [docs](https://example.com/docs) and `wings plugin`.\n\n- one\n- two\n";
    let bytes = zip_of(
        &[
            ("wings-plugin.json", &manifest("readme")),
            ("dist/main.js", ""),
            ("ReadMe.MD", readme),
        ],
        &[],
    );
    let (_app, mut window) = open_plugins(&data, &Arc::default(), |_| {});
    let view = install(&mut window, bytes, Source::File, None).unwrap();
    window.update(|w, window, cx| w.show_plugins("readme", &[], window, cx));
    window.draw();
    assert!(crate::ui::spot_at("plugin-readme").is_some());
    let blocks = window.read(|w, _| w.plugin_readme(&view)).unwrap();
    assert_eq!(*blocks, crate::markdown::parse(readme));
    assert!(matches!(&blocks[0], crate::markdown::Block::Heading(1, text) if text.text == "Demo"));
    let folder = data.paths().dir.join("readme");

    // Read once per version: a new version reads it again.
    std::fs::remove_file(folder.join("ReadMe.MD")).unwrap();
    std::fs::write(folder.join("README.md"), vec![b'a'; 256 * 1024 + 1]).unwrap();
    let mut newer = view.clone();
    newer.manifest.version = "1.0.1".into();
    assert!(window.read(|w, _| w.plugin_readme(&newer)).is_none());

    let outside = data.path().join("outside.md");
    std::fs::write(&outside, "# Outside").unwrap();
    std::fs::remove_file(folder.join("README.md")).unwrap();
    std::os::unix::fs::symlink(&outside, folder.join("README.md")).unwrap();
    newer.manifest.version = "1.0.2".into();
    assert!(window.read(|w, _| w.plugin_readme(&newer)).is_none());
}
