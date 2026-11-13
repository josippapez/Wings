//! What a pull request's provider is called and looks like, for the badge, the block and the PR tab.

use crate::types::{Link, Provider};

pub struct Info {
    pub name: &'static str,
    pub tone: &'static str,
    /// `#`, or `!` on GitLab, which numbers merge requests `!12`.
    pub prefix: &'static str,
}

pub fn info(provider: Provider) -> Info {
    match provider {
        Provider::Github => Info { name: "GitHub", tone: "merged", prefix: "#" },
        Provider::Azure => Info { name: "Azure DevOps", tone: "warning", prefix: "#" },
        Provider::Gitlab => Info { name: "GitLab", tone: "info", prefix: "!" },
        Provider::Bitbucket => Info { name: "Bitbucket", tone: "danger", prefix: "#" },
        // The only host the manifest's `fetch` reaches is codeberg.org.
        Provider::Gitea => Info { name: "Codeberg", tone: "success", prefix: "#" },
    }
}

pub fn ref_of(provider: Provider, number: u64) -> String {
    format!("{}{number}", info(provider).prefix)
}

pub fn link_ref(link: &Link) -> String {
    ref_of(link.provider, link.number)
}
