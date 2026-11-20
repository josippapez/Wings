//! The `pull_request` tool: the PR for the branch a folder is on, as one small JSON object. The Claude chat plugin calls
//! it (`callTools`) for its side panel, and a chat tab is no terminal pane, so the lookup is by folder. It shares the
//! pane lookups' cache and the status cache, so a chat and a pane on one branch cost one network call per minute.

use super::{problems, state::{self, Loaded}, status::refresh_status, links::find_for_folder};
use crate::{providers, remote::remote_for, types::{Link, PrState, Status}};
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
    st.state == PrState::Open && st.checks.failed == 0 && st.checks.running == 0 && matches!(st.label.as_str(), "Approved" | "Open")
}

fn state_word(state: PrState) -> &'static str {
    match state {
        PrState::Open => "open",
        PrState::Merged => "merged",
        PrState::Closed => "closed",
    }
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
        "checks": { "passed": st.checks.passed, "failed": st.checks.failed, "running": st.checks.running },
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
    use crate::types::{Counts, Provider};

    fn status(label: &str, state: PrState, checks: Counts) -> Status {
        Status { key: String::new(), title: "Add the cart".into(), label: label.into(), state, tone: "info".into(), icon: "pr-open".into(), checks, review: "Approved".into(), failing: vec![], items: None, changes: None, branch: "cart → main".into(), updated: String::new(), merge: None, base: None, head: None, base_commit: None, head_commit: None }
    }

    fn link() -> Link {
        Link { provider: Provider::Github, url: "https://github.com/acme/shop/pull/12".into(), number: 12, repo: "acme/shop".into(), remote: None, cwd: None, raw: None }
    }

    #[test]
    fn only_an_open_pr_with_nothing_failing_or_running_and_no_review_missing_can_merge() {
        let passed = Counts { passed: 3, failed: 0, running: 0 };
        assert!(mergeable(&status("Approved", PrState::Open, passed)));
        assert!(mergeable(&status("Open", PrState::Open, passed)));
        assert!(!mergeable(&status("In review", PrState::Open, passed)));
        assert!(!mergeable(&status("Draft", PrState::Open, passed)));
        assert!(!mergeable(&status("Conflicts", PrState::Open, passed)));
        assert!(!mergeable(&status("Approved", PrState::Merged, passed)));
        assert!(!mergeable(&status("Approved", PrState::Open, Counts { passed: 2, failed: 1, running: 0 })));
        assert!(!mergeable(&status("Approved", PrState::Open, Counts { passed: 2, failed: 0, running: 1 })));
    }

    #[test]
    fn the_tool_describes_the_pr_in_what_a_side_panel_needs() {
        let said = describe(&link(), &status("Draft", PrState::Open, Counts { passed: 1, failed: 0, running: 2 }));
        assert_eq!(said["number"], 12);
        assert_eq!(said["ref"], "#12");
        assert_eq!(said["state"], "open");
        assert_eq!(said["draft"], true);
        assert_eq!(said["github"], true);
        assert_eq!(said["mergeable"], false);
        assert_eq!(said["checks"], json!({ "passed": 1, "failed": 0, "running": 2 }));
        let merged = describe(&link(), &status("Merged", PrState::Merged, Counts::default()));
        assert_eq!((merged["state"].as_str(), merged["draft"].as_bool()), (Some("merged"), Some(false)));
    }
}
