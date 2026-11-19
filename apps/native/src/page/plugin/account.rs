//! The Claude Code account a plugin's `spawn` or `exec` runs under. A plugin asks with `account`, and the host maps it to
//! a folder it already knows: the account that holds a session's transcript, else the account the plugin picked by name
//! (`pick`), else the account of the project the program runs in, as a new terminal of that project gets. A plugin never
//! names a folder: a picked name has to be one of the account folders there are.

use std::path::{Path, PathBuf};

use serde::Deserialize;
use wings_plugins::transcript::find_transcript;
use wings_spaces::claude_accounts::{label, valid_name};

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AccountRequest {
    /// A Claude Code session's id: the program runs under the account its transcript is in.
    session: Option<String>,
    /// An account picked by name for a new session, `{ "name": null }` being the default one. A session's transcript
    /// still decides over it.
    #[serde(default)]
    pick: Option<Pick>,
}

/// An account picked by its name, `None` for the default one.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Pick {
    name: Option<String>,
}

impl AccountRequest {
    /// Whether it picks an account by name, which needs the `claudeAccounts` permission.
    pub(super) fn picks(&self) -> bool {
        self.pick.is_some()
    }
}

/// The folder for `CLAUDE_CONFIG_DIR`, or `None` to leave it as Wings has it, which is the default account's.
/// `dirs` are the account folders there are, the default one's first. `project` is the account folder of the project
/// the program runs in, asked only when neither a transcript nor a pick decides. A picked name that isn't one of `dirs`
/// is refused.
pub(super) fn config_dir(
    request: &AccountRequest,
    dirs: &[PathBuf],
    project: impl FnOnce() -> Option<PathBuf>,
) -> Result<Option<PathBuf>, String> {
    let in_session = request
        .session
        .as_deref()
        .and_then(|id| find_transcript(dirs, id))
        .and_then(|transcript| dirs.iter().find(|dir| transcript.starts_with(dir)));
    if let Some(dir) = in_session {
        return Ok((Some(dir) != dirs.first()).then(|| dir.clone()));
    }
    match &request.pick {
        Some(Pick { name: None }) => Ok(None),
        Some(Pick { name: Some(name) }) => picked(name, dirs).map(Some),
        None => Ok(project()),
    }
}

/// The folder of the account named `name`, among `dirs` after the default one's.
fn picked(name: &str, dirs: &[PathBuf]) -> Result<PathBuf, String> {
    let default = dirs.first().ok_or("There are no Claude Code accounts")?;
    let found = valid_name(name)
        .then(|| dirs.iter().skip(1).find(|dir| label(dir, default) == Some(name)))
        .flatten();
    found.cloned().ok_or_else(|| format!("{name:?} isn't a Claude Code account here"))
}

/// The project that holds `cwd`: the deepest one, as Past sessions picks where a session opens.
pub(super) fn holder<'a>(projects: &'a [PathBuf], cwd: &Path) -> Option<&'a PathBuf> {
    projects
        .iter()
        .filter(|project| cwd.starts_with(project))
        .max_by_key(|project| project.as_os_str().len())
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "11111111-2222-4333-8444-555555555555";

    /// Account folders in a scratch folder, the default first, with the session in the second one.
    fn accounts(name: &str) -> (PathBuf, Vec<PathBuf>) {
        let base = std::env::temp_dir().join(format!("wings-account-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let dirs = vec![base.join(".claude"), base.join(".claude-work")];
        std::fs::create_dir_all(dirs[0].join("projects/p")).unwrap();
        let folder = dirs[1].join("projects/p");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join(format!("{ID}.jsonl")), "{}\n").unwrap();
        (base, dirs)
    }

    fn asking(session: Option<&str>) -> AccountRequest {
        AccountRequest { session: session.map(str::to_owned), pick: None }
    }

    #[test]
    fn a_resumed_session_runs_under_the_account_its_transcript_is_in() {
        let (base, dirs) = accounts("session");
        let project = || Some(PathBuf::from("/accounts/project"));
        assert_eq!(config_dir(&asking(Some(ID)), &dirs, project), Ok(Some(dirs[1].clone())));
        std::fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn a_session_in_the_default_account_leaves_wings_own_environment() {
        let (base, dirs) = accounts("default");
        // Even when the project has an account of its own: the transcript is where the session can be resumed.
        let folder = dirs[0].join("projects/p");
        std::fs::write(folder.join(format!("{ID}.jsonl")), "{}\n").unwrap();
        std::fs::remove_file(dirs[1].join(format!("projects/p/{ID}.jsonl"))).unwrap();
        let project = || Some(PathBuf::from("/accounts/project"));
        assert_eq!(config_dir(&asking(Some(ID)), &dirs, project), Ok(None));
        std::fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn a_new_chat_runs_under_the_account_of_its_project() {
        let (base, dirs) = accounts("project");
        let project = || Some(PathBuf::from("/accounts/project"));
        assert_eq!(config_dir(&asking(None), &dirs, project), Ok(Some(PathBuf::from("/accounts/project"))));
        // A session with no transcript yet is a new one.
        let unknown = "99999999-2222-4333-8444-555555555555";
        assert_eq!(config_dir(&asking(Some(unknown)), &dirs, project), Ok(Some(PathBuf::from("/accounts/project"))));
        assert_eq!(config_dir(&asking(None), &dirs, || None), Ok(None));
        std::fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn an_id_that_is_not_a_session_id_finds_nothing() {
        let (base, dirs) = accounts("shape");
        let project = || None;
        assert_eq!(config_dir(&asking(Some("../../etc/passwd")), &dirs, project), Ok(None));
        std::fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn a_plugin_cannot_name_a_folder() {
        for raw in [r#"{"dir":"/etc"}"#, r#"{"session":null,"configDir":"/etc"}"#, r#"{"path":"/etc"}"#] {
            assert!(serde_json::from_str::<AccountRequest>(raw).is_err(), "{raw}");
        }
        assert!(serde_json::from_str::<AccountRequest>(r#"{"session":"a"}"#).is_ok());
        assert!(serde_json::from_str::<AccountRequest>("{}").is_ok());
        // A pick names an account, never a folder.
        assert!(serde_json::from_str::<AccountRequest>(r#"{"pick":{"name":"work","dir":"/etc"}}"#).is_err());
        assert!(serde_json::from_str::<AccountRequest>(r#"{"pick":{"name":"work"}}"#).is_ok());
    }

    fn picking(session: Option<&str>, name: Option<&str>) -> AccountRequest {
        AccountRequest { session: session.map(str::to_owned), pick: Some(Pick { name: name.map(str::to_owned) }) }
    }

    #[test]
    fn a_new_chat_runs_under_the_account_it_picked() {
        let (base, dirs) = accounts("pick");
        let project = || Some(PathBuf::from("/accounts/project"));
        // `.claude-work` is the named account `work`.
        assert_eq!(config_dir(&picking(None, Some("work")), &dirs, project), Ok(Some(dirs[1].clone())));
        // The default account, even in a project that has an account of its own.
        assert_eq!(config_dir(&picking(None, None), &dirs, project), Ok(None));
        // A name that isn't an account folder here, or isn't a name at all, is refused.
        assert!(config_dir(&picking(None, Some("home")), &dirs, project).is_err());
        assert!(config_dir(&picking(None, Some("../work")), &dirs, project).is_err());
        // A resumed session stays in the account its transcript is in.
        assert_eq!(config_dir(&picking(Some(ID), None), &dirs, project), Ok(Some(dirs[1].clone())));
        std::fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn the_deepest_project_holds_a_folder() {
        let projects = [PathBuf::from("/a"), PathBuf::from("/a/b"), PathBuf::from("/c")];
        assert_eq!(holder(&projects, Path::new("/a/b/c")), Some(&projects[1]));
        assert_eq!(holder(&projects, Path::new("/a/x")), Some(&projects[0]));
        assert_eq!(holder(&projects, Path::new("/d")), None);
    }
}
