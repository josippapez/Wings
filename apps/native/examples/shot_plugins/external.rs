//! `--external <folder>`: the separately shipped Sprint board, Cyclops and Claude accounts plugins, from their repos in
//! `<folder>` (`wings-sprint-board`, `wings-cyclops`, `wings-claude-accounts`, each with `dist/native` built), installed
//! in a temporary plugins folder and shown in the app at 1000 and 640 points wide: each sidebar in its states, and the
//! popovers under their title bar buttons.
//!
//! Nothing reaches the network or your accounts. Cyclops' and Claude accounts' bundles have their calls to Cyclops' API
//! and to your Claude Code accounts swapped, in the text of the bundle, for made-up answers (`CYCLOPS`, `ACCOUNTS`).
//! Sprint board runs a fixture main that answers its sidebar's queries with a made-up board (`SPRINT_MAIN`), and an `az`
//! of the shots' own, first on PATH, that fails, for its setup.

use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use gpui::{AnyWindowHandle, AppContext, Entity, VisualTestAppContext, WindowHandle, px, size};
use wings_native::{host::Config, installed::fixture::Data, page::PageView, workspace::Workspace};

use super::{app, save, wait};

/// Made-up Cyclops answers in place of `wings.fetch`, and a token that is there in place of `wings.secrets.has`: a timer
/// running for an hour and five minutes, and a week of entries, two with a task as long as real ones run.
const CYCLOPS: &str = r#"
globalThis.__shot = (() => {
  const now = Date.now(), minute = 60e3, hour = 60 * minute;
  const iso = (t) => new Date(t).toISOString();
  const pad = (n) => String(n).padStart(2, '0');
  const local = (t) => { const d = new Date(t); return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`; };
  const at = (days, h, m) => { const d = new Date(now - days * 24 * hour); d.setHours(h, m, 0, 0); return d.getTime(); };
  const projects = [{ id: 'p1', name: 'Wings', clientName: 'Prototyp' }, { id: 'p2', name: 'Website', clientName: 'Acme' }];
  const timers = [{ id: 't1', projectId: 'p1', task: 'Native plugin popovers', startedAt: iso(now - 65 * minute), endedAt: null, entryDate: local(now) }];
  const entry = (id, project, task, days, h, m, minutes) => {
    const start = at(days, h, m);
    return { id, projectId: project, task, startedAt: iso(start), endedAt: iso(start + minutes * minute), entryDate: local(start), durationMinutes: minutes };
  };
  const entries = [
    entry('e1', 'p1', 'Code review', 0, 8, 30, 45),
    entry('e2', 'p2', 'Checkout redesign', 1, 9, 0, 210),
    entry('e3', 'p1', 'Sidebar labels', 1, 13, 30, 150),
    entry('e4', 'p2', 'Standup', 2, 9, 30, 15),
    entry('e5', 'p1', 'Password fields', 2, 10, 0, 240),
    entry('e6', 'p2', 'Report table keeps the "Unassigned" group last when sorted ascending', 0, 7, 30, 60),
    entry('e7', 'p2', 'FE: keep the search results cache in memory only (web and mobile)', 1, 7, 0, 120),
  ];
  const answer = (body) => ({ status: 200, contentType: 'application/json', body: JSON.stringify(body) });
  return {
    has: async () => true,
    fetch: async (url, options = {}) => {
      const path = String(url).replace('https://cyclops-api.prototyp.digital/api', '');
      if ((options.method ?? 'GET') !== 'GET') return answer({});
      if (path.startsWith('/auth/me')) return answer({ name: 'Ada Lovelace', timezone: 'Europe/Zagreb' });
      if (path.startsWith('/projects')) return answer(projects);
      if (path.startsWith('/time-entries/timers')) return answer(timers);
      if (path.startsWith('/time-entries')) return answer(entries);
      return answer({});
    },
  };
})();
"#;

/// Made-up Claude Code accounts in place of `wings.claudeAccounts`, with the project on screen on the work account; the
/// calls that would change an account, or start a terminal in one, do nothing.
const ACCOUNTS: &str = r#"
globalThis.__shot = {
  claudeAccounts: async () => ({
    accounts: [
      { name: null, dir: '/Users/ada/.claude', email: 'ada@example.com', signedIn: true, authMethod: 'claude.ai', sharesSetup: false },
      { name: 'work', dir: '/Users/ada/.claude-work', email: 'ada@acme.example', signedIn: true, authMethod: 'claude.ai', sharesSetup: true },
      { name: 'client-a', dir: '/Users/ada/.claude-client-a', email: null, signedIn: false, authMethod: 'none', sharesSetup: true },
    ],
    projects: Object.fromEntries([[__SHOT_PROJECT__, 'work']]),
  }),
  setClaudeAccount: async () => null,
  shareClaudeSetup: async () => null,
  openPane: async () => ({ paneId: '0' }),
};
"#;

/// Sprint board's main for the shots: its sidebar's `state` and `board` queries answered with a made-up sprint (the
/// repo's `preview/sidebar.tsx`), and its label, as its real main sets it.
const SPRINT_MAIN: &str = r##"
const now = new Date().toISOString();
const settings = { org: 'https://dev.azure.com/example', project: 'Web', team: 'Web Team', areaPath: '', members: ['Ada Lovelace'],
  storyTypes: ['User Story', 'Product Backlog Item'], scopeChildTypes: ['Task'], childTypes: ['Task', 'Bug'], itemTypes: ['Bug'], titlePrefixes: [],
  codeReviewStates: ['Code Review', 'In Code Review'], blockedByField: '', goalTag: '', addedTags: [], pointTypes: [],
  pointFields: ['Microsoft.VSTS.Scheduling.StoryPoints'], parkedPrPrefix: '', staleDays: 3 };
const color = { Story: '#009ccc', Task: '#f2cb1d', Bug: '#cc293d' };
const workflow = (type) => ({ color: color[type], states: [
  { name: 'New', category: 'Proposed', color: '#b2b2b2' },
  { name: 'Active', category: 'InProgress', color: '#007acc' },
  { name: 'Closed', category: 'Completed', color: '#339933' },
] });
const item = (id, title, type, state, category, extra = {}) => ({ id, rev: 1, title, type, state, category, assignee: 'Ada Lovelace',
  login: 'ada@example.com', parent: null, tags: [], blocked: false, blockedBy: null, comments: 0, prs: null, checks: [], ...extra });
const snapshot = {
  checkedAt: now,
  sprint: { name: 'Sprint 12', start: '2026-10-05T00:00:00Z', finish: '2026-10-16T00:00:00Z' },
  items: [
    item(101, 'Sign-in page', 'Story', 'Active', 'InProgress'),
    item(102, 'Build the form', 'Task', 'Active', 'InProgress', { parent: 101, comments: 2, prs: [{ id: 7, status: 'active' }] }),
    item(103, 'Wire up the API', 'Task', 'New', 'Proposed', { parent: 101, blocked: true, blockedBy: '#150', checks: ['Blocked By resolved'] }),
    item(104, 'Fix the logout redirect', 'Bug', 'Closed', 'Completed', { assignee: null, login: null }),
    item(105, 'Password reset', 'Story', 'New', 'Proposed', { assignee: null, login: null }),
  ],
  types: { Story: workflow('Story'), Task: workflow('Task'), Bug: workflow('Bug') },
};
const result = {
  sprint: snapshot.sprint, checkedAt: now,
  findings: [{ id: 103, title: 'Wire up the API', url: 'https://dev.azure.com/example/Web/_workitems/edit/103', check: 'Blocked By resolved',
    problem: '#150 in Blocked By is closed.', fix: 'Remove #150 from Blocked By.', dependency: false }],
  people: [{ name: 'Ada Lovelace', items: [], lowPriority: null }],
  goalTag: null, notCovered: [], stats: { azCalls: 12, ms: 3000, board: 5, scoped: 3, dependencies: 0, pullRequests: 1 },
};
const phase = __SPRINT_PHASE__;
const state = phase === 'ready'
  ? { phase, settings, missing: [], result, error: null, running: false }
  : { phase, settings: { ...settings, org: '', project: '', team: '' }, missing: ['org', 'project', 'team'], result: null, error: null, running: false };
const answer = (name) => (name === 'state' ? state : phase === 'ready' ? snapshot : null);
wings.onBroadcast((m) => {
  if (m?.['wings:sdk'] !== 1 || m.kind !== 'sub') return;
  for (const [name, key] of m.subs) void wings.broadcast({ 'wings:sdk': 1, kind: 'state', name, key, data: answer(name), at: Date.now() });
});
if (phase === 'ready') wings.setSidebarLabel('board', '2 in progress · 1 blocked', { tone: 'warning', rows: [
  { label: 'Sprint', value: 'Sprint 12' }, { label: 'Board checks', value: '1 finding' }, { label: 'Ada Lovelace', value: '3 open, 1 blocked' }] });
"##;

/// `source` with each of `calls` (`wings.fetch`) going to the made-up `__shot` object of `prelude` instead.
fn swapped(prelude: &str, source: &str, calls: &[(&str, &str)]) -> String {
    let mut source = source.to_owned();
    for (call, fake) in calls {
        source = source.replace(&format!("wings.{call}("), &format!("__shot.{fake}("));
    }
    format!("{prelude}\n{source}")
}

fn read(path: PathBuf) -> String {
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

/// A sidebar or popover state: its name in the PNG, the buttons to press on the page to reach it (by text or by an icon
/// button's name), and a text it shows once there.
struct State {
    name: &'static str,
    popover: bool,
    presses: &'static [&'static str],
    shows: &'static str,
    /// Typed into the field that has the keyboard once there.
    typed: Option<&'static str>,
}

const fn sidebar(
    name: &'static str,
    presses: &'static [&'static str],
    shows: &'static str,
) -> State {
    State {
        name,
        popover: false,
        presses,
        shows,
        typed: None,
    }
}

const fn popover(name: &'static str, shows: &'static str) -> State {
    State {
        name,
        popover: true,
        presses: &[],
        shows,
        typed: None,
    }
}

pub(super) fn external(out: PathBuf, root: PathBuf) {
    // An `az` that fails, first on PATH, and no login shell to find another: Sprint board's setup never reaches Azure.
    let bin = out.join("fake-az-external");
    std::fs::create_dir_all(&bin).unwrap();
    let az = bin.join("az");
    std::fs::write(
        &az,
        "#!/bin/sh\necho \"Please run 'az login' to setup account.\" >&2\nexit 1\n",
    )
    .unwrap();
    std::process::Command::new("chmod")
        .arg("+x")
        .arg(&az)
        .status()
        .unwrap();
    unsafe {
        std::env::set_var("SHELL", "/nonexistent-shell");
        std::env::set_var(
            "PATH",
            format!(
                "{}:{}",
                bin.display(),
                std::env::var("PATH").unwrap_or_default()
            ),
        );
    }
    let mut cx = app();

    let cyclops = root.join("wings-cyclops");
    let fake_cyclops = |page: &str| {
        swapped(
            CYCLOPS,
            &read(cyclops.join(format!("dist/native/{page}.js"))),
            &[("fetch", "fetch"), ("secrets.has", "has")],
        )
    };
    shoot(
        &mut cx,
        &out,
        &cyclops,
        "cyclops",
        "tracker",
        &fake_cyclops("main"),
        &[
            ("sidebar", fake_cyclops("sidebar")),
            ("popover", fake_cyclops("popover")),
        ],
        &[
            sidebar("running", &[], "Native plugin popovers"),
            // The recent tasks that match what is typed in the task field, under it.
            State {
                typed: Some("re"),
                ..sidebar("suggest", &[], "Native plugin popovers")
            },
            sidebar("edit", &["Edit Code review"], "Save"),
            popover("popover", "Native plugin popovers"),
        ],
    );
    // Not connected yet: the API token field, with a token typed in, which shows as dots once the bundle is built with
    // a wings-plugin-ui that hands `type="password"` on.
    let signed_out = CYCLOPS.replace("has: async () => true", "has: async () => false");
    let signed_out = |page: &str| {
        swapped(
            &signed_out,
            &read(cyclops.join(format!("dist/native/{page}.js"))),
            &[("fetch", "fetch"), ("secrets.has", "has")],
        )
    };
    shoot(
        &mut cx,
        &out,
        &cyclops,
        "cyclops",
        "tracker",
        &signed_out("main"),
        &[
            ("sidebar", signed_out("sidebar")),
            ("popover", signed_out("popover")),
        ],
        &[State {
            typed: Some("cyc_live_token_1234"),
            ..sidebar("connect", &[], "API token")
        }],
    );

    let accounts = root.join("wings-claude-accounts");
    let project = out.join("external-projects").join("web");
    std::fs::create_dir_all(&project).unwrap();
    let prelude = ACCOUNTS.replace(
        "__SHOT_PROJECT__",
        &serde_json::to_string(&project).unwrap(),
    );
    let fake_accounts = |page: &str| {
        swapped(
            &prelude,
            &read(accounts.join(format!("dist/native/{page}.js"))),
            &[
                ("claudeAccounts", "claudeAccounts"),
                ("setClaudeAccount", "setClaudeAccount"),
                ("shareClaudeSetup", "shareClaudeSetup"),
                ("openPane", "openPane"),
            ],
        )
    };
    shoot_in(
        &mut cx,
        &out,
        &accounts,
        "claude-accounts",
        "accounts",
        &fake_accounts("main"),
        &[
            ("sidebar", fake_accounts("sidebar")),
            ("popover", fake_accounts("popover")),
        ],
        &[
            sidebar("project", &[], "ada@acme.example"),
            sidebar("add", &["Add account"], "Account name"),
            popover("popover", "Start Claude as"),
        ],
        &project,
    );

    let board = root.join("wings-sprint-board");
    let page = read(board.join("dist/native/sidebar.js"));
    for (phase, states) in [
        (
            "ready",
            &[
                sidebar("board", &[], "Sign-in page"),
                sidebar("item", &["Password reset"], "Couldn't read the item"),
                sidebar("checks", &["Checks · 1"], "Remove #150 from Blocked By."),
            ][..],
        ),
        ("setup", &[sidebar("setup", &[], "")][..]),
    ] {
        shoot(
            &mut cx,
            &out,
            &board,
            &format!("sprint-board-{phase}"),
            "board",
            &SPRINT_MAIN.replace("__SPRINT_PHASE__", &format!("'{phase}'")),
            &[("sidebar", page.clone())],
            states,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn shoot(
    cx: &mut VisualTestAppContext,
    out: &Path,
    repo: &Path,
    name: &str,
    sidebar_id: &str,
    main: &str,
    pages: &[(&str, String)],
    states: &[State],
) {
    let project = out.join("external-projects").join("web");
    std::fs::create_dir_all(&project).unwrap();
    shoot_in(
        cx, out, repo, name, sidebar_id, main, pages, states, &project,
    );
}

#[allow(clippy::too_many_arguments)]
fn shoot_in(
    cx: &mut VisualTestAppContext,
    out: &Path,
    repo: &Path,
    name: &str,
    sidebar_id: &str,
    main: &str,
    pages: &[(&str, String)],
    states: &[State],
    project: &Path,
) {
    let mut manifest: serde_json::Value =
        serde_json::from_str(&read(repo.join("wings-plugin.json"))).unwrap();
    // Claude accounts asks for 0.1.123, and this branch's desktop, whose version the native app takes, is older.
    manifest.as_object_mut().unwrap().remove("minWingsVersion");
    let id = manifest["id"].as_str().unwrap().to_owned();
    for width in [1000.0, 640.0] {
        for state in states {
            // A fresh install for each, so a state starts from the plugin's first page.
            let data = Data::temp();
            let pages: Vec<(&str, &str)> = pages
                .iter()
                .map(|(stem, source)| (*stem, source.as_str()))
                .collect();
            data.install_script(&id, manifest.clone(), main, &pages, "shot");
            let args = ["--project".into(), project.as_os_str().to_owned()];
            let mut config = Config::parse(args, project).unwrap().unwrap();
            config.state = None;
            config.projects = None;
            config.agents = None;
            config.plugins = Some(data.paths());
            let handle: WindowHandle<Workspace> = cx
                .open_offscreen_window(size(px(width), px(640.0)), move |window, cx| {
                    cx.new(|cx| Workspace::new(config, window, cx))
                })
                .unwrap();
            let window: AnyWindowHandle = handle.into();
            wait(cx, window, Duration::from_millis(500));
            let page: Entity<PageView> = if state.popover {
                handle
                    .update(cx, |w, _, cx| w.shot_popover(&id, sidebar_id, cx))
                    .unwrap();
                wait(cx, window, Duration::from_millis(200));
                handle
                    .read_with(cx, |w, _| w.popover_page(&id, sidebar_id))
                    .unwrap()
                    .expect("a popover page")
            } else {
                handle
                    .update(cx, |w, _, cx| w.shot_sidebar(&id, sidebar_id, cx))
                    .unwrap();
                wait(cx, window, Duration::from_millis(200));
                handle
                    .read_with(cx, |w, _| w.sidebar_page(&id, sidebar_id))
                    .unwrap()
                    .expect("a sidebar page")
            };
            let shows = |cx: &mut VisualTestAppContext, text: &str| {
                let deadline = Instant::now() + Duration::from_secs(10);
                loop {
                    let shown = cx.read(|cx| page.read(cx).text());
                    if shown.contains(text) {
                        return;
                    }
                    if Instant::now() > deadline {
                        eprintln!("{name} {} never showed {text:?}: {shown}", state.name);
                        return;
                    }
                    wait(cx, window, Duration::from_millis(50));
                }
            };
            for press in state.presses {
                let deadline = Instant::now() + Duration::from_secs(10);
                // A button by its text or name, else a chip, a list row or a row that opens.
                let pressed = |cx: &mut VisualTestAppContext| {
                    cx.read(|cx| {
                        let page = page.read(cx);
                        page.press(press, 0)
                            || page.press_chip(press)
                            || page.press_list_button(press)
                            || page.press_row(press, 0)
                    })
                };
                while !pressed(cx) {
                    if Instant::now() > deadline {
                        eprintln!(
                            "{name} {}: nothing to press for {press}: {}",
                            state.name,
                            cx.read(|cx| page.read(cx).text())
                        );
                        break;
                    }
                    wait(cx, window, Duration::from_millis(50));
                }
                wait(cx, window, Duration::from_millis(200));
            }
            shows(cx, state.shows);
            if let Some(text) = state.typed {
                let focus = cx.read(|cx| page.read(cx).first_input_focus(cx));
                if let Some(focus) = focus {
                    let _ = cx.update_window(window, |_, window, cx| focus.focus(window, cx));
                }
                cx.simulate_input(window, text);
            }
            wait(cx, window, Duration::from_millis(400));
            save(
                cx,
                window,
                out.join(format!("{name}-{}-{width}.png", state.name)),
            );
            let _ = cx.update_window(window, |_, window, _| window.remove_window());
        }
    }
}
