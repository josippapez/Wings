//! A PR to and from the args its tab is opened with.

use crate::types::{Link, PrArgs, Provider};
use serde_json::Value;
use wings_plugin::remote::Remote;

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
    fn tabs_on_one_pr_share_a_key_whatever_their_title_and_project_and_two_prs_dont() {
        assert_eq!(pr_key(&args_of(&gitlab(), "Fix", "/work/r")), pr_key(&args_of(&gitlab(), "Fix it", "/elsewhere/r")));
        let other = Link { url: "https://github.com/o/r/pull/6".into(), number: 6, ..github() };
        assert_ne!(pr_key(&args_of(&github(), "Fix", "/work/r")), pr_key(&args_of(&other, "Fix", "/work/r")));
    }
}
