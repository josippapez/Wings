//! Claude Code accounts. Each has a config folder of its own, picked with `CLAUDE_CONFIG_DIR`: settings, `.claude.json`
//! (user-scope MCP servers, the signed-in account), transcripts, and the sign-in itself. The default account is the
//! folder Wings runs under (`claude::claude_dir`); a named one is `~/.claude-<name>`. The terminals of a project use the
//! account assigned to it (`Space::claude_account`). A named account uses the default account's setup (settings, rules,
//! skills, plugins) unless you turn that off; see `set_shared_setup`.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
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

/// The folder for `CLAUDE_CONFIG_DIR` in a pane that was asked for an account of its own: `None` for the default account,
/// which is what the app itself runs under.
pub fn pick_dir(pick: &AccountPick) -> Result<Option<PathBuf>, String> {
    pick.name.as_deref().map(|name| named(name).map(|name| dir(Some(name)))).transpose()
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
    /// The account uses the default account's setup (see `set_shared_setup`). Always false for the default account.
    pub shares_setup: bool,
}

/// The account a pane is asked to run Claude Code under, apart from the one its project has: `name` is a named account,
/// or `None` for the default one. Absent (an `Option<AccountPick>` that is `None`) is the project's own.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountPick {
    pub name: Option<String>,
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
    let dir = dir(name);
    Account { name: name.map(String::from), shares_setup: name.is_some() && shares(&dir), dir: dir.to_string_lossy().into_owned(), email, signed_in }
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

// --- The default account's setup, shared with the named ones -------------------------------------------------------------

/// Where an account keeps what it had of its own while it uses the default account's setup.
const BACKUPS: &str = ".before-shared-setup";
/// Left in an account's folder when you turned sharing off, so it stays off.
const OWN_SETUP: &str = ".wings-own-setup";
/// Folders of the default account an account links to: your rules, skills, agents, commands, hooks, output styles and plugins.
const SHARED_DIRS: [&str; 7] = ["rules", "skills", "agents", "commands", "hooks", "output-styles", "plugins"];

/// Whether `name` is a top-level item of the default folder that accounts share: `settings.json`, `settings.local.json`,
/// the Markdown files (`CLAUDE.md`, `RTK.md`), the status line scripts (`*.sh`) and `SHARED_DIRS`. Never the account's own
/// state: its sign-in, `.claude.json`, projects, sessions, history or caches.
fn shared_name(name: &str) -> bool {
    SHARED_DIRS.contains(&name)
        || matches!(name, "settings.json" | "settings.local.json")
        || (!name.starts_with('.') && (name.ends_with(".md") || name.ends_with(".sh")))
}

/// The items of the default folder to share, by name: the right kind (a folder for `SHARED_DIRS`, a file for the rest).
fn shared_items(default_dir: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(default_dir) else { return Vec::new() };
    let mut names: Vec<String> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let path = entry.path();
            let right_kind = if SHARED_DIRS.contains(&name.as_str()) { path.is_dir() } else { path.is_file() };
            (shared_name(&name) && right_kind).then_some(name)
        })
        .collect();
    names.sort();
    names
}

/// Whether a named account uses the default account's setup: it does, unless you turned it off.
fn shares(account: &Path) -> bool {
    !account.join(OWN_SETUP).exists()
}

fn taken(path: &Path) -> bool {
    path.symlink_metadata().is_ok()
}

fn same_folder(a: &Path, b: &Path) -> bool {
    matches!((a.canonicalize(), b.canonicalize()), (Ok(a), Ok(b)) if a == b)
}

#[cfg(unix)]
fn link(target: &Path, at: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, at)
}

#[cfg(not(unix))]
fn link(_target: &Path, _at: &Path) -> std::io::Result<()> {
    Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "Sharing the default account's setup isn't available on Windows yet"))
}

/// Makes `account/name` a link to `default_dir/name`. What is there already moves into the account's backup folder first
/// (`BACKUPS`), under a numbered name when that one is taken: nothing is ever deleted. A link that is already right stays.
fn link_item(default_dir: &Path, account: &Path, name: &str) -> std::io::Result<()> {
    let (target, at) = (default_dir.join(name), account.join(name));
    match at.symlink_metadata() {
        Ok(meta) => {
            if meta.is_symlink() && std::fs::read_link(&at).is_ok_and(|now| now == target) {
                return Ok(());
            }
            let backups = account.join(BACKUPS);
            make_private(&backups)?;
            let kept = std::iter::once(backups.join(name))
                .chain((2..).map(|n| backups.join(format!("{name}.{n}"))))
                .find(|path| !taken(path))
                .expect("an unbounded range always finds a free name");
            std::fs::rename(&at, kept)?;
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    link(&target, &at)
}

/// A backed up item's name in the account, and which of its backups it is (1 is the first). `settings.json.2` is the second
/// backup of `settings.json`. `None` for a name Wings doesn't give a backup.
fn backup_of(entry: &str) -> Option<(&str, u32)> {
    if shared_name(entry) {
        return Some((entry, 1));
    }
    let (name, n) = entry.rsplit_once('.')?;
    (n.bytes().all(|b| b.is_ascii_digit()) && shared_name(name)).then(|| n.parse().ok().map(|n| (name, n)))?
}

/// Takes the links `link_item` made out of the account and moves the backups back where they were. A backup whose place is
/// taken (you made a new file there since) stays in the backup folder next to the rest. Links that aren't Wings' stay:
/// only one to `default_dir/<its own name>` goes.
fn unshare_from(default_dir: &Path, account: &Path) -> Result<(), String> {
    let mut errors = Vec::new();
    // Listed first: a directory being read while its entries go may skip some.
    let entries: Vec<_> = std::fs::read_dir(account).map_err(|e| e.to_string())?.flatten().collect();
    for entry in entries {
        let Ok(name) = entry.file_name().into_string() else { continue };
        let ours = shared_name(&name) && std::fs::read_link(entry.path()).is_ok_and(|target| target == default_dir.join(&name));
        if ours {
            if let Err(e) = std::fs::remove_file(entry.path()) {
                errors.push(format!("{name}: {e}"));
            }
        }
    }
    let backups = account.join(BACKUPS);
    let mut kept: Vec<(u32, String, String)> = std::fs::read_dir(&backups)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let entry = entry.file_name().into_string().ok()?;
            let (name, n) = backup_of(&entry)?;
            Some((n, name.to_string(), entry))
        })
        .collect();
    kept.sort();
    for (_, name, entry) in kept {
        let at = account.join(&name);
        if taken(&at) {
            continue;
        }
        if let Err(e) = std::fs::rename(backups.join(&entry), at) {
            errors.push(format!("{name}: {e}"));
        }
    }
    // Only goes when empty.
    let _ = std::fs::remove_dir(&backups);
    if errors.is_empty() { Ok(()) } else { Err(errors.join("; ")) }
}

/// Writes a Claude Code config file whole or not at all, with the permissions the file has, else for you only.
fn write_config(path: &Path, config: &Value) -> std::io::Result<()> {
    use std::io::Write;
    let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let mut tmp = path.clone().into_os_string();
    tmp.push(".wings-tmp");
    let tmp = PathBuf::from(tmp);
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let written = (|| {
        let mut file = options.open(&tmp)?;
        file.write_all(serde_json::to_string_pretty(config)?.as_bytes())?;
        if let Ok(meta) = std::fs::metadata(&path) {
            file.set_permissions(meta.permissions())?;
        }
        file.sync_all()?;
        std::fs::rename(&tmp, &path)
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    written
}

/// Copies each user-scope MCP server in the default account's config the account doesn't have yet into the account's, and
/// keeps every other key of it. `wings` stays out, which Wings registers itself. Those in `.claude.json` can't be linked,
/// since that file also holds the signed-in account. A server that is in both is the account's own, and stays as it is.
fn copy_mcp_servers(default_config: &Path, account_config: &Path) -> Result<(), String> {
    let read = |path: &Path| -> Result<Option<Value>, String> {
        match std::fs::read_to_string(path) {
            Ok(text) => serde_json::from_str(&text).map(Some).map_err(|_| format!("{} isn't valid JSON, so Wings left it alone", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(format!("{}: {e}", path.display())),
        }
    };
    let Some(default) = read(default_config).ok().flatten() else { return Ok(()) };
    let Some(servers) = default.get("mcpServers").and_then(Value::as_object) else { return Ok(()) };
    let mut account = read(account_config)?.unwrap_or_else(|| Value::Object(Default::default()));
    let Some(config) = account.as_object_mut() else { return Err(format!("{} isn't a JSON object, so Wings left it alone", account_config.display())) };
    let have = config.get("mcpServers").and_then(Value::as_object);
    let missing: Vec<(&String, &Value)> = servers.iter().filter(|(name, _)| *name != "wings" && !have.is_some_and(|have| have.contains_key(*name))).collect();
    if missing.is_empty() {
        return Ok(());
    }
    let list = config.entry("mcpServers").or_insert_with(|| Value::Object(Default::default()));
    if list.is_null() {
        *list = Value::Object(Default::default());
    }
    let Some(list) = list.as_object_mut() else { return Err(format!("mcpServers in {} isn't an object, so Wings left it alone", account_config.display())) };
    for (name, server) in missing {
        list.insert(name.clone(), server.clone());
    }
    write_config(account_config, &account).map_err(|e| format!("{}: {e}", account_config.display()))
}

/// Links the default account's shared items into an account and copies its MCP servers. Takes the default account's folder
/// and `.claude.json`, and the account's folder, so the tests can use temporary ones.
fn share_into(default_dir: &Path, default_config: &Path, account: &Path) -> Result<(), String> {
    if same_folder(default_dir, account) {
        return Err(format!("{} is the folder Wings runs Claude Code under, so there is nothing to share with it", account.display()));
    }
    let mut errors = Vec::new();
    for name in shared_items(default_dir) {
        if let Err(e) = link_item(default_dir, account, &name) {
            errors.push(format!("{name}: {e}"));
        }
    }
    if let Err(e) = copy_mcp_servers(default_config, &account.join(".claude.json")) {
        errors.push(e);
    }
    if errors.is_empty() { Ok(()) } else { Err(errors.join("; ")) }
}

fn set_shared_setup_in(default_dir: &Path, default_config: &Path, account: &Path, on: bool) -> Result<(), String> {
    make_private(account).map_err(|e| e.to_string())?;
    if on {
        match std::fs::remove_file(account.join(OWN_SETUP)) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.to_string()),
            _ => {}
        }
        share_into(default_dir, default_config, account)
    } else {
        unshare_from(default_dir, account)?;
        std::fs::write(account.join(OWN_SETUP), "").map_err(|e| e.to_string())
    }
}

fn named(name: &str) -> Result<&str, String> {
    if valid_name(name) {
        Ok(name)
    } else {
        Err(format!("{name:?} isn't an account name: up to 32 lowercase letters, digits and dashes, starting with a letter or digit"))
    }
}

/// Makes a named account use the default account's setup, or gives it its own again. This is on for every account until
/// you turn it off, and Wings applies it each time it connects to the account (`sync_shared_setup`).
///
/// On: every shared item the default folder has (`shared_name`) becomes a link to it in the account's folder, and the
/// default account's MCP servers the account lacks are copied into its `.claude.json`. What the account had at a name moves
/// to `.before-shared-setup/` first, and nothing is deleted. Off: the links go and the backups move back. The MCP servers
/// stay where they were copied: they sit in a file with the account's sign-in, and may be ones you added there yourself since.
pub fn set_shared_setup(name: &str, on: bool) -> Result<(), String> {
    set_shared_setup_in(&dir(None), &user_config(None), &dir(Some(named(name)?)), on)
}

fn sync_in(default_dir: &Path, default_config: &Path, account: &Path) -> Result<(), String> {
    if !account.is_dir() || !shares(account) {
        return Ok(());
    }
    share_into(default_dir, default_config, account)
}

/// Applies the sharing of a named account that has it on, and does nothing for one you turned it off for or whose folder
/// isn't there. Safe to repeat: it links what is new in the default folder, and copies the servers added since.
pub fn sync_shared_setup(name: &str) -> Result<(), String> {
    sync_in(&dir(None), &user_config(None), &dir(Some(named(name)?)))
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

    #[test]
    fn a_pane_asked_for_an_account_gets_that_one_or_the_apps_own() {
        assert_eq!(pick_dir(&AccountPick { name: Some("work".into()) }), Ok(Some(home().join(".claude-work"))));
        assert_eq!(pick_dir(&AccountPick { name: None }), Ok(None), "None leaves the app's own CLAUDE_CONFIG_DIR, the default account");
        for bad in ["", "../etc", "Work", "a/b", "~", "."] {
            assert!(pick_dir(&AccountPick { name: Some(bad.into()) }).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn a_bad_account_name_never_reaches_the_file_system() {
        for bad in ["", "-work", "Work", "a_b", ".", "..", "../x", "a/b", "/etc", "~", "work\n", &"a".repeat(33)] {
            assert!(set_shared_setup(bad, true).is_err(), "{bad:?}");
            assert!(set_shared_setup(bad, false).is_err(), "{bad:?}");
            assert!(sync_shared_setup(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn only_the_setup_you_made_is_shared_and_never_the_accounts_own_state() {
        for yes in ["settings.json", "settings.local.json", "CLAUDE.md", "RTK.md", "statusline.sh", "rules", "skills", "agents", "commands", "hooks", "output-styles", "plugins"] {
            assert!(shared_name(yes), "{yes:?}");
        }
        for no in [".claude.json", ".credentials.json", "credentials.json", "projects", "sessions", "history.jsonl", "todos", "shell-snapshots", "statsig", "ide", "cache", "backups", "notes.txt", ".hidden.md", ".md", "md", "x.shx", ".before-shared-setup", ".wings-own-setup"] {
            assert!(!shared_name(no), "{no:?}");
        }
    }

    #[test]
    fn a_backups_name_says_what_it_was_and_which_one_it_is() {
        assert_eq!(backup_of("settings.json"), Some(("settings.json", 1)));
        assert_eq!(backup_of("settings.json.2"), Some(("settings.json", 2)));
        assert_eq!(backup_of("RTK.md.13"), Some(("RTK.md", 13)));
        assert_eq!(backup_of("rules.2"), Some(("rules", 2)));
        for other in ["notes.txt", "x.2", "settings.json.", "settings.json.+2", "settings.json.x", "projects.2", ".2"] {
            assert_eq!(backup_of(other), None, "{other:?}");
        }
    }

    #[cfg(unix)]
    mod sharing {
        use super::*;
        use std::fs;

        /// A default account's folder and `.claude.json`, and the folder of an account that has setup of its own.
        struct Folders {
            root: PathBuf,
            default: PathBuf,
            config: PathBuf,
            account: PathBuf,
        }

        impl Drop for Folders {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.root);
            }
        }

        fn put(path: &Path, text: &str) {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }

        fn folders(name: &str) -> Folders {
            let root = std::env::temp_dir().join(format!("wings-share-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&root);
            let (default, account) = (root.join("default"), root.join(".claude-work"));
            for (file, text) in [
                ("settings.json", r#"{"theme":"dark"}"#),
                ("settings.local.json", "{}"),
                ("CLAUDE.md", "# mine"),
                ("RTK.md", "# rtk"),
                ("statusline.sh", "#!/bin/sh"),
                ("notes.txt", "not setup"),
                (".hidden.md", "hidden"),
                ("rules/a.md", "rule a"),
                ("skills/s/SKILL.md", "skill"),
                ("commands/c.md", "command"),
                ("hooks/h.sh", "hook"),
                ("plugins/installed_plugins.json", "[]"),
                (".credentials.json", "default secret"),
                ("history.jsonl", "default history"),
                ("projects/p/x.jsonl", "default session"),
                ("cache/c", "cache"),
            ] {
                put(&default.join(file), text);
            }
            for (file, text) in [
                ("settings.json", "own settings"),
                ("rules/own.md", "own rule"),
                (".credentials.json", "own secret"),
                ("history.jsonl", "own history"),
                ("projects/mine/y.jsonl", "own session"),
                (".claude.json", r#"{"oauthAccount":{"emailAddress":"work@example.com"}}"#),
            ] {
                put(&account.join(file), text);
            }
            Folders { config: root.join("default.claude.json"), root, default, account }
        }

        /// Every path under a folder: a folder, a link and where it points, or a file and its bytes. Nothing is followed.
        fn tree(root: &Path) -> BTreeMap<String, String> {
            fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<String, String>) {
                for entry in fs::read_dir(dir).unwrap().flatten() {
                    let path = entry.path();
                    let rel = path.strip_prefix(root).unwrap().to_string_lossy().into_owned();
                    let meta = fs::symlink_metadata(&path).unwrap();
                    if meta.is_symlink() {
                        out.insert(rel, format!("link:{}", fs::read_link(&path).unwrap().display()));
                    } else if meta.is_dir() {
                        out.insert(rel, "dir".into());
                        walk(root, &path, out);
                    } else {
                        out.insert(rel, format!("file:{}", fs::read_to_string(&path).unwrap()));
                    }
                }
            }
            let mut out = BTreeMap::new();
            walk(root, root, &mut out);
            out
        }

        fn is_link_to(account: &Path, default: &Path, name: &str) -> bool {
            fs::read_link(account.join(name)).is_ok_and(|target| target == default.join(name))
        }

        #[test]
        fn turning_it_on_links_the_default_setup_and_moves_what_the_account_had_aside() {
            let f = folders("on");
            let default_before = tree(&f.default);
            share_into(&f.default, &f.config, &f.account).unwrap();
            for item in ["settings.json", "settings.local.json", "CLAUDE.md", "RTK.md", "statusline.sh", "rules", "skills", "commands", "hooks", "plugins"] {
                assert!(is_link_to(&f.account, &f.default, item), "{item}");
            }
            for absent in ["agents", "output-styles"] {
                assert!(!taken(&f.account.join(absent)), "{absent} isn't in the default folder, so it isn't made up");
            }
            // Not setup: the account's own state stays as it was, and is never a link.
            for own in ["notes.txt", ".hidden.md", "cache", "sessions", "backups"] {
                assert!(!taken(&f.account.join(own)), "{own}");
            }
            assert_eq!(fs::read_to_string(f.account.join(".credentials.json")).unwrap(), "own secret");
            assert_eq!(fs::read_to_string(f.account.join("history.jsonl")).unwrap(), "own history");
            assert_eq!(fs::read_to_string(f.account.join("projects/mine/y.jsonl")).unwrap(), "own session");
            assert!(!f.account.join("projects").is_symlink() && !f.account.join(".claude.json").is_symlink());
            // What the account had at those names is kept, byte for byte.
            let backups = f.account.join(BACKUPS);
            assert_eq!(fs::read_to_string(backups.join("settings.json")).unwrap(), "own settings");
            assert_eq!(fs::read_to_string(backups.join("rules/own.md")).unwrap(), "own rule");
            assert_eq!(fs::read_dir(&backups).unwrap().count(), 2);
            // The account reads the default's file, and a write through the link (as Claude Code's and Wings' own are) lands there.
            assert_eq!(fs::read_to_string(f.account.join("settings.json")).unwrap(), r#"{"theme":"dark"}"#);
            fs::write(f.account.join("settings.json"), r#"{"theme":"light"}"#).unwrap();
            assert!(f.account.join("settings.json").is_symlink());
            assert_eq!(fs::read_to_string(f.default.join("settings.json")).unwrap(), r#"{"theme":"light"}"#);
            // The default folder itself is only written through the links, never reshaped.
            let mut expected = default_before;
            expected.insert("settings.json".into(), r#"file:{"theme":"light"}"#.into());
            assert_eq!(tree(&f.default), expected);
        }

        #[test]
        fn turning_it_off_gives_the_account_back_exactly_what_it_had() {
            let f = folders("off");
            // A link of your own to somewhere else: where the default has the item, it is set aside and comes back; where it has
            // none, it is left alone throughout.
            let elsewhere = f.root.join("elsewhere");
            put(&elsewhere.join("x"), "x");
            std::os::unix::fs::symlink(&elsewhere, f.account.join("hooks")).unwrap();
            std::os::unix::fs::symlink(&elsewhere, f.account.join("agents")).unwrap();
            let (account_before, default_before) = (tree(&f.account), tree(&f.default));
            set_shared_setup_in(&f.default, &f.config, &f.account, true).unwrap();
            assert!(is_link_to(&f.account, &f.default, "hooks"));
            assert_eq!(fs::read_link(f.account.join("agents")).unwrap(), elsewhere);
            set_shared_setup_in(&f.default, &f.config, &f.account, false).unwrap();
            let mut expected = account_before;
            expected.insert(OWN_SETUP.into(), "file:".into());
            // The MCP servers (none here) and `.claude.json` are the one thing sharing may add to; the account had none to add.
            assert_eq!(tree(&f.account), expected);
            assert_eq!(tree(&f.default), default_before);
            assert!(!f.account.join(BACKUPS).exists(), "an empty backup folder is not left behind");
            assert!(!shares(&f.account));
            // Again changes nothing.
            set_shared_setup_in(&f.default, &f.config, &f.account, false).unwrap();
            assert_eq!(tree(&f.account), expected);
        }

        #[test]
        fn a_right_link_stays_and_what_the_default_gains_is_linked_next_time() {
            let f = folders("again");
            share_into(&f.default, &f.config, &f.account).unwrap();
            let once = tree(&f.account);
            share_into(&f.default, &f.config, &f.account).unwrap();
            assert_eq!(tree(&f.account), once, "nothing moved to the backups again");
            put(&f.default.join("agents/a.md"), "agent");
            share_into(&f.default, &f.config, &f.account).unwrap();
            assert!(is_link_to(&f.account, &f.default, "agents"));
            assert_eq!(fs::read_dir(f.account.join(BACKUPS)).unwrap().count(), 2);
        }

        #[test]
        fn a_second_backup_of_a_name_gets_a_number_and_a_place_in_use_is_not_overwritten() {
            let f = folders("numbered");
            share_into(&f.default, &f.config, &f.account).unwrap();
            // Something replaced the link with a file of its own, as a tool that writes by rename would.
            fs::remove_file(f.account.join("settings.json")).unwrap();
            put(&f.account.join("settings.json"), "replacement");
            share_into(&f.default, &f.config, &f.account).unwrap();
            let backups = f.account.join(BACKUPS);
            assert_eq!(fs::read_to_string(backups.join("settings.json")).unwrap(), "own settings");
            assert_eq!(fs::read_to_string(backups.join("settings.json.2")).unwrap(), "replacement");
            assert!(is_link_to(&f.account, &f.default, "settings.json"));
            // Off: the first comes back; the second has nowhere to go but stays, in the backup folder.
            unshare_from(&f.default, &f.account).unwrap();
            assert_eq!(fs::read_to_string(f.account.join("settings.json")).unwrap(), "own settings");
            assert_eq!(fs::read_to_string(backups.join("settings.json.2")).unwrap(), "replacement");
            // A place the account made use of meanwhile is never overwritten by a backup.
            share_into(&f.default, &f.config, &f.account).unwrap();
            fs::remove_file(f.account.join("rules")).unwrap();
            put(&f.account.join("rules/new.md"), "made after");
            unshare_from(&f.default, &f.account).unwrap();
            assert_eq!(fs::read_to_string(f.account.join("rules/new.md")).unwrap(), "made after");
            assert!(!f.account.join("rules/own.md").exists());
        }

        #[test]
        fn mcp_servers_are_copied_into_the_account_and_everything_else_in_its_config_is_kept() {
            let f = folders("mcp");
            put(
                &f.config,
                r#"{"numStartups":3,"oauthAccount":{"emailAddress":"me@example.com"},"mcpServers":{"a":{"command":"a"},"shared":{"command":"default"},"wings":{"command":"w"}}}"#,
            );
            let config = f.account.join(".claude.json");
            put(&config, r#"{"oauthAccount":{"emailAddress":"work@example.com"},"projects":{"/p":{"allowedTools":[]}},"mcpServers":{"shared":{"command":"mine"}}}"#);
            copy_mcp_servers(&f.config, &config).unwrap();
            let after: Value = serde_json::from_str(&fs::read_to_string(&config).unwrap()).unwrap();
            assert_eq!(after["oauthAccount"]["emailAddress"], "work@example.com");
            assert_eq!(after["projects"]["/p"]["allowedTools"], serde_json::json!([]));
            assert_eq!(after["mcpServers"]["a"]["command"], "a");
            assert_eq!(after["mcpServers"]["shared"]["command"], "mine", "the account's own entry wins");
            assert!(after["mcpServers"].get("wings").is_none(), "Wings registers itself");
            assert!(after.get("numStartups").is_none());
            assert_eq!(after.as_object().unwrap().keys().collect::<Vec<_>>(), ["oauthAccount", "projects", "mcpServers"], "keys keep their order");
            // Nothing new: the file is not rewritten.
            let text = fs::read_to_string(&config).unwrap();
            copy_mcp_servers(&f.config, &config).unwrap();
            assert_eq!(fs::read_to_string(&config).unwrap(), text);
        }

        #[test]
        fn a_missing_account_config_is_made_and_one_that_is_not_json_is_left_alone() {
            let f = folders("mcp-files");
            put(&f.config, r#"{"mcpServers":{"a":{"command":"a"},"wings":{"command":"w"}}}"#);
            let config = f.root.join("new").join(".claude.json");
            fs::create_dir_all(config.parent().unwrap()).unwrap();
            copy_mcp_servers(&f.config, &config).unwrap();
            let made: Value = serde_json::from_str(&fs::read_to_string(&config).unwrap()).unwrap();
            assert_eq!(made, serde_json::json!({ "mcpServers": { "a": { "command": "a" } } }));
            let mode = std::os::unix::fs::PermissionsExt::mode(&fs::metadata(&config).unwrap().permissions()) & 0o777;
            assert_eq!(mode, 0o600, "it holds a sign-in");
            assert!(!config.with_file_name(".claude.json.wings-tmp").exists());
            // Not JSON, or no object: Wings does not overwrite what it can't read.
            for text in ["not json", "[1]"] {
                let broken = f.account.join(".claude.json");
                put(&broken, text);
                assert!(copy_mcp_servers(&f.config, &broken).is_err(), "{text}");
                assert_eq!(fs::read_to_string(&broken).unwrap(), text);
            }
            // No default config, or none that names servers: nothing to copy, and no file made.
            let none = f.root.join("none").join(".claude.json");
            copy_mcp_servers(&f.root.join("missing.json"), &none).unwrap();
            put(&f.config, r#"{"mcpServers":{"wings":{}}}"#);
            copy_mcp_servers(&f.config, &none).unwrap();
            assert!(!none.exists());
        }

        #[test]
        fn an_account_shares_until_it_is_turned_off_and_stays_off() {
            let f = folders("default-on");
            // No marker: on. Connecting to the account applies it, so an account that has existed all along gets it too.
            assert!(shares(&f.account));
            sync_in(&f.default, &f.config, &f.account).unwrap();
            assert!(is_link_to(&f.account, &f.default, "settings.json"));
            // Off, and Wings connecting again does not link it anew.
            set_shared_setup_in(&f.default, &f.config, &f.account, false).unwrap();
            assert!(!shares(&f.account));
            sync_in(&f.default, &f.config, &f.account).unwrap();
            assert_eq!(fs::read_to_string(f.account.join("settings.json")).unwrap(), "own settings");
            // On again: the marker goes and the links come back.
            set_shared_setup_in(&f.default, &f.config, &f.account, true).unwrap();
            assert!(shares(&f.account) && !f.account.join(OWN_SETUP).exists());
            assert!(is_link_to(&f.account, &f.default, "settings.json"));
            // A folder that isn't there is not made by a sync.
            let nowhere = f.root.join(".claude-nowhere");
            sync_in(&f.default, &f.config, &nowhere).unwrap();
            assert!(!nowhere.exists());
            // A new account folder, as Add account makes it, starts out with the default setup.
            let fresh = f.root.join(".claude-fresh");
            make_private(&fresh).unwrap();
            sync_in(&f.default, &f.config, &fresh).unwrap();
            assert!(is_link_to(&fresh, &f.default, "CLAUDE.md"));
            assert!(!fresh.join(BACKUPS).exists(), "a new folder has nothing to set aside");
        }

        #[test]
        fn the_folder_wings_runs_under_has_nothing_to_share_with_itself() {
            let f = folders("same");
            let before = tree(&f.default);
            assert!(share_into(&f.default, &f.config, &f.default).is_err());
            let alias = f.root.join(".claude-alias");
            std::os::unix::fs::symlink(&f.default, &alias).unwrap();
            assert!(share_into(&f.default, &f.config, &alias).is_err());
            assert_eq!(tree(&f.default), before);
        }
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
