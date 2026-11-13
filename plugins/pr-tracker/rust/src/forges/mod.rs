//! The forges by provider, and the calls that hand a link, a remote or a tab's args to the one it belongs to. This is
//! the only place that knows which providers there are (`lib/forges/registry.ts`).

pub mod azure;
pub mod bitbucket;
pub mod gitea;
pub mod github;
pub mod gitlab;
pub mod shared;
pub mod token;

use crate::{
    forge::{ForgeError, Words},
    types::{Comment, Failing, FailureDetail, Link, ListedPr, Me, Overview, PrArgs, Provider, Push, Run, RunRef, RunStep, SnippetLine, Status},
};
use wings_plugin::{cli::Cli, remote::Remote};

type Res<T> = Result<T, ForgeError>;

/// What a provider's forge can do beyond reading: approving as you, voting (Azure DevOps), a thread's code lines, and a check's run.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Can {
    pub approve: bool,
    pub vote: bool,
    pub snippet: bool,
    pub run: bool,
}

pub fn can(provider: Provider) -> Can {
    match provider {
        Provider::Github => Can { approve: true, vote: false, snippet: false, run: true },
        Provider::Gitlab => Can { approve: true, vote: false, snippet: true, run: true },
        Provider::Azure => Can { approve: false, vote: true, snippet: true, run: true },
        Provider::Bitbucket => Can { approve: true, vote: false, snippet: true, run: false },
        Provider::Gitea => Can { approve: true, vote: false, snippet: false, run: false },
    }
}

/// Whether a remote names a provider: not a folder with no remote, nor a host that no CLI knows yet.
pub fn is_known(remote: &Remote) -> bool {
    provider_of(remote).is_some()
}

pub fn provider_of(remote: &Remote) -> Option<Provider> {
    Some(match remote {
        Remote::Github { .. } => Provider::Github,
        Remote::Azure(_) => Provider::Azure,
        Remote::Gitlab { .. } => Provider::Gitlab,
        Remote::Bitbucket { .. } => Provider::Bitbucket,
        Remote::Gitea { .. } => Provider::Gitea,
        Remote::Unknown { .. } | Remote::None { .. } => return None,
    })
}

/// Where a problem with this host is kept: github.com and an Enterprise host each have their own sign-in.
pub fn problem_key(remote: &Remote) -> String {
    match provider_of(remote) {
        Some(Provider::Github) => github::problem_key(remote),
        Some(Provider::Gitlab) => gitlab::problem_key(remote),
        Some(Provider::Azure) => azure::problem_key(remote),
        Some(Provider::Bitbucket) => bitbucket::problem_key(remote),
        Some(Provider::Gitea) => gitea::problem_key(remote),
        None => String::new(),
    }
}

/// What names the repository in what is kept per repository, whatever form its remote URL had.
pub fn repo_key(remote: &Remote) -> String {
    match provider_of(remote) {
        Some(Provider::Github) => github::repo_key(remote),
        Some(Provider::Gitlab) => gitlab::repo_key(remote),
        Some(Provider::Azure) => azure::repo_key(remote),
        Some(Provider::Bitbucket) => bitbucket::repo_key(remote),
        Some(Provider::Gitea) => gitea::repo_key(remote),
        None => String::new(),
    }
}

/// The CLI a provider works through, which the badge offers to install and to sign in. Bitbucket and Codeberg take a
/// token pasted in Settings, so they have none.
pub fn cli_of(provider: Provider) -> Option<Cli> {
    match provider {
        Provider::Github => Some(github::cli()),
        Provider::Gitlab => Some(gitlab::cli()),
        Provider::Azure => Some(azure::cli()),
        Provider::Bitbucket | Provider::Gitea => None,
    }
}

pub fn clis() -> Vec<Cli> {
    [Provider::Github, Provider::Gitlab, Provider::Azure].into_iter().filter_map(cli_of).collect()
}

/// What the badge and the block say when the CLI is missing or signed out.
pub fn words(remote: &Remote) -> Words {
    match remote {
        Remote::Github { .. } => github::words(remote),
        Remote::Gitlab { .. } => gitlab::words(remote),
        Remote::Azure(_) => azure::words(remote),
        _ => Words { signed_out: String::new(), missing: String::new() },
    }
}

/// Runs the provider's login. `on_code` hears the one-time code a login prints, for the badge to show.
pub async fn sign_in(remote: &Remote, on_code: impl Fn(&str) + 'static) -> Result<(), String> {
    match remote {
        Remote::Github { .. } => github::sign_in(remote, on_code).await,
        Remote::Gitlab { .. } => gitlab::sign_in(remote, on_code).await,
        Remote::Azure(_) => azure::sign_in(remote, on_code).await,
        _ => Ok(()),
    }
}

/// Forgets what the forges kept about the account, like who you are, after a sign-in or Try again.
pub fn forget() {
    github::forget();
    gitlab::forget();
    azure::forget();
    bitbucket::forget();
    gitea::forget();
}

/// The PR Claude Code recorded for a session, for a forge whose pane links come from the transcript (GitHub).
pub async fn from_session(remote: &Remote, session_id: &str) -> Option<Link> {
    match remote {
        Remote::Github { .. } => github::from_session(session_id).await,
        _ => None,
    }
}

/// Whether the forge of this remote has the PR Claude Code recorded for a session.
pub fn has_session_links(remote: &Remote) -> bool {
    matches!(remote, Remote::Github { .. })
}

/// The PR for a branch, or none.
pub async fn find(cwd: &str, remote: &Remote, branch: &str) -> Res<Option<Link>> {
    match remote {
        Remote::Github { .. } => github::find(cwd, remote, branch).await,
        Remote::Gitlab { .. } => gitlab::find(cwd, remote, branch).await,
        Remote::Azure(_) => azure::find(cwd, remote, branch).await,
        Remote::Bitbucket { .. } => bitbucket::find(cwd, remote, branch).await,
        Remote::Gitea { .. } => gitea::find(cwd, remote, branch).await,
        _ => Ok(None),
    }
}

/// The signed-in account, which a comment's `by` is matched against.
pub async fn me(remote: &Remote) -> Res<Me> {
    match remote {
        Remote::Github { .. } => github::me(remote).await,
        Remote::Gitlab { .. } => gitlab::me(remote).await,
        Remote::Azure(_) => azure::me(remote).await,
        Remote::Bitbucket { .. } => bitbucket::me(remote).await,
        Remote::Gitea { .. } => gitea::me(remote).await,
        _ => Err("no account".into()),
    }
}

/// The repo's open PRs.
pub async fn list(cwd: &str, remote: &Remote) -> Res<Vec<ListedPr>> {
    match remote {
        Remote::Github { .. } => github::list(cwd, remote).await,
        Remote::Gitlab { .. } => gitlab::list(cwd, remote).await,
        Remote::Azure(_) => azure::list(cwd, remote).await,
        Remote::Bitbucket { .. } => bitbucket::list(cwd, remote).await,
        Remote::Gitea { .. } => gitea::list(cwd, remote).await,
        _ => Ok(Vec::new()),
    }
}

/// The link a tab's args name, which may need the provider asked again. With the project's folder on it where the
/// provider diffs there.
pub async fn link_of(args: &PrArgs) -> Res<Link> {
    let mut link = match args.provider {
        Provider::Github => github::link_of(args).await?,
        Provider::Gitlab => gitlab::link_of(args).await?,
        Provider::Azure => azure::link_of(args).await?,
        Provider::Bitbucket => bitbucket::link_of(args).await?,
        Provider::Gitea => gitea::link_of(args).await?,
    };
    if matches!(args.provider, Provider::Azure | Provider::Bitbucket) {
        link.cwd = Some(args.cwd.clone());
    }
    Ok(link)
}

/// `fresh` is a read the user asked for, which a forge that keeps what it listed reads again.
pub async fn status(link: &Link, fresh: bool) -> Res<Status> {
    match link.provider {
        Provider::Github => github::status(link, fresh).await,
        Provider::Gitlab => gitlab::status(link, fresh).await,
        Provider::Azure => azure::status(link, fresh).await,
        Provider::Bitbucket => bitbucket::status(link, fresh).await,
        Provider::Gitea => gitea::status(link, fresh).await,
    }
}

/// The status, author and description, for the PR tab.
pub async fn overview(link: &Link) -> Res<Overview> {
    match link.provider {
        Provider::Github => github::overview(link).await,
        Provider::Gitlab => gitlab::overview(link).await,
        Provider::Azure => azure::overview(link).await,
        Provider::Bitbucket => bitbucket::overview(link).await,
        Provider::Gitea => gitea::overview(link).await,
    }
}

/// The patch and the comments the diff viewer shows. `st` is the PR's status, whose branches or commits a forge that
/// diffs in the project's folder reads.
pub async fn diff(link: &Link, st: &Status) -> Res<(String, Vec<Comment>)> {
    match link.provider {
        Provider::Github => github::diff(link, st).await,
        Provider::Gitlab => gitlab::diff(link, st).await,
        Provider::Azure => azure::diff(link, st).await,
        Provider::Bitbucket => bitbucket::diff(link, st).await,
        Provider::Gitea => gitea::diff(link, st).await,
    }
}

/// The PR's conversation for the tab.
pub async fn comments(link: &Link) -> Res<Vec<Comment>> {
    match link.provider {
        Provider::Github => github::comments(link).await,
        Provider::Gitlab => gitlab::comments(link).await,
        Provider::Azure => azure::comments(link).await,
        Provider::Bitbucket => bitbucket::comments(link).await,
        Provider::Gitea => gitea::comments(link).await,
    }
}

/// Why one failed check failed: its failed step and the end of its log.
pub async fn failure(link: &Link, failing: &Failing) -> Res<Vec<FailureDetail>> {
    match link.provider {
        Provider::Github => github::failure(link, failing).await,
        Provider::Gitlab => gitlab::failure(link, failing).await,
        Provider::Azure => azure::failure(link, failing).await,
        Provider::Bitbucket => bitbucket::failure(link, failing).await,
        Provider::Gitea => gitea::failure(link, failing).await,
    }
}

/// A check's run: its jobs and steps, where the provider's CI has them.
pub async fn run(link: &Link, run: &RunRef) -> Res<Run> {
    match link.provider {
        Provider::Github => github::run(link, run).await,
        Provider::Gitlab => gitlab::run(link, run).await,
        Provider::Azure => azure::run(link, run).await,
        _ => Err("This provider's CI has no run to show here.".into()),
    }
}

/// One step's log as the CI wrote it, colours and markup kept.
pub async fn run_log(link: &Link, run: &RunRef, step: &RunStep) -> Res<Vec<String>> {
    match link.provider {
        Provider::Github => github::run_log(link, run, step).await,
        Provider::Gitlab => gitlab::run_log(link, run, step).await,
        Provider::Azure => azure::run_log(link, run, step).await,
        _ => Err("This provider's CI has no run to show here.".into()),
    }
}

/// Posts a reply to a thread.
pub async fn reply(link: &Link, root: &Comment, body: &str) -> Res<Comment> {
    match link.provider {
        Provider::Github => github::reply(link, root, body).await,
        Provider::Gitlab => gitlab::reply(link, root, body).await,
        Provider::Azure => azure::reply(link, root, body).await,
        Provider::Bitbucket => bitbucket::reply(link, root, body).await,
        Provider::Gitea => gitea::reply(link, root, body).await,
    }
}

/// Approves as you. `sha` is the head commit you were looking at, which GitLab approves only.
pub async fn approve(link: &Link, sha: &str) -> Res<()> {
    match link.provider {
        Provider::Github => github::approve(link, sha).await,
        Provider::Gitlab => gitlab::approve(link, sha).await,
        Provider::Bitbucket => bitbucket::approve(link, sha).await,
        Provider::Gitea => gitea::approve(link, sha).await,
        Provider::Azure => Err("Azure DevOps votes instead of approving.".into()),
    }
}

/// Casts an Azure DevOps vote.
pub async fn vote(link: &Link, vote: &str) -> Res<()> {
    match link.provider {
        Provider::Azure => azure::vote(link, vote).await,
        _ => Err("This provider has no votes.".into()),
    }
}

/// The PR's pushes with their commits, where the provider lists them.
pub async fn pushes(link: &Link) -> Res<Vec<Push>> {
    match link.provider {
        Provider::Azure => azure::pushes(link).await,
        _ => Ok(Vec::new()),
    }
}

/// The lines around a code thread's comment, from the commit it was written on, or with `now` where they sit at the
/// latest push. None where they can't be had.
pub async fn snippet(link: &Link, root: &Comment, pushes: &[Push], head: Option<&str>, now: bool) -> Res<Option<Vec<SnippetLine>>> {
    match link.provider {
        Provider::Azure => azure::snippet(link, root, pushes, head, now).await,
        Provider::Gitlab => gitlab::snippet(link, root, pushes, head, now).await,
        Provider::Bitbucket => bitbucket::snippet(link, root, pushes, head, now).await,
        Provider::Github | Provider::Gitea => Ok(None),
    }
}
