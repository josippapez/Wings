//! Work item's tab and Sprint block over a fake `az`.
use super::*;

/// Work item's built tab and Sprint block. The pages call `az` themselves, so the shots run an `az` of their own, first on
/// PATH, that answers from `shot_plugins/work_item_az.json`, in a project folder that is a git repo with an Azure DevOps
/// origin, which is how a page knows what to ask for.
pub(super) fn work_item(out: PathBuf) {
    let answers: serde_json::Map<String, serde_json::Value> = serde_json::from_str(include_str!("work_item_az.json")).unwrap();
    let bin = out.join("fake-az");
    std::fs::create_dir_all(&bin).unwrap();
    let mut script = String::from("#!/bin/sh\ncase \"$*\" in\n");
    for (index, (words, answer)) in answers.iter().filter(|(words, _)| !words.starts_with('_')).enumerate() {
        let file = bin.join(format!("{index}.json"));
        std::fs::write(&file, answer.to_string()).unwrap();
        script += &format!("  *'{words}'*) cat '{}' ;;\n", file.display());
    }
    script += "  *) echo \"no made-up answer for az $*\" >&2; exit 1 ;;\nesac\n";
    let az = bin.join("az");
    std::fs::write(&az, script).unwrap();
    std::process::Command::new("chmod").arg("+x").arg(&az).status().unwrap();
    // Programs are found on the login shell's PATH, then this process's: no login shell leaves this one.
    unsafe {
        std::env::set_var("SHELL", "/nonexistent-shell");
        std::env::set_var("PATH", format!("{}:{}", bin.display(), std::env::var("PATH").unwrap_or_default()));
    }
    let (host, project) = repo_host(&out, "work-item", include_str!("../../../../plugins/work-item/wings-plugin.json"));
    let git = |args: &[&str]| {
        let done = std::process::Command::new("git").args(args).current_dir(&project).output().unwrap();
        assert!(done.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&done.stderr));
    };
    git(&["init", "-q"]);
    let _ = std::process::Command::new("git").args(["remote", "remove", "origin"]).current_dir(&project).output();
    git(&["remote", "add", "origin", "git@ssh.dev.azure.com:v3/acme/Web/web"]);
    // The project on screen, which the app always says and the Sprint block follows.
    host.publish_project(Some(std::path::Path::new(&project)));
    let module = |frame: &str| {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../plugins/work-item/dist/native/{frame}.wasm"));
        format!("{}{}", wings_native::page::runtime::wasm::MARKER, path.canonicalize().expect("sh build-rust.sh work-item, in plugins/").display())
    };
    let (item, sprint) = (module("item"), module("sprint"));
    let args = json!({ "id": 1432, "org": "https://dev.azure.com/acme" });
    // The fake `az` has no work item 9999.
    let missing = json!({ "id": 9999, "org": "https://dev.azure.com/acme" });
    use wings_native::page::plugin::StatePlace::{Block, Tab};
    shoot_pages(
        &out,
        &host,
        "work-item",
        &[
            ("no-item", &item, None, Tab, &[], "Open a work item from a pane's badge"),
            ("missing", &item, Some(&missing), Tab, &[], "doesn't exist, or you can't see it"),
            ("overview", &item, Some(&args), Tab, &[], "Pull request is up"),
            ("links", &item, Some(&args), Tab, &["Links"], "Make the sync client reliable"),
            ("history", &item, Some(&args), Tab, &["History"], "Remaining work"),
            ("sprint", &sprint, None, Block, &[], "Add retry with backoff"),
            ("sprint-all", &sprint, None, Block, &["Whole sprint"], "Show a notice while retrying"),
        ],
    );
}
