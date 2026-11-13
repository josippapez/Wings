//! Which forge a git remote URL points at, as `wings-plugin-sdk/git`'s `parseRemote` tells it for the forges whose CLIs a
//! plugin runs: Azure DevOps (`azure::parse_remote`), GitHub, and GitLab (gitlab.com, or a host with "gitlab" in its
//! name). Any other host, and what isn't a remote URL, is `Other`.

use crate::azure::{AzureRemote, parse_remote as parse_azure};

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Forge {
    Azure(AzureRemote),
    GitHub { host: String, path: String },
    GitLab { host: String, path: String },
    Other,
}

fn host_ok(name: &str) -> bool {
    if name.starts_with('[') && name.ends_with(']') && name.len() > 2 {
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
fn split(text: &str) -> Option<(String, String)> {
    let scheme = text.find("://").filter(|at| {
        let scheme = &text[..*at];
        scheme.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) && scheme.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-'))
    });
    let (host, path) = if let Some(at) = scheme {
        let rest = &text[at + 3..];
        let rest = match rest.split_once('@') {
            Some((user, after)) if !user.is_empty() && !user.contains('/') => after,
            _ => rest,
        };
        let (authority, path) = rest.split_once('/')?;
        let host = if authority.starts_with('[') {
            authority.find(']').map(|end| &authority[..=end])?
        } else {
            authority.split(':').next().unwrap_or_default()
        };
        if text[..at].eq_ignore_ascii_case("file") {
            return None;
        }
        (host, path)
    } else {
        let rest = match text.split_once('@') {
            Some((user, after)) if !user.is_empty() && !user.contains(['/', ':']) => after,
            _ => text,
        };
        let (host, path) = rest.split_once(':')?;
        if host.is_empty() || host.contains('/') || path.starts_with('/') {
            return None;
        }
        (host, path)
    };
    if path.is_empty() || !host_ok(host) {
        return None;
    }
    let path = path.trim_end_matches('/');
    Some((host.to_lowercase(), path.strip_suffix(".git").unwrap_or(path).to_owned()))
}

pub fn parse_remote(url: &str) -> Forge {
    if let Some(azure) = parse_azure(url) {
        return Forge::Azure(azure);
    }
    let Some((host, path)) = split(url.trim()) else {
        return Forge::Other;
    };
    // ssh.github.com on port 443 is github.com's ssh over the https port.
    if host == "github.com" || host == "ssh.github.com" {
        return Forge::GitHub { host: "github.com".into(), path };
    }
    if host.contains("gitlab") {
        return Forge::GitLab { host, path };
    }
    Forge::Other
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn github_and_gitlab_come_from_https_ssh_and_scp_remotes_and_other_hosts_are_other() {
        let github = Forge::GitHub { host: "github.com".into(), path: "acme/web".into() };
        for url in ["https://github.com/acme/web.git", "git@github.com:acme/web.git", "ssh://git@ssh.github.com:443/acme/web", "https://token@github.com/acme/web/\n"] {
            assert_eq!(parse_remote(url), github, "{url}");
        }
        assert_eq!(parse_remote("git@gitlab.com:acme/group/web.git"), Forge::GitLab { host: "gitlab.com".into(), path: "acme/group/web".into() });
        assert_eq!(parse_remote("https://gitlab.corp.example:8443/a/b.git"), Forge::GitLab { host: "gitlab.corp.example".into(), path: "a/b".into() });
        for url in ["https://bitbucket.org/acme/web.git", "git@git.corp.example:acme/web.git", "file:///repo", "/local/path", "", "git@github.com:/x"] {
            assert_eq!(parse_remote(url), Forge::Other, "{url}");
        }
        assert!(matches!(parse_remote("git@ssh.dev.azure.com:v3/acme/Shop/web"), Forge::Azure(a) if a.project == "Shop"));
    }
}
