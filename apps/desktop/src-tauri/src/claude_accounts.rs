//! Claude Code accounts. Each has a config folder of its own, picked with `CLAUDE_CONFIG_DIR`: settings, `.claude.json`
//! (user-scope MCP servers, the signed-in account), transcripts, and the sign-in itself. The default account is the
//! folder Wings runs under (`claude::claude_dir`); a named one is `~/.claude-<name>`. The terminals of a project use the
//! account assigned to it (`Space::claude_account`).

use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

use serde::Serialize;
use serde_json::Value;

use crate::{claude::claude_dir, spaces::Space};

const PREFIX: &str = ".claude-";

/// `^[a-z0-9][a-z0-9-]{0,31}$`. The name becomes part of a folder name, so nothing else gets through.
pub fn valid_name(name: &str) -> bool {
    let plain = |b: u8| b.is_ascii_lowercase() || b.is_ascii_digit();
    (1..=32).contains(&name.len()) && name.bytes().next().is_some_and(plain) && name.bytes().all(|b| plain(b) || b == b'-')
}

fn home() -> PathBuf {
    std::env::home_dir().unwrap_or_default()
}

/// The config folder of an account, `None` being the default one. `name` has to be valid.
pub fn dir(name: Option<&str>) -> PathBuf {
    match name {
        Some(name) => {
            debug_assert!(valid_name(name), "{name:?}");
            home().join(format!("{PREFIX}{name}"))
        }
        None => claude_dir(),
    }
}

/// Claude Code's user-scope config, where `mcpServers` and the signed-in account are. Without `CLAUDE_CONFIG_DIR` the
/// default account's sits in the home folder, not in `~/.claude`.
pub fn user_config(name: Option<&str>) -> PathBuf {
    match (name, std::env::var_os("CLAUDE_CONFIG_DIR")) {
        (None, None) => home().join(".claude.json"),
        _ => dir(name).join(".claude.json"),
    }
}

/// Makes the folder of a named account unless it is there.
pub fn create_dir(name: &str) -> std::io::Result<PathBuf> {
    let dir = dir(Some(name));
    make_private(&dir)?;
    Ok(dir)
}

/// For you only, like Claude Code's own folder. One that exists keeps the permissions it has.
fn make_private(dir: &Path) -> std::io::Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(dir)
}

/// The names of the `~/.claude-<name>` folders there are.
pub fn existing_names() -> BTreeSet<String> {
    let Ok(entries) = std::fs::read_dir(home()) else { return BTreeSet::new() };
    entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let name = name.strip_prefix(PREFIX)?;
            (valid_name(name) && entry.path().is_dir()).then(|| name.to_string())
        })
        .collect()
}

/// Every folder Claude Code sessions may be in, the default account's first.
pub fn dirs() -> Vec<PathBuf> {
    let mut dirs = vec![dir(None)];
    for name in existing_names() {
        let account = dir(Some(&name));
        if !dirs.contains(&account) {
            dirs.push(account);
        }
    }
    dirs
}

/// Each project with the account assigned to it. A name that isn't valid, from a spaces.json edited by hand, counts
/// as none.
fn assigned(spaces: &[Space]) -> impl Iterator<Item = (&str, &str)> + '_ {
    spaces.iter().filter_map(|space| Some((space.id.as_str(), space.claude_account.as_deref().filter(|name| valid_name(name))?)))
}

/// The folder for `CLAUDE_CONFIG_DIR` in a new terminal of the project, `None` for the default account.
pub fn pane_dir(spaces: &[Space], project_id: &str) -> Option<PathBuf> {
    assigned(spaces).find(|(id, _)| *id == project_id).map(|(_, name)| dir(Some(name)))
}

/// The accounts Wings connects itself to: the default and each one a project uses.
pub fn connect_targets(spaces: &[Space]) -> Vec<Option<String>> {
    let names: BTreeSet<&str> = assigned(spaces).map(|(_, name)| name).collect();
    std::iter::once(None).chain(names.into_iter().map(|name| Some(name.to_string()))).collect()
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    /// `None` for the default account.
    pub name: Option<String>,
    pub dir: String,
    pub email: Option<String>,
    /// Claude Code's config names an account. A hint: it can't tell an expired sign-in from a good one.
    pub signed_in: bool,
}

#[derive(Debug, Serialize)]
pub struct Accounts {
    pub accounts: Vec<Account>,
    /// The account name of each project that has one, by project id.
    pub projects: BTreeMap<String, String>,
}

/// The email of the account a Claude Code config file names, and whether it names one. Both are absent for a file that
/// is missing or isn't JSON.
fn signed_in_as(config: &Path) -> (Option<String>, bool) {
    let config = std::fs::read_to_string(config).ok().and_then(|text| serde_json::from_str::<Value>(&text).ok());
    let oauth = config.as_ref().and_then(|config| config.get("oauthAccount")).filter(|oauth| oauth.is_object());
    (oauth.and_then(|oauth| oauth.get("emailAddress")).and_then(Value::as_str).map(String::from), oauth.is_some())
}

fn account(name: Option<&str>) -> Account {
    let (email, signed_in) = signed_in_as(&user_config(name));
    Account { name: name.map(String::from), dir: dir(name).to_string_lossy().into_owned(), email, signed_in }
}

/// The default account, every `~/.claude-<name>` folder signed in to an account, and the names projects are assigned
/// even if nobody signed in there yet. Other tools keep folders named `~/.claude-<something>` too, so a folder alone
/// isn't an account.
pub fn list(spaces: &[Space]) -> Accounts {
    let projects: BTreeMap<String, String> = assigned(spaces).map(|(id, name)| (id.to_string(), name.to_string())).collect();
    let assigned: BTreeSet<&String> = projects.values().collect();
    let signed_in = existing_names().into_iter().map(|name| account(Some(&name))).filter(|a| a.signed_in);
    let mut named: Vec<Account> = signed_in.filter(|a| !a.name.as_ref().is_some_and(|n| assigned.contains(n))).collect();
    named.extend(assigned.iter().map(|name| account(Some(name))));
    named.sort_by(|a, b| a.name.cmp(&b.name));
    let accounts = std::iter::once(account(None)).chain(named).collect();
    Accounts { accounts, projects }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn space(id: &str, account: Option<&str>) -> Space {
        Space { id: id.into(), name: id.into(), path: id.into(), parent: None, added_at: None, pinned: false, worktree_include: None, worktree_setup: None, claude_account: account.map(String::from) }
    }

    #[test]
    fn a_name_is_a_short_slug_and_never_a_path() {
        for good in ["work", "a", "0", "client-2", "a-", &"a".repeat(32)] {
            assert!(valid_name(good), "{good:?}");
        }
        for bad in ["", "-work", "Work", "a_b", "a b", "a.b", ".", "..", "../x", "a/b", "/etc", "~", "work\n", "é", &"a".repeat(33)] {
            assert!(!valid_name(bad), "{bad:?}");
        }
    }

    #[test]
    fn a_named_account_lives_beside_the_home_folder_and_the_default_stays_put() {
        assert_eq!(dir(Some("work")), home().join(".claude-work"));
        assert_eq!(dir(None), claude_dir());
        assert_eq!(user_config(Some("work")), home().join(".claude-work").join(".claude.json"));
    }

    #[test]
    fn a_terminal_gets_the_account_of_its_project_only() {
        let spaces = [space("/a", Some("work")), space("/b", None), space("/c", Some("../etc"))];
        assert_eq!(pane_dir(&spaces, "/a"), Some(home().join(".claude-work")));
        assert_eq!(pane_dir(&spaces, "/b"), None);
        assert_eq!(pane_dir(&spaces, "/c"), None, "a name edited into spaces.json by hand is not a folder");
        assert_eq!(pane_dir(&spaces, "/nowhere"), None);
    }

    #[test]
    fn wings_connects_to_the_default_and_each_account_in_use_once() {
        let spaces = [space("/a", Some("work")), space("/b", Some("work")), space("/c", Some("home")), space("/d", None)];
        assert_eq!(connect_targets(&spaces), [None, Some("home".to_string()), Some("work".to_string())]);
        assert_eq!(connect_targets(&[]), [None]);
    }

    #[cfg(unix)]
    #[test]
    fn a_new_account_folder_is_for_you_only_and_an_old_one_is_left_as_it_is() {
        use std::os::unix::fs::PermissionsExt;
        let mode = |dir: &Path| std::fs::metadata(dir).unwrap().permissions().mode() & 0o777;
        let root = std::env::temp_dir().join(format!("wings-accounts-mode-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let (new, old) = (root.join(".claude-new"), root.join(".claude-old"));
        make_private(&new).unwrap();
        assert_eq!(mode(&new), 0o700);
        std::fs::create_dir(&old).unwrap();
        std::fs::set_permissions(&old, std::fs::Permissions::from_mode(0o755)).unwrap();
        make_private(&old).unwrap();
        assert_eq!(mode(&old), 0o755);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_account_list_reads_the_signed_in_email() {
        let dir = std::env::temp_dir().join(format!("wings-accounts-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let read = |text: &str| {
            std::fs::write(dir.join(".claude.json"), text).unwrap();
            signed_in_as(&dir.join(".claude.json"))
        };
        assert_eq!(read(r#"{"oauthAccount":{"emailAddress":"me@example.com"}}"#), (Some("me@example.com".into()), true));
        assert_eq!(read(r#"{"oauthAccount":{}}"#), (None, true));
        assert_eq!(read(r#"{"numStartups":3}"#), (None, false));
        assert_eq!(read("not json"), (None, false));
        assert_eq!(signed_in_as(&dir.join("missing.json")), (None, false));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
