//! The Issues board, list, issue tab and block, over made-up issues.
use super::*;

/// The Issues plugin's board, list, issue tab and block with its real main, over the made-up issues
/// `tasks_seed` stores first.
pub(super) fn tasks(out: PathBuf) {
    let _ = std::fs::remove_dir_all(out.join("fixture-storage-tasks"));
    let (host, project) = repo_host(
        &out,
        "tasks",
        include_str!("../../../../plugins/tasks/wings-plugin.json"),
    );
    // The project on screen, which the app always says and the pages follow.
    host.publish_project(Some(std::path::Path::new(&project)));
    let (seeded, done) = std::sync::mpsc::channel();
    let seed = host
        .open_page(
            &tasks_seed(&project),
            move |event| match event {
                wings_native::page::runtime::Event::Tree(tree)
                    if tree.text().contains("Seeded") =>
                {
                    let _ = seeded.send(());
                }
                other => fail_on_error(other),
            },
        )
        .unwrap();
    done.recv_timeout(Duration::from_secs(10))
        .expect("the issues to be stored");
    drop(seed);
    let module = |frame: &str| {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../plugins/tasks/dist/native/{frame}.wasm"));
        format!("{}{}", wings_native::page::runtime::wasm::MARKER, path.canonicalize().expect("sh build-rust.sh tasks, in plugins/").display())
    };
    host.start_main(&module("main"), fail_on_error).unwrap();
    let (board, issue, block) = (module("board"), module("issue"), module("block"));
    let (board, issue, block) = (board.as_str(), issue.as_str(), block.as_str());
    let args = json!({ "key": "WEB-4", "projectId": project });
    use wings_native::page::plugin::StatePlace::{Block, Tab};
    shoot_pages(
        &out,
        &host,
        "tasks",
        &[
            (
                "board",
                board,
                None,
                Tab,
                &["Board"],
                "Checkout summary card",
            ),
            ("list", board, None, Tab, &["List"], "Checkout redesign"),
            ("board-updated", board, None, Tab, &["Board", "Updated"], "Checkout summary card"),
            (
                "issue",
                issue,
                Some(&args),
                Tab,
                &[],
                "Waiting on the release branch",
            ),
            // A real issue's weight: 12 commits, 3 related issues, two sessions and a list of long file names.
            ("issue-real", issue, Some(&json!({ "key": "WEB-12", "projectId": project })), Tab, &[], "Files changed"),
            ("block", block, None, Block, &[], "Open board"),
            (
                "issue-missing",
                issue,
                Some(&json!({ "key": "WEB-99", "projectId": project })),
                Tab,
                &[],
                "may have been removed",
            ),
            ("settings", &module("settings"), None, Tab, &[], "Keep issues local"),
            // The toolbar's … opens its menu over the page, and Delete issue asks in a dialog.
            ("issue-more", issue, Some(&args), Tab, &["menu:More"], "Delete issue"),
            ("issue-delete", issue, Some(&args), Tab, &["Delete issue"], "Keys are never used again"),
            // The header's New issue opens the line to add one under the filters.
            ("board-new", board, None, Tab, &["Board", "New issue"], "Cancel"),
        ],
    );
    // A project with no issues: the board's empty columns and the block's empty state.
    let (empty_host, empty_project) = repo_host(&out, "tasks-empty", include_str!("../../../../plugins/tasks/wings-plugin.json"));
    empty_host.publish_project(Some(std::path::Path::new(&empty_project)));
    shoot_pages(
        &out,
        &empty_host,
        "tasks-empty",
        &[
            ("board", board, None, Tab, &["Board"], "Nothing waiting"),
            ("block", block, None, Block, &[], "No issues yet"),
        ],
    );
    // A project past the first steps: a Done column of 40 and an issue with 130 log entries, each with its Show more.
    let (long_host, long_project) = repo_host(&out, "tasks-long", include_str!("../../../../plugins/tasks/wings-plugin.json"));
    long_host.publish_project(Some(std::path::Path::new(&long_project)));
    let (seeded, done) = std::sync::mpsc::channel();
    let seed = long_host
        .open_page(&tasks_seed_long(&long_project), move |event| match event {
            wings_native::page::runtime::Event::Tree(tree) if tree.text().contains("Seeded") => {
                let _ = seeded.send(());
            }
            other => fail_on_error(other),
        })
        .unwrap();
    done.recv_timeout(Duration::from_secs(10)).expect("the issues to be stored");
    drop(seed);
    long_host.start_main(&module("main"), fail_on_error).unwrap();
    let long_args = json!({ "key": "WEB-41", "projectId": long_project });
    shoot_pages(
        &out,
        &long_host,
        "tasks-long",
        &[
            ("board", board, None, Tab, &["Board"], "Show 10 more"),
            ("issue", issue, Some(&long_args), Tab, &[], "Show 30 earlier"),
        ],
    );
    // On its own, after the others: Group by parent stays on for every Issues tab once it's turned on.
    shoot_pages(
        &out,
        &host,
        "tasks",
        &[(
            "list-parent",
            board,
            None,
            Tab,
            &["List", "Group by parent"],
            "No parent",
        )],
    );
}

/// Made-up issues past the Issues pages' first steps: 40 done tasks and one issue with 130 log entries.
pub(super) fn tasks_seed_long(project: &str) -> String {
    struct Seed(String);
    impl Guest for Seed {
        fn start(&mut self, cx: &mut Cx<'_>) {
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64;
            let slot = slot_of(&self.0);
            for n in 1..=40u64 {
                let at = now - n * 3_600_000;
                let issue = json!({ "key": format!("WEB-{n}"), "title": format!("Finished task number {n}"), "status": "done", "type": "task", "sessions": [],
                                    "log": [{ "at": at, "by": "you", "kind": "created", "text": "Created" }], "createdAt": at, "updatedAt": at });
                cx.save(&format!("issues/{slot}/WEB-{n}"), &issue);
            }
            let log: Vec<serde_json::Value> = (0..130u64).map(|n| json!({ "at": now - (130 - n) * 60_000, "by": "you", "kind": "note", "text": format!("Step {n} of the long run.") })).collect();
            let long = json!({ "key": "WEB-41", "title": "A long-running migration", "status": "in_progress", "type": "task", "sessions": [], "log": log, "createdAt": now - 130 * 60_000, "updatedAt": now });
            cx.save(&format!("issues/{slot}/WEB-41"), &long);
            cx.save(&format!("issues/{slot}"), &json!({ "v": 3, "path": self.0, "prefix": "WEB", "next": 42 }));
            cx.render(&json!({ "id": 0, "type": "#root", "props": {}, "children": [{ "id": 1, "type": "#text", "text": "Seeded" }] }));
        }
    }
    let project = project.to_owned();
    fake::source("page", move || Box::new(Seed(project.clone())))
}

/// The slot the plugin keeps a project under (`plugins/tasks/rust/src/storage.rs` `slot_of`).
pub(super) fn slot_of(project_id: &str) -> String {
    let (mut h1, mut h2) = (0xdead_beef_u32, 0x41c6_ce57_u32);
    for unit in project_id.encode_utf16() {
        let c = u32::from(unit);
        h1 = (h1 ^ c).wrapping_mul(2_654_435_761);
        h2 = (h2 ^ c).wrapping_mul(1_597_334_677);
    }
    h1 = (h1 ^ (h1 >> 16)).wrapping_mul(2_246_822_507) ^ (h2 ^ (h2 >> 13)).wrapping_mul(3_266_489_909);
    h2 = (h2 ^ (h2 >> 16)).wrapping_mul(2_246_822_507) ^ (h1 ^ (h1 >> 13)).wrapping_mul(3_266_489_909);
    format!("{h2:08x}{h1:08x}")
}

/// Made-up issues for the screenshots of Issues: run as a page of the plugin before its main starts, it writes them to
/// storage the way `plugins/tasks/rust/src/storage.rs` keeps them, for the project `project`, then shows "Seeded".
pub(super) fn tasks_seed(project: &str) -> String {
    struct Seed(String);
    impl Guest for Seed {
        fn start(&mut self, cx: &mut Cx<'_>) {
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64;
            let ago = |hours: u64| now - hours * 3_600_000;
            let agent = |seen: u64| json!({ "id": "6f1c2a90-3b7e-4d51-9a02-5c8e1d4b7a11", "name": "checkout summary", "seen": seen });
            let created = |hours: u64| json!({ "at": ago(hours), "by": "you", "kind": "created", "text": "Created" });
            let issue = |key: &str, title: &str, status: &str, kind: &str, hours: u64, more: serde_json::Value| {
                let mut issue = json!({ "key": key, "title": title, "status": status, "type": kind, "sessions": [], "log": [created(hours)],
                                        "createdAt": ago(hours), "updatedAt": ago(hours / 2) });
                issue.as_object_mut().unwrap().extend(more.as_object().unwrap().clone());
                issue
            };
            let by = json!({ "id": "6f1c2a90-3b7e-4d51-9a02-5c8e1d4b7a11", "name": "checkout summary" });
            let issues = [
                issue("WEB-1", "Show the delivery estimate under the cart total", "todo", "task", 30, json!({ "parent": "WEB-10", "priority": "medium", "description": "## Goal\nShow when the order arrives, under the total.\n\n## Done when\n- [ ] The estimate shows for a saved address\n- [ ] A cart with no address shows the default" })),
                issue("WEB-2", "Fix the focus ring on the promo code field", "todo", "bug", 28, json!({ "priority": "high", "links": { "branch": "fix/promo-focus" } })),
                issue("WEB-3", "Backfill order totals for rows created before the currency column existed", "in_progress", "task", 20, json!({ "parent": "WEB-10", "sessions": [agent(ago(2))] })),
                issue("WEB-4", "Checkout summary card", "in_review", "feature", 52, json!({
                    "parent": "WEB-10",
                    "priority": "urgent",
                    "description": "## Goal\nShow the buyer what they are about to pay for in one place, right above the pay button.\n\n## Scope\n- One card built from `useCart()`, no new data fetching\n- Rounding happens once, on the sum\n\n## Cases\n| Cart | Address | Shows |\n| --- | --- | --- |\n| Has items | Saved address in the same country as the shop | The card, with each line and the carrier's name and a date range |\n| Empty | none | Nothing |\n\n## Done when\n- [x] The card shows items, shipping, tax and total\n- [ ] An empty cart hides the card",
                    "related": ["WEB-8"],
                    "blockedBy": ["WEB-3"],
                    "links": { "branch": "feature/checkout-summary", "pr": "https://example.com/acme/web/pull/412", "commits": ["3fa91c2", "8b27de0"] },
                    "sessions": [agent(ago(3))],
                    "log": [
                        created(52),
                        { "at": ago(50), "by": by, "kind": "note", "text": "Read the cart code. Plan: one card with items, shipping, tax and a total, no new data fetching." },
                        { "at": ago(26), "by": by, "kind": "status", "text": "In progress → In review" },
                        { "at": ago(5), "by": "you", "kind": "note", "text": "Approved. Waiting on the release branch." },
                    ],
                })),
                issue("WEB-5", "Rename the cart store to match the checkout naming", "done", "task", 60, json!({})),
                issue("WEB-7", "Hide the summary card for an empty cart", "todo", "task", 20, json!({ "parent": "WEB-4", "priority": "low" })),
                issue("WEB-8", "Order history landing page: tabs, design alignment, page and tab events", "todo", "feature", 18, json!({ "parent": "WEB-10" })),
                issue("WEB-6", "Upgrade the date picker", "done", "task", 70, json!({ "parent": "WEB-10" })),
                issue("WEB-12", "186963: rebase PR 6399 on main, align breadcrumb with Figma design system, add drift fixture", "in_progress", "task", 20, json!({
                    "parent": "WEB-10",
                    "priority": "medium",
                    "description": "## Goal\nBring PR 6399 up to date with main, check the Figma design-system breadcrumb changes, and add a figma-component-drift fixture for the breadcrumb.\n\n## Done when\n- [ ] Branch rebased, typecheck/lint/test green for affected projects\n- [ ] Storybook checked in Chrome",
                    "related": ["WEB-8", "WEB-1", "WEB-3"],
                    "links": { "commits": ["3c890c6", "b936060", "a09b1a4", "61a763b", "f20539e", "2c0c9a6", "a4acaac", "bdc27ed", "5f02c5f", "b133b44", "1f3980f", "ea1aaca"] },
                    "sessions": [agent(ago(1)), { "id": "0e4c8d12-7a6b-4f3e-8c15-2b9d6a3e5f77", "name": "Check this PR, it's the older one and it has a very long name", "seen": ago(4) }],
                    "files": [
                        { "path": "tools/scripts/figma-component-drift/fixtures/figma-component-drift-handoff.md", "what": "`dprf-field=textarea` height and padding measured against the Figma frame", "at": ago(2), "session": by },
                        { "path": "tools/scripts/figma-component-drift/component-index.json", "what": "`component-index.json`: Updates the breadcrumb entry", "at": ago(2), "session": by },
                        { "path": "tools/scripts/figma-component-drift/component-map.json", "what": "`padding-top` and `padding-bottom` for the breadcrumb", "at": ago(3), "session": by },
                        { "path": "docs/web-reusable-components.md", "what": "Updates `breadcrumb` spacing token notes", "at": ago(3), "session": by },
                    ],
                })),
                issue("WEB-10", "Checkout redesign", "in_progress", "epic", 80, json!({ "description": "The checkout pages on the new design system." })),
            ];
            let slot = slot_of(&self.0);
            for issue in &issues {
                cx.save(&format!("issues/{slot}/{}", issue["key"].as_str().unwrap()), issue);
            }
            cx.save(&format!("issues/{slot}"), &json!({ "v": 3, "path": self.0, "prefix": "WEB", "next": 13 }));
            cx.render(&json!({ "id": 0, "type": "#root", "props": {}, "children": [{ "id": 1, "type": "#text", "text": "Seeded" }] }));
        }
    }
    let project = project.to_owned();
    fake::source("page", move || Box::new(Seed(project.clone())))
}
