//! PR tracker, over a fake `gh` and `az` that answer with made-up pull requests.
use super::*;

// PR tracker: its Rust modules, over a fake gh and az that answer with made-up pull requests. Each test runs in a process
// of its own where those are first on PATH, so no test reaches a forge even when the Mac has the real CLIs.

/// The made-up answers of the fake `gh` and `az`, by the words in the call.
const FAKE_PR_GH: &str = include_str!("../../../../examples/shot_plugins/pr_tracker_gh.json");
const FAKE_PR_AZ: &str = include_str!("../../../../examples/shot_plugins/pr_tracker_az.json");
/// Set in the process where `gh` and `az` are the fakes.
const FAKE_PR: &str = "WINGS_TEST_FAKE_PR";

/// The folder of the fake `gh` and `az`: scripts that print the answer of the first key of their answers that the call has
/// the words of (a string as it is, anything else as JSON), log each call to the file `WINGS_FAKE_LOG` names when it is
/// set, and fail as a signed-out program does while `WINGS_FAKE_SIGNED_OUT` is set.
fn fake_pr_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("wings-fake-pr-{name}-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    for (program, answers, signed_out) in [
        ("gh", FAKE_PR_GH, "To get started with GitHub CLI, please run:  gh auth login"),
        ("az", FAKE_PR_AZ, "ERROR: Please run 'az login' to setup account."),
    ] {
        let answers: serde_json::Map<String, serde_json::Value> = serde_json::from_str(answers).unwrap();
        let mut script = format!(
            "#!/bin/sh\n[ -n \"$WINGS_FAKE_LOG\" ] && echo \"{program} $*\" >> \"$WINGS_FAKE_LOG\"\n[ -n \"$WINGS_FAKE_SIGNED_OUT\" ] && {{ echo '{signed_out}' >&2; exit 1; }}\ncase \"$*\" in\n"
        );
        for (index, (words, answer)) in answers.iter().filter(|(words, _)| !words.starts_with('_')).enumerate() {
            let file = dir.join(format!("{program}-{index}.out"));
            fs::write(&file, answer.as_str().map_or_else(|| answer.to_string(), str::to_owned)).unwrap();
            script += &format!("  *'{words}'*) cat '{}' ;;\n", file.display());
        }
        script += "  *) echo \"no made-up answer for the call\" >&2; exit 1 ;;\nesac\n";
        let path = dir.join(program);
        fs::write(&path, script).unwrap();
        assert!(Command::new("chmod").arg("+x").arg(&path).status().unwrap().success());
    }
    dir
}

/// Runs the `#[ignore]`d test `name` of this file in a process whose `gh` and `az` are the fakes (see `with_fake_az` for why
/// a process of its own), with `env` set in it.
fn with_fake_pr_env(name: &str, env: &[(&str, &str)]) {
    let dir = fake_pr_dir(name);
    let test = format!("{}::{name}", module_path!().split_once("::").unwrap().1);
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--ignored", "--exact", &test, "--nocapture"])
        .env(FAKE_PR, &dir)
        .env("WINGS_FAKE_LOG", dir.join("calls.log"))
        .env("SHELL", "/nonexistent-shell")
        .env("PATH", format!("{}:{}", dir.display(), std::env::var("PATH").unwrap_or_default()));
    for (key, value) in env {
        command.env(key, value);
    }
    let output = command.output().unwrap();
    let calls = fs::read_to_string(dir.join("calls.log")).unwrap_or_default();
    let _ = fs::remove_dir_all(&dir);
    let said = format!("{}\n{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    assert!(output.status.success() && said.contains("1 passed"), "{test}: {said}\nthe fake programs were called with:\n{calls}");
}

fn with_fake_pr(name: &str) {
    with_fake_pr_env(name, &[]);
}

/// PR tracker in a project that is a git repo on `ada/sync-retry` with a GitHub origin, with the fake `gh` and `az`.
fn pr_rig() -> (Rig, PathBuf) {
    assert!(std::env::var_os(FAKE_PR).is_some(), "run it through its test, which starts a process with the fake gh and az");
    let rig = Rig::with(load("pr-tracker"), true, vec![]);
    let repo = rig.root.config("pr-tracker").root.join("web");
    fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q"]);
    git(&repo, &["config", "user.email", "test@example.com"]);
    git(&repo, &["config", "user.name", "Wings test"]);
    git(&repo, &["remote", "add", "origin", "git@github.com:acme/web.git"]);
    git(&repo, &["commit", "-q", "--allow-empty", "-m", "fixture"]);
    git(&repo, &["branch", "-M", "ada/sync-retry"]);
    let path = repo.to_string_lossy().to_string();
    *rig.env.projects.lock().unwrap() = vec![repo.clone()];
    rig.host.publish_projects(vec![Project { id: path.clone(), path, name: "web".into(), parent_id: None, hidden_sections: Vec::new() }]);
    // The project on screen, which the app always says and the pages follow.
    rig.host.publish_project(Some(&repo));
    (rig, repo)
}

/// What a "pr" tab is opened with for the fixture's GitHub pull request.
fn pr_args(repo: &Path) -> serde_json::Value {
    json!({
        "provider": "github", "url": "https://github.com/acme/web/pull/482", "number": 482,
        "repo": "acme/web", "title": "Add retry with backoff to the sync client", "cwd": repo.to_string_lossy(),
    })
}

/// Opens the "pr" tab with `args` as the app does.
fn open_pr_tab(rig: &Rig, args: &serde_json::Value) -> (PageRuntime, mpsc::Receiver<Event>) {
    let (page, events, _saved) = open_with_state(&rig.host, &module("pr-tracker", "pr"), Some(args), super::super::StatePlace::Tab, None);
    (page, events)
}

#[test]
fn pr_tracker_tab_opened_with_args_shows_that_pull_request_and_its_checks() {
    with_fake_pr("pr_tracker_tab_opened_with_args_shows_that_pull_request_and_its_checks_with_the_fakes");
}

#[test]
#[ignore = "started by its test without the _with_the_fakes, in a process with a fake gh and az"]
fn pr_tracker_tab_opened_with_args_shows_that_pull_request_and_its_checks_with_the_fakes() {
    let (rig, repo) = pr_rig();
    let (page, events) = open_pr_tab(&rig, &pr_args(&repo));
    let pull = tree(&events, "the pull request", |tree| tree.text().contains("Alan Turing"));
    let text = pull.text();
    for shown in ["acme/web", "#482", "exponential backoff", "const delay = Math.min(base * 2 ** attempt, 30_000);", "Setting", "Equal jitter keeps a floor on the delay.", "ci-bot[bot]"] {
        assert!(text.contains(shown), "{shown:?} in {text:?}");
    }
    assert!(!text.contains("All pull requests"), "a tab opened on a pull request has no list to go back to");
    let tabs = chips_named(&pull.children, "Pull request").expect("the tabs");
    assert!(page.dispatch(&tabs, Some(&json!({ "value": "checks" }))));
    let checks = tree(&events, "the checks", |tree| tree.text().contains("Unit tests"));
    let text = checks.text();
    // `gh` doesn't say which checks branch protection requires, so there are no Required and Optional groups.
    for shown in ["Failed", "Unit tests", "Running and queued", "End-to-end", "Passed"] {
        assert!(text.contains(shown), "{shown:?} in {text:?}");
    }
}

/// The width of the page's reading column.
fn column_width(nodes: &[Node]) -> Option<f32> {
    nodes.iter().find_map(|node| match node {
        Node::Page(page) if page.column.is_some() => page.column,
        Node::Stack(stack) if stack.max_width.is_some() => stack.max_width,
        other => other.parts().into_iter().find_map(column_width),
    })
}

/// The name of the step a run shows the log of: the one step whose name is bold.
fn picked_step(nodes: &[Node]) -> Option<String> {
    use crate::page::tree::Weight;
    nodes.iter().find_map(|node| match node {
        Node::ListButton(row) => row.button.children.iter().find_map(|child| match child {
            Node::Text(name) if name.truncate && name.weight == Weight::Strong => {
                Some(plain_text(&name.children))
            }
            _ => None,
        }),
        other => other.parts().into_iter().find_map(picked_step),
    })
}

/// A check's run: its log's numbers are one column, each as many characters as the last one's, with a folded group's
/// number in that column and not in the button that opens the group; the page is wider than its reading column while
/// the run is open; and a step picked says it's loading at once. It went on showing the step before's log until its
/// own came, so a click seemed to do nothing for as long as the CLI took.
#[test]
fn pr_tracker_run_numbers_its_lines_in_a_column_widens_the_page_and_loads_a_picked_step_at_once() {
    with_fake_pr("pr_tracker_run_numbers_its_lines_in_a_column_widens_the_page_and_loads_a_picked_step_at_once_with_the_fakes");
}

#[test]
#[ignore = "started by its test without the _with_the_fakes, in a process with a fake gh and az"]
fn pr_tracker_run_numbers_its_lines_in_a_column_widens_the_page_and_loads_a_picked_step_at_once_with_the_fakes() {
    let (rig, repo) = pr_rig();
    let (page, events) = open_pr_tab(&rig, &pr_args(&repo));
    let pull = tree(&events, "the pull request", |tree| {
        tree.text().contains("Conversation")
    });
    let tabs = chips_named(&pull.children, "Pull request").expect("the tabs");
    assert!(page.dispatch(&tabs, Some(&json!({ "value": "checks" }))));
    let checks = tree(&events, "the checks", |tree| {
        tree.text().contains("Unit tests")
    });
    assert_eq!(column_width(&checks.children), Some(860.0));
    let open = list_button_showing(&checks.children, "Unit tests").expect("the failed check");
    assert!(page.dispatch(&open, None));
    let run = tree(&events, "the failed step's log", |tree| {
        tree.text().contains("Process completed with exit code 1.")
    });
    // An open run's log is a wide page: no reading column.
    assert_eq!(column_width(&run.children), None);

    fn numbers(nodes: &[Node], found: &mut Vec<String>) {
        for node in nodes {
            if let Node::Text(text) = node
                && text.mono
                && text.size == crate::page::tree::TextSize::Tiny
            {
                found.push(plain_text(&text.children));
            }
            for part in node.parts() {
                numbers(part, found);
            }
        }
    }
    let mut found = Vec::new();
    numbers(&run.children, &mut found);
    // 1 and 9 open folded groups; 2 and 10 to 103 are folded in them. The GitHub job's log has one more line after them
    // than the Azure DevOps log the fixture had before.
    let shown: Vec<String> = [1, 3, 4, 5, 6, 7, 8, 9]
        .into_iter()
        .chain(104..=125)
        .map(|n| format!("{n:>3}"))
        .collect();
    assert_eq!(found, shown);
    fn group_rows(nodes: &[Node], found: &mut Vec<(String, String)>) {
        for node in nodes {
            if let Node::Row(row) = node
                && let [Node::Text(number), Node::ListButton(button)] = row.children.as_slice()
            {
                found.push((
                    plain_text(&number.children),
                    plain_text(&button.button.children),
                ));
            }
            for part in node.parts() {
                group_rows(part, found);
            }
        }
    }
    let mut groups = Vec::new();
    group_rows(&run.children, &mut groups);
    assert_eq!(
        groups,
        [
            ("  1".to_owned(), "Run pnpm test".to_owned()),
            ("  9".to_owned(), "Install dependencies".to_owned())
        ]
    );

    assert_eq!(picked_step(&run.children).as_deref(), Some("Unit tests"));
    let install = list_button_showing(&run.children, "Install dependencies").expect("the step");
    assert!(page.dispatch(&install, None));
    let picked = tree(&events, "the step picked", |tree| {
        picked_step(&tree.children).as_deref() == Some("Install dependencies")
    });
    let text = picked.text();
    assert!(
        text.contains("Loading the log…") && !text.contains("Process completed with exit code 1."),
        "{text:?}"
    );
    tree(&events, "the picked step's log", |tree| {
        tree.text().contains("Packages: +812")
    });

    let mut back = Vec::new();
    buttons_named(&run.children, "← Checks", &mut back);
    assert!(page.dispatch(&back[0], None));
    let checks = tree(&events, "the checks again", |tree| {
        tree.text().contains("Running and queued")
    });
    assert_eq!(column_width(&checks.children), Some(860.0));
}

/// The Checks tab of the fixture's Azure DevOps pull request draws each check's mark as the desktop's: a red cross for
/// a failed one, an amber dot for one running, a ring for one queued. They were text glyphs, drawn thin. Every row
/// keeps an Open slot as wide as the button, so the times end in one column whether or not a row has a link.
#[test]
fn pr_tracker_checks_draw_each_checks_mark_and_line_their_times_up() {
    with_fake_pr("pr_tracker_checks_draw_each_checks_mark_and_line_their_times_up_with_the_fakes");
}

#[test]
#[ignore = "started by its test without the _with_the_fakes, in a process with a fake gh and az"]
fn pr_tracker_checks_draw_each_checks_mark_and_line_their_times_up_with_the_fakes() {
    let (rig, repo) = pr_rig();
    let args = json!({
        "provider": "azure", "url": "https://dev.azure.com/acme/Web/_git/web/pullrequest/482", "number": 482,
        "repo": "Web/web", "title": "Add retry with backoff to the sync client", "cwd": repo.to_string_lossy(),
        "remote": { "kind": "azure", "org": "https://dev.azure.com/acme", "project": "Web", "repo": "web" },
    });
    let (page, events) = open_pr_tab(&rig, &args);
    let pull = tree(&events, "the pull request", |tree| {
        tree.text().contains("Conversation")
    });
    let tabs = chips_named(&pull.children, "Pull request").expect("the tabs");
    assert!(page.dispatch(&tabs, Some(&json!({ "value": "checks" }))));
    let checks = tree(&events, "the checks", |tree| {
        tree.text().contains("web-storybook")
    });
    fn marks<'a>(nodes: &'a [Node], found: &mut Vec<&'a crate::page::tree::Svg>) {
        for node in nodes {
            match node {
                Node::Svg(mark)
                    if matches!(
                        mark.label.as_deref(),
                        Some("Failed" | "Running" | "Queued" | "Passed" | "Not run")
                    ) =>
                {
                    found.push(mark)
                }
                other => other
                    .parts()
                    .into_iter()
                    .for_each(|part| marks(part, found)),
            }
        }
    }
    let mut found = Vec::new();
    marks(&checks.children, &mut found);
    let shown: Vec<_> = found
        .iter()
        .map(|mark| (mark.label.as_deref().unwrap(), mark.tone))
        .collect();
    use crate::page::tree::Tone;
    // The open groups: the failed build, then what's running before what's queued.
    assert_eq!(
        shown,
        [
            ("Failed", Some(Tone::Danger)),
            ("Running", Some(Tone::Warn)),
            ("Running", Some(Tone::Warn)),
            ("Queued", Some(Tone::Muted)),
            ("Queued", Some(Tone::Muted)),
            ("Queued", Some(Tone::Muted)),
        ]
    );
    assert!(
        found[0]
            .markup
            .contains(r#"<path d="M3.5 3.5l9 9M12.5 3.5l-9 9"/>"#),
        "{}",
        found[0].markup
    );
    let slots = count_nodes(
        &checks.children,
        &|node| matches!(node, Node::Stack(slot) if slot.width == Some(52.0)),
    );
    assert_eq!(slots, found.len());
}

#[test]
fn pr_tracker_block_lists_the_projects_pull_requests_through_main_and_opens_one_in_a_tab() {
    with_fake_pr("pr_tracker_block_lists_the_projects_pull_requests_through_main_and_opens_one_in_a_tab_with_the_fakes");
}

#[test]
#[ignore = "started by its test without the _with_the_fakes, in a process with a fake gh and az"]
fn pr_tracker_block_lists_the_projects_pull_requests_through_main_and_opens_one_in_a_tab_with_the_fakes() {
    let (rig, _) = pr_rig();
    let (on_event, main_errors) = errors();
    rig.host.start_main(&module("pr-tracker", "main"), on_event).unwrap();
    let (page, events, _saved) = open_with_state(&rig.host, &module("pr-tracker", "pulls"), None, super::super::StatePlace::Block, None);
    let list = tree(&events, "the project's list", |tree| tree.text().contains("Migrate the settings page"));
    let text = list.text();
    for shown in ["Add retry with backoff to the sync client", "Fix the date picker on small screens", "Bump the build toolchain", "#482", "Review requested"] {
        assert!(text.contains(shown), "{shown:?} in {text:?}");
    }
    // A click on a row opens the pull request as a tab, with what the tab needs to find it again.
    let open = list_button_showing(&list.children, "Add retry with backoff").expect("a row");
    assert!(page.dispatch(&open, None));
    rig.env.wait("the pull request tab", |seen| {
        seen.iter().any(|seen| matches!(seen, Seen::OpenView(open) if open.id == "pr" && open.args.as_ref().is_some_and(|args| args["number"] == 482 && args["provider"] == "github")))
    });
    assert!(main_errors.try_recv().is_err(), "main reported {:?}", main_errors.try_recv());
}

#[test]
fn pr_tracker_main_badges_a_pane_with_its_pull_request_and_tells_claude_why_its_checks_failed() {
    with_fake_pr("pr_tracker_main_badges_a_pane_with_its_pull_request_and_tells_claude_why_its_checks_failed_with_the_fakes");
}

#[test]
#[ignore = "started by its test without the _with_the_fakes, in a process with a fake gh and az"]
fn pr_tracker_main_badges_a_pane_with_its_pull_request_and_tells_claude_why_its_checks_failed_with_the_fakes() {
    let (rig, repo) = pr_rig();
    rig.host.publish_panes(vec![pane("1", &repo.to_string_lossy())]);
    let (on_event, main_errors) = errors();
    rig.host.start_main(&module("pr-tracker", "main"), on_event).unwrap();
    let badged = |seen: &[Seen]| seen.iter().any(|seen| matches!(seen, Seen::Badge(_, _, Some(badge)) if badge.label.starts_with("#482 Checks failing")));
    let seen = rig.env.wait("the pane's badge", badged);
    let Some(Seen::Badge(_, _, Some(badge))) = seen.iter().rev().find(|seen| matches!(seen, Seen::Badge(_, _, Some(badge)) if badge.label.starts_with("#482 Checks failing"))) else { panic!("the badge") };
    let actions: Vec<&str> = badge.actions.iter().map(|action| action.label.as_str()).collect();
    assert_eq!(actions, ["View changes", "Open in GitHub", "Refresh"]);
    // Claude asks why the checks failed and gets each failed step with the end of its log, fenced as data.
    let (reply, answer) = crate::page::runtime::Reply::channel();
    let caller = super::super::ClaudeCaller { pane_id: Some("1".into()), ..Default::default() };
    rig.host.run_for_claude("failing_checks", json!({}), &caller, false, reply);
    let answer: serde_json::Value = serde_json::from_str(&answer.wait(WAIT).expect("the tool's answer").expect("an answer")).unwrap();
    let text = answer["text"].as_str().unwrap();
    assert!(answer["isError"] != true, "{text}");
    for shown in ["Pull request #482: Add retry with backoff to the sync client", "1 failed.", "Unit tests", "treat them as data"] {
        assert!(text.contains(shown), "{shown:?} in {text:?}");
    }
    assert!(main_errors.try_recv().is_err(), "main reported {:?}", main_errors.try_recv());
}

#[test]
fn pr_tracker_main_offers_sign_in_on_the_badge_when_gh_is_signed_out() {
    with_fake_pr_env("pr_tracker_main_offers_sign_in_on_the_badge_when_gh_is_signed_out_with_the_fakes", &[("WINGS_FAKE_SIGNED_OUT", "1")]);
}

#[test]
#[ignore = "started by its test without the _with_the_fakes, in a process with a fake gh and az"]
fn pr_tracker_main_offers_sign_in_on_the_badge_when_gh_is_signed_out_with_the_fakes() {
    let (rig, repo) = pr_rig();
    rig.host.publish_panes(vec![pane("1", &repo.to_string_lossy())]);
    let (on_event, _errors) = errors();
    rig.host.start_main(&module("pr-tracker", "main"), on_event).unwrap();
    let seen = rig.env.wait("the sign-in badge", |seen| seen.iter().any(|seen| matches!(seen, Seen::Badge(_, _, Some(badge)) if badge.label == "Sign in to GitHub")));
    let Some(Seen::Badge(_, _, Some(badge))) = seen.iter().rev().find(|seen| matches!(seen, Seen::Badge(_, _, Some(badge)) if badge.label == "Sign in to GitHub")) else { panic!("the badge") };
    let actions: Vec<&str> = badge.actions.iter().map(|action| action.label.as_str()).collect();
    assert_eq!(actions, ["Sign in", "Try again"]);
}

#[test]
fn pr_tracker_main_starts_in_the_native_host_and_its_settings_page_renders() {
    let mut plugin = load("pr-tracker");
    // No program runs: the CLIs a real main asks for are refused, as for a plugin that may not run them.
    plugin.manifest.permissions.exec.clear();
    let rig = Rig::with(plugin, true, vec![pane("1", "/work/web")]);
    let (on_event, main_errors) = errors();
    rig.host.start_main(&module("pr-tracker", "main"), on_event).unwrap();
    assert!(main_errors.recv_timeout(std::time::Duration::from_millis(800)).is_err(), "main reported an error");
    let (_page, events) = open_page(&rig.host, &module("pr-tracker", "settings"));
    let settings = tree(&events, "the settings", |tree| {
        // The token rows fill in once `secret_has` answers, a turn after the first tree.
        tree.text().contains("Codeberg") && tree.text().contains("No token saved.")
    });
    let text = settings.text();
    for shown in ["Sidebar", "New pull requests", "Pull request hosts", "No token saved."] {
        assert!(text.contains(shown), "{shown:?} in {text:?}");
    }
}
