use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use gpui::{TestApp, TestAppWindow};

use super::*;
use crate::workspace::{plugin_tests::until, settings::Section, tests::open_with};

/// A Mac with Claude Code from its own installer and RTK from Homebrew, each a version behind. It counts the checks.
#[derive(Default)]
struct Mac {
    passes: AtomicUsize,
    gh: Mutex<&'static str>,
    /// What `update` was asked to run, in order.
    updates: Mutex<Vec<String>>,
    /// Held by a test while an update must stay running.
    gate: Mutex<()>,
    /// What a failing update prints.
    fails: Option<&'static str>,
}

impl Lookup for Mac {
    fn find(&self, program: &str) -> Option<(PathBuf, PathBuf)> {
        let resolved = match program {
            "brew" => "/opt/homebrew/Library/brew.sh",
            "claude" => "/Users/me/.local/share/claude/versions/2.1.330",
            "gh" => {
                self.passes.fetch_add(1, Ordering::SeqCst);
                "/opt/homebrew/Cellar/gh/1.2.0/bin/gh"
            }
            _ => return None,
        };
        Some((PathBuf::from("/bin").join(program), resolved.into()))
    }

    fn run(&self, program: &Path, _: &[&str]) -> Option<String> {
        Some(match program.file_name()?.to_str()? {
            "claude" => "2.1.330 (Claude Code)".to_owned(),
            _ => format!("gh version {}", self.gh.lock().unwrap()),
        })
    }

    fn update(&self, program: &Path, args: &[String]) -> Result<(), String> {
        self.updates.lock().unwrap().push(format!("{} {}", program.display(), args.join(" ")));
        // A test holds the gate to see an update while it runs; it never waits for more than a few seconds.
        let until = std::time::Instant::now() + Duration::from_secs(5);
        while self.gate.try_lock().is_err() && std::time::Instant::now() < until {
            std::thread::sleep(Duration::from_millis(5));
        }
        match self.fails {
            Some(error) => Err(error.to_owned()),
            None => {
                *self.gh.lock().unwrap() = "1.2.1";
                Ok(())
            }
        }
    }

    fn get(&self, url: &str, _: u64) -> Option<String> {
        let formula = "https://formulae.brew.sh/api/formula/gh.json";
        Some(match url {
            u if u == formula => r#"{"versions":{"stable":"1.2.1"}}"#.to_owned(),
            tools::CLAUDE_LATEST => "2.1.331\n".to_owned(),
            tools::CHANGELOG => "## 2.1.331\n\n- Added `/rewind`\n\n## 2.1.330\n\n- Older note\n".to_owned(),
            _ => return None,
        })
    }
}

fn open_with_mac() -> (TestApp, TestAppWindow<Workspace>, Arc<Mac>) {
    open_with_updates(None)
}

fn open_with_updates(fails: Option<&'static str>) -> (TestApp, TestAppWindow<Workspace>, Arc<Mac>) {
    let (app, mut window) = open_with(None);
    let mac = Arc::new(Mac { gh: Mutex::new("1.2.0"), fails, ..Mac::default() });
    window.update(|w, _, _| w.tools.lookup = mac.clone());
    (app, window, mac)
}

fn checked(app: &TestApp, window: &mut TestAppWindow<Workspace>) {
    window.update(|w, _, cx| w.check_tools(cx));
    until(app, window, "the check never finished", |w| !w.tools.checking && w.tools.list.is_some());
}

#[test]
fn a_check_finds_the_tools_their_updates_and_what_is_new_since_the_installed_version() {
    let (app, mut window, _) = open_with_mac();
    checked(&app, &mut window);
    window.update(|w, _, cx| {
        let list = w.tools.list.as_ref().unwrap();
        let rows: Vec<_> = list
            .iter()
            .map(|t| (t.program.as_str(), t.found, t.installed.as_deref(), t.latest.as_deref(), t.source.label()))
            .collect();
        assert_eq!(
            rows,
            [
                ("claude", true, Some("2.1.330"), Some("2.1.331"), "Claude installer"),
                // Git is listed missing, as the app needs it. OpenCode isn't, as only the app would use it, and nor is RTK.
                ("gh", true, Some("1.2.0"), Some("1.2.1"), "Homebrew"),
                ("git", false, None, None, "Unknown source"),
            ]
        );
        assert_eq!(w.tools.updates(), 2);
        // The first time, what's new starts after the version you have, so only the newer entry is news.
        assert_eq!(w.tools.read.as_deref(), Some("2.1.330"));
        let unread = tools::unread(&w.tools.changelog, "2.1.330");
        assert_eq!(unread.len(), 1);
        // The tools, then Claude Code's news.
        assert_eq!(w.tools_cards(cx).len(), 2);
    });
}

#[test]
fn the_section_draws_and_marking_the_news_read_moves_the_mark() {
    let (app, mut window, _) = open_with_mac();
    checked(&app, &mut window);
    window.update(|w, window, cx| {
        w.open_settings(window, cx);
        w.set_settings_section(Section::Tools, cx);
    });
    window.draw();
    window.update(|w, _, cx| {
        w.mark_changelog_read(cx);
        assert_eq!(w.tools.read.as_deref(), Some("2.1.331"));
        // Nothing is new now, so the card is one row.
        assert_eq!(w.tools_cards(cx)[1].len(), 1);
    });
    window.draw();
}

#[test]
fn update_types_the_command_in_a_new_tab_and_checks_again_when_it_ends() {
    let (app, mut window, mac) = open_with_mac();
    checked(&app, &mut window);
    assert_eq!(mac.passes.load(Ordering::SeqCst), 1);
    let tabs = window.read(|w, _| w.terminals.len());
    window.update(|w, window, cx| {
        w.open_settings(window, cx);
        w.update_tool("brew upgrade gh".into(), window, cx);
    });
    let pane = window.read(|w, cx| {
        assert!(w.settings.is_none());
        assert_eq!(w.terminals.len(), tabs + 1);
        let tab = w.terminals.last().unwrap();
        assert_eq!(w.active, crate::workspace::Active::Terminal(tab.id));
        assert_eq!(w.terminal(tab.focused).unwrap().read(cx).written(), ["brew upgrade gh && exit\r"]);
        tab.focused
    });
    // Nothing is checked while the command runs. Its shell ending, after the command worked, is.
    assert_eq!(mac.passes.load(Ordering::SeqCst), 1);
    *mac.gh.lock().unwrap() = "1.2.1";
    window.update(|w, _, cx| {
        w.terminal(pane).unwrap().update(cx, |_, cx| cx.emit(TerminalEvent::Exited));
    });
    until(&app, &mut window, "no check after the update", |w| {
        !w.tools.checking && w.tools.list.as_ref().is_some_and(|l| l.iter().any(|t| t.installed.as_deref() == Some("1.2.1")))
    });
    assert_eq!(mac.passes.load(Ordering::SeqCst), 2);
    window.read(|w, _| assert_eq!(w.tools.updates(), 1));
}

fn open_tools(window: &mut TestAppWindow<Workspace>) {
    window.update(|w, window, cx| {
        w.open_settings(window, cx);
        w.set_settings_section(Section::Tools, cx);
    });
    window.draw();
}

#[test]
fn update_runs_in_its_row_with_settings_open_and_shows_the_new_version() {
    let (app, mut window, mac) = open_with_mac();
    checked(&app, &mut window);
    open_tools(&mut window);
    let tabs = window.read(|w, _| w.terminals.len());
    let held = mac.gate.lock().unwrap();
    // Checked in the one call that starts it: the window's calls wait for background work, which the gate holds.
    window.update(|w, _, cx| {
        w.start_update("gh".into(), cx);
        // It runs, with nothing typed anywhere and Settings still open, and its row is built meanwhile.
        assert!(w.tools.is_updating());
        assert!(matches!(w.tools.runs.get("gh"), Some(Run::Going(_))));
        assert!(w.settings.is_some());
        assert_eq!(w.terminals.len(), tabs);
        assert_eq!(w.tools_cards(cx)[0].len(), 1 + w.tools.list.as_ref().unwrap().len());
        // Another update waits for it: the buttons are off, and a click would start nothing.
        w.start_update("claude".into(), cx);
        assert!(!w.tools.runs.contains_key("claude"));
    });
    drop(held);
    until(&app, &mut window, "the update never ended", |w| {
        !w.tools.is_updating() && w.tools.list.as_ref().is_some_and(|l| l.iter().any(|t| t.installed.as_deref() == Some("1.2.1")))
    });
    window.draw();
    // Its program and arguments were fixed, and only that tool was looked at again.
    assert_eq!(*mac.updates.lock().unwrap(), ["/bin/brew upgrade gh"]);
    assert_eq!(mac.passes.load(Ordering::SeqCst), 2);
    window.read(|w, _| {
        assert!(w.tools.runs.is_empty());
        assert!(w.settings.is_some());
        assert_eq!(w.tools.updates(), 1);
        let gh = w.tools.list.as_ref().unwrap().iter().find(|t| t.program == "gh").unwrap();
        assert_eq!(gh.installed.as_deref(), Some("1.2.1"));
        assert!(!gh.has_update());
    });
}

#[test]
fn a_failed_update_shows_what_it_printed_and_offers_the_terminal() {
    let (app, mut window, mac) = open_with_updates(Some("Error: needs a password"));
    checked(&app, &mut window);
    open_tools(&mut window);
    window.update(|w, _, cx| w.start_update("gh".into(), cx));
    until(&app, &mut window, "the update never failed", |w| matches!(w.tools.runs.get("gh"), Some(Run::Failed { .. })));
    window.draw();
    window.read(|w, _| {
        let Some(Run::Failed { error, command }) = w.tools.runs.get("gh") else { unreachable!() };
        assert_eq!(error.as_ref(), "Error: needs a password");
        assert_eq!(command, "brew upgrade gh");
        // Nothing was rechecked, and the tool is as it was; the others may be updated.
        assert!(!w.tools.is_updating());
        assert!(w.settings.is_some());
    });
    assert_eq!(mac.passes.load(Ordering::SeqCst), 1);
    let tabs = window.read(|w, _| w.terminals.len());
    window.update(|w, window, cx| w.run_in_terminal("gh", window, cx));
    window.read(|w, cx| {
        assert!(w.settings.is_none());
        assert!(w.tools.runs.is_empty());
        assert_eq!(w.terminals.len(), tabs + 1);
        let tab = w.terminals.last().unwrap();
        assert_eq!(w.terminal(tab.focused).unwrap().read(cx).written(), ["brew upgrade gh && exit\r"]);
    });
}

#[test]
fn checks_run_after_launch_and_every_six_hours_and_never_beforehand() {
    let (app, mut window, mac) = open_with_mac();
    window.update(|w, window, cx| {
        // Only an app that serves the socket checks on its own.
        w.config.socket = Some(std::env::temp_dir().join("wings-tools-never-bound.sock"));
        w.start_tool_checks(window, cx);
        window.activate_window();
    });
    app.run_until_parked();
    app.advance_clock(Duration::from_secs(7));
    app.run_until_parked();
    assert_eq!(mac.passes.load(Ordering::SeqCst), 0);
    app.advance_clock(Duration::from_secs(1));
    app.run_until_parked();
    until(&app, &mut window, "the first check never ran", |w| !w.tools.checking && w.tools.list.is_some());
    assert_eq!(mac.passes.load(Ordering::SeqCst), 1);
    app.advance_clock(Duration::from_secs(6 * 60 * 60 - 1));
    app.run_until_parked();
    assert_eq!(mac.passes.load(Ordering::SeqCst), 1);
    app.advance_clock(Duration::from_secs(1));
    app.run_until_parked();
    until(&app, &mut window, "the six hour check never ran", |w| !w.tools.checking);
    assert_eq!(mac.passes.load(Ordering::SeqCst), 2);
}

#[test]
fn a_window_without_the_socket_checks_nothing_by_itself() {
    let (app, mut window, mac) = open_with_mac();
    window.update(|w, window, cx| {
        w.start_tool_checks(window, cx);
        w.open_settings(window, cx);
        w.set_settings_section(Section::Tools, cx);
    });
    app.advance_clock(Duration::from_secs(6 * 60 * 60));
    app.run_until_parked();
    assert_eq!(mac.passes.load(Ordering::SeqCst), 0);
    window.read(|w, _| assert!(w.tools.list.is_none()));
    window.draw();
}
