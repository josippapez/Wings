//! Work item, over a fake `az`.
use super::*;

/// The made-up answers of the fake `az`, by the words in the call.
const FAKE_AZ_ANSWERS: &str = include_str!("../../../../examples/shot_plugins/work_item_az.json");
/// Set in the process where `az` is the fake, which the work item tests below run in.
const FAKE_AZ: &str = "WINGS_TEST_FAKE_AZ";

/// The folder of the fake `az`: a script that prints the answer of the first key of `FAKE_AZ_ANSWERS` its call has the
/// words of.
fn fake_az_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("wings-fake-az-{name}-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let answers: serde_json::Map<String, serde_json::Value> = serde_json::from_str(FAKE_AZ_ANSWERS).unwrap();
    let mut script = String::from("#!/bin/sh\ncase \"$*\" in\n");
    for (index, (words, answer)) in answers.iter().filter(|(words, _)| !words.starts_with('_')).enumerate() {
        let file = dir.join(format!("{index}.json"));
        fs::write(&file, answer.to_string()).unwrap();
        script += &format!("  *'{words}'*) cat '{}' ;;\n", file.display());
    }
    script += "  *) echo \"no made-up answer for az $*\" >&2; exit 1 ;;\nesac\n";
    let az = dir.join("az");
    fs::write(&az, script).unwrap();
    assert!(Command::new("chmod").arg("+x").arg(&az).status().unwrap().success());
    dir
}

/// Runs the `#[ignore]`d test `name` of this file in a process whose `az` is the fake. Programs are found on the login
/// shell's PATH first, which is asked once for the whole run, and the Mac running the tests may have the real `az`
/// there, whose calls would reach Azure DevOps. A process of its own has no login shell to ask and the fake first on
/// its PATH.
fn with_fake_az(name: &str) {
    let dir = fake_az_dir(name);
    let test = format!("{}::{name}", module_path!().split_once("::").unwrap().1);
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--ignored", "--exact", &test, "--nocapture"])
        .env(FAKE_AZ, &dir)
        .env("SHELL", "/nonexistent-shell")
        .env("PATH", format!("{}:{}", dir.display(), std::env::var("PATH").unwrap_or_default()))
        .output()
        .unwrap();
    let _ = fs::remove_dir_all(&dir);
    let said = format!("{}\n{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    assert!(output.status.success() && said.contains("1 passed"), "{test}: {said}");
}

/// Work item in a project that is a git repo on `feature/1432-sync` with an Azure DevOps origin, with the fake `az`.
fn work_item_rig() -> (Rig, PathBuf) {
    assert!(std::env::var_os(FAKE_AZ).is_some(), "run it through its test, which starts a process with the fake az");
    let rig = Rig::with(load("work-item"), true, vec![]);
    let repo = rig.root.config("work-item").root.join("web");
    fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q"]);
    git(&repo, &["config", "user.email", "test@example.com"]);
    git(&repo, &["config", "user.name", "Wings test"]);
    git(&repo, &["remote", "add", "origin", "git@ssh.dev.azure.com:v3/acme/Web/web"]);
    git(&repo, &["commit", "-q", "--allow-empty", "-m", "fixture"]);
    git(&repo, &["branch", "-M", "feature/1432-sync"]);
    let path = repo.to_string_lossy().to_string();
    *rig.env.projects.lock().unwrap() = vec![repo.clone()];
    rig.host.publish_projects(vec![Project { id: path.clone(), path, name: "web".into(), parent_id: None, hidden_sections: Vec::new() }]);
    // The project on screen, which the app always says and the pages follow.
    rig.host.publish_project(Some(&repo));
    (rig, repo)
}

#[test]
fn work_item_badges_a_pane_on_its_branch_and_lists_the_sprints_items_under_its_project() {
    with_fake_az("work_item_badges_a_pane_on_its_branch_and_lists_the_sprints_items_under_its_project_with_the_fake_az");
}

#[test]
#[ignore = "started by its test without the _with_the_fake_az, in a process with a fake az"]
fn work_item_badges_a_pane_on_its_branch_and_lists_the_sprints_items_under_its_project_with_the_fake_az() {
    let (rig, repo) = work_item_rig();
    rig.host.publish_panes(vec![pane("1", &repo.to_string_lossy())]);
    let (on_event, main_errors) = errors();
    rig.host.start_main(&module("work-item", "main"), on_event).unwrap();
    // The pane's branch names work item 1432: its badge has its state and the next state to move to.
    let badge = rig.env.wait("the pane's badge", |seen| seen.iter().any(|seen| matches!(seen, Seen::Badge(_, _, Some(badge)) if badge.label == "#1432 Active")));
    let Some(Seen::Badge(_, _, Some(badge))) = badge.iter().rev().find(|seen| matches!(seen, Seen::Badge(_, _, Some(badge)) if badge.label == "#1432 Active")) else { panic!("the badge") };
    let actions: Vec<&str> = badge.actions.iter().map(|action| action.label.as_str()).collect();
    // The app keeps the first four actions of a badge, so the second next state doesn't show.
    assert_eq!(actions, ["Open details", "Open in Azure DevOps", "Refresh", "Move to Resolved"]);
    // Open details opens the tab with the args every other way of opening this work item gives.
    action(&rig, "1", "details").unwrap();
    let args = json!({ "id": 1432, "org": "https://dev.azure.com/acme" });
    rig.env.wait("the item tab", |seen| seen.iter().any(|seen| matches!(seen, Seen::OpenView(open) if open.id == "item" && open.args.as_ref() == Some(&args))));
    // The unfinished items assigned to you in the sprint running now, under the project: Closed is left out.
    let rows = rig.env.wait("the work items section", |seen| seen.iter().any(|seen| matches!(seen, Seen::ProjectSection(_, _, section, Some(rows)) if section == "work-items" && !rows.is_empty())));
    let Some(Seen::ProjectSection(_, _, _, Some(rows))) = rows.iter().rev().find(|seen| matches!(seen, Seen::ProjectSection(_, _, _, Some(rows)) if !rows.is_empty())) else { panic!("the rows") };
    assert_eq!(rows.iter().map(|row| row.id.as_str()).collect::<Vec<_>>(), ["1432", "1433"]);
    assert_eq!(rows[0].detail.as_deref(), Some("Bug #1432"));
    assert_eq!(rows[0].args.as_ref(), Some(&args));
    assert!(main_errors.try_recv().is_err());
}

#[test]
fn work_item_sprint_block_draws_a_column_per_state_and_saves_its_filter_as_block_state() {
    with_fake_az("work_item_sprint_block_draws_a_column_per_state_and_saves_its_filter_as_block_state_with_the_fake_az");
}

#[test]
#[ignore = "started by its test without the _with_the_fake_az, in a process with a fake az"]
fn work_item_sprint_block_draws_a_column_per_state_and_saves_its_filter_as_block_state_with_the_fake_az() {
    let (rig, _) = work_item_rig();
    let (page, events, saved) = open_with_state(&rig.host, &module("work-item", "sprint"), None, super::super::StatePlace::Block, None);
    let mine = tree(&events, "the sprint", |tree| tree.text().contains("Add retry with backoff"));
    assert_eq!(chips_value(&mine.children, "Show").as_deref(), Some("mine"));
    assert!(!mine.text().contains("Show a notice while retrying"));
    assert_eq!(count_nodes(&mine.children, &|node| matches!(node, Node::BoardColumn(_))), 2, "Active and Closed: {}", mine.text());

    let show = chips_named(&mine.children, "Show").unwrap();
    assert!(page.dispatch(&show, Some(&json!({ "value": "all" }))));
    tree(&events, "the whole sprint", |tree| tree.text().contains("Show a notice while retrying"));
    assert_eq!(*saved.lock().unwrap(), [json!({ "mine": false })]);
}

#[test]
fn work_item_sprint_block_starts_from_its_saved_filter() {
    with_fake_az("work_item_sprint_block_starts_from_its_saved_filter_with_the_fake_az");
}

#[test]
#[ignore = "started by its test without the _with_the_fake_az, in a process with a fake az"]
fn work_item_sprint_block_starts_from_its_saved_filter_with_the_fake_az() {
    let (rig, _) = work_item_rig();
    let (_page, events, _saved) = open_with_state(&rig.host, &module("work-item", "sprint"), None, super::super::StatePlace::Block, Some(json!({ "mine": false })));
    let all = tree(&events, "the whole sprint", |tree| tree.text().contains("Show a notice while retrying"));
    assert_eq!(chips_value(&all.children, "Show").as_deref(), Some("all"));
}

#[test]
fn work_item_tab_shows_the_work_item_its_links_and_history_each_from_its_own_az_call() {
    with_fake_az("work_item_tab_shows_the_work_item_its_links_and_history_each_from_its_own_az_call_with_the_fake_az");
}

#[test]
#[ignore = "started by its test without the _with_the_fake_az, in a process with a fake az"]
fn work_item_tab_shows_the_work_item_its_links_and_history_each_from_its_own_az_call_with_the_fake_az() {
    let (rig, _) = work_item_rig();
    let args = json!({ "id": 1432, "org": "https://dev.azure.com/acme" });
    let (page, events, _saved) = open_with_state(&rig.host, &module("work-item", "item"), Some(&args), super::super::StatePlace::Tab, None);
    let overview = tree(&events, "the overview", |tree| tree.text().contains("Pull request is up") && tree.text().contains("Child of #1400 Make the sync client reliable"));
    let text = overview.text();
    for shown in ["Bug", "#1432", "Sync client gives up after the first failed request", "Active", "Ada Lovelace", "When the server answers"] {
        assert!(text.contains(shown), "{shown:?} in {text:?}");
    }
    let tabs = chips_named(&overview.children, "Work item").unwrap();
    assert!(page.dispatch(&tabs, Some(&json!({ "value": "links" }))));
    let links = tree(&events, "the links", |tree| tree.text().contains("Add retry with backoff to the sync client"));
    assert!(links.text().contains("Pull requests"), "{}", links.text());
    assert!(page.dispatch(&tabs, Some(&json!({ "value": "history" }))));
    tree(&events, "the history", |tree| tree.text().contains("Remaining work"));
}

#[test]
fn work_item_tab_without_a_work_item_says_where_to_open_one() {
    let rig = Rig::with(load("work-item"), true, vec![]);
    let (_page, events) = open_page(&rig.host, &module("work-item", "item"));
    tree(&events, "the empty tab", |tree| tree.text().contains("Open a work item from a pane's badge"));
}
