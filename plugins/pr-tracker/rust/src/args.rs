//! A PR to and from the args its tab is opened with.

use crate::types::{Link, PrArgs, Provider};
use serde_json::Value;
use wings_plugin::{azure::parse_remote as azure_remote, remote::Remote};

/// The args a "pr" tab opens with: the PR's own fields, and the remote of a provider that needs more than the URL.
pub fn args_of(link: &Link, title: &str, cwd: &str) -> PrArgs {
    PrArgs { provider: link.provider, url: link.url.clone(), number: link.number, repo: link.repo.clone(), title: title.to_owned(), cwd: cwd.to_owned(), remote: link.remote.clone() }
}

fn provider_of(name: &str) -> Option<Provider> {
    Some(match name {
        "github" => Provider::Github,
        "azure" => Provider::Azure,
        "gitlab" => Provider::Gitlab,
        "bitbucket" => Provider::Bitbucket,
        "gitea" => Provider::Gitea,
        _ => return None,
    })
}

/// The args a tab was opened with, or none when they aren't a pull request's. They come from Wings' saved state as JSON, so they are checked, not trusted.
pub fn read_args(raw: &Value) -> Option<PrArgs> {
    let text = |name: &str| raw.get(name).and_then(Value::as_str).map(str::to_owned);
    // A link in a page (`contributes.links`) opens the tab with the address alone.
    if raw.get("provider").is_none() {
        return args_of_url(raw.get("url")?.as_str()?, &text("cwd").unwrap_or_default());
    }
    let remote = match raw.get("remote") {
        None => None,
        Some(remote) => Some(Remote::from_json(remote)?),
    };
    Some(PrArgs {
        provider: provider_of(raw.get("provider")?.as_str()?)?,
        url: text("url")?,
        number: raw.get("number")?.as_u64()?,
        repo: text("repo")?,
        title: text("title")?,
        cwd: text("cwd")?,
        remote,
    })
}

/// The args for a pull request's web address, as a link in a page gives it: GitHub's `/<owner>/<repo>/pull/<n>` and Azure
/// DevOps' `/<project>/_git/<repo>/pullrequest/<n>` on dev.azure.com or `<org>.visualstudio.com`, with nothing after the
/// number but a fragment, a query or a further page of it. The title is the PR's own to fill in, and `cwd` may be empty.
pub fn args_of_url(url: &str, cwd: &str) -> Option<PrArgs> {
    let bare = url.split(['#', '?']).next()?;
    let number = |text: &str| text.split('/').next()?.parse::<u64>().ok().filter(|n| *n > 0);
    let args = |provider, url: String, number, repo: String, remote| PrArgs { provider, url, number, repo, title: String::new(), cwd: cwd.to_owned(), remote };
    if let Some(path) = bare.strip_prefix("https://github.com/") {
        let parts: Vec<&str> = path.split('/').collect();
        if let [owner, repo, "pull", n, ..] = parts[..]
            && !owner.is_empty()
            && !repo.is_empty()
            && let Some(number) = number(n)
        {
            return Some(args(Provider::Github, format!("https://github.com/{owner}/{repo}/pull/{number}"), number, format!("{owner}/{repo}"), None));
        }
        return None;
    }
    let (repo_url, rest) = bare.split_once("/pullrequest/")?;
    let number = number(rest)?;
    let remote = azure_remote(repo_url)?;
    let url = format!("{repo_url}/pullrequest/{number}");
    let repo = format!("{}/{}", remote.project, remote.repo);
    Some(args(Provider::Azure, url, number, repo, Some(Remote::Azure(remote))))
}

/// The key a tab's PR is kept under: its args without the title and the project, so tabs opened from two projects share it.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct PrKey {
    pub provider: Provider,
    pub url: String,
    pub number: u64,
    pub repo: String,
    pub remote: Option<Remote>,
}

pub fn pr_key(args: &PrArgs) -> PrKey {
    PrKey { provider: args.provider, url: args.url.clone(), number: args.number, repo: args.repo.clone(), remote: args.remote.clone() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wings_plugin::remote::parse_remote;

    fn github() -> Link {
        Link { provider: Provider::Github, url: "https://github.com/o/r/pull/5".into(), number: 5, repo: "o/r".into(), remote: None, cwd: None, raw: None }
    }

    fn gitlab() -> Link {
        Link { provider: Provider::Gitlab, url: "https://gitlab.com/g/p/-/merge_requests/5".into(), number: 5, repo: "g/p".into(), remote: Some(parse_remote("https://gitlab.com/g/p.git")), cwd: None, raw: None }
    }

    #[test]
    fn a_tabs_args_are_the_prs_fields_and_for_a_provider_that_needs_one_its_remote() {
        let args = args_of(&github(), "Fix", "/work/r");
        assert_eq!((args.url.as_str(), args.title.as_str(), args.cwd.as_str(), args.remote.is_none()), ("https://github.com/o/r/pull/5", "Fix", "/work/r", true));
        assert_eq!(serde_json::to_value(&args).unwrap().get("remote"), None);
        assert_eq!(args_of(&gitlab(), "Fix", "/work/r").remote, gitlab().remote);
    }

    #[test]
    fn what_a_tab_saved_reads_back_as_the_same_args_whatever_json_did_to_it_in_between() {
        for link in [github(), gitlab()] {
            let args = args_of(&link, "Fix", "/work/r");
            let saved = serde_json::to_string(&args).unwrap();
            assert_eq!(read_args(&serde_json::from_str(&saved).unwrap()), Some(args));
        }
    }

    #[test]
    fn args_that_arent_a_pull_requests_are_refused_so_a_tab_opened_with_something_else_lists_the_projects_prs() {
        let ok = serde_json::to_value(args_of(&github(), "Fix", "/work/r")).unwrap();
        let with = |name: &str, value: Value| {
            let mut v = ok.clone();
            v[name] = value;
            v
        };
        let mut no_title = ok.clone();
        no_title.as_object_mut().unwrap().remove("title");
        for bad in [Value::Null, json!("x"), json!(5), json!([]), json!({}), with("provider", json!("svn")), with("provider", json!("toString")), with("number", json!("5")), with("url", json!(5)), no_title, with("remote", json!("gitlab")), with("remote", json!({}))] {
            assert_eq!(read_args(&bad), None, "{bad}");
        }
    }

    #[test]
    fn a_pull_requests_address_alone_opens_a_tab_for_github_and_azure_devops() {
        let github = read_args(&json!({ "url": "https://github.com/acme/widgets/pull/12/files#diff" })).unwrap();
        assert_eq!((github.provider, github.url.as_str(), github.number, github.repo.as_str(), github.cwd.as_str(), github.remote.is_none()), (Provider::Github, "https://github.com/acme/widgets/pull/12", 12, "acme/widgets", "", true));
        let azure = read_args(&json!({ "url": "https://dev.azure.com/acme/My%20Project/_git/app/pullrequest/42" })).unwrap();
        assert_eq!((azure.provider, azure.number, azure.repo.as_str()), (Provider::Azure, 42, "My Project/app"));
        assert_eq!(azure.remote, Some(Remote::Azure(wings_plugin::azure::AzureRemote { org: "https://dev.azure.com/acme".into(), project: "My Project".into(), repo: "app".into() })));
        let old = read_args(&json!({ "url": "https://acme.visualstudio.com/Proj/_git/app/pullrequest/7", "cwd": "/work/app" })).unwrap();
        assert_eq!((old.number, old.cwd.as_str(), old.url.as_str()), (7, "/work/app", "https://acme.visualstudio.com/Proj/_git/app/pullrequest/7"));
        for bad in ["https://github.com/acme/widgets/issues/12", "https://github.com/acme/widgets/pull/0", "https://example.com/acme/widgets/pull/1", "https://dev.azure.com/acme/Proj/_git/app/pullrequest/x", "https://dev.azure.com/acme/Proj/pullrequest/4"] {
            assert_eq!(read_args(&json!({ "url": bad })), None, "{bad}");
        }
    }

    #[test]
    fn tabs_on_one_pr_share_a_key_whatever_their_title_and_project_and_two_prs_dont() {
        assert_eq!(pr_key(&args_of(&gitlab(), "Fix", "/work/r")), pr_key(&args_of(&gitlab(), "Fix it", "/elsewhere/r")));
        let other = Link { url: "https://github.com/o/r/pull/6".into(), number: 6, ..github() };
        assert_ne!(pr_key(&args_of(&github(), "Fix", "/work/r")), pr_key(&args_of(&other, "Fix", "/work/r")));
    }
}
