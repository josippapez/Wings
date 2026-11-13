//! Repository detection: which forge a folder's `origin` is on.

use crate::{forges, types::PrArgs};
use serde_json::Value;
use std::{cell::RefCell, collections::{HashMap, HashSet}};
use wings_plugin::{cache::Cached, git, host, remote::{Remote, parse_remote}, task::join};

const HOSTS_TTL_MS: f64 = 60_000.0;
/// How long a remote that no CLI knows stays unresolved without asking the CLIs again, unless Try again or a sign-in in Wings forgets it.
const UNRESOLVED_TTL_MS: f64 = 30.0 * 60_000.0;

/// The hosts `gh` and `glab` are set up for, which is how a host with no telling name (`git.corp.example`) is known to be GitHub or GitLab.
#[derive(Clone, Default, PartialEq, Debug)]
pub struct Hosts {
    pub github: HashSet<String>,
    pub gitlab: HashSet<String>,
}

thread_local! {
    /// cwd → what its `origin` is, with a host no provider's name gives away replaced once `gh` or `glab` is known to be signed in to it.
    static REMOTES: RefCell<HashMap<String, Remote>> = RefCell::new(HashMap::new());
    /// `host/path` → when to look again.
    static UNRESOLVED: RefCell<HashMap<String, f64>> = RefCell::new(HashMap::new());
    static SIGNED_IN: Cached<(), Hosts> = Cached::new(HOSTS_TTL_MS, 0.0, |()| async { Ok(read_hosts().await) });
}

/// A glab host line: `gitlab.example.com` or `host:8443`, on a line of its own.
fn is_host_line(line: &str) -> bool {
    let (name, port) = match line.split_once(':') {
        Some((name, port)) => (name, Some(port)),
        None => (line, None),
    };
    let labels: Vec<&str> = name.split('.').collect();
    labels.len() > 1
        && labels.iter().all(|label| !label.is_empty() && label.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'))
        && port.is_none_or(|port| !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()))
}

/// Takes the colour codes out of a line.
fn plain(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            while let Some(next) = chars.next() {
                if next == 'm' {
                    break;
                }
                if !(next.is_ascii_digit() || next == ';') {
                    out.push('\u{1b}');
                    out.push('[');
                    out.push(next);
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// `gh auth status --json hosts` prints `{"hosts":{"<host>":[...]}}` and exits 0 whatever the state; glab names each
/// host on a line of its own, on stderr.
pub fn hosts_from(gh_stdout: &str, glab_output: &str) -> Hosts {
    let mut hosts = Hosts::default();
    // Not JSON: an old gh without --json knows no hosts this way.
    if let Ok(value) = serde_json::from_str::<Value>(gh_stdout) {
        hosts.github.extend(value["hosts"].as_object().into_iter().flat_map(|map| map.keys().map(|host| host.to_lowercase())));
    }
    hosts.gitlab.extend(plain(glab_output).lines().filter(|line| is_host_line(line)).map(str::to_lowercase));
    hosts
}

async fn read_hosts() -> Hosts {
    let args = |words: &[&str]| words.iter().map(|word| (*word).to_owned()).collect::<Vec<_>>();
    let (gh_args, glab_args) = (args(&["auth", "status", "--json", "hosts"]), args(&["auth", "status", "--all"]));
    // Quiet: asking which hosts are set up must not turn a pane's badge into a sign-in prompt.
    let (gh, glab) = join(forges::github::cli().exec(&gh_args, "", true), forges::gitlab::cli().exec(&glab_args, "", true)).await;
    let (gh, glab) = (gh.ok(), glab.ok());
    let glab_output = glab.map(|out| format!("{}\n{}", out.stdout, out.stderr)).unwrap_or_default();
    hosts_from(gh.as_ref().map_or("{}", |out| out.stdout.as_str()), &glab_output)
}

/// Forgets which hosts the CLIs know, after signing in or Try again.
pub fn forget_hosts() {
    SIGNED_IN.with(Cached::clear);
    UNRESOLVED.with(|unresolved| unresolved.borrow_mut().clear());
}

/// The remote of a folder's `origin`, when it has been read.
pub fn known(cwd: &str) -> Option<Remote> {
    REMOTES.with(|remotes| remotes.borrow().get(cwd).cloned())
}

async fn resolve(host: &str, path: &str) -> Remote {
    let hosts = SIGNED_IN.with(Cached::clone).get(()).await.unwrap_or_default();
    if hosts.github.contains(host) {
        Remote::Github { host: host.to_owned(), path: path.to_owned() }
    } else if hosts.gitlab.contains(host) {
        Remote::Gitlab { host: host.to_owned(), path: path.to_owned() }
    } else {
        Remote::None { host: Some(host.to_owned()) }
    }
}

/// The remote of a folder's `origin`. A host with no telling name is `None` until `gh` or `glab` is signed in to it,
/// which is asked again after a while.
pub async fn remote_for(cwd: &str) -> Remote {
    if known(cwd).is_none() {
        let remote = parse_remote(&git::ok(cwd, &["remote", "get-url", "origin"]).await.unwrap_or_default());
        REMOTES.with(|remotes| remotes.borrow_mut().insert(cwd.to_owned(), remote));
    }
    let remote = known(cwd).unwrap_or(Remote::None { host: None });
    let Remote::Unknown { host, path } = &remote else { return remote };
    let key = format!("{host}/{path}");
    if UNRESOLVED.with(|unresolved| unresolved.borrow().get(&key).copied()).unwrap_or(0.0) > host::now() {
        return Remote::None { host: Some(host.clone()) };
    }
    let resolved = resolve(host, path).await;
    if matches!(resolved, Remote::None { .. }) {
        UNRESOLVED.with(|unresolved| unresolved.borrow_mut().insert(key, host::now() + UNRESOLVED_TTL_MS));
    } else {
        REMOTES.with(|remotes| remotes.borrow_mut().insert(cwd.to_owned(), resolved.clone()));
    }
    resolved
}

/// The host a tab's PR is on, for its problems: the args say for most forges, the project's folder for GitHub.
pub async fn remote_of_args(args: &PrArgs) -> Option<Remote> {
    if let Some(remote) = args.remote.clone().filter(forges::is_known) {
        return Some(remote);
    }
    if args.cwd.is_empty() {
        return None;
    }
    Some(remote_for(&args.cwd).await).filter(forges::is_known)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_hosts_gh_and_glab_know_come_from_their_status_output() {
        let glab = "gitlab.com\n  ✓ Logged in to gitlab.com as me\n\u{1b}[1;32mgit.corp.example:8443\u{1b}[0m\nnot a host\n";
        let hosts = hosts_from(r#"{"hosts":{"GitHub.com":[],"ghe.example.com":[]}}"#, glab);
        assert_eq!(hosts.github, HashSet::from(["github.com".to_owned(), "ghe.example.com".to_owned()]));
        assert_eq!(hosts.gitlab, HashSet::from(["gitlab.com".to_owned(), "git.corp.example:8443".to_owned()]));
        assert_eq!(hosts_from("not json", ""), Hosts::default());
    }
}
