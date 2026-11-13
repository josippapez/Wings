//! Signing in to a provider's CLI from the badge, installing it, and trying again from scratch.

use super::{badges, links::{self, Which, refresh_links}, problems, projects, status};
use crate::{forges, ipc, remote};
use serde_json::{Value, json};
use wings_plugin::{host, remote::Remote, task::join_all};

/// Forgets what the tracker kept about its accounts and their pull requests, so the next look at them asks again.
fn clear_caches() {
    remote::forget_hosts();
    projects::forget_lists();
    problems::forget_problems();
    links::forget_lookups();
    status::forget_all();
    forges::forget();
}

/// The badges follow each CLI's sign-in and problems, and whichever CLI is tried again or signed in to, the caches start over.
pub fn connect_clis() {
    for cli in forges::clis() {
        cli.on_change(badges::render);
        cli.on_retry(|| async { clear_caches() });
    }
}

/// Runs the provider's own login so you never need a terminal. `az` and `glab` open the browser themselves. `gh`
/// doesn't without a terminal: it prints a one-time code and a URL, so the badge shows the code and Wings opens the URL.
pub async fn sign_in(remote: Option<Remote>) -> Result<(), String> {
    let Some(remote) = remote.filter(forges::is_known) else { return Ok(()) };
    // Bitbucket and Codeberg have no CLI to sign in with: the token is pasted in Settings.
    if forges::provider_of(&remote).and_then(forges::cli_of).is_none() {
        return host::call("openSettings", json!({})).await.map(drop);
    }
    let key = forges::problem_key(&remote);
    let shown = key.clone();
    let signed = forges::sign_in(&remote, move |code| {
        problems::set_code(&shown, code);
        badges::render();
    })
    .await;
    problems::clear_code(&key);
    signed?;
    refresh_links(true, Which::All).await;
    refresh_tabs();
    Ok(())
}

/// Asks Wings to install the CLI a remote's forge works through.
pub async fn install(remote: Option<Remote>) -> Result<(), String> {
    match remote.as_ref().and_then(forges::provider_of).and_then(forges::cli_of) {
        Some(cli) => cli.install().await,
        None => Ok(()),
    }
}

pub async fn retry() {
    join_all(forges::clis().into_iter().map(|cli| Box::pin(async move { cli.retry().await }) as std::pin::Pin<Box<dyn std::future::Future<Output = ()>>>).collect()).await;
    refresh_links(true, Which::All).await;
    refresh_tabs();
}

/// The open tabs' PRs load again, after a sign-in or Try again.
fn refresh_tabs() {
    ipc::send(ipc::REFRESH, &Value::Null);
}
