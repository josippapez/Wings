//! Tests that each plugin shipped in this repo can make the calls its own manifest declares, under the rules in this crate.

use std::{fs, path::Path};

use crate::{
    cli_rules::{is_unsafe_flag, AZURE_DEVOPS_RESOURCE},
    exec::{exec_check, may_exec},
    fetch::{fetch_url, may_send_secret},
    manifest::{Manifest, Plugin},
    post::{post_args, post_entry},
    test_support::post_request,
};

/// One of the plugins in this repo. Its manifest is read without `load`, whose Wings version check refuses a plugin
/// that needs the Wings this commit becomes.
fn own_plugin(name: &str) -> Plugin {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins").join(name);
    let manifest: Manifest = serde_json::from_str(&fs::read_to_string(dir.join("wings-plugin.json")).unwrap()).unwrap();
    Plugin { manifest, dir, icon_svg: None }
}

/// Each call is one the plugin's manifest allows, with no flag Wings refuses outright.
fn assert_calls_pass(p: &Plugin, calls: &[(&str, &[&str])]) {
    for (program, args) in calls {
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        assert!(may_exec(p, program, &args), "{program} {args:?} not declared");
        assert!(!args.iter().any(|a| is_unsafe_flag(program, a)), "{program} {args:?} refused");
    }
}

#[test]
fn pr_tracker_calls_pass_its_own_manifest() {
    let p = own_plugin("pr-tracker");
    let dir = p.dir.as_path();
    assert!(p.manifest.contributes.mcp_tools.iter().any(|t| t.name == "failing_checks"));
    let calls: &[(&str, &[&str])] = &[
        ("git", &["remote", "get-url", "origin"]),
        ("git", &["rev-parse", "--show-toplevel"]),
        ("git", &["branch", "--show-current"]),
        ("git", &["rev-list", "--left-right", "--count", "@{upstream}...HEAD"]),
        ("git", &["fetch", "--quiet", "origin", "main", "feature/x"]),
        ("git", &["fetch", "--quiet", "--end-of-options", "origin", "main", "feature/x"]),
        ("git", &["diff", "--no-color", "--no-ext-diff", "origin/main...origin/feature/x"]),
        ("git", &["diff", "--no-color", "--no-ext-diff", "-U0", "abc123", "def456", "--", "src/a.ts"]),
        ("git", &["show", "abc123:src/a.ts"]),
        ("gh", &["pr", "list", "--head", "x", "--state", "all", "--limit", "1", "--json", "url,number"]),
        ("gh", &["pr", "list", "--repo", "ghe.example.com/o/r", "--head", "x", "--state", "all", "--limit", "1", "--json", "url,number"]),
        ("gh", &["pr", "view", "https://github.com/o/r/pull/1", "--json", "comments,reviews"]),
        ("gh", &["pr", "diff", "https://github.com/o/r/pull/1"]),
        ("gh", &["api", "repos/o/r/pulls/1/comments", "--paginate", "--slurp"]),
        ("gh", &["api", "repos/o/r/actions/jobs/123"]),
        ("gh", &["api", "--hostname", "ghe.example.com", "user", "--jq", ".login"]),
        ("gh", &["api", "repos/o/r/actions/jobs/123/logs", "--allow-escape-sequences"]),
        ("gh", &["auth", "status", "--json", "hosts"]),
        ("gh", &["auth", "login", "--web", "--clipboard", "--hostname", "github.com"]),
        ("glab", &["api", "--hostname", "gitlab.com", "--paginate", "projects/g%2Fp/merge_requests/1/discussions?per_page=100"]),
        ("glab", &["api", "--hostname", "gitlab.com", "projects/1/jobs/123/trace"]),
        ("glab", &["mr", "diff", "1", "--raw", "--repo", "https://gitlab.com/g/p"]),
        ("glab", &["auth", "status", "--all"]),
        ("glab", &["auth", "status", "--hostname", "gitlab.com"]),
        ("glab", &["auth", "login", "--web", "--hostname", "gitlab.com"]),
        ("az", &["repos", "pr", "list", "--organization", "https://dev.azure.com/o", "--source-branch", "x", "-o", "json"]),
        (
            "az",
            &[
                "repos",
                "pr",
                "list",
                "--organization",
                "https://dev.azure.com/o",
                "--project",
                "P",
                "--repository",
                "r",
                "--status",
                "active",
                "--top",
                "100",
                "-o",
                "json",
            ],
        ),
        ("az", &["repos", "pr", "show", "--id", "1", "--organization", "https://dev.azure.com/o", "-o", "json"]),
        ("az", &["repos", "pr", "policy", "list", "--id", "1", "-o", "json"]),
        ("az", &["account", "show", "--query", "user.name", "-o", "tsv"]),
        ("az", &["devops", "invoke", "--area", "git", "--resource", "pullRequestThreads", "--api-version", "7.1"]),
        (
            "az",
            &[
                "devops",
                "invoke",
                "--area",
                "git",
                "--resource",
                "pullRequestIterations",
                "--route-parameters",
                "project=P",
                "--query-parameters",
                "includeCommits=true",
                "--organization",
                "https://dev.azure.com/o",
                "--api-version",
                "7.1",
                "-o",
                "json",
            ],
        ),
        (
            "az",
            &[
                "devops",
                "invoke",
                "--area",
                "build",
                "--resource",
                "timeline",
                "--route-parameters",
                "project=My Project",
                "buildId=123",
                "--organization",
                "https://dev.azure.com/o",
                "--api-version",
                "7.1",
                "-o",
                "json",
            ],
        ),
        (
            "az",
            &[
                "devops",
                "invoke",
                "--area",
                "build",
                "--resource",
                "logs",
                "--route-parameters",
                "project=My Project",
                "buildId=123",
                "logId=4",
                "--organization",
                "https://dev.azure.com/o",
                "--api-version",
                "7.1",
                "-o",
                "json",
            ],
        ),
        ("az", &["login", "--allow-no-subscriptions", "--output", "none"]),
        ("az", &["repos", "pr", "set-vote", "--id", "42", "--vote", "approve-with-suggestions", "--organization", "https://dev.azure.com/o", "-o", "json"]),
    ];
    for (program, args) in calls {
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        assert_eq!(exec_check(&p, program, &args, Some(dir)), Ok(()), "{program} {args:?}");
    }
    assert!(!may_exec(&p, "gh", &["auth".into(), "token".into()]));
    // Only the flags its calls pass: no header on gh api, no forced fetch, no options on a diff that takes none.
    for (program, args) in [
        ("gh", &["api", "-H", "Host: example.com", "user"][..]),
        ("gh", &["api", "--cache", "1h", "user"]),
        ("git", &["fetch", "--force", "origin", "main"]),
        ("git", &["fetch", "-f", "origin", "main"]),
        ("gh", &["pr", "diff", "1", "--patch"]),
        ("git", &["show", "--stat", "abc123"]),
    ] {
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        assert!(!may_exec(&p, program, &args), "{program} {args:?} allowed");
    }
    // az's own file and method flags still can't ride along on a vote.
    let vote =
        |extra: &[&str]| -> Vec<String> { ["repos", "pr", "set-vote", "--id", "1", "--vote", "reset"].iter().chain(extra).map(|a| a.to_string()).collect() };
    assert!(may_exec(&p, "az", &vote(&[])));
    assert!(!may_exec(&p, "az", &vote(&["--detect"])), "a flag that isn't listed");
    assert!(vote(&["--file", "x"]).iter().any(|a| is_unsafe_flag("az", a)));
    assert!(is_unsafe_flag("az", "@/etc/passwd"));
    let posts = [
        ("gh", "repos/o/r/pulls/1/comments/2/replies", None),
        ("gh", "repos/o/r/pulls/1/comments/2/replies", Some("ghe.example.com")),
        ("glab", "projects/g%2Fp/merge_requests/1/approve", Some("gitlab.com")),
        ("glab", "projects/g%2Fp/merge_requests/1/discussions/ab12/notes", Some("gitlab.com")),
        ("az", "https://dev.azure.com/o/My%20Project/_apis/git/repositories/Repo.Name/pullRequests/1/threads/44590/comments?api-version=7.1", None),
    ];
    for (program, url, host) in posts {
        assert!(post_args(&p, &post_request(program, url, host)).is_ok(), "{program} {url} not declared");
    }
    // The lists' GraphQL only reads: a mutation is refused though the entry allows `gh graphql`.
    let graphql = |query: &str| {
        let mut r = post_request("gh", "graphql", None);
        r.fields = serde_json::json!({ "query": query }).as_object().unwrap().clone();
        r
    };
    assert!(post_args(&p, &graphql("query { viewer { login } }")).is_ok());
    for query in ["mutation { addStar(input: {starrableId: \"x\"}) { clientMutationId } }", "query { a } mutation{b}", "subscription { x }"] {
        assert!(post_args(&p, &graphql(query)).is_err(), "{query}");
    }
    // Approving on GitHub is pinned to the APPROVE event, on github.com and on an Enterprise host.
    let review = |event: &str, host: Option<&str>| {
        let mut r = post_request("gh", "repos/o/r/pulls/1/reviews", host);
        r.fields = serde_json::json!({ "event": event }).as_object().unwrap().clone();
        r
    };
    assert!(post_args(&p, &review("APPROVE", None)).is_ok());
    assert!(post_args(&p, &review("APPROVE", Some("ghe.example.com"))).is_ok());
    assert!(post_args(&p, &review("REQUEST_CHANGES", None)).is_err());
    assert!(post_args(&p, &review("COMMENT", None)).is_err());
    let bb = "https://api.bitbucket.org/2.0/repositories/ws/repo/pullrequests/4";
    let reply = Some(r#"{"content":{"raw":"Thanks"},"parent":{"id":12}}"#);
    assert!(fetch_url(&p, "GET", &format!("{bb}/comments?pagelen=100"), None).is_some());
    assert!(fetch_url(&p, "POST", &format!("{bb}/comments"), reply).is_some());
    assert!(fetch_url(&p, "POST", &format!("{bb}/approve"), None).is_some());
    for method in ["PUT", "DELETE", "PATCH"] {
        assert!(fetch_url(&p, method, &format!("{bb}/comments"), reply).is_none(), "{method}");
    }
    assert!(fetch_url(&p, "POST", &format!("{bb}/merge"), Some("{}")).is_none());
    assert!(fetch_url(&p, "DELETE", &format!("{bb}/approve"), None).is_none());
    let cb = "https://codeberg.org/api/v1/repos/o/r";
    assert!(fetch_url(&p, "GET", &format!("{cb}/pulls/4.diff"), None).is_some());
    assert!(fetch_url(&p, "POST", &format!("{cb}/issues/4/comments"), Some(r#"{"body":"> quoted\n\nThanks"}"#)).is_some());
    assert!(fetch_url(&p, "POST", &format!("{cb}/pulls/4/reviews"), Some(r#"{"event":"APPROVED"}"#)).is_some());
    assert!(fetch_url(&p, "POST", &format!("{cb}/pulls/4/reviews"), Some(r#"{"event":"REQUEST_CHANGES","body":"no"}"#)).is_none());
    assert!(fetch_url(&p, "POST", &format!("{cb}/pulls/4/merge"), Some(r#"{"Do":"merge"}"#)).is_none());
    // Each token goes only to its own host.
    assert!(may_send_secret(&p, "bitbucket", &format!("{bb}/comments")) && !may_send_secret(&p, "bitbucket", &format!("{cb}/pulls/4.diff")));
    assert!(may_send_secret(&p, "codeberg", &format!("{cb}/pulls/4.diff")) && !may_send_secret(&p, "codeberg", &format!("{bb}/comments")));
}

#[test]
fn pr_tracker_replies_on_azure_at_dev_azure_com_only() {
    let p = own_plugin("pr-tracker");
    let tail = "_apis/git/repositories/repo/pullRequests/7/threads/44590/comments?api-version=7.1";
    let reply = |address: &str| post_args(&p, &post_request("az", &format!("{address}/{tail}"), None));
    // An organization on {org}.visualstudio.com is reachable at dev.azure.com too, so the plugin posts there.
    assert!(reply("https://dev.azure.com/org/proj").is_ok());
    // The legacy address can't be covered: the host of an `az` entry has no `*`, and no organization is known in advance.
    assert!(reply("https://org.visualstudio.com/proj").is_err());
    assert!(reply("https://org.visualstudio.com/DefaultCollection/proj").is_err());
    assert!(post_entry("az https://*.visualstudio.com/*/_apis/git/repositories/*/pullRequests/*/threads/*/comments?api-version=7.1").is_none());
}

#[test]
fn changes_calls_pass_its_own_manifest() {
    let p = own_plugin("changes");
    let dir = p.dir.as_path();
    assert!(p.manifest.contributes.mcp_tools.iter().any(|t| t.name == "changed_files"));
    assert!(p.manifest.contributes.mcp_tools.iter().any(|t| t.name == "recent_commits"));
    assert!(p.manifest.contributes.views.iter().any(|v| v.id == "graph" && v.opens == ["window", "modal"]));
    let color = ["--no-color", "--no-ext-diff", "--no-textconv", "--no-relative"];
    let diff = |extra: &[&str]| -> Vec<String> { ["diff", "abc123"].iter().chain(extra).chain(&color).map(|a| a.to_string()).collect() };
    let grep = |extra: &[&str]| -> Vec<String> {
        ["grep", "--untracked"].iter().chain(extra).chain(&["--no-color", "-e", "", "--", ":(literal)new file.txt"]).map(|a| a.to_string()).collect()
    };
    let log_format = ["-z", "--no-color", "--decorate=full", "--format=%H%x00%P%x00%an%x00%at%x00%ct%x00%D%x00%s"];
    let log = |extra: &[&str]| -> Vec<String> { ["log"].iter().chain(&log_format).chain(extra).map(|a| a.to_string()).collect() };
    let show = |extra: &[&str]| -> Vec<String> {
        ["show", "--format=", "--diff-merges=first-parent", "-M"].iter().chain(&color).chain(extra).chain(&["abc123", "--"]).map(|a| a.to_string()).collect()
    };
    let calls: Vec<Vec<String>> = vec![
        log(&["--max-count=51", "--skip=50"]),
        log(&["--max-count=51", "--skip=0", "--all"]),
        log(&["--regexp-ignore-case", "--fixed-strings", "--max-count=101", "--grep=fix the -c flag", "--all"]),
        log(&["--regexp-ignore-case", "--fixed-strings", "--max-count=101", "--author=someone"]),
        log(&["--max-count=1", "abc123", "--"]),
        ["log", "--all", "--topo-order"].iter().chain(&log_format).chain(&["--max-count=10000"]).map(|a| a.to_string()).collect(),
        show(&["--numstat", "-z"]),
        show(&["--patch"]),
        ["rev-parse", "--verify", "--quiet", "abc123^{commit}"].map(String::from).to_vec(),
        ["rev-parse", "--show-toplevel"].map(String::from).to_vec(),
        ["rev-parse", "--abbrev-ref", "origin/HEAD"].map(String::from).to_vec(),
        ["rev-parse", "--verify", "--quiet", "main^{commit}"].map(String::from).to_vec(),
        ["branch", "--show-current"].map(String::from).to_vec(),
        ["merge-base", "HEAD", "origin/main"].map(String::from).to_vec(),
        ["ls-files", "--others", "--exclude-standard", "-z"].map(String::from).to_vec(),
        diff(&["--numstat", "-z", "-M"]),
        diff(&["--name-status", "-z", "-M"]),
        diff(&["-M", "--", ":(literal)src/a.ts"]),
        grep(&["-I", "-z", "--count"]),
        grep(&["-I", "-z", "--no-line-number", "--no-column"]),
    ];
    for args in &calls {
        assert_eq!(exec_check(&p, "git", args, Some(dir)), Ok(()), "git {args:?}");
    }
    // Read-only: git grep can't open a pager, and branch can't delete or rename.
    assert!(!may_exec(&p, "git", &["grep".into(), "-Ovi".into(), "x".into()]));
    assert!(!may_exec(&p, "git", &["branch".into(), "--show-current".into(), "-D".into(), "x".into()]));
    assert!(!may_exec(&p, "git", &["symbolic-ref".into(), "HEAD".into(), "refs/heads/x".into()]));
    // log and show can't write a file, run a diff or textconv driver, or take flags outside their lists.
    for args in [["log", "--output=x"], ["log", "--ext-diff"], ["show", "--output=x"], ["show", "--textconv"], ["log", "--follow"], ["show", "--stat"]] {
        let args: Vec<String> = args.map(String::from).to_vec();
        assert!(!may_exec(&p, "git", &args), "git {args:?} allowed");
    }
}

#[test]
fn worktrees_calls_pass_its_own_manifest() {
    let p = own_plugin("worktrees");
    let dir = p.dir.as_path();
    assert!(p.manifest.permissions.panes.as_ref().is_some_and(|panes| panes.is_empty()), "plain shells only");
    let calls: &[&[&str]] = &[
        &["rev-parse", "--show-toplevel"],
        &["rev-parse", "--abbrev-ref", "origin/HEAD"],
        &["rev-parse", "--verify", "--quiet", "origin/main^{commit}"],
        &["worktree", "list", "--porcelain"],
        &["worktree", "remove", "--", "/Users/me/dev/app-feature"],
        &["worktree", "prune", "--verbose"],
        &["status", "--porcelain"],
        &["log", "--max-count=1", "--format=%ct"],
        &["branch", "--list", "--merged=origin/main", "--format=%(refname:short)"],
        &["branch", "--list", "--format=%(refname:short)%09%(upstream:track)"],
        &["branch", "-d", "feature/x"],
    ];
    for args in calls {
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        assert_eq!(exec_check(&p, "git", &args, Some(dir)), Ok(()), "git {args:?}");
    }
    // Nothing can force a removal, delete an unmerged branch, or change anything else.
    for args in [
        &["worktree", "remove", "--force", "/p"][..],
        &["worktree", "remove", "-f", "/p"],
        &["worktree", "remove", "-f", "-f", "/p"],
        &["worktree", "remove", "-ff", "/p"],
        &["worktree", "remove", "--forc", "/p"],
        &["worktree", "remove", "/p", "--force"],
        &["worktree", "prune", "--expire=now"],
        &["worktree", "add", "/p"],
        &["worktree", "move", "/a", "/b"],
        &["worktree", "lock", "/p"],
        &["branch", "-D", "x"],
        &["branch", "-d", "-D", "x"],
        &["branch", "-d", "--force", "x"],
        &["branch", "-df", "x"],
        &["branch", "--delete", "x"],
        &["branch", "newbranch"],
        &["branch", "--list", "-D", "x"],
        &["branch", "--list", "--set-upstream-to=origin/x"],
        &["status", "--porcelain", "--ignore-submodules"],
        &["symbolic-ref", "HEAD", "refs/heads/x"],
        &["config", "user.name", "x"],
        &["reset", "--hard"],
    ] {
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        assert!(!may_exec(&p, "git", &args), "git {args:?} allowed");
    }
}

#[test]
fn disk_calls_pass_its_own_manifest() {
    let p = own_plugin("disk");
    let file = "/Users/me/Downloads/big.zip";
    assert_calls_pass(
        &p,
        &[
            ("df", &["-k", "-P"]),
            ("open", &["-R", file]),
            ("qlmanage", &["-p", file]),
            ("trash", &[file]),
            ("tmutil", &["listlocalsnapshotdates", "/"]),
            ("tmutil", &["deletelocalsnapshots", "2026-10-30-101500"]),
            ("tmutil", &["thinlocalsnapshots", "/", "999999999999", "4"]),
        ],
    );
    assert!(!may_exec(&p, "trash", &["-v".into(), file.into()]));
    assert!(!may_exec(&p, "tmutil", &["deletelocalsnapshots".into(), "-d".into(), "/".into()]));
}

#[test]
fn hud_calls_pass_its_own_manifest() {
    let p = own_plugin("hud");
    assert_calls_pass(&p, &[("vm_stat", &[]), ("sysctl", &["-n", "hw.memsize"])]);
    assert!(!may_exec(&p, "sysctl", &["-n".into(), "hw.memsize".into(), "-w".into()]));
}

#[test]
fn schedules_calls_pass_its_own_manifest() {
    let p = own_plugin("schedules");
    let org = "https://dev.azure.com/contoso";
    assert_calls_pass(
        &p,
        &[
            ("git", &["remote", "get-url", "origin"]),
            ("git", &["for-each-ref", "--format=%(refname:short)", "refs/heads", "refs/remotes/origin"]),
            ("az", &["boards", "query", "--wiql", "SELECT [System.Id] FROM WorkItems", "--organization", org, "--project", "Web", "-o", "json"]),
            ("gh", &["issue", "list", "--assignee", "@me", "--state", "open", "--json", "number,title,url", "--limit", "50"]),
            ("glab", &["issue", "list", "--assignee=@me", "--output", "json", "--per-page", "50"]),
            ("glab", &["auth", "status", "--hostname", "gitlab.com"]),
            ("az", &["login", "--allow-no-subscriptions", "--output", "none"]),
            ("gh", &["auth", "login", "--web", "--clipboard", "--hostname", "github.com"]),
            ("glab", &["auth", "login", "--web", "--hostname", "gitlab.com"]),
        ],
    );
    // A sign-in opens the browser: it can't be handed a token, which would sign the CLI in as someone else.
    assert!(!may_exec(&p, "gh", &["auth".into(), "login".into(), "--with-token".into()]));
    assert!(!may_exec(&p, "glab", &["auth".into(), "login".into(), "--token".into(), "x".into()]));
}

#[test]
fn session_detail_calls_pass_its_own_manifest() {
    let p = own_plugin("session-detail");
    assert_calls_pass(
        &p,
        &[
            ("git", &["diff", "HEAD", "--no-color", "--no-ext-diff"]),
            ("git", &["diff", "HEAD", "--no-color", "--no-ext-diff", "--", "src/a.ts"]),
            ("git", &["ls-files", "--error-unmatch", "--", "src/a.ts"]),
        ],
    );
}

#[test]
fn tasks_calls_pass_its_own_manifest() {
    let p = own_plugin("tasks");
    let org = "https://dev.azure.com/contoso";
    assert_calls_pass(
        &p,
        &[
            ("git", &["remote", "get-url", "origin"]),
            ("git", &["diff", "abc123^", "--no-color", "--no-ext-diff", "--", "src/a.ts"]),
            ("gh", &["pr", "view", "https://github.com/o/r/pull/1", "--json", "state", "--jq", ".state"]),
            ("gh", &["pr", "list", "--head", "feature/x", "--state", "all", "--limit", "5", "--json", "url,state"]),
            ("az", &["repos", "pr", "show", "--id", "7", "--organization", org, "--query", "status", "--output", "tsv"]),
            (
                "az",
                &[
                    "repos",
                    "pr",
                    "list",
                    "--organization",
                    org,
                    "--project",
                    "Web",
                    "--repository",
                    "web",
                    "--source-branch",
                    "feature/x",
                    "--status",
                    "all",
                    "--top",
                    "5",
                    "--output",
                    "json",
                ],
            ),
        ],
    );
    assert!(!may_exec(&p, "git", &["remote".into(), "get-url".into(), "--push".into(), "origin".into()]));
}

#[test]
fn work_item_calls_pass_its_own_manifest() {
    let dir = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../plugins/work-item"));
    let manifest: Manifest = serde_json::from_str(&fs::read_to_string(dir.join("wings-plugin.json")).unwrap()).unwrap();
    let p = Plugin { manifest, dir: dir.to_path_buf(), icon_svg: None };
    let org = "https://dev.azure.com/o";
    let calls: &[(&str, &[&str])] = &[
        ("git", &["remote", "get-url", "origin"]),
        ("git", &["branch", "--show-current"]),
        ("git", &["rev-parse", "--show-toplevel"]),
        ("git", &["for-each-ref", "--format=%(refname:short)", "refs/heads", "refs/remotes"]),
        (
            "az",
            &[
                "devops",
                "invoke",
                "--area",
                "wit",
                "--resource",
                "workItemTypes",
                "--route-parameters",
                "project=My Project",
                "--api-version",
                "7.1",
                "--query",
                "value[].{name:name,states:states[].{name:name,category:category}}",
                "--organization",
                org,
                "-o",
                "json",
            ],
        ),
        (
            "az",
            &[
                "rest",
                "--resource",
                AZURE_DEVOPS_RESOURCE,
                "--url",
                "https://dev.azure.com/o/_apis/wit/workitems?ids=1,2&$expand=relations&errorPolicy=omit&api-version=7.1",
                "-o",
                "json",
            ],
        ),
        (
            "az",
            &[
                "devops",
                "invoke",
                "--area",
                "wit",
                "--resource",
                "workItemTypeStates",
                "--route-parameters",
                "project=My Project",
                "type=Tech Story",
                "--api-version",
                "7.1",
                "--organization",
                org,
                "-o",
                "json",
            ],
        ),
        ("az", &["boards", "work-item", "update", "--id", "123456", "--state", "Ready for Peer Review/QA", "--organization", org, "-o", "json"]),
        ("az", &["login", "--allow-no-subscriptions", "--output", "none"]),
        // The updates check: reads only.
        (
            "az",
            &[
                "boards",
                "query",
                "--project",
                "My Project",
                "--wiql",
                "SELECT [System.Id] FROM WorkItems WHERE [System.TeamProject] = 'My Project' AND [System.AssignedTo] = @Me",
                "--organization",
                org,
                "-o",
                "json",
            ],
        ),
        (
            "az",
            &[
                "devops",
                "invoke",
                "--area",
                "wit",
                "--resource",
                "updates",
                "--api-version",
                "7.1",
                "--route-parameters",
                "project=My Project",
                "id=123456",
                "--query-parameters",
                "$skip=200",
                "--organization",
                org,
                "-o",
                "json",
            ],
        ),
        (
            "az",
            &[
                "devops",
                "invoke",
                "--area",
                "git",
                "--resource",
                "pullRequests",
                "--api-version",
                "7.1",
                "--route-parameters",
                "project=My Project",
                "--query-parameters",
                "searchCriteria.reviewerId=00000000-0000-4000-8000-000000000000",
                "searchCriteria.status=active",
                "$top=100",
                "--organization",
                org,
                "-o",
                "json",
            ],
        ),
        (
            "az",
            &[
                "devops",
                "invoke",
                "--area",
                "git",
                "--resource",
                "pullRequestThreads",
                "--api-version",
                "7.1",
                "--route-parameters",
                "project=My Project",
                "repositoryId=r1",
                "pullRequestId=7",
                "--organization",
                org,
                "-o",
                "json",
            ],
        ),
    ];
    for (program, args) in calls {
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        assert_eq!(exec_check(&p, program, &args, Some(dir)), Ok(()), "{program} {args:?}");
    }
    assert!(!p.manifest.permissions.notify);
    // The query entry only takes the flags it lists, and no other `boards` subcommand but the declared ones.
    let query = |extra: &[&str]| -> Vec<String> { ["boards", "query"].iter().chain(extra).map(|a| a.to_string()).collect() };
    assert!(may_exec(&p, "az", &query(&["--wiql", "SELECT 1"])));
    assert!(!may_exec(&p, "az", &query(&["--path", "Shared Queries/x"])));
    // The update entry can't read a file into a field or the state, and can't reach other commands.
    let update = |extra: &[&str]| -> Vec<String> { ["boards", "work-item", "update", "--id", "1"].iter().chain(extra).map(|a| a.to_string()).collect() };
    for extra in [&["--state", "@/etc/passwd"][..], &["--fields", "System.Title=@/etc/passwd"], &["--description=@notes.md"], &["-f", "x=1"], &["-d", "x"]] {
        assert!(update(extra).iter().any(|a| is_unsafe_flag("az", a)), "{extra:?}");
    }
    assert!(!may_exec(&p, "az", &["boards".into(), "work-item".into(), "delete".into(), "--id".into(), "1".into()]));
    assert!(!may_exec(&p, "az", &["rest".into(), "--headers".into(), "x=1".into()]));
    assert!(p.manifest.permissions.post.is_empty());
    // Only the flags its entry lists: state, not the title, fields or anything else.
    assert!(may_exec(&p, "az", &update(&["--state=Done"])));
    for extra in [&["--title", "x"][..], &["--fields", "System.Title=x"], &["--assigned-to", "x"], &["--stat", "Done"], &["--discussion", "x"]] {
        assert!(!may_exec(&p, "az", &update(extra)), "{extra:?}");
    }
}
