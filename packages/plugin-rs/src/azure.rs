//! Azure DevOps, as `wings-plugin-sdk/git` and `wings-plugin-sdk/azure` read it: an Azure remote, and the work items of
//! an organization, which one `az` call answers for many ids. The plugin lists `az rest [--resource --url -o]` in
//! `permissions.exec`.

use crate::{cache::Batch, cli::Cli};
use serde_json::Value;
use std::future::Future;

/// The resource `az rest` gets a token for when it calls Azure DevOps.
const DEVOPS_RESOURCE: &str = "499b84ac-1321-427f-aa17-267ca6975798";
/// "Maximum 200 ids allowed" (learn.microsoft.com/en-us/rest/api/azure/devops/wit/work-items/list).
pub const WORK_ITEMS_MAX: usize = 200;

/// Where a git remote is on Azure DevOps. `org` is `https://dev.azure.com/{org}`, or `https://{org}.visualstudio.com`
/// for an old address.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct AzureRemote {
    pub org: String,
    pub project: String,
    pub repo: String,
}

fn decoded(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        let hex = bytes.get(at + 1..at + 3).and_then(|pair| std::str::from_utf8(pair).ok()).and_then(|pair| u8::from_str_radix(pair, 16).ok());
        match (bytes[at], hex) {
            (b'%', Some(byte)) => {
                out.push(byte);
                at += 3;
            }
            (byte, _) => {
                out.push(byte);
                at += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The segments after `marker` in `text`, when there are `count` of them (or one more, `DefaultCollection`, which
/// `allow_collection` drops) and none is empty. The repository's `.git` is dropped.
fn segments<'a>(text: &'a str, marker: &str, count: usize, allow_collection: bool) -> Option<Vec<&'a str>> {
    text.match_indices(marker).find_map(|(at, _)| {
        let mut parts: Vec<&str> = text[at + marker.len()..].split('/').collect();
        if allow_collection && parts.len() == count + 1 && parts[0] == "DefaultCollection" {
            parts.remove(0);
        }
        (parts.len() == count && parts.iter().all(|part| !part.is_empty())).then_some(parts)
    })
}

fn repo_of(name: &str) -> String {
    decoded(name.strip_suffix(".git").filter(|name| !name.is_empty()).unwrap_or(name))
}

/// The Azure DevOps remote that `url` names (ssh.dev.azure.com, dev.azure.com, `{org}.visualstudio.com` and
/// vs-ssh.visualstudio.com), or none for another host.
pub fn parse_remote(url: &str) -> Option<AzureRemote> {
    let text = url.trim();
    for marker in ["ssh.dev.azure.com:v3/", "vs-ssh.visualstudio.com:v3/"] {
        if let Some(parts) = segments(text, marker, 3, false) {
            return Some(AzureRemote { org: format!("https://dev.azure.com/{}", parts[0]), project: decoded(parts[1]), repo: repo_of(parts[2]) });
        }
    }
    if let Some(parts) = segments(text, "dev.azure.com/", 4, false).filter(|parts| parts[2] == "_git") {
        return Some(AzureRemote { org: format!("https://dev.azure.com/{}", parts[0]), project: decoded(parts[1]), repo: repo_of(parts[3]) });
    }
    for (at, _) in text.match_indices(".visualstudio.com/") {
        let name = text[..at].rsplit(['/', '.', '@']).next().filter(|name| !name.is_empty());
        let rest = &text[at + ".visualstudio.com/".len()..];
        let mut parts: Vec<&str> = rest.split('/').collect();
        if parts.len() == 4 && parts[0] == "DefaultCollection" {
            parts.remove(0);
        }
        if let Some(name) = name && parts.len() == 3 && parts[1] == "_git" && !parts[0].is_empty() && !parts[2].is_empty() {
            return Some(AzureRemote { org: format!("https://{name}.visualstudio.com"), project: decoded(parts[0]), repo: repo_of(parts[2]) });
        }
    }
    None
}

/// An organization at its dev.azure.com address, for a call that must name that host. An organization's REST APIs
/// accept both forms.
pub fn api_org(org: &str) -> String {
    match org.strip_prefix("https://").and_then(|rest| rest.strip_suffix(".visualstudio.com")).filter(|name| !name.is_empty() && !name.contains(['.', '/'])) {
        Some(name) => format!("https://dev.azure.com/{name}"),
        None => org.to_owned(),
    }
}

/// Work items of one Azure DevOps organization, with their relations, loaded through `az rest`: one call for every id
/// asked together, 200 ids at most.
#[derive(Clone)]
pub struct WorkItems(Batch<u64, Option<Value>>);

/// `cli` is the plugin's `az`; `org` is the `org` of an `AzureRemote`. A call goes out under `org` as the CLI's sign-in
/// key, unless `quiet`.
pub fn work_items(cli: &Cli, org: &str, quiet: bool) -> WorkItems {
    let (cli, org) = (cli.clone(), org.to_owned());
    WorkItems(Batch::new(WORK_ITEMS_MAX, move |ids: Vec<u64>| {
        let (cli, org) = (cli.clone(), org.clone());
        async move {
            // `$expand=relations` can't go with `fields`, so an item comes whole.
            let list = ids.iter().map(u64::to_string).collect::<Vec<_>>().join(",");
            let url = format!("{}/_apis/wit/workitems?ids={list}&$expand=relations&errorPolicy=omit&api-version=7.1", api_org(&org));
            let args = ["rest", "--resource", DEVOPS_RESOURCE, "--url", &url, "-o", "json"].map(str::to_owned);
            let answer = cli.json(&args, &org, quiet, None).await?;
            let found = answer["value"].as_array().map_or(&[][..], Vec::as_slice);
            Ok(ids.iter().map(|id| found.iter().find(|item| item["id"].as_u64() == Some(*id)).cloned()).collect())
        }
    }))
}

impl WorkItems {
    /// The work item with its relations, or none for an id that doesn't exist or that you can't see.
    pub fn get(&self, id: u64) -> impl Future<Output = Result<Option<Value>, String>> + use<> {
        self.0.get(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{host, task::{block_on, join_all}};
    use serde_json::json;
    use std::{cell::RefCell, pin::Pin, rc::Rc};

    #[test]
    fn a_remote_keeps_its_organization_project_and_repository() {
        let remote = |org: &str, project: &str, repo: &str| Some(AzureRemote { org: org.into(), project: project.into(), repo: repo.into() });
        assert_eq!(parse_remote("git@ssh.dev.azure.com:v3/org/My%20Proj/my-repo"), remote("https://dev.azure.com/org", "My Proj", "my-repo"));
        assert_eq!(parse_remote("https://org.visualstudio.com/DefaultCollection/P/_git/r.git"), remote("https://org.visualstudio.com", "P", "r"));
        assert_eq!(parse_remote("https://user@dev.azure.com/acme/Web/_git/web\n"), remote("https://dev.azure.com/acme", "Web", "web"));
        assert_eq!(parse_remote("org@vs-ssh.visualstudio.com:v3/org/P/r.git"), remote("https://dev.azure.com/org", "P", "r"));
        assert_eq!(parse_remote("git@github.com:acme/web.git"), None);
        assert_eq!(parse_remote("https://dev.azure.com/acme/Web/_git/web/extra"), None);
        assert_eq!(api_org("https://acme.visualstudio.com"), "https://dev.azure.com/acme");
        assert_eq!(api_org("https://dev.azure.com/acme"), "https://dev.azure.com/acme");
    }

    // `az devops invoke --resource workItems` fails on azure-cli 2.91 with `KeyError: 'type'`, so the items are read through `az rest`.
    #[test]
    fn work_items_asked_together_are_one_az_rest_and_one_that_isnt_there_is_none() {
        let calls: Rc<RefCell<Vec<Vec<String>>>> = Rc::default();
        let seen = calls.clone();
        host::testing::answer(move |_, params| {
            let args: Vec<String> = params["args"].as_array().unwrap().iter().map(|arg| arg.as_str().unwrap().to_owned()).collect();
            let url = args[args.iter().position(|arg| arg == "--url").unwrap() + 1].clone();
            seen.borrow_mut().push(args);
            let ids: Vec<u64> = url.split("ids=").nth(1).unwrap().split('&').next().unwrap().split(',').map(|id| id.parse().unwrap()).collect();
            let value: Vec<_> = ids.iter().map(|id| if *id == 3 { json!(null) } else { json!({ "id": id, "fields": { "System.Title": format!("T{id}") }, "relations": [] }) }).collect();
            Ok(json!({ "code": 0, "stdout": json!({ "count": ids.len(), "value": value }).to_string(), "stderr": "" }))
        });
        let cli = Cli::new("az", "Azure CLI", |_| false, &[]);
        let loader = work_items(&cli, "https://contoso.visualstudio.com", false);
        let asked: Vec<Pin<Box<dyn Future<Output = Result<Option<Value>, String>>>>> = [1, 2, 3].map(|id| Box::pin(loader.get(id)) as Pin<Box<dyn Future<Output = _>>>).into();
        let got = block_on(join_all(asked));
        assert_eq!(got[0].clone().unwrap().unwrap()["fields"]["System.Title"], "T1");
        assert_eq!(got[1].clone().unwrap().unwrap()["id"], 2);
        assert_eq!(got[2], Ok(None));
        let calls = calls.borrow();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0][0], "rest");
        assert!(calls[0].iter().any(|arg| arg.starts_with("https://dev.azure.com/contoso/_apis/wit/workitems?ids=1,2,3&")));
    }
}
