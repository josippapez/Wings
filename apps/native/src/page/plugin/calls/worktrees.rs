//! `createWorktree`, `worktreeSetup` and `setWorktreeSetup`, run against a real git repo and a project list in a temp
//! folder. The words and the order of the checks are the desktop's.

use std::path::PathBuf;

use serde_json::json;
use wings_spaces::worktree::DEFAULT_INCLUDE;

use crate::{
    page::plugin::testing::{Rig, plugin},
    projects::{Projects, test_dir, test_repo},
};

/// A plugin with `worktrees`, in an app whose only project is a repo `app`, which is on screen.
fn app(builtin: bool) -> (Rig, PathBuf, PathBuf) {
    let dir = test_dir("calls-worktrees");
    let repo = test_repo(&dir);
    let file = dir.join("spaces.json");
    let list = json!([{ "id": repo, "name": "app", "path": repo }]);
    std::fs::write(&file, serde_json::to_vec(&list).unwrap()).unwrap();
    let rig = Rig::with(
        plugin(json!({ "permissions": { "worktrees": true } })),
        builtin,
        vec![],
    );
    *rig.env.spaces.lock().unwrap() = Projects::open(Some(file), repo.clone());
    *rig.env.projects.lock().unwrap() = vec![repo.clone()];
    (rig, dir, repo)
}

#[test]
fn a_worktree_is_made_beside_the_repo_and_listed_under_its_project() {
    let (rig, dir, repo) = app(false);
    let one = dir.join("app-worktrees/wings-one");
    // The project on screen is the default, and the answer is the desktop's: the new project's id and folder.
    assert_eq!(
        rig.allowed("return await wings.createWorktree({ branch: 'wings/one' });"),
        json!({ "projectId": one, "path": one })
    );
    let rows = |rig: &Rig| {
        let spaces = rig.env.spaces.lock().unwrap();
        spaces
            .entries()
            .into_iter()
            .map(|p| (p.name, p.child, p.branch))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        rows(&rig),
        [
            ("app".to_owned(), false, Some("main".to_owned())),
            ("wings-one".to_owned(), true, Some("wings/one".to_owned()))
        ]
    );

    // Another branch from a named base, in an explicit project.
    let create = |extra: &str| {
        format!(
            "return await wings.createWorktree({{ projectId: {}, {extra} }});",
            json!(repo)
        )
    };
    let two = dir.join("app-worktrees/wings-two");
    assert_eq!(
        rig.allowed(&create("branch: 'wings/two', base: 'wings/one'")),
        json!({ "projectId": two, "path": two })
    );
    // git's own words for a branch or folder that exists, and the checks before git.
    assert_eq!(
        rig.refused(&create("branch: 'wings/one'")),
        format!("{} already exists", one.display())
    );
    assert!(
        rig.refused(&create("branch: 'a b'"))
            .contains("isn't a usable branch name")
    );
    assert_eq!(
        rig.refused(&create("branch: 'wings/x', base: 3")),
        "createWorktree base must be a branch name"
    );
    assert_eq!(
        rig.refused(&create("branch: 'wings/x', base: 'nope'")),
        "nope isn't a branch or commit in this repo"
    );
    assert_eq!(
        rig.refused(
            "return await wings.createWorktree({ projectId: '/elsewhere', branch: 'wings/x' });"
        ),
        "No such project in Wings"
    );
    assert_eq!(rows(&rig).len(), 3);

    // Without a project on screen or named, there is none to make it of.
    for params in [
        "{ branch: 'wings/x', projectId: '' }",
        "{ branch: 'wings/x' }",
    ] {
        *rig.env.projects.lock().unwrap() = vec![];
        assert_eq!(
            rig.refused(&format!("return await wings.createWorktree({params});")),
            "No project is open in Wings",
            "{params}"
        );
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_projects_worktree_setup_is_read_and_changed_by_field_and_a_worktree_means_its_project() {
    let (rig, dir, repo) = app(true);
    let setup = |project: &str| format!("return await wings.worktreeSetup({project});");
    let read = json!({
        "project": "app", "path": repo, "include": null, "defaults": DEFAULT_INCLUDE,
        "hasIncludeFile": false, "setup": null,
    });
    assert_eq!(rig.allowed(&setup("")), read);

    let change =
        |options: &str| format!("return await wings.setWorktreeSetup(undefined, {options});");
    assert_eq!(
        rig.allowed(&change(
            "{ include: ['.env', '!big/'], setup: ' pnpm install ' }"
        )),
        json!(null)
    );
    let both = rig.allowed(&setup(""));
    assert_eq!(
        (&both["include"], &both["setup"]),
        (&json!([".env", "!big/"]), &json!("pnpm install"))
    );
    // Absent leaves a field as it is; null resets the list, or clears the line.
    rig.allowed(&change("{ include: null }"));
    let reset = rig.allowed(&setup(""));
    assert_eq!(
        (&reset["include"], &reset["setup"]),
        (&json!(null), &json!("pnpm install"))
    );
    rig.allowed(&change("{ setup: null }"));
    assert_eq!(rig.allowed(&setup("")), read);

    // A worktree has its project's settings.
    rig.allowed(&change("{ setup: 'make' }"));
    let tree = rig.allowed("return await wings.createWorktree({ branch: 'wings/one' });");
    let of_tree = rig.allowed(&setup(&tree["projectId"].to_string()));
    assert_eq!(
        (&of_tree["project"], &of_tree["path"], &of_tree["setup"]),
        (&json!("app"), &json!(repo), &json!("make"))
    );

    // What the desktop refuses before it saves anything.
    for (options, refused) in [
        (
            "{ include: 'x' }",
            "setWorktreeSetup include is an array of strings, or null for the default list",
        ),
        (
            "{ include: [1] }",
            "setWorktreeSetup include is an array of strings, or null for the default list",
        ),
        (
            "{ setup: 3 }",
            "setWorktreeSetup setup is a shell line, or null to clear it",
        ),
        (
            "{ setup: 'a\\nb' }",
            "The setup command is one line of plain ASCII characters",
        ),
        (
            "{ setup: 'x'.repeat(501) }",
            "The setup command is at most 500 characters",
        ),
    ] {
        assert_eq!(rig.refused(&change(options)), refused, "{options}");
    }
    assert!(
        rig.refused(&change("{ include: ['a\\tb'] }"))
            .starts_with("A pattern is one line with no control characters")
    );
    assert_eq!(rig.allowed(&setup("")), {
        let mut now = read.clone();
        now["setup"] = json!("make");
        now
    });
    assert_eq!(rig.refused(&setup("'/elsewhere'")), "unknown space");
    assert_eq!(
        rig.refused("return await wings.setWorktreeSetup('/elsewhere', { setup: 'make' });"),
        "unknown space"
    );
    *rig.env.projects.lock().unwrap() = vec![];
    assert_eq!(rig.refused(&setup("")), "No project is open in Wings");
    assert_eq!(
        rig.refused(&change("{ setup: 'make' }")),
        "No project is open in Wings"
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn only_wings_own_plugins_change_a_setup_command_and_the_list_can_be_changed_by_any_plugin_with_worktrees()
 {
    let (rig, dir, _) = app(false);
    let change =
        |options: &str| format!("return await wings.setWorktreeSetup(undefined, {options});");
    for options in ["{ setup: 'make' }", "{ include: ['.env'], setup: null }"] {
        assert_eq!(
            rig.refused(&change(options)),
            "Only Wings' own plugins can change a project's setup command",
            "{options}"
        );
    }
    // The include list alone is no more than making worktrees.
    assert_eq!(rig.allowed(&change("{ include: ['.env'] }")), json!(null));
    let now = rig.allowed("return await wings.worktreeSetup();");
    assert_eq!(
        (&now["include"], &now["setup"]),
        (&json!([".env"]), &json!(null))
    );
    std::fs::remove_dir_all(dir).unwrap();
}
