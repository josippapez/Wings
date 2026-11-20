//! The `pull_request` tool: the PR for the branch a folder is on, as one small JSON object. The Claude chat plugin calls
//! it (`callTools`) for its side panel, and a chat tab is no terminal pane, so the lookup is by folder. It shares the
//! pane lookups' cache and the status cache, so a chat and a pane on one branch cost one network call per minute.

use super::{problems, state::{self, Loaded}, status::refresh_status, links::find_for_folder};
use crate::{checklist::iso_to_ms, providers, remote::remote_for, types::{CheckState, Link, PrState, Status}};
use serde_json::{Value, json};
use wings_plugin::host;

/// A status this young is answered as it is, as the pane poll does (`PR_STALE_MS`), so a chat polling each minute
/// reads the cache the pane poll just filled.
const FRESH_MS: f64 = 55_000.0;

/// The PR a folder's branch has, remembered so its status can load. None for a folder no forge is known for, and for a
/// branch with no PR. A host that needs signing in is an error saying so, not a missing PR.
pub async fn folder_link(cwd: &str) -> Result<Option<Link>, String> {
    let remote = remote_for(cwd).await;
    if !crate::forges::is_known(&remote) {
        return Ok(None);
    }
    let found = find_for_folder(cwd, remote.clone()).await?;
    match found {
        Some(link) => {
            state::remember(link.clone());
            Ok(Some(link))
        }
        None => match problems::problem_of(Some(&remote)) {
            Some(problem) => Err(problem.message().to_owned()),
            None => Ok(None),
        },
    }
}

/// Whether the PR can be merged as it stands: open, no check failing or running, and either approved or asking for no
/// review. The forge has the last word (branch protection), so a merge it refuses shows its own message.
pub fn mergeable(st: &Status) -> bool {
    st.state == PrState::Open && st.checks.failed == 0 && st.checks.pending() == 0 && matches!(st.label.as_str(), "Approved" | "Open")
}

fn state_word(state: PrState) -> &'static str {
    match state {
        PrState::Open => "open",
        PrState::Merged => "merged",
        PrState::Closed => "closed",
    }
}

/// How many checks the answer names, so it stays small.
const PENDING_SHOWN: usize = 3;

/// The checks still to finish, required ones first, then running before queued, then by name. The reviewer and merge
/// strategy policies aren't checks and are left out, as the counts leave them.
fn pending(st: &Status) -> Vec<Value> {
    let mut items: Vec<_> = st
        .items
        .iter()
        .flatten()
        .filter(|c| matches!(c.state, CheckState::Running | CheckState::Queued) && !matches!(c.kind.as_deref(), Some("Minimum reviewers" | "Merge strategy")))
        .collect();
    items.sort_by_key(|c| (c.required != Some(true), c.state != CheckState::Running, c.name.to_lowercase()));
    items
        .into_iter()
        .take(PENDING_SHOWN)
        .map(|c| json!({ "name": c.name, "state": if c.state == CheckState::Running { "running" } else { "queued" }, "since": c.at.as_deref().and_then(iso_to_ms) }))
        .collect()
}

fn describe(link: &Link, st: &Status) -> Value {
    json!({
        "supported": true,
        "found": true,
        "provider": providers::info(link.provider).name,
        "github": link.provider == crate::types::Provider::Github,
        "number": link.number,
        "ref": providers::link_ref(link),
        "title": st.title,
        "url": link.url,
        "state": state_word(st.state),
        "draft": st.label == "Draft",
        "label": st.label,
        "tone": st.tone,
        "review": st.review,
        "checks": { "passed": st.checks.passed, "failed": st.checks.failed, "running": st.checks.running, "queued": st.checks.queued },
        "pending": pending(st),
        "mergeable": mergeable(st),
    })
}

/// `fresh` reads the status again whatever its age, which a caller that just changed the PR (merged it) asks for.
pub async fn pull_request(cwd: &str, fresh: bool) -> Result<Value, String> {
    let Some(link) = folder_link(cwd).await? else {
        let supported = crate::forges::is_known(&remote_for(cwd).await);
        return Ok(json!({ "supported": supported, "found": false }));
    };
    let young = state::status_of(&link.url).is_some_and(|loaded| host::now() - loaded.at < FRESH_MS);
    if fresh || !young {
        refresh_status(&link.url, fresh).await;
    }
    match state::status_of(&link.url) {
        Some(Loaded { entry: Ok(st), .. }) => Ok(describe(&link, &st)),
        Some(Loaded { entry: Err(error), .. }) => Err(error),
        None => Err("The pull request's status didn't load.".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{CheckState, Counts, Provider};

    fn status(label: &str, state: PrState, checks: Counts) -> Status {
        Status { key: String::new(), title: "Add the cart".into(), label: label.into(), state, tone: "info".into(), icon: "pr-open".into(), checks, review: "Approved".into(), failing: vec![], items: None, changes: None, branch: "cart → main".into(), updated: String::new(), merge: None, base: None, head: None, base_commit: None, head_commit: None }
    }

    fn link() -> Link {
        Link { provider: Provider::Github, url: "https://github.com/acme/shop/pull/12".into(), number: 12, repo: "acme/shop".into(), remote: None, cwd: None, raw: None }
    }

    #[test]
    fn only_an_open_pr_with_nothing_failing_or_running_and_no_review_missing_can_merge() {
        let passed = Counts { passed: 3, failed: 0, running: 0, queued: 0 };
        assert!(mergeable(&status("Approved", PrState::Open, passed)));
        assert!(mergeable(&status("Open", PrState::Open, passed)));
        assert!(!mergeable(&status("In review", PrState::Open, passed)));
        assert!(!mergeable(&status("Draft", PrState::Open, passed)));
        assert!(!mergeable(&status("Conflicts", PrState::Open, passed)));
        assert!(!mergeable(&status("Approved", PrState::Merged, passed)));
        assert!(!mergeable(&status("Approved", PrState::Open, Counts { passed: 2, failed: 1, running: 0, queued: 0 })));
        assert!(!mergeable(&status("Approved", PrState::Open, Counts { passed: 2, failed: 0, running: 1, queued: 0 })));
        assert!(!mergeable(&status("Approved", PrState::Open, Counts { passed: 2, failed: 0, running: 0, queued: 1 })));
    }

    #[test]
    fn the_tool_describes_the_pr_in_what_a_side_panel_needs() {
        let said = describe(&link(), &status("Draft", PrState::Open, Counts { passed: 1, failed: 0, running: 2, queued: 0 }));
        assert_eq!(said["number"], 12);
        assert_eq!(said["ref"], "#12");
        assert_eq!(said["state"], "open");
        assert_eq!(said["draft"], true);
        assert_eq!(said["github"], true);
        assert_eq!(said["mergeable"], false);
        assert_eq!(said["checks"], json!({ "passed": 1, "failed": 0, "running": 2, "queued": 0 }));
        assert_eq!(said["pending"], json!([]));
        let merged = describe(&link(), &status("Merged", PrState::Merged, Counts::default()));
        assert_eq!((merged["state"].as_str(), merged["draft"].as_bool()), (Some("merged"), Some(false)));
    }

    #[test]
    fn the_tool_names_up_to_three_pending_checks_required_and_running_first_and_leaves_out_reviewer_policies() {
        use crate::types::CheckItem;
        let item = |name: &str, state: CheckState, required: Option<bool>, kind: &str, at: Option<&str>| CheckItem { id: name.into(), name: name.into(), state, required, kind: Some(kind.into()), at: at.map(str::to_owned), seconds: None, url: None, note: None, failing: None, run: None, needs: None };
        let mut st = status("Checks queued", PrState::Open, Counts { passed: 4, failed: 0, running: 1, queued: 3 });
        st.items = Some(vec![
            item("lint", CheckState::Queued, Some(false), "Build", None),
            item("AI guardrail PR check", CheckState::Queued, Some(true), "Build", Some("2026-01-01T00:00:00Z")),
            item("Minimum number of reviewers", CheckState::Queued, Some(true), "Minimum reviewers", None),
            item("Require a merge strategy", CheckState::Queued, Some(true), "Merge strategy", None),
            item("e2e", CheckState::Running, Some(false), "Build", None),
            item("unit", CheckState::Passed, Some(true), "Build", None),
            item("zeta", CheckState::Queued, Some(false), "Build", None),
        ]);
        assert_eq!(
            describe(&link(), &st)["pending"],
            json!([
                { "name": "AI guardrail PR check", "state": "queued", "since": 1_767_225_600_000.0_f64 },
                { "name": "e2e", "state": "running", "since": null },
                { "name": "lint", "state": "queued", "since": null },
            ])
        );
    }
}
