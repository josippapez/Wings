//! The commits of a repo, a page at a time.

use crate::log::{Commit, LOG, parse_log};
use wings_plugin::{
    git::{ok, raw},
    task::join,
};

/// Commits the History tab loads at a time.
pub const PAGE: usize = 50;

/// One page of the current branch's commits, or every branch's, newest first, and whether there are more. A search
/// matches the message, the author or the start of a hash: git ANDs `--grep` with `--author`, so they're two lookups
/// merged by date.
pub async fn history_page(root: &str, query: &str, all: bool, skip: usize) -> Result<(Vec<Commit>, bool), String> {
    let scope: &[&str] = if all { &["--all"] } else { &[] };
    if query.is_empty() {
        let (max, from) = (format!("--max-count={}", PAGE + 1), format!("--skip={skip}"));
        let mut commits = parse_log(&raw(root, &[&["log"], &LOG[..], &[&max, &from], scope].concat()).await?);
        let more = commits.len() > PAGE;
        commits.truncate(PAGE);
        return Ok((commits, more));
    }
    let max = format!("--max-count={}", skip + PAGE + 1);
    let matching = [&["log"], &LOG[..], &["--regexp-ignore-case", "--fixed-strings", &max]].concat();
    let hash = if (4..=40).contains(&query.len()) && query.bytes().all(|b| b.is_ascii_hexdigit()) {
        ok(root, &["rev-parse", "--verify", "--quiet", &format!("{query}^{{commit}}")]).await.filter(|hash| !hash.is_empty())
    } else {
        None
    };
    let (grep, author) = (format!("--grep={query}"), format!("--author={query}"));
    let (by_message, by_author) = join(raw(root, &[&matching[..], &[&grep], scope].concat()), raw(root, &[&matching[..], &[&author], scope].concat())).await;
    let by_hash = match &hash {
        Some(hash) => raw(root, &[&["log"], &LOG[..], &["--max-count=1", hash, "--"]].concat()).await?,
        None => String::new(),
    };
    let mut found = [parse_log(&by_message?), parse_log(&by_author?)].concat();
    found.sort_by(|a, b| b.ct.total_cmp(&a.ct));
    let mut merged: Vec<Commit> = Vec::new();
    for commit in parse_log(&by_hash).into_iter().chain(found) {
        if merged.iter().all(|seen| seen.hash != commit.hash) {
            merged.push(commit);
        }
    }
    let more = merged.len() > skip + PAGE;
    Ok((merged.into_iter().skip(skip).take(PAGE).collect(), more))
}

/// The History tab's rows once a first page arrives for the list it already shows. The rows after that page that are
/// still in the old list stay, so a reload after a new commit doesn't drop the pages Load more added (and scroll you
/// up). When the page's last commit isn't among the shown rows (history was rewritten), the page replaces them.
pub fn with_first_page(shown: &[Commit], page: Vec<Commit>) -> Vec<Commit> {
    let at = page.last().and_then(|last| shown.iter().position(|commit| commit.hash == last.hash));
    match at {
        Some(at) => page.into_iter().chain(shown[at + 1..].iter().cloned()).collect(),
        None => page,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{changes::tests::fake_git, log::tests::log_of};
    use wings_plugin::task::block_on;

    fn rows(from: usize, to: usize) -> Vec<Commit> {
        (from..to).map(|n| Commit { hash: format!("c{n}"), ..Commit::default() }).collect()
    }
    fn hashes(rows: &[Commit]) -> Vec<&str> {
        rows.iter().map(|row| row.hash.as_str()).collect()
    }

    #[test]
    fn a_first_page_after_a_new_commit_keeps_the_rows_load_more_added() {
        // 100 rows shown (two pages), a commit lands and the reload brings the new first 50.
        let page: Vec<Commit> = [rows(1000, 1001), rows(0, 49)].concat();
        let next = with_first_page(&rows(0, 100), page);
        assert_eq!(next.len(), 101);
        assert_eq!(hashes(&next)[..3], ["c1000", "c0", "c1"]);
        assert_eq!(next.last().unwrap().hash, "c99");
        // An unchanged first page leaves every row.
        assert_eq!(with_first_page(&rows(0, 100), rows(0, 50)), rows(0, 100));
        // A page whose last commit isn't shown any more replaces the rows.
        assert_eq!(with_first_page(&rows(0, 100), rows(500, 502)), rows(500, 502));
        assert!(with_first_page(&rows(0, 100), Vec::new()).is_empty());
    }

    #[test]
    fn a_search_merges_the_commits_that_match_by_message_author_and_hash_once_each() {
        let calls = fake_git(&[("log -z", 0, "")]);
        let (commits, more) = block_on(async { history_page("/r", "", true, 50).await }).unwrap();
        assert!(commits.is_empty() && !more);
        assert!(calls.borrow()[0].ends_with("--max-count=51 --skip=50 --all"), "{:?}", calls.borrow());

        thread_local!(static LOGS: [String; 2] = [log_of(&[("aaaa1111", "", "", "fix the cart")]), log_of(&[("bbbb2222", "", "", "by cart-bot"), ("aaaa1111", "", "", "fix the cart")])]);
        let calls: std::rc::Rc<std::cell::RefCell<Vec<String>>> = Default::default();
        let seen = calls.clone();
        wings_plugin::host::testing::answer(move |_, params| {
            let line = params["args"].as_array().unwrap().iter().filter_map(serde_json::Value::as_str).collect::<Vec<_>>().join(" ");
            seen.borrow_mut().push(line.clone());
            let stdout = LOGS.with(|logs| if line.contains("--grep=cart") { logs[0].clone() } else if line.contains("--author=cart") { logs[1].clone() } else { String::new() });
            Ok(serde_json::json!({ "code": 0, "stdout": stdout, "stderr": "" }))
        });
        let (commits, more) = block_on(async { history_page("/r", "cart", false, 0).await }).unwrap();
        assert_eq!(hashes(&commits), ["aaaa1111", "bbbb2222"]);
        assert!(!more);
        // "cart" isn't the start of a hash, so git isn't asked for one.
        assert!(calls.borrow().iter().all(|call| !call.starts_with("rev-parse")));
    }
}
