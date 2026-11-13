//! Which forge a git remote URL points at, as `wings-plugin-sdk/git`'s `parseRemote` tells.

use crate::azure::{self, AzureRemote};
use serde_json::{Value, json};

/// Hosts of Gitea and Forgejo that are known by name.
const GITEA_HOSTS: &[&str] = &["codeberg.org"];

/// Where a git remote is. `path` is `owner/repo`. A host that no provider's name gives away is `Unknown`, which a
/// signed-in `gh` or `glab` can settle; what isn't a remote URL is `None`.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum Remote {
    Github { host: String, path: String },
    Azure(AzureRemote),
    Gitlab { host: String, path: String },
    Bitbucket { workspace: String, repo: String },
    Gitea { host: String, owner: String, repo: String },
    Unknown { host: String, path: String },
    None { host: Option<String> },
}

impl Remote {
    /// The remote as the TypeScript's `Remote` object, the form kept in a tab's args.
    pub fn to_json(&self) -> Value {
        match self {
            Self::Github { host, path } => json!({ "kind": "github", "host": host, "path": path }),
            Self::Azure(a) => json!({ "kind": "azure", "org": a.org, "project": a.project, "repo": a.repo }),
            Self::Gitlab { host, path } => json!({ "kind": "gitlab", "host": host, "path": path }),
            Self::Bitbucket { workspace, repo } => json!({ "kind": "bitbucket", "workspace": workspace, "repo": repo }),
            Self::Gitea { host, owner, repo } => json!({ "kind": "gitea", "host": host, "owner": owner, "repo": repo }),
            Self::Unknown { host, path } => json!({ "kind": "unknown", "host": host, "path": path }),
            Self::None { host: Some(host) } => json!({ "kind": "none", "host": host }),
            Self::None { host: None } => json!({ "kind": "none" }),
        }
    }

    /// The remote `to_json` wrote, or none for anything that isn't one.
    pub fn from_json(value: &Value) -> Option<Self> {
        let text = |name: &str| value[name].as_str().map(str::to_owned);
        Some(match value["kind"].as_str()? {
            "github" => Self::Github { host: text("host")?, path: text("path")? },
            "azure" => Self::Azure(AzureRemote { org: text("org")?, project: text("project")?, repo: text("repo")? }),
            "gitlab" => Self::Gitlab { host: text("host")?, path: text("path")? },
            "bitbucket" => Self::Bitbucket { workspace: text("workspace")?, repo: text("repo")? },
            "gitea" => Self::Gitea { host: text("host")?, owner: text("owner")?, repo: text("repo")? },
            "unknown" => Self::Unknown { host: text("host")?, path: text("path")? },
            "none" => Self::None { host: text("host") },
            _ => return None,
        })
    }
}

/// Bitbucket workspace and repository slugs, which the API takes in the path as they are.
fn is_slug(text: &str) -> bool {
    let mut chars = text.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphanumeric()) && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// Whether `text` starts with `scheme://`.
fn scheme_of(text: &str) -> Option<&str> {
    let (scheme, _) = text.split_once("://")?;
    let mut chars = scheme.chars();
    (chars.next()?.is_ascii_alphabetic() && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-'))).then_some(scheme)
}

/// A host name or a bracketed address: letters and digits with dots and dashes inside.
fn is_host(name: &str) -> bool {
    if name.len() > 2 && name.starts_with('[') && name.ends_with(']') {
        return true;
    }
    let bytes = name.as_bytes();
    !bytes.is_empty()
        && bytes[0].is_ascii_alphanumeric()
        && bytes[bytes.len() - 1].is_ascii_alphanumeric()
        && bytes.iter().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-'))
}

/// The host and repository path of an https, ssh or git URL, or scp-style `user@host:path`. A port is dropped: an ssh
/// port is the git server's, not the web server's that the CLIs and APIs talk to.
fn split_remote(text: &str) -> Option<(String, String)> {
    let (scheme, name, path) = if let Some(scheme) = scheme_of(text) {
        let rest = &text[scheme.len() + 3..];
        let (authority, path) = rest.split_once('/')?;
        let hostport = match authority.rsplit_once('@') {
            Some((user, hostport)) if !user.is_empty() => hostport,
            Some(_) => return None,
            None => authority,
        };
        let name = if hostport.starts_with('[') {
            let end = hostport.find(']')?;
            let (name, after) = hostport.split_at(end + 1);
            (after.is_empty() || (after.starts_with(':') && after.len() > 1 && after[1..].bytes().all(|b| b.is_ascii_digit()))).then_some(name)?
        } else {
            match hostport.split_once(':') {
                Some((name, port)) => (!port.is_empty() && port.bytes().all(|b| b.is_ascii_digit())).then_some(name)?,
                None => hostport,
            }
        };
        (Some(scheme), name, path)
    } else {
        // scp-style `[user@]host:path`: what follows the colon is a path even when it starts with digits.
        let (front, path) = text.split_once(':')?;
        let name = match front.split_once('@') {
            Some((user, name)) if !user.is_empty() && !user.contains('/') => name,
            Some(_) => return None,
            None => front,
        };
        if name.is_empty() || name.contains('/') || path.is_empty() || path.starts_with('/') {
            return None;
        }
        (None, name, path)
    };
    if path.is_empty() || scheme.is_some_and(|scheme| scheme.eq_ignore_ascii_case("file")) || !is_host(name) {
        return None;
    }
    let trimmed = path.trim_end_matches('/');
    let trimmed = trimmed.strip_suffix(".git").unwrap_or(trimmed);
    Some((name.to_lowercase(), trimmed.to_owned()))
}

/// Which forge a remote URL points at, from its form: Azure DevOps (ssh.dev.azure.com, dev.azure.com, `{org}.visualstudio.com`
/// and vs-ssh.visualstudio.com), github.com, gitlab.com and hosts with "gitlab" in their name, Bitbucket Cloud and
/// Codeberg. Any other host is `Unknown`, and what isn't a remote URL is `None`.
pub fn parse_remote(url: &str) -> Remote {
    if let Some(remote) = azure::parse_remote(url) {
        return Remote::Azure(remote);
    }
    let Some((host, path)) = split_remote(url.trim()) else {
        return Remote::None { host: None };
    };
    // ssh.github.com on port 443 is github.com's ssh over the https port.
    if host == "github.com" || host == "ssh.github.com" {
        return Remote::Github { host: "github.com".into(), path };
    }
    let segments: Vec<&str> = path.split('/').collect();
    if host == "bitbucket.org" {
        return if segments.len() == 2 && segments.iter().all(|segment| is_slug(segment)) {
            Remote::Bitbucket { workspace: segments[0].into(), repo: segments[1].into() }
        } else {
            Remote::None { host: Some(host) }
        };
    }
    if GITEA_HOSTS.contains(&host.as_str()) {
        return if segments.len() == 2 { Remote::Gitea { host, owner: segments[0].into(), repo: segments[1].into() } } else { Remote::None { host: Some(host) } };
    }
    // gitlab.com, or a self-managed server with "gitlab" in its host name.
    if host.contains("gitlab") {
        return Remote::Gitlab { host, path };
    }
    Remote::Unknown { host, path }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_remote_is_told_from_its_form() {
        assert_eq!(parse_remote("git@github.com:acme/widgets.git"), Remote::Github { host: "github.com".into(), path: "acme/widgets".into() });
        assert_eq!(parse_remote("ssh://git@ssh.github.com:443/acme/widgets.git"), Remote::Github { host: "github.com".into(), path: "acme/widgets".into() });
        assert_eq!(parse_remote("https://gitlab.example.com/group/sub/app.git"), Remote::Gitlab { host: "gitlab.example.com".into(), path: "group/sub/app".into() });
        assert_eq!(parse_remote("git@bitbucket.org:team/app.git"), Remote::Bitbucket { workspace: "team".into(), repo: "app".into() });
        assert_eq!(parse_remote("https://codeberg.org/someone/app"), Remote::Gitea { host: "codeberg.org".into(), owner: "someone".into(), repo: "app".into() });
        assert_eq!(parse_remote("https://git.corp.example/team/app.git/"), Remote::Unknown { host: "git.corp.example".into(), path: "team/app".into() });
        assert!(matches!(parse_remote("https://dev.azure.com/acme/Proj/_git/app"), Remote::Azure(_)));
    }

    #[test]
    fn what_is_not_a_remote_is_none_and_a_bitbucket_or_codeberg_path_of_another_shape_keeps_only_its_host() {
        assert_eq!(parse_remote("/home/me/app"), Remote::None { host: None });
        assert_eq!(parse_remote("file:///home/me/app"), Remote::None { host: None });
        assert_eq!(parse_remote(""), Remote::None { host: None });
        assert_eq!(parse_remote("https://bitbucket.org/team"), Remote::None { host: Some("bitbucket.org".into()) });
        assert_eq!(parse_remote("https://codeberg.org/a/b/c"), Remote::None { host: Some("codeberg.org".into()) });
        // A port is the git server's, not the web server's.
        assert_eq!(parse_remote("ssh://git@git.example.com:2222/team/app.git"), Remote::Unknown { host: "git.example.com".into(), path: "team/app".into() });
        // scp-style: what follows the colon is a path even when it starts with digits.
        assert_eq!(parse_remote("git@git.example.com:1234/app.git"), Remote::Unknown { host: "git.example.com".into(), path: "1234/app".into() });
    }

    #[test]
    fn a_remote_survives_json() {
        for url in ["git@github.com:acme/w.git", "https://dev.azure.com/o/P/_git/r", "git@bitbucket.org:t/a", "https://codeberg.org/a/b", "https://gitlab.com/g/a"] {
            let remote = parse_remote(url);
            assert_eq!(Remote::from_json(&remote.to_json()), Some(remote));
        }
    }
}
