//! Finding the PR for a pane.

use super::{badges, problems, state::{self, Pane}, status::refresh_status};
use crate::{forges, remote::remote_for, types::Link};
use std::{cell::RefCell, collections::HashMap, hash::{Hash, Hasher}};
use wings_plugin::{cache::Cached, git, host, remote::Remote, task::join_all};

const LOOKUP_TTL_MS: f64 = 60_000.0;

/// What a branch's PR is looked up by. Two panes on the same repo and branch are one lookup, whatever folder each is in.
#[derive(Clone)]
struct Lookup {
    key: String,
    cwd: String,
    remote: Remote,
    branch: String,
}

impl PartialEq for Lookup {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}
impl Eq for Lookup {}
impl Hash for Lookup {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.key.hash(state);
    }
}

thread_local! {
    /// `repo branch` → the PR the branch has, so panes on the same branch share one network lookup. A failed one isn't
    /// kept, so the next pane asks again.
    static LOOKUPS: Cached<Lookup, Option<Link>> = Cached::new(LOOKUP_TTL_MS, 0.0, |lookup: Lookup| async move {
        match forges::find(&lookup.cwd, &lookup.remote, &lookup.branch).await {
            Ok(link) => {
                problems::answered(&lookup.remote);
                Ok(link)
            }
            // The account can't say, and the pane's badge says why: there is no PR to show for now.
            Err(error) if problems::explains(&lookup.remote, &error) => Ok(None),
            Err(error) => Err(error.to_wire()),
        }
    });
    /// paneId → the project whose repo the pane is in, or none when no project has it, so Wings sends no change for it.
    static WATCHED: RefCell<HashMap<String, Option<String>>> = RefCell::new(HashMap::new());
    /// paneId → the branch read last, so the 60 s lookup runs no git.
    static BRANCHES: RefCell<HashMap<String, Option<String>>> = RefCell::new(HashMap::new());
    /// sessionId → the PR Claude Code recorded for it: read once, then kept up to date from the transcript.
    static SESSION_LINKS: RefCell<HashMap<String, Option<Link>>> = RefCell::new(HashMap::new());
}

/// The project whose repo the pane is in, or none when no project has it: only those panes get a poll for their branch.
pub fn project_of(pane: &Pane) -> Option<String> {
    WATCHED.with(|watched| watched.borrow().get(&pane.id).cloned().flatten())
}

/// What is kept per pane goes when the pane closes.
pub fn forget_panes(open: &[String]) {
    WATCHED.with(|watched| watched.borrow_mut().retain(|id, _| open.contains(id)));
    BRANCHES.with(|branches| branches.borrow_mut().retain(|id, _| open.contains(id)));
}

/// Claude Code recorded a PR for a session.
pub fn session_linked(session_id: &str, link: Link) {
    SESSION_LINKS.with(|links| links.borrow_mut().insert(session_id.to_owned(), Some(link)));
}

pub fn forget_lookups() {
    LOOKUPS.with(Cached::clear);
}

async fn find_for_pane(pane: &Pane, local: bool) -> Result<Option<Link>, String> {
    if local {
        let projects = host::projects().await.unwrap_or_default();
        let project = git::watched_project(&pane.cwd, &projects).await;
        WATCHED.with(|watched| watched.borrow_mut().insert(pane.id.clone(), project));
    }
    let remote = remote_for(&pane.cwd).await;
    if !forges::is_known(&remote) {
        return Ok(None);
    }
    // A Claude pane's PR is the one Claude Code recorded, for a forge whose links come from there. Only Claude Code
    // records one: an opencode session has no transcript.
    if let Some(session) = pane.session.as_ref().filter(|session| session.kind.as_deref().unwrap_or("claude") == "claude")
        && forges::has_session_links(&remote)
    {
        if !SESSION_LINKS.with(|links| links.borrow().contains_key(&session.id)) {
            let link = forges::from_session(&remote, &session.id).await;
            SESSION_LINKS.with(|links| links.borrow_mut().insert(session.id.clone(), link));
        }
        if let Some(link) = SESSION_LINKS.with(|links| links.borrow().get(&session.id).cloned().flatten()) {
            return Ok(Some(link));
        }
    }
    if local || !BRANCHES.with(|branches| branches.borrow().contains_key(&pane.id)) {
        let branch = git::ok(&pane.cwd, &["branch", "--show-current"]).await;
        BRANCHES.with(|branches| branches.borrow_mut().insert(pane.id.clone(), branch));
    }
    let Some(branch) = BRANCHES.with(|branches| branches.borrow().get(&pane.id).cloned().flatten()).filter(|branch| !branch.is_empty()) else {
        return Ok(None);
    };
    let lookup = Lookup { key: format!("{} {branch}", forges::repo_key(&remote)), cwd: pane.cwd.clone(), remote, branch };
    LOOKUPS.with(Cached::clone).get(lookup).await
}

/// Which panes `refresh_links` looks at.
#[derive(Clone)]
pub enum Which {
    All,
    /// The panes in a repo one of these projects has.
    Projects(Vec<String>),
    /// The panes in a repo some project has.
    InProject,
    /// The panes in a repo no project has.
    NoProject,
    Pane(String),
}

impl Which {
    fn picks(&self, pane: &Pane) -> bool {
        match self {
            Self::All => true,
            Self::Projects(ids) => project_of(pane).is_some_and(|id| ids.contains(&id)),
            Self::InProject => project_of(pane).is_some(),
            Self::NoProject => project_of(pane).is_none(),
            Self::Pane(id) => pane.id == *id,
        }
    }
}

async fn refresh_pane(pane: Pane, local: bool) {
    let found = find_for_pane(&pane, local).await;
    let link = match found {
        Ok(Some(link)) => link,
        Ok(None) => return drop(state::with(|s| s.links.remove(&pane.id))),
        // A lookup that failed for a reason that isn't the account's leaves the pane as it was.
        Err(_) => return,
    };
    let known = state::with(|s| s.links.insert(pane.id.clone(), link.clone()));
    if local {
        // Fails without an upstream branch, which means nothing to push or pull yet.
        let counts = git::ok(&pane.cwd, &["rev-list", "--left-right", "--count", "@{upstream}...HEAD"]).await.unwrap_or_else(|| "0 0".into());
        let mut numbers = counts.split_whitespace().map(|n| n.parse::<i64>().unwrap_or(0));
        let (behind, ahead) = (numbers.next().unwrap_or(0), numbers.next().unwrap_or(0));
        state::with(|s| s.sync.insert(pane.id.clone(), (ahead, behind)));
    }
    if known.is_none_or(|known| known.url != link.url) && state::status_of(&link.url).is_none() {
        let url = link.url.clone();
        wings_plugin::task::spawn(async move { refresh_status(&url, false).await });
    }
}

/// Finds the PR of the panes `which` picks. `local` reads each pane's branch and its commits to push and pull with git:
/// a repo change, a new pane and the poll of a repo no project has do that. Without it the branch read last is looked up
/// again, which is the 60 s cycle's work and runs no git.
pub async fn refresh_links(local: bool, which: Which) {
    let panes: Vec<Pane> = state::with(|s| s.panes.clone()).into_iter().filter(|pane| which.picks(pane)).collect();
    join_all(panes.into_iter().map(|pane| Box::pin(refresh_pane(pane, local)) as std::pin::Pin<Box<dyn std::future::Future<Output = ()>>>).collect()).await;
    badges::render();
}
