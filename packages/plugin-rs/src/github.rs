//! GitHub reads that one `gh` call answers for many repositories, as `wings-plugin-sdk/github` does, so a plugin that
//! lists the pull requests of every project starts one process, not two per project. The plugin lists `gh graphql` in
//! `permissions.post`: `gh api graphql` takes its query in a field, which `exec` refuses.

use crate::{
    cache::Batch,
    cli::{Cli, CliError, Kind},
};
use serde_json::{Value, json};
use std::future::Future;

const DOTCOM: &str = "github.com";
/// Open PRs read per repository: what `gh pr list --limit 100` read.
const PULLS_PER_REPO: u32 = 100;
/// Repositories per query. GitHub refuses a query that could return over 500,000 nodes, and a repository of 100 PRs
/// with 100 checks each counts about 10,000.
const REPOS_PER_QUERY: usize = 25;

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct GithubRepo {
    pub owner: String,
    pub name: String,
}

/// A repository's open PRs, newest first, and the numbers of the ones among them that ask for your review.
#[derive(Clone, Debug)]
pub struct Pulls {
    pub pulls: Vec<Value>,
    pub asked: Vec<u64>,
}

/// The open pull requests of GitHub repositories, read through `gh api graphql`: one call for every repository asked in
/// the same turn, and in it the numbers of the PRs that ask for your review (yours or your team's), which one search
/// answers for every repository. It keeps nothing: put a `Cached` over it for a TTL.
#[derive(Clone)]
pub struct GithubPulls(Batch<GithubRepo, Result<Pulls, CliError>>);

/// `selection` is what to read of each PullRequest, as GraphQL text. `host` is a GitHub Enterprise Server (or
/// github.com), `key` what the CLI records a sign-in problem under.
pub fn github_pulls(cli: &Cli, selection: &str, host: &str, key: &str) -> GithubPulls {
    let (cli, selection, host, key) = (cli.clone(), selection.to_owned(), host.to_owned(), key.to_owned());
    GithubPulls(Batch::new(REPOS_PER_QUERY, move |repos: Vec<GithubRepo>| {
        let (cli, selection, host, key) = (cli.clone(), selection.clone(), host.clone(), key.clone());
        async move { Ok(read(&cli, &selection, &host, &key, &repos).await) }
    }))
}

impl GithubPulls {
    /// The repository's PRs. A repository that can't be read fails its own `get`.
    pub fn get(&self, repo: GithubRepo) -> impl Future<Output = Result<Pulls, CliError>> + use<> {
        let asked = self.0.get(repo);
        // The batch itself never fails: each repository's answer carries its own error.
        async move { asked.await.map_err(|message| CliError { kind: Kind::Failed, message })? }
    }
}

fn query_for(repos: &[GithubRepo], selection: &str) -> String {
    let quote = |text: &str| Value::from(text).to_string();
    let reads: Vec<String> = repos
        .iter()
        .enumerate()
        .map(|(i, r)| format!("r{i}: repository(owner: {}, name: {}) {{ pullRequests(states: OPEN, first: {PULLS_PER_REPO}, orderBy: {{ field: CREATED_AT, direction: DESC }}) {{ nodes {{ {selection} }} }} }}", quote(&r.owner), quote(&r.name)))
        .collect();
    let ask = r#"asked: search(query: "is:pr is:open review-requested:@me", type: ISSUE, first: 100) { nodes { ... on PullRequest { number repository { nameWithOwner } } } }"#;
    format!("query {{ {} {ask} }}", reads.join(" "))
}

async fn read(cli: &Cli, selection: &str, host: &str, key: &str, repos: &[GithubRepo]) -> Vec<Result<Pulls, CliError>> {
    let query = query_for(repos, selection);
    let host = (host != DOTCOM).then_some(host);
    let text = match cli.post("graphql", &json!({ "query": query }), host, key, false).await {
        Ok(text) => text,
        // `gh api` prints the answer and exits 1 when it has errors.
        Err(failure) => match partial(&failure.stdout) {
            Some(_) => failure.stdout,
            None => return repos.iter().map(|_| Err(failure.error.clone())).collect(),
        },
    };
    let failed = |message: String| repos.iter().map(|_| Err(CliError { kind: Kind::Failed, message: message.clone() })).collect();
    let Ok(answer) = serde_json::from_str::<Value>(&text) else {
        return failed("gh printed something that isn't JSON".into());
    };
    if answer["data"].is_null() {
        return failed("gh api graphql answered with no data".into());
    }
    let asked = answer["data"]["asked"]["nodes"].as_array().cloned().unwrap_or_default();
    repos
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let found = &answer["data"][format!("r{i}")];
            let Some(nodes) = found["pullRequests"]["nodes"].as_array() else {
                let message = answer["errors"]
                    .as_array()
                    .and_then(|errors| errors.iter().find(|e| e["path"][0] == format!("r{i}")))
                    .and_then(|e| e["message"].as_str())
                    .map_or_else(|| format!("{}/{} couldn't be read", r.owner, r.name), str::to_owned);
                return Err(CliError { kind: Kind::Failed, message });
            };
            let name = format!("{}/{}", r.owner, r.name).to_lowercase();
            let asked = asked
                .iter()
                .filter(|n| n["repository"]["nameWithOwner"].as_str().is_some_and(|of| of.to_lowercase() == name))
                .filter_map(|n| n["number"].as_u64())
                .collect();
            Ok(Pulls { pulls: nodes.clone(), asked })
        })
        .collect()
}

/// The data of a GraphQL answer that came with errors, or none when it has none.
fn partial(stdout: &str) -> Option<Value> {
    serde_json::from_str::<Value>(stdout).ok().filter(|answer| !answer["data"].is_null())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        host,
        task::{block_on, join_all},
    };
    use std::{cell::RefCell, future::Future, pin::Pin, rc::Rc};

    fn gh() -> Cli {
        Cli::new("gh", "GitHub CLI", |stderr| stderr.contains("gh auth login"), &[])
    }

    fn repo(owner: &str, name: &str) -> GithubRepo {
        GithubRepo { owner: owner.into(), name: name.into() }
    }

    #[test]
    fn every_repository_of_a_turn_and_the_review_search_are_one_gh_post_and_a_repository_that_cant_be_read_fails_alone() {
        let posts: Rc<RefCell<Vec<Value>>> = Rc::default();
        let seen = posts.clone();
        host::testing::answer(move |method, params| {
            assert_eq!(method, "post");
            seen.borrow_mut().push(params.clone());
            let data = json!({
                "r0": { "pullRequests": { "nodes": [{ "number": 1 }, { "number": 2 }] } },
                "r1": null,
                "r2": { "pullRequests": { "nodes": [{ "number": 3 }] } },
                "asked": { "nodes": [
                    { "number": 2, "repository": { "nameWithOwner": "Octo/Repo" } },
                    { "number": 3, "repository": { "nameWithOwner": "octo/other" } },
                    {}
                ] },
            });
            // `gh api` prints the answer and exits 1 when it has errors.
            let stdout = json!({ "data": data, "errors": [{ "path": ["r1"], "message": "Could not resolve to a Repository" }] }).to_string();
            Ok(json!({ "code": 1, "stdout": stdout, "stderr": "gh: Could not resolve to a Repository" }))
        });
        let pulls = github_pulls(&gh(), "number", "github.com", "github");
        let asked: Vec<Pin<Box<dyn Future<Output = Result<Pulls, CliError>>>>> =
            [repo("octo", "repo"), repo("octo", "gone"), repo("octo", "other")].map(|r| Box::pin(pulls.get(r)) as Pin<Box<dyn Future<Output = _>>>).into();
        let mut answers = block_on(join_all(asked)).into_iter();
        let (a, b, c) = (answers.next().unwrap(), answers.next().unwrap(), answers.next().unwrap());
        let posts = posts.borrow();
        assert_eq!(posts.len(), 1);
        assert_eq!((posts[0]["program"].as_str(), posts[0]["url"].as_str(), posts[0]["host"].is_null()), (Some("gh"), Some("graphql"), true));
        assert!(posts[0]["fields"]["query"].as_str().unwrap().contains(r#"r0: repository(owner: "octo", name: "repo")"#));
        let (a, c) = (a.unwrap(), c.unwrap());
        // The search is split by repository, whatever the case.
        assert_eq!((a.pulls.len(), a.asked, c.asked), (2, vec![2], vec![3]));
        assert_eq!(b.unwrap_err().message, "Could not resolve to a Repository");
    }

    #[test]
    fn an_enterprise_host_is_named_to_post_and_a_signed_out_gh_is_the_clis_problem() {
        let host_seen: Rc<RefCell<Value>> = Rc::default();
        let seen = host_seen.clone();
        host::testing::answer(move |_, params| {
            *seen.borrow_mut() = params["host"].clone();
            Ok(json!({ "code": 1, "stdout": "", "stderr": "To get started with GitHub CLI, please run:  gh auth login" }))
        });
        let cli = gh();
        let reads = github_pulls(&cli, "number", "ghe.example.com", "github:ghe.example.com");
        let error = block_on(async move { reads.get(repo("o", "r")).await }).unwrap_err();
        assert_eq!(error.kind, Kind::SignedOut);
        assert_eq!(*host_seen.borrow(), "ghe.example.com");
        assert_eq!(cli.problem("github:ghe.example.com").unwrap().0, Kind::SignedOut);
    }
}
