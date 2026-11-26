//! PR tracker over a fake `gh` and `az`.
use super::*;

/// A page, what it opens with, what to press to reach a view of it, and a text it shows.
pub(super) type View<'a> = (
    &'a str,
    &'a str,
    Option<&'a serde_json::Value>,
    wings_native::page::plugin::StatePlace,
    &'a [&'a str],
    &'a str,
);

/// The fake `gh` and `az` of PR tracker's shots: scripts that answer from `shot_plugins/pr_tracker_gh.json` and
/// `pr_tracker_az.json` by the words in the call, and act on the word in the file `mode` beside them: `slow` waits before
/// it answers, `error` fails as a forge that is down does, `signed-out` fails as a program with no sign-in does. A project
/// whose repository is called `empty` has no pull requests.
pub(super) fn pr_tracker_programs(bin: &std::path::Path) {
    std::fs::create_dir_all(bin).unwrap();
    for (program, answers, signed_out) in [
        ("gh", include_str!("pr_tracker_gh.json"), "To get started with GitHub CLI, please run:  gh auth login"),
        ("az", include_str!("pr_tracker_az.json"), "ERROR: Please run 'az login' to setup account."),
    ] {
        let answers: serde_json::Map<String, serde_json::Value> = serde_json::from_str(answers).unwrap();
        let mut script = format!(
            "#!/bin/sh\nmode=$(cat \"$(dirname \"$0\")/mode\" 2>/dev/null)\ncase \"$mode\" in\n  signed-out) echo '{signed_out}' >&2; exit 1 ;;\n  error) echo 'HTTP 502: Bad Gateway' >&2; exit 1 ;;\n  slow) sleep 6 ;;\nesac\ncase \"$*\" in\n"
        );
        if program == "gh" {
            script += "  *'name: \"empty\"'*) echo '{\"data\":{\"r0\":{\"pullRequests\":{\"nodes\":[]}},\"asked\":{\"nodes\":[]}}}' ;;\n";
        }
        for (index, (words, answer)) in answers.iter().filter(|(words, _)| !words.starts_with('_')).enumerate() {
            let file = bin.join(format!("{program}-{index}.out"));
            std::fs::write(&file, answer.as_str().map_or_else(|| answer.to_string(), str::to_owned)).unwrap();
            script += &format!("  *'{words}'*) cat '{}' ;;\n", file.display());
        }
        script += "  *) echo \"no made-up answer for $*\" >&2; exit 1 ;;\nesac\n";
        let path = bin.join(program);
        std::fs::write(&path, script).unwrap();
        std::process::Command::new("chmod").arg("+x").arg(&path).status().unwrap();
    }
    // Programs are found on the login shell's PATH, then this process's: no login shell leaves this one.
    unsafe {
        std::env::set_var("SHELL", "/nonexistent-shell");
        std::env::set_var("PATH", format!("{}:{}", bin.display(), std::env::var("PATH").unwrap_or_default()));
    }
}

/// A folder that is a git repo on `branch` whose origin is `origin`, which is how a page knows which forge to ask.
pub(super) fn pr_tracker_repo(folder: &std::path::Path, origin: &str, branch: &str) {
    std::fs::create_dir_all(folder).unwrap();
    let git = |args: &[&str]| {
        let done = std::process::Command::new("git").args(args).current_dir(folder).output().unwrap();
        assert!(done.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&done.stderr));
    };
    git(&["init", "-q"]);
    let _ = std::process::Command::new("git").args(["remote", "remove", "origin"]).current_dir(folder).output();
    git(&["remote", "add", "origin", origin]);
    git(&["-c", "user.email=shot@example.com", "-c", "user.name=Shot", "commit", "-q", "--allow-empty", "-m", "fixture"]);
    git(&["branch", "-M", branch]);
}

/// PR tracker's Rust modules over a fake `gh` and `az`: the project's list (the Pull requests block, and the picker a tab
/// with no pull request shows), a pull request opened from it with its description and conversation, its Checks and
/// Files tabs, a check's run, the same pull request on Azure DevOps with its pushes and its policies as checks, the
/// settings, and the states a host can be in: loading, an error, a CLI that is signed out and a project with no remote. The
/// pages run the forges themselves and ask the main script, which runs here too, for the lists.
pub(super) fn pr_tracker(out: PathBuf) {
    use wings_native::page::plugin::StatePlace::{Block, Tab};
    let bin = out.join("fake-pr-programs");
    pr_tracker_programs(&bin);
    let mode = |mode: &str| std::fs::write(bin.join("mode"), mode).unwrap();
    mode("");
    let (host, project) = repo_host(&out, "pr-tracker", include_str!("../../../../plugins/pr-tracker/wings-plugin.json"));
    pr_tracker_repo(std::path::Path::new(&project), "git@github.com:acme/web.git", "ada/sync-retry");
    let empty = out.join("fixture-project-pr-tracker-empty");
    pr_tracker_repo(&empty, "git@github.com:acme/empty.git", "main");
    let empty_path = empty.to_string_lossy().into_owned();
    host.publish_projects(vec![
        wings_native::page::plugin::Project { hidden_sections: Vec::new(), id: project.clone(), name: "web".into(), parent_id: None, path: project.clone() },
        wings_native::page::plugin::Project { hidden_sections: Vec::new(), id: empty_path.clone(), name: "empty".into(), parent_id: None, path: empty_path.clone() },
    ]);
    // The project on screen, which the app always says and the block and the picker follow.
    host.publish_project(Some(std::path::Path::new(&project)));
    let module = |frame: &str| {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../plugins/pr-tracker/dist/native/{frame}.wasm"));
        format!("{}{}", wings_native::page::runtime::wasm::MARKER, path.canonicalize().expect("sh build-rust.sh pr-tracker, in plugins/").display())
    };
    host.start_main(&module("main"), fail_on_error).unwrap();
    let (tab, block, settings) = (module("pr"), module("pulls"), module("settings"));
    let args = json!({
        "provider": "github", "url": "https://github.com/acme/web/pull/482", "number": 482,
        "repo": "acme/web", "title": "Add retry with backoff to the sync client", "cwd": project,
    });
    let azure = json!({
        "provider": "azure", "url": "https://dev.azure.com/acme/Web/_git/web/pullrequest/482", "number": 482,
        "repo": "Web/web", "title": "Add retry with backoff to the sync client", "cwd": project,
        "remote": { "kind": "azure", "org": "https://dev.azure.com/acme", "project": "Web", "repo": "web" },
    });
    // The page, what it opens with, what to press to reach each view, and a text it shows.
    let views: [View; 7] = [
        ("pull", &tab, Some(&args), Tab, &[], "Alan Turing"),
        ("checks", &tab, Some(&args), Tab, &["Checks"], "Unit tests"),
        ("files", &tab, Some(&args), Tab, &["Files"], "diff viewer"),
        ("azure", &tab, Some(&azure), Tab, &[], "Alan Turing"),
        ("azure-checks", &tab, Some(&azure), Tab, &["Checks"], "web-storybook"),
        ("run", &tab, Some(&args), Tab, &["Checks", "Unit tests"], "Install dependencies"),
        ("settings", &settings, None, Tab, &[], "Bitbucket Cloud"),
    ];
    let mut cx = app();
    let shoot = |cx: &mut VisualTestAppContext, name: &str, source: &str, args: Option<&serde_json::Value>, place, presses: &[&str], expected: &str| {
        for width in [1000.0, 640.0] {
            let state = wings_native::page::plugin::FrameState::new(place, None, |_| Ok(()));
            let handle = cx
                .open_offscreen_window(size(px(width), px(1500.0)), |_, cx| cx.new(|cx| PageView::in_plugin_with_state(&host, source, args, state, cx)))
                .unwrap();
            let window = handle.into();
            handle.read_with(cx, |view, _| view.set_visible(true)).unwrap();
            let until = |cx: &mut VisualTestAppContext, text: &str| {
                let deadline = Instant::now() + Duration::from_secs(30);
                while !handle.read_with(cx, |view, _| view.text().contains(text)).unwrap() {
                    assert!(Instant::now() < deadline, "pr-tracker {name} did not show {text:?}: {}", handle.read_with(cx, |view, _| view.text()).unwrap());
                    wait(cx, window, Duration::from_millis(20));
                }
            };
            if presses.is_empty() {
                until(cx, expected);
            }
            for (step, label) in presses.iter().enumerate() {
                let deadline = Instant::now() + Duration::from_secs(10);
                while !handle.read_with(cx, |view, _| if step == 0 { view.press_chip(label) } else { view.press_list_button(label) }).unwrap() {
                    assert!(Instant::now() < deadline, "nothing to press for {label}: {}", handle.read_with(cx, |view, _| view.text()).unwrap());
                    wait(cx, window, Duration::from_millis(20));
                }
                until(cx, if step + 1 == presses.len() { expected } else { "Unit tests" });
            }
            wait(cx, window, Duration::from_millis(300));
            save(cx, window, out.join(format!("pr-tracker-{name}-{width}.png")));
            let _ = cx.update_window(window, |_, window, _| window.remove_window());
        }
    };
    for (name, source, args, place, presses, expected) in views {
        shoot(&mut cx, name, source, args, place, presses, expected);
    }
    // What a host being slow, down or signed out looks like, and a project with no pull requests.
    mode("slow");
    shoot(&mut cx, "loading", &tab, Some(&args), Tab, &[], "");
    mode("error");
    shoot(&mut cx, "error", &tab, Some(&args), Tab, &[], "Couldn't load this pull request");
    mode("signed-out");
    shoot(&mut cx, "signed-out", &tab, Some(&args), Tab, &[], "Sign in");
    mode("");
    // Last, since they all read the project's list from main.
    shoot(&mut cx, "list", &block, None, Block, &[], "Migrate the settings page");
    shoot(&mut cx, "picker", &tab, None, Tab, &[], "Migrate the settings page");
    // A project whose folder the fixture's app doesn't hold: its remote can't be read.
    host.publish_project(Some(&empty));
    shoot(&mut cx, "no-remote", &block, None, Block, &[], "No ");
}
