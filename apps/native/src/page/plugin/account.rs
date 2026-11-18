//! The Claude Code account a plugin's `spawn` or `exec` runs under. A plugin asks with `account`, and the host maps it to
//! a folder it already knows: the account that holds a session's transcript, else the account of the project the program
//! runs in, as a new terminal of that project gets. A plugin never names a folder, and nothing but `session` is read.

use std::path::{Path, PathBuf};

use serde::Deserialize;
use wings_plugins::transcript::find_transcript;

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AccountRequest {
    /// A Claude Code session's id: the program runs under the account its transcript is in.
    session: Option<String>,
}

/// The folder for `CLAUDE_CONFIG_DIR`, or `None` to leave it as Wings has it, which is the default account's.
/// `dirs` are the account folders there are, the default one's first. `project` is the account folder of the project
/// the program runs in, asked only when no transcript decides.
pub(super) fn config_dir(
    request: &AccountRequest,
    dirs: &[PathBuf],
    project: impl FnOnce() -> Option<PathBuf>,
) -> Option<PathBuf> {
    let in_session = request
        .session
        .as_deref()
        .and_then(|id| find_transcript(dirs, id))
        .and_then(|transcript| dirs.iter().find(|dir| transcript.starts_with(dir)));
    match in_session {
        Some(dir) => (Some(dir) != dirs.first()).then(|| dir.clone()),
        None => project(),
    }
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
        AccountRequest { session: session.map(str::to_owned) }
    }

    #[test]
    fn a_resumed_session_runs_under_the_account_its_transcript_is_in() {
        let (base, dirs) = accounts("session");
        let project = || Some(PathBuf::from("/accounts/project"));
        assert_eq!(config_dir(&asking(Some(ID)), &dirs, project), Some(dirs[1].clone()));
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
        assert_eq!(config_dir(&asking(Some(ID)), &dirs, project), None);
        std::fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn a_new_chat_runs_under_the_account_of_its_project() {
        let (base, dirs) = accounts("project");
        let project = || Some(PathBuf::from("/accounts/project"));
        assert_eq!(config_dir(&asking(None), &dirs, project), Some(PathBuf::from("/accounts/project")));
        // A session with no transcript yet is a new one.
        let unknown = "99999999-2222-4333-8444-555555555555";
        assert_eq!(config_dir(&asking(Some(unknown)), &dirs, project), Some(PathBuf::from("/accounts/project")));
        assert_eq!(config_dir(&asking(None), &dirs, || None), None);
        std::fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn an_id_that_is_not_a_session_id_finds_nothing() {
        let (base, dirs) = accounts("shape");
        let project = || None;
        assert_eq!(config_dir(&asking(Some("../../etc/passwd")), &dirs, project), None);
        std::fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn a_plugin_cannot_name_a_folder() {
        for raw in [r#"{"dir":"/etc"}"#, r#"{"session":null,"configDir":"/etc"}"#, r#"{"path":"/etc"}"#] {
            assert!(serde_json::from_str::<AccountRequest>(raw).is_err(), "{raw}");
        }
        assert!(serde_json::from_str::<AccountRequest>(r#"{"session":"a"}"#).is_ok());
        assert!(serde_json::from_str::<AccountRequest>("{}").is_ok());
    }

    #[test]
    fn the_deepest_project_holds_a_folder() {
        let projects = [PathBuf::from("/a"), PathBuf::from("/a/b"), PathBuf::from("/c")];
        assert_eq!(holder(&projects, Path::new("/a/b/c")), Some(&projects[1]));
        assert_eq!(holder(&projects, Path::new("/a/x")), Some(&projects[0]));
        assert_eq!(holder(&projects, Path::new("/d")), None);
    }
}
