//! Azure DevOps votes through `az repos pr set-vote`.

use crate::types::{PrState, Status};

/// The votes `az repos pr set-vote --vote` takes, as Azure's own UI words them.
pub const AZURE_VOTES: [(&str, &str); 5] = [
    ("approve", "Approve"),
    ("approve-with-suggestions", "Approve with suggestions"),
    ("wait-for-author", "Wait for author"),
    ("reject", "Reject"),
    ("reset", "Reset vote"),
];

/// The organization URLs the tracker parses a remote into.
fn is_org(org: &str) -> bool {
    let Some(rest) = org.strip_prefix("https://") else { return false };
    if let Some(name) = rest.strip_prefix("dev.azure.com/") {
        return !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'));
    }
    rest.strip_suffix(".visualstudio.com").is_some_and(|name| !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'))
}

/// The arguments of one vote. The manifest can't pin `--vote`'s value, so the vote, the PR id and the organization are checked here before `az` runs.
pub fn vote_args(number: u64, org: &str, vote: &str) -> Result<Vec<String>, String> {
    if number < 1 || number > (1 << 53) - 1 {
        return Err(format!("Not a pull request id: {number}"));
    }
    if !is_org(org) {
        return Err("Not an Azure DevOps organization URL.".into());
    }
    if !AZURE_VOTES.iter().any(|(name, _)| *name == vote) {
        return Err(format!("Not an Azure DevOps vote: {vote}"));
    }
    Ok(["repos", "pr", "set-vote", "--id", &number.to_string(), "--vote", vote, "--organization", org, "-o", "json"].map(str::to_owned).into())
}

/// Whether the PR tab offers Approve and the Vote menu: only while the PR is open, whatever its forge calls a closed one.
pub fn reviewable(status: Option<&Status>) -> bool {
    status.is_some_and(|status| status.state == PrState::Open)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_the_set_vote_command_for_each_vote_azure_offers() {
        for (vote, _) in AZURE_VOTES {
            assert_eq!(vote_args(42, "https://dev.azure.com/acme", vote).unwrap(), ["repos", "pr", "set-vote", "--id", "42", "--vote", vote, "--organization", "https://dev.azure.com/acme", "-o", "json"]);
        }
        assert_eq!(vote_args(1, "https://acme.visualstudio.com", "reset").unwrap()[8], "https://acme.visualstudio.com");
    }

    #[test]
    fn refuses_a_pr_id_organization_or_vote_that_isnt_what_the_tracker_parsed() {
        assert!(vote_args(0, "https://dev.azure.com/acme", "approve").unwrap_err().contains("pull request id"));
        for org in ["http://dev.azure.com/acme", "https://dev.azure.com/acme/", "https://dev.azure.com.evil.net/acme", "https://evil.example/dev.azure.com/acme", "https://dev.azure.com/a/b", "@/etc/passwd", ""] {
            assert!(vote_args(1, org, "approve").unwrap_err().contains("organization"), "{org}");
        }
        for vote in ["APPROVE", "10", "--vote", "@file", "constructor", "__proto__", ""] {
            assert!(vote_args(1, "https://dev.azure.com/acme", vote).unwrap_err().contains("vote"), "{vote}");
        }
    }

    #[test]
    fn approve_and_the_vote_menu_are_only_for_an_open_pr() {
        let status = |state| Status { key: String::new(), title: String::new(), label: String::new(), state, tone: String::new(), icon: String::new(), checks: Default::default(), review: String::new(), failing: vec![], items: None, changes: None, branch: String::new(), updated: String::new(), merge: None, base: None, head: None, base_commit: None, head_commit: None };
        assert!(reviewable(Some(&status(PrState::Open))));
        assert!(!reviewable(Some(&status(PrState::Closed))));
        assert!(!reviewable(Some(&status(PrState::Merged))));
        assert!(!reviewable(None));
    }
}
