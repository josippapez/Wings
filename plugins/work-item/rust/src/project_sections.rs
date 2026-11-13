//! The Work items section under each project in Wings' sidebar: the unfinished work items assigned to you in the sprint
//! running now, that link to the project's repo or have a branch of it.

use crate::{
    az::{az, invoke, query, remote_for, sprint_paths, work_item},
    sections::{QueryRow, RepoRef, SECTION_ID, SectionRow, StateCategories, assigned_query, is_finished, repo_links_of, repo_rows, state_categories},
};
use serde_json::{Value, json};
use std::{collections::HashMap, future::Future, pin::Pin, rc::Rc};

type Read = Pin<Box<dyn Future<Output = Result<Option<Value>, String>>>>;
use wings_plugin::{
    azure::AzureRemote,
    cache::Cached,
    git, host, poll,
    task::{SingleFlight, join_all},
};

const POLL_MS: u32 = 120_000;
/// How long a project's rows are reused: a bit under the poll, so each poll reads again.
const TEAM_ITEMS_TTL_MS: f64 = 100_000.0;
const CATEGORIES_TTL_MS: f64 = 3_600_000.0;
const FAILED_CATEGORIES_MS: f64 = 120_000.0;
/// The most unfinished items one team project's section looks at: one `workItems` read takes 200 ids.
const ITEMS_CAP: usize = 100;

struct TeamItems {
    rows: Vec<QueryRow>,
    /// work item id → ids of the repositories its links point at
    links: HashMap<u64, Vec<String>>,
}

thread_local! {
    /// Each work item type's state categories. A failed read is remembered briefly, so the next refresh tries again soon,
    /// and the state names decide meanwhile.
    static CATEGORIES: Cached<(String, String), StateCategories> = Cached::new(CATEGORIES_TTL_MS, FAILED_CATEGORIES_MS, |(org, project): (String, String)| async move {
        // `--query` keeps what az prints to the names and categories: the whole answer is over a megabyte.
        let args = invoke("wit", "workItemTypes", &[("project", &project)], &[], Some("value[].{name:name,states:states[].{name:name,category:category}}"));
        Ok(state_categories(&az(&org, &args, true, Some(json!([]))).await?))
    });
    /// A repository's id, which links carry in place of its name. A failed read is asked again at the next refresh.
    static REPO_IDS: Cached<AzureRemote, Option<String>> = Cached::new(f64::INFINITY, 0.0, |remote: AzureRemote| async move {
        let args = invoke("git", "repositories", &[("project", &remote.project), ("repositoryId", &remote.repo)], &[], None);
        Ok(az(&remote.org, &args, true, Some(json!({}))).await?["id"].as_str().map(str::to_owned))
    });
    /// The unfinished items assigned to you in an Azure project's running sprints, with their repository links. Every
    /// repo of the project shares it.
    static TEAM_ITEMS: Cached<(String, String), Rc<TeamItems>> = Cached::new(TEAM_ITEMS_TTL_MS, 0.0, |(org, project): (String, String)| async move {
        let paths = sprint_paths(&org, &project).await?;
        if paths.is_empty() {
            return Ok(Rc::new(TeamItems { rows: Vec::new(), links: HashMap::new() }));
        }
        let found = query(&org, &project, &assigned_query(&project, &paths)).await?;
        let categories = CATEGORIES.with(Cached::clone).get((org.clone(), project.clone())).await.ok();
        let text = |row: &QueryRow, name: &str| row.fields[name].as_str().unwrap_or_default().to_owned();
        let rows: Vec<QueryRow> = found.into_iter().filter(|row| !is_finished(&text(row, "System.WorkItemType"), &text(row, "System.State"), categories.as_ref())).take(ITEMS_CAP).collect();
        // The items asked together are one read for all the links, never one per item.
        let reads: Vec<Read> = rows.iter().map(|row| { let (org, id) = (org.clone(), row.id); Box::pin(async move { work_item(&org, id, true).await }) as _ }).collect();
        let items = join_all(reads).await.into_iter().collect::<Result<Vec<_>, _>>()?;
        Ok(Rc::new(TeamItems { links: repo_links_of(&items), rows }))
    });
    static REFRESH: SingleFlight = SingleFlight::new(refresh);
}

/// The local and remote branches of a repo: cheap and local, so read at each refresh.
async fn branches_of(cwd: &str) -> Vec<String> {
    git::ok(cwd, &["for-each-ref", "--format=%(refname:short)", "refs/heads", "refs/remotes"]).await.unwrap_or_default().lines().filter(|line| !line.is_empty()).map(str::to_owned).collect()
}

async fn rows_for(remote: &AzureRemote, cwd: &str) -> Result<Vec<SectionRow>, String> {
    let team = TEAM_ITEMS.with(Cached::clone).get((remote.org.clone(), remote.project.clone())).await?;
    // Without the repository's id, only branches match.
    let repo = RepoRef { id: REPO_IDS.with(Cached::clone).get(remote.clone()).await.ok().flatten(), branches: branches_of(cwd).await };
    Ok(repo_rows(&remote.org, &team.rows, &team.links, &repo))
}

/// One query per Azure project, whatever number of Wings projects point at it. Stale rows stay when a read fails.
async fn refresh() {
    // Wings in the background: the poll runs once when it comes back.
    if !host::focused() {
        return;
    }
    let projects = host::projects().await.unwrap_or_default();
    for project in projects.as_array().into_iter().flatten() {
        let (id, path) = (project["id"].as_str().unwrap_or_default(), project["path"].as_str().unwrap_or_default());
        let rows = match remote_for(path).await {
            Some(remote) => match rows_for(&remote, path).await {
                Ok(rows) => Value::Array(rows.iter().map(SectionRow::json).collect()),
                Err(_) => continue,
            },
            None => Value::Null,
        };
        drop(host::set_project_section(id, SECTION_ID, &rows).await);
    }
}

pub fn start() {
    host::on_projects(|_| drop(REFRESH.with(SingleFlight::clone).call()));
    poll::while_focused(POLL_MS, false, || drop(REFRESH.with(SingleFlight::clone).call()));
}
