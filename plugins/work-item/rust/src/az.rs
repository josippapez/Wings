//! The Azure CLI as this plugin calls it, and what each frame needs to know about a folder's Azure DevOps project. A frame
//! has one instance of this, so the main script keeps the sign-in problems its badges show, and a page has its own,
//! which nothing reads.

use crate::sections::{current_iterations, query_rows};
use serde_json::{Value, json};
use std::{cell::RefCell, collections::HashMap};
use wings_plugin::{
    azure::{AzureRemote, WorkItems, parse_remote, work_items},
    cache::Cached,
    cli::{Cli, CliError},
    git, host,
};

const SPRINT_TTL_MS: f64 = 3_600_000.0;

thread_local! {
    static AZ: Cli = Cli::new("az", "Azure CLI", |stderr| stderr.to_lowercase().contains("login"), &["login", "--allow-no-subscriptions", "--output", "none"]);
    /// `(org, quiet)` → the loader of that organization's work items.
    static LOADERS: RefCell<HashMap<(String, bool), WorkItems>> = RefCell::new(HashMap::new());
    static REMOTES: Cached<String, Option<AzureRemote>> = Cached::new(f64::INFINITY, 0.0, |cwd: String| async move {
        Ok(git::ok(&cwd, &["remote", "get-url", "origin"]).await.and_then(|url| parse_remote(&url)))
    });
    static SPRINTS: Cached<(String, String), Vec<String>> = Cached::new(SPRINT_TTL_MS, 0.0, |(org, project): (String, String)| async move {
        let args = invoke("wit", "classificationNodes", &[("project", &project), ("structureGroup", "iterations")], &["$depth=4"], None);
        let root = az(&org, &args, true, Some(json!({}))).await.map_err(String::from)?;
        Ok(current_iterations(&root, host::now()))
    });
}

pub fn cli() -> Cli {
    AZ.with(Cli::clone)
}

fn for_org(args: &[String], org: &str) -> Vec<String> {
    args.iter().cloned().chain(["--organization", org, "-o", "json"].map(str::to_owned)).collect()
}

/// Runs `az args` for the organization `org` and gives what it printed, parsed. Nothing printed, which is what
/// `az boards query` does for no rows, gives `empty`, and is an error when there is none. A failure has the stderr as its
/// message. A call records "not installed" and "signed out" for the pane badges, unless it is `quiet`, a background
/// read that must not turn them into a sign-in prompt.
pub async fn az(org: &str, args: &[String], quiet: bool, empty: Option<Value>) -> Result<Value, CliError> {
    cli().json(&for_org(args, org), org, quiet, empty).await
}

/// `az` for a command whose output isn't read, like an update: nothing is parsed.
pub async fn az_run(org: &str, args: &[String], quiet: bool) -> Result<String, CliError> {
    cli().run(&for_org(args, org), org, quiet).await
}

/// The arguments of `az devops invoke`, a call of Azure DevOps' REST API. `query` are the URL's query parameters, and
/// `jmes` cuts the answer with a JMESPath.
pub fn invoke(area: &str, resource: &str, route: &[(&str, &str)], query: &[&str], jmes: Option<&str>) -> Vec<String> {
    let mut args: Vec<String> = ["devops", "invoke", "--area", area, "--resource", resource, "--route-parameters"].map(str::to_owned).into();
    args.extend(route.iter().map(|(name, value)| format!("{name}={value}")));
    if !query.is_empty() {
        args.push("--query-parameters".into());
        args.extend(query.iter().map(|q| (*q).to_owned()));
    }
    args.extend(["--api-version", "7.1"].map(str::to_owned));
    if let Some(jmes) = jmes {
        args.extend(["--query".to_owned(), jmes.to_owned()]);
    }
    args
}

/// One work item with its relations, or none when there is none or you can't see it. Every item asked in the same turn
/// is one `az rest` call, so a badge per pane, a parent title and the Work items section cost one process between them,
/// not one each. `quiet` is for a background read, as in `az`. This replaces `az boards work-item show` and
/// `az devops invoke --resource workItems`, which the installed azure-cli fails on (`KeyError: 'type'`).
pub async fn work_item(org: &str, id: u64, quiet: bool) -> Result<Option<Value>, String> {
    let loader = LOADERS.with(|loaders| loaders.borrow_mut().entry((org.to_owned(), quiet)).or_insert_with(|| work_items(&cli(), org, quiet)).clone());
    loader.get(id).await
}

/// The Azure DevOps remote of the folder's origin, or none when it isn't one. A folder is read once.
pub async fn remote_for(cwd: &str) -> Option<AzureRemote> {
    REMOTES.with(Cached::clone).get(cwd.to_owned()).await.ok().flatten()
}

/// The paths of the sprints running now in an Azure project, read at most once an hour.
pub async fn sprint_paths(org: &str, project: &str) -> Result<Vec<String>, String> {
    SPRINTS.with(Cached::clone).get((org.to_owned(), project.to_owned())).await
}

/// The rows `az boards query` prints for `wiql` in `project`: a background read, which no sign-in problem comes of.
pub async fn query(org: &str, project: &str, wiql: &str) -> Result<Vec<crate::sections::QueryRow>, CliError> {
    let args = ["boards", "query", "--project", project, "--wiql", wiql].map(str::to_owned);
    Ok(query_rows(&az(org, &args, true, Some(json!([]))).await?))
}
