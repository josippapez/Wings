//! What every forge's mappers share: the label a PR gets, which PR a branch has, a second page, and a failure with no
//! log.

use crate::forge::ForgeError;
use crate::types::{Counts, FailureDetail, Failing, Link, PrState};
use std::future::Future;
use wings_plugin::cli::Kind;

pub fn counts(results: &[&str]) -> Counts {
    let mut c = Counts::default();
    for result in results {
        match *result {
            "passed" => c.passed += 1,
            "failed" => c.failed += 1,
            _ => c.running += 1,
        }
    }
    c
}

/// A step of a ladder: whether it holds, then its label, tone and icon (`pr-open` when none).
pub type Step<'a> = (bool, &'a str, &'a str, Option<&'a str>);

/// The first step that holds names the state, as its label, tone and icon; `rest` (default "In review") when none does.
pub fn ladder(steps: &[Step], rest: Option<&str>) -> (String, String, String) {
    match steps.iter().find(|step| step.0) {
        Some((_, label, tone, icon)) => ((*label).into(), (*tone).into(), icon.unwrap_or("pr-open").into()),
        None => (rest.unwrap_or("In review").into(), "info".into(), "pr-open".into()),
    }
}

#[derive(Default)]
pub struct Facts {
    pub merged: bool,
    pub closed: bool,
    pub draft: bool,
    pub conflicts: bool,
    /// Azure DevOps: a reviewer rejected it.
    pub rejected: bool,
    pub checks: Counts,
    /// A reviewer asked for changes.
    pub changes: bool,
    /// GitLab: discussions are still to resolve.
    pub unresolved: bool,
    pub approved: bool,
    /// GitHub: a review is required and hasn't been given.
    pub reviewing: bool,
}

/// What a provider calls the states it words its own way, and what an open PR with nothing to say is called.
#[derive(Default)]
pub struct Words<'a> {
    pub closed: Option<&'a str>,
    pub changes: Option<&'a str>,
    pub rest: Option<&'a str>,
}

/// The label, tone and icon every forge's PR goes through, in the order they apply; what is true first wins. Then its state.
pub fn label_of(f: &Facts, words: &Words) -> (String, String, String, PrState) {
    let (label, tone, icon) = ladder(
        &[
            (f.merged, "Merged", "merged", Some("pr-merged")),
            (f.closed, words.closed.unwrap_or("Closed"), "neutral", Some("pr-closed")),
            (f.draft, "Draft", "neutral", Some("pr-draft")),
            (f.conflicts, "Conflicts", "danger", None),
            (f.rejected, "Rejected", "danger", None),
            (f.checks.failed > 0, "Checks failing", "danger", None),
            (f.changes, words.changes.unwrap_or("Changes requested"), "danger", None),
            (f.checks.running > 0, "Checks running", "warning", None),
            (f.unresolved, "Unresolved threads", "warning", None),
            (f.approved, "Approved", "success", None),
            (f.reviewing, "In review", "info", None),
        ],
        words.rest,
    );
    let state = if f.merged { PrState::Merged } else if f.closed { PrState::Closed } else { PrState::Open };
    (label, tone, icon, state)
}

/// Whether a login or name from a list is the user's, which providers spell with any case.
pub fn same(a: Option<&str>, b: &str) -> bool {
    a.is_some_and(|a| !a.is_empty() && a.to_lowercase() == b.to_lowercase())
}

/// The open PR for a branch wins; otherwise the newest, which a provider lists first.
pub fn open_or_newest<T>(prs: &[T], is_open: impl Fn(&T) -> bool) -> Option<&T> {
    prs.iter().find(|pr| is_open(pr)).or(prs.first())
}

/// A list call answers a page of `size`. A repo with more gets the second page too, which is as far as a list goes.
pub async fn with_second_page<T, Fut: Future<Output = Result<Vec<T>, ForgeError>>>(mut first: Vec<T>, size: usize, second: impl FnOnce() -> Fut) -> Result<Vec<T>, ForgeError> {
    if first.len() == size {
        first.extend(second().await?);
    }
    Ok(first)
}

/// A failed check with no log to read: what it says, and where the CI shows it.
pub fn no_log(check: Option<&str>, summary: &str, url: Option<&str>) -> Vec<FailureDetail> {
    vec![FailureDetail { check: check.map(str::to_owned), step: None, summary: summary.to_owned(), log: String::new(), url: url.map(str::to_owned) }]
}

/// The failure of a check on a forge whose CI has no log to fetch, only the summary and a link to the CI's own page.
pub async fn plain_failure(_link: &Link, f: &Failing) -> Result<Vec<FailureDetail>, ForgeError> {
    Ok(no_log(f.name.as_deref(), f.summary.as_deref().unwrap_or("Failed"), f.url.as_deref()))
}

/// A read whose failure only costs what it adds: `fallback` when its CLI ran and failed. A CLI that isn't installed
/// still fails, and so does any other kind of error.
pub fn or_else<T>(read: Result<T, ForgeError>, fallback: T) -> Result<T, ForgeError> {
    match read {
        Err(ForgeError::Cli(error)) if error.kind != Kind::Missing => Ok(fallback),
        other => other,
    }
}

/// A value in a query string. `wings.fetch` takes only a URL already in normal form, which leaves `%XX` as written
/// but would rewrite a bare `'`, so everything but the unreserved characters is escaped.
pub fn encode_query(value: &str) -> String {
    let mut out = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use wings_plugin::{cli::CliError, task::block_on};

    fn label(facts: Facts) -> String {
        label_of(&facts, &Words::default()).0
    }

    fn checks(failed: u32, running: u32) -> Counts {
        Counts { passed: 0, failed, running }
    }

    #[test]
    fn a_query_value_stays_in_the_normal_form_wings_fetch_needs() {
        assert_eq!(encode_query("a b&c=d"), "a%20b%26c%3Dd");
        assert_eq!(encode_query(r#"source.branch.name="feature/it's (x)!*""#), "source.branch.name%3D%22feature%2Fit%27s%20%28x%29%21%2A%22");
        assert_eq!(encode_query("ü/é"), "%C3%BC%2F%C3%A9");
        assert_eq!(encode_query("plain-._~"), "plain-._~");
    }

    #[test]
    fn the_label_of_a_pr_is_the_first_thing_that_is_true_from_merged_down_to_approved() {
        assert_eq!(label(Facts { merged: true, closed: true, draft: true, conflicts: true, ..Default::default() }), "Merged");
        assert_eq!(label(Facts { closed: true, draft: true, ..Default::default() }), "Closed");
        assert_eq!(label(Facts { draft: true, conflicts: true, ..Default::default() }), "Draft");
        assert_eq!(label(Facts { conflicts: true, rejected: true, checks: checks(1, 0), ..Default::default() }), "Conflicts");
        assert_eq!(label(Facts { rejected: true, checks: checks(1, 0), ..Default::default() }), "Rejected");
        assert_eq!(label(Facts { checks: checks(1, 1), changes: true, ..Default::default() }), "Checks failing");
        assert_eq!(label(Facts { changes: true, checks: checks(0, 2), ..Default::default() }), "Changes requested");
        assert_eq!(label(Facts { checks: checks(0, 2), unresolved: true, ..Default::default() }), "Checks running");
        assert_eq!(label(Facts { unresolved: true, approved: true, ..Default::default() }), "Unresolved threads");
        assert_eq!(label(Facts { approved: true, reviewing: true, ..Default::default() }), "Approved");
        assert_eq!(label(Facts { reviewing: true, ..Default::default() }), "In review");
        assert_eq!(label(Facts::default()), "In review");
    }

    #[test]
    fn a_provider_can_word_closed_and_changes_its_own_way_and_the_state_says_open_merged_or_closed() {
        let said = |facts: Facts, words: Words| {
            let (a, b, c, d) = label_of(&facts, &words);
            (a, b, c, d)
        };
        assert_eq!(said(Facts { closed: true, ..Default::default() }, Words { closed: Some("Abandoned"), ..Default::default() }), ("Abandoned".into(), "neutral".into(), "pr-closed".into(), PrState::Closed));
        assert_eq!(said(Facts { changes: true, ..Default::default() }, Words { changes: Some("Waiting for author"), ..Default::default() }), ("Waiting for author".into(), "danger".into(), "pr-open".into(), PrState::Open));
        assert_eq!(said(Facts::default(), Words { rest: Some("Open"), ..Default::default() }), ("Open".into(), "info".into(), "pr-open".into(), PrState::Open));
        assert_eq!(said(Facts { reviewing: true, ..Default::default() }, Words { rest: Some("Open"), ..Default::default() }).0, "In review");
        assert_eq!(said(Facts { merged: true, ..Default::default() }, Words::default()), ("Merged".into(), "merged".into(), "pr-merged".into(), PrState::Merged));
        assert_eq!(said(Facts { merged: true, closed: true, ..Default::default() }, Words::default()).3, PrState::Merged);
    }

    #[test]
    fn a_ladder_of_its_own_keeps_the_first_step_that_holds_and_the_icon_open_unless_a_step_names_one() {
        assert_eq!(ladder(&[(false, "A", "info", None), (true, "B", "danger", None), (true, "C", "success", None)], None), ("B".into(), "danger".into(), "pr-open".into()));
        assert_eq!(ladder(&[(true, "Draft", "neutral", Some("pr-draft"))], None), ("Draft".into(), "neutral".into(), "pr-draft".into()));
        assert_eq!(ladder(&[], None), ("In review".into(), "info".into(), "pr-open".into()));
    }

    #[test]
    fn the_open_pr_of_a_branch_wins_over_a_newer_one_else_the_newest() {
        let prs = [(3, false), (2, true), (1, false)];
        assert_eq!(open_or_newest(&prs, |p| p.1).unwrap().0, 2);
        assert_eq!(open_or_newest(&[(9, false), (8, false)], |p| p.1).unwrap().0, 9);
        assert!(open_or_newest::<(i32, bool)>(&[], |_| true).is_none());
    }

    #[test]
    fn a_list_that_fills_its_page_gets_the_second_page_too_and_a_short_one_doesnt_ask() {
        let asked = std::rc::Rc::new(std::cell::Cell::new(0));
        let (a, b) = (asked.clone(), asked.clone());
        let full = block_on(with_second_page(vec!["a", "b"], 2, move || async move { a.set(a.get() + 1); Ok(vec!["c"]) }));
        assert_eq!(full.unwrap(), ["a", "b", "c"]);
        let short = block_on(with_second_page(vec!["a"], 2, move || async move { b.set(b.get() + 1); Ok(vec!["c"]) }));
        assert_eq!(short.unwrap(), ["a"]);
        assert_eq!(asked.get(), 1);
    }

    #[test]
    fn a_failed_check_with_no_log_has_its_summary_and_link_and_failed_when_nothing_says_why() {
        assert_eq!(no_log(Some("build"), "Timed out", Some("https://ci/1")), [FailureDetail { check: Some("build".into()), step: None, summary: "Timed out".into(), log: String::new(), url: Some("https://ci/1".into()) }]);
        let link = Link { provider: crate::types::Provider::Github, url: String::new(), number: 1, repo: String::new(), remote: None, cwd: None, raw: None };
        let f = Failing { id: "1".into(), name: Some("build".into()), url: Some("https://ci/1".into()), ..Default::default() };
        assert_eq!(block_on(async move { plain_failure(&link, &f).await }).unwrap()[0].summary, "Failed");
    }

    #[test]
    fn a_read_that_only_adds_to_what_is_shown_gives_its_fallback_when_the_cli_ran_and_failed() {
        let cli = |kind| ForgeError::Cli(CliError { kind, message: "x".into() });
        assert_eq!(or_else(Ok(vec!["a"]), vec![]).unwrap(), ["a"]);
        assert!(or_else(Err(cli(Kind::Failed)), vec!["x"]).unwrap() == ["x"]);
        assert!(or_else(Err(cli(Kind::SignedOut)), vec!["x"]).unwrap() == ["x"]);
        assert!(matches!(or_else(Err(cli(Kind::Missing)), vec!["x"]), Err(ForgeError::Cli(_))));
        assert!(or_else(Err(ForgeError::Other("not JSON".into())), vec!["x"]).is_err());
    }

    #[test]
    fn counts_tally_results_and_a_name_matches_whatever_its_case() {
        assert_eq!(counts(&["passed", "failed", "passed", "running"]), Counts { passed: 2, failed: 1, running: 1 });
        assert!(same(Some("Ada"), "ada"));
        assert!(!same(None, "ada"));
        assert!(!same(Some(""), ""));
    }
}

/// Writes through a CLI (`wings.post`): a reply or an approval, to the paths the manifest's `post` lists. Gives what
/// the CLI printed, parsed, or its error's last line. A failure records no sign-in problem, as a read would.
pub async fn post_json(cli: &wings_plugin::cli::Cli, url: &str, fields: serde_json::Value, host: Option<&str>, key: &str) -> Result<serde_json::Value, ForgeError> {
    let out = cli.post(url, &fields, host, key, true).await.map_err(|failure| ForgeError::Other(wings_plugin::cli::last_line(&failure.error.message).to_owned()))?;
    serde_json::from_str(&out).map_err(|error| ForgeError::Other(error.to_string()))
}
