//! Command-line tools plugins depend on, like the GitHub CLI. A plugin needs a tool when its manifest lets it
//! run that program. Wings knows how to install a few of them the vendor's own way, from this fixed table:
//! plugins only name a program, and never supply what gets run to install it.

use std::path::PathBuf;

use serde::Serialize;

use crate::plugins::Permissions;

pub struct KnownTool {
    pub program: &'static str,
    pub name: &'static str,
    /// The vendor's Homebrew install command, typed into a new Wings tab once you confirm.
    pub brew: &'static str,
    /// The vendor's install page, for when Homebrew can't be used.
    pub docs: &'static str,
}

/// Commands from each vendor's macOS install docs.
pub const KNOWN: [KnownTool; 3] = [
    KnownTool {
        program: "az",
        name: "Azure CLI",
        brew: "brew update && brew install azure-cli",
        docs: "https://learn.microsoft.com/cli/azure/install-azure-cli",
    },
    KnownTool { program: "gh", name: "GitHub CLI", brew: "brew install gh", docs: "https://github.com/cli/cli#installation" },
    KnownTool { program: "glab", name: "GitLab CLI", brew: "brew install glab", docs: "https://gitlab.com/gitlab-org/cli#installation" },
];

pub fn known(program: &str) -> Option<&'static KnownTool> {
    KNOWN.iter().find(|t| t.program == program)
}

/// The known tools a plugin runs: the programs its `exec` entries name, and the CLIs its `post` entries go
/// through. In table order, each once.
pub fn needed(permissions: &Permissions) -> Vec<&'static KnownTool> {
    let programs: Vec<&str> = permissions.exec.iter().chain(&permissions.post).filter_map(|e| e.split_whitespace().next()).collect();
    KNOWN.iter().filter(|t| programs.contains(&t.program)).collect()
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolStatus {
    pub program: String,
    pub name: String,
    pub installed: bool,
    /// What Wings runs to install it: set when it's missing, on macOS, with Homebrew installed.
    pub command: Option<String>,
    /// Missing on macOS without Homebrew, so the UI links to brew.sh too.
    pub needs_homebrew: bool,
    pub docs: String,
}

/// Where Wings looks for programs, so tests can stand in for the real PATH.
pub type Find<'a> = &'a dyn Fn(&str) -> Option<PathBuf>;

pub fn status(tool: &KnownTool, find: Find, macos: bool) -> ToolStatus {
    let installed = find(tool.program).is_some();
    let brew = !installed && macos && find("brew").is_some();
    ToolStatus {
        program: tool.program.into(),
        name: tool.name.into(),
        installed,
        command: brew.then(|| tool.brew.into()),
        needs_homebrew: !installed && macos && !brew,
        docs: tool.docs.into(),
    }
}

pub fn statuses(permissions: &Permissions, find: Find, macos: bool) -> Vec<ToolStatus> {
    needed(permissions).into_iter().map(|t| status(t, find, macos)).collect()
}

/// A tool a plugin asked Wings to install: only one in the table that the plugin's manifest runs.
pub fn requested(plugin_id: &str, permissions: &Permissions, program: &str, find: Find, macos: bool) -> Result<ToolStatus, String> {
    let tool = needed(permissions).into_iter().find(|t| t.program == program);
    tool.map(|t| status(t, find, macos)).ok_or_else(|| match known(program) {
        Some(_) => format!("{plugin_id} doesn't run {program}, so it can't ask to install it"),
        None => format!("Wings doesn't know how to install {program}. It can install {}", KNOWN.map(|t| t.program).join(", ")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn perms(exec: &[&str], post: &[&str]) -> Permissions {
        Permissions { exec: exec.iter().map(|s| s.to_string()).collect(), post: post.iter().map(|s| s.to_string()).collect(), ..Default::default() }
    }

    /// A PATH holding only `programs`.
    fn path_with(programs: &'static [&'static str]) -> impl Fn(&str) -> Option<PathBuf> {
        move |p| programs.contains(&p).then(|| PathBuf::from("/fake/bin").join(p))
    }

    #[test]
    fn the_table_has_each_vendors_command() {
        assert_eq!(known("az").unwrap().brew, "brew update && brew install azure-cli");
        assert_eq!(known("gh").unwrap().brew, "brew install gh");
        assert_eq!(known("glab").unwrap().brew, "brew install glab");
        assert!(KNOWN.iter().all(|t| t.docs.starts_with("https://")));
        assert!(known("git").is_none() && known("brew").is_none() && known("").is_none());
    }

    #[test]
    fn a_plugin_needs_the_known_programs_its_manifest_runs() {
        let p = perms(&["git branch --show-current", "gh pr list", "gh api", "az boards work-item update [--id --state]", "rm"], &[]);
        assert_eq!(needed(&p).iter().map(|t| t.program).collect::<Vec<_>>(), ["az", "gh"]);
        let p = perms(&["git status"], &["glab projects/*/merge_requests/*/notes"]);
        assert_eq!(needed(&p).iter().map(|t| t.program).collect::<Vec<_>>(), ["glab"], "post goes through the CLI too");
        // A program name that only starts like one in the table isn't it.
        assert!(needed(&perms(&["ghost run", "azure x"], &[])).is_empty());
    }

    #[test]
    fn the_shipped_plugins_need_their_clis() {
        let need = |id: &str| {
            let dir = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../../plugins")).join(id);
            needed(&crate::plugins::load(&dir).unwrap().manifest.permissions).iter().map(|t| t.program).collect::<Vec<_>>()
        };
        assert_eq!(need("pr-tracker"), ["az", "gh", "glab"]);
        assert_eq!(need("work-item"), ["az"]);
    }

    #[test]
    fn a_missing_tool_offers_homebrew_only_when_it_is_there() {
        let az = known("az").unwrap();
        let s = status(az, &path_with(&["brew"]), true);
        assert!(!s.installed && !s.needs_homebrew);
        assert_eq!(s.command.as_deref(), Some("brew update && brew install azure-cli"));

        let s = status(az, &path_with(&[]), true);
        assert!(!s.installed && s.needs_homebrew && s.command.is_none(), "no Homebrew: links instead of running anything");

        let s = status(az, &path_with(&["brew"]), false);
        assert!(!s.installed && !s.needs_homebrew && s.command.is_none(), "off macOS it's a link only");

        let s = status(az, &path_with(&["az"]), true);
        assert!(s.installed && !s.needs_homebrew && s.command.is_none());
    }

    #[test]
    fn a_plugin_can_only_ask_for_tools_it_runs_and_wings_knows() {
        let p = perms(&["gh pr list", "git status"], &[]);
        let find = path_with(&["brew"]);
        assert_eq!(requested("pr", &p, "gh", &find, true).unwrap().command.as_deref(), Some("brew install gh"));
        assert!(requested("pr", &p, "az", &find, true).unwrap_err().contains("doesn't run az"));
        assert!(requested("pr", &p, "git", &find, true).unwrap_err().contains("doesn't know how to install git"));
        assert!(requested("pr", &p, "gh; rm -rf ~", &find, true).is_err());
    }

    #[test]
    fn the_real_path_check_sees_a_missing_program() {
        let find = |p: &str| crate::plugins::find_program(p);
        let fake = KnownTool { program: "wings-no-such-program-xyz", name: "Fake", brew: "brew install fake", docs: "https://example.com/" };
        assert!(!status(&fake, &find, cfg!(target_os = "macos")).installed);
    }
}
