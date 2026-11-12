//! Claude Code accounts. Each has a config folder of its own, picked with `CLAUDE_CONFIG_DIR`: settings, `.claude.json`
//! (user-scope MCP servers, the signed-in account), transcripts, and the sign-in itself. The default account is the
//! folder Wings runs under (`claude_dir`); a named one is `~/.claude-<name>`. The terminals of a project use the
//! account assigned to it (`Space::claude_account`). A named account uses the default account's setup (settings, rules,
//! skills, plugins) unless you turn that off, linked to it, except `settings.json`, which is a copy that keeps the account's
//! own `model`; see `set_shared_setup`. The desktop and the native app share this.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::spaces::Space;

const PREFIX: &str = ".claude-";

/// `^[a-z0-9][a-z0-9-]{0,31}$`. The name becomes part of a folder name, so nothing else gets through.
pub fn valid_name(name: &str) -> bool {
    let plain = |b: u8| b.is_ascii_lowercase() || b.is_ascii_digit();
    (1..=32).contains(&name.len()) && name.bytes().next().is_some_and(plain) && name.bytes().all(|b| plain(b) || b == b'-')
}

fn home() -> PathBuf {
    std::env::home_dir().unwrap_or_default()
}

/// The default account's folder: where `CLAUDE_CONFIG_DIR` points in Wings' own environment, else `~/.claude`.
pub fn claude_dir() -> PathBuf {
    std::env::var_os("CLAUDE_CONFIG_DIR").map(PathBuf::from).unwrap_or_else(|| home().join(".claude"))
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
    dirs_with_default(dir(None))
}

/// `dirs` for an app that is told the default account's folder, `default`, instead of finding it.
pub fn dirs_with_default(default: PathBuf) -> Vec<PathBuf> {
    let mut dirs = vec![default];
    for name in existing_names() {
        let account = dir(Some(&name));
        if !dirs.contains(&account) {
            dirs.push(account);
        }
    }
    dirs
}

/// What a session that ran under the account in folder `dir` shows of it: a named account's name, and nothing for the
/// default account's folder `default` or a folder that isn't an account's.
pub fn label<'a>(dir: &'a Path, default: &Path) -> Option<&'a str> {
    if dir == default {
        return None;
    }
    dir.file_name()?.to_str()?.strip_prefix(PREFIX).filter(|name| valid_name(name))
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
    connect_targets_with(spaces, BTreeSet::new())
}

/// `connect_targets`, and the accounts `others` too: those you are signed in to that no project uses, which you still
/// start Claude as in a pane. Left out, such an account's sessions told Wings nothing.
pub fn connect_targets_with(spaces: &[Space], others: BTreeSet<String>) -> Vec<Option<String>> {
    let names: BTreeSet<String> = assigned(spaces).map(|(_, name)| name.to_string()).chain(others).collect();
    std::iter::once(None).chain(names.into_iter().map(Some)).collect()
}

/// The named accounts whose config names who signed in, as the accounts list shows them.
pub fn signed_in_names() -> BTreeSet<String> {
    existing_names().into_iter().filter(|name| signed_in_as(&user_config(Some(name))).1).collect()
}

/// The folder for `CLAUDE_CONFIG_DIR` in a pane that was asked for an account of its own: `None` for the default account,
/// which is what the app itself runs under.
pub fn pick_dir(pick: &AccountPick) -> Result<Option<PathBuf>, String> {
    pick.name.as_deref().map(|name| named(name).map(|name| dir(Some(name)))).transpose()
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    /// `None` for the default account.
    pub name: Option<String>,
    pub dir: String,
    /// From the account Claude Code's config names. It stays after the sign-in itself is gone, so it doesn't say the account is signed in.
    pub email: Option<String>,
    /// What `claude auth status` says for the account. False when `claude` isn't installed or its answer isn't understood.
    pub signed_in: bool,
    /// How `claude auth status` says the account signs in (`claude.ai`, `none`, ...), as it prints it. `None` when `claude` wasn't there or its answer wasn't understood.
    pub auth_method: Option<String>,
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

/// What `claude auth status` printed for an account.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct AuthStatus {
    pub logged_in: bool,
    pub method: Option<String>,
}

/// Reads the JSON `claude auth status` prints on stdout, like `{"loggedIn": false, "authMethod": "none", ...}`: it exits 1
/// when logged out and prints it all the same. Output without a boolean `loggedIn` counts as logged out.
pub fn read_auth_status(out: &str) -> AuthStatus {
    let read = || {
        let status: Value = serde_json::from_str(out).ok()?;
        Some(AuthStatus { logged_in: status.get("loggedIn")?.as_bool()?, method: status.get("authMethod").and_then(Value::as_str).map(String::from) })
    };
    read().unwrap_or_default()
}

/// The email of the account a Claude Code config file names, and whether it names one. Both are absent for a file that
/// is missing or isn't JSON.
fn signed_in_as(config: &Path) -> (Option<String>, bool) {
    let config = std::fs::read_to_string(config).ok().and_then(|text| serde_json::from_str::<Value>(&text).ok());
    let oauth = config.as_ref().and_then(|config| config.get("oauthAccount")).filter(|oauth| oauth.is_object());
    (oauth.and_then(|oauth| oauth.get("emailAddress")).and_then(Value::as_str).map(String::from), oauth.is_some())
}

/// An account with the email its config names and `auth`'s answer for whether it is signed in. `auth` runs `claude`, so it takes a while.
fn account(name: Option<&str>, auth: &(impl Fn(Option<&str>) -> AuthStatus + Sync)) -> Account {
    let (email, _) = signed_in_as(&user_config(name));
    let AuthStatus { logged_in, method } = auth(name);
    let dir = dir(name);
    Account {
        name: name.map(String::from),
        shares_setup: name.is_some() && shares(&dir),
        dir: dir.to_string_lossy().into_owned(),
        email,
        signed_in: logged_in,
        auth_method: method,
    }
}

/// `list_with`, where whether an account is signed in is the hint its config gives: an account it names counts, though the
/// sign-in may be gone since, and no `authMethod`.
pub fn list(spaces: &[Space]) -> Accounts {
    list_with(spaces, |name| AuthStatus { logged_in: signed_in_as(&user_config(name)).1, method: None })
}

/// The default account, every `~/.claude-<name>` folder whose config names an account (even if the sign-in is gone since),
/// and the names projects are assigned even if nobody signed in there yet. Other tools keep folders named
/// `~/.claude-<something>` too, so a folder alone isn't an account. `auth` says whether each is signed in; the accounts are
/// asked side by side.
pub fn list_with(spaces: &[Space], auth: impl Fn(Option<&str>) -> AuthStatus + Sync) -> Accounts {
    let projects: BTreeMap<String, String> = assigned(spaces).map(|(id, name)| (id.to_string(), name.to_string())).collect();
    let used: BTreeSet<String> = projects.values().cloned().collect();
    let named: BTreeSet<String> = existing_names().into_iter().filter(|name| signed_in_as(&user_config(Some(name))).1).chain(used).collect();
    let names: Vec<Option<&str>> = std::iter::once(None).chain(named.iter().map(|name| Some(name.as_str()))).collect();
    let auth = &auth;
    let accounts = std::thread::scope(|s| {
        names.iter().map(|name| s.spawn(move || account(*name, auth))).collect::<Vec<_>>().into_iter().map(|t| t.join().unwrap()).collect()
    });
    Accounts { accounts, projects }
}

// --- The default account's setup, shared with the named ones -------------------------------------------------------------

/// Where an account keeps what it had of its own while it uses the default account's setup.
const BACKUPS: &str = ".before-shared-setup";
/// Left in an account's folder when you turned sharing off, so it stays off.
const OWN_SETUP: &str = ".wings-own-setup";
/// The one shared item that is copied, not linked: each account keeps its own `model`. See `sync_settings`.
const SETTINGS: &str = "settings.json";
/// Left in an account's folder while its `settings.json` is a copy Wings made, which tells it from settings of the
/// account's own.
const SYNCED_SETTINGS: &str = ".wings-synced-settings";
/// Folders of the default account an account links to: your rules, skills, agents, commands, hooks, output styles and plugins.
const SHARED_DIRS: [&str; 7] = ["rules", "skills", "agents", "commands", "hooks", "output-styles", "plugins"];

/// Whether `name` is a top-level item of the default folder that accounts share: `settings.json` (copied, the rest are
/// linked), `settings.local.json`, the Markdown files (`CLAUDE.md`, `RTK.md`), the status line scripts (`*.sh`) and
/// `SHARED_DIRS`. Never the account's own state: its sign-in, `.claude.json`, projects, sessions, history or caches.
fn shared_name(name: &str) -> bool {
    SHARED_DIRS.contains(&name)
        || matches!(name, SETTINGS | "settings.local.json")
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

/// Moves `account/name` into the account's backup folder (`BACKUPS`), under a numbered name when that one is taken, and
/// returns where it went: nothing is ever deleted.
fn set_aside(account: &Path, name: &str) -> std::io::Result<PathBuf> {
    let backups = account.join(BACKUPS);
    make_private(&backups)?;
    let kept = std::iter::once(backups.join(name))
        .chain((2..).map(|n| backups.join(format!("{name}.{n}"))))
        .find(|path| !taken(path))
        .expect("an unbounded range always finds a free name");
    std::fs::rename(account.join(name), &kept)?;
    Ok(kept)
}

/// Makes `account/name` a link to `default_dir/name`. What is there already moves into the account's backup folder first
/// (`set_aside`). A link that is already right stays.
fn link_item(default_dir: &Path, account: &Path, name: &str) -> std::io::Result<()> {
    let (target, at) = (default_dir.join(name), account.join(name));
    match at.symlink_metadata() {
        Ok(meta) => {
            if meta.is_symlink() && std::fs::read_link(&at).is_ok_and(|now| now == target) {
                return Ok(());
            }
            set_aside(account, name)?;
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

/// Takes what Wings made out of the account, the links `link_item` made and the `settings.json` copy `sync_settings` made,
/// and moves the backups back where they were. A backup whose place is taken (you made a new file there since) stays in
/// the backup folder next to the rest. Links that aren't Wings' stay: only one to `default_dir/<its own name>` goes. A
/// `settings.json` that is a file goes only while the marker says Wings made it; without the marker it is yours.
fn unshare_from(default_dir: &Path, account: &Path) -> Result<(), String> {
    let mut errors = Vec::new();
    let synced = account.join(SYNCED_SETTINGS);
    // Listed first: a directory being read while its entries go may skip some.
    let entries: Vec<_> = std::fs::read_dir(account).map_err(|e| e.to_string())?.flatten().collect();
    for entry in entries {
        let Ok(name) = entry.file_name().into_string() else { continue };
        let link = shared_name(&name) && std::fs::read_link(entry.path()).is_ok_and(|target| target == default_dir.join(&name));
        let copy = name == SETTINGS && synced.exists() && entry.path().symlink_metadata().is_ok_and(|meta| meta.is_file());
        if link || copy {
            if let Err(e) = std::fs::remove_file(entry.path()) {
                errors.push(format!("{name}: {e}"));
            }
        }
    }
    match std::fs::remove_file(&synced) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => errors.push(format!("{SYNCED_SETTINGS}: {e}")),
        _ => {}
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
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

/// Writes a Claude Code config file whole or not at all, with the permissions the file has, else for you only.
fn write_config(path: &Path, config: &Value) -> std::io::Result<()> {
    write_whole(path, serde_json::to_string_pretty(config)?.as_bytes(), path)
}

/// Writes `bytes` to `path` whole or not at all (a temporary file, then a rename), with the permissions `like` has, else
/// those `path` has, else for you only.
fn write_whole(path: &Path, bytes: &[u8], like: &Path) -> std::io::Result<()> {
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
        file.write_all(bytes)?;
        if let Ok(meta) = std::fs::metadata(like).or_else(|_| std::fs::metadata(&path)) {
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

/// A settings file as an object: `None` when it isn't there, an error when it isn't JSON or isn't an object, which Wings
/// leaves alone.
fn read_settings(path: &Path) -> Result<Option<Map<String, Value>>, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => match serde_json::from_str(&text) {
            Ok(Value::Object(settings)) => Ok(Some(settings)),
            _ => Err(format!("{} isn't valid JSON, so Wings left it alone", path.display())),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

/// Only one `sync_settings` at a time: the app restoring terminals and Wings connecting to the account on start both run it.
static SETTINGS_SYNC: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Writes the account's `settings.json` as a copy of the default account's with the account's own `model`, so the account
/// keeps its model and shares everything else. A link would give it the default account's model too, which an account on
/// another plan may not have.
///
/// Every key, and the order of them, comes from the default's file, Wings' status line and hooks among them. `model` is the
/// account's: while the file is Wings' copy (`SYNCED_SETTINGS` is there), the one it has now, so the `/model` you ran in the
/// account stays; the first time, the one in what the account had (set aside in `BACKUPS`), and none if that had none, even
/// when the default has one. The first time also replaces the link older Wings made. Nothing is written when the file
/// already says what it should, and the default's file is only read.
fn sync_settings(default_dir: &Path, account: &Path) -> Result<(), String> {
    let _one_at_a_time = SETTINGS_SYNC.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let (source, at, marker) = (default_dir.join(SETTINGS), account.join(SETTINGS), account.join(SYNCED_SETTINGS));
    let Some(default) = read_settings(&source)? else { return Ok(()) };
    let io = |e: std::io::Error| format!("{SETTINGS}: {e}");
    let meta = at.symlink_metadata();
    let (current, own_model) = if marker.exists() && meta.as_ref().is_ok_and(|meta| meta.is_file()) {
        let current = read_settings(&at)?.unwrap_or_default();
        let model = current.get("model").cloned();
        (Some(current), model)
    } else {
        let was = match meta {
            // What `link_item` made in 0.1.121: the default's own file, not the account's, so there is nothing to keep.
            Ok(meta) if meta.is_symlink() && std::fs::read_link(&at).is_ok_and(|now| now == source) => {
                std::fs::remove_file(&at).map_err(io)?;
                None
            }
            Ok(_) => Some(set_aside(account, SETTINGS).map_err(io)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(io(e)),
        };
        let was = was.unwrap_or_else(|| account.join(BACKUPS).join(SETTINGS));
        // What the account had may not be JSON: then it has no model to keep, and is still kept aside as it was.
        (None, read_settings(&was).ok().flatten().and_then(|had| had.get("model").cloned()))
    };
    let mut next = Map::new();
    let mut model = own_model;
    for (key, value) in &default {
        if key == "model" {
            // In the place it has in the default's file, with the account's value, or not at all.
            if let Some(own) = model.take() {
                next.insert(key.clone(), own);
            }
        } else {
            next.insert(key.clone(), value.clone());
        }
    }
    // The default has none, and the account does: it goes last.
    if let Some(own) = model {
        next.insert("model".into(), own);
    }
    if current.as_ref() == Some(&next) {
        return Ok(());
    }
    let mut text = serde_json::to_string_pretty(&next).map_err(|e| e.to_string())?;
    text.push('\n');
    // The file first: with the marker but no file, the next sync would take the account's own settings for Wings' copy.
    write_whole(&at, text.as_bytes(), &source).map_err(io)?;
    std::fs::write(&marker, "").map_err(|e| format!("{SYNCED_SETTINGS}: {e}"))
}

/// Links the default account's shared items into an account, writes its `settings.json` (`sync_settings`) and copies its MCP
/// servers. Takes the default account's folder and `.claude.json`, and the account's folder, so the tests can use temporary
/// ones.
fn share_into(default_dir: &Path, default_config: &Path, account: &Path) -> Result<(), String> {
    if same_folder(default_dir, account) {
        return Err(format!("{} is the folder Wings runs Claude Code under, so there is nothing to share with it", account.display()));
    }
    let mut errors = Vec::new();
    for name in shared_items(default_dir) {
        if name == SETTINGS {
            continue;
        }
        if let Err(e) = link_item(default_dir, account, &name) {
            errors.push(format!("{name}: {e}"));
        }
    }
    if let Err(e) = sync_settings(default_dir, account) {
        errors.push(e);
    }
    if let Err(e) = copy_mcp_servers(default_config, &account.join(".claude.json")) {
        errors.push(e);
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
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
/// On: every shared item the default folder has (`shared_name`) becomes a link to it in the account's folder, except
/// `settings.json`, which is a copy with the account's own `model` (`sync_settings`), and the default account's MCP servers
/// the account lacks are copied into its `.claude.json`. What the account had at a name moves to `.before-shared-setup/`
/// first, and nothing is deleted. Off: the links and the settings copy go and the backups move back. The MCP servers stay
/// where they were copied: they sit in a file with the account's sign-in, and may be ones you added there yourself since.
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
/// isn't there. Safe to repeat: it links what is new in the default folder, brings the settings copy up to date, and copies
/// the servers added since.
pub fn sync_shared_setup(name: &str) -> Result<(), String> {
    sync_in(&dir(None), &user_config(None), &dir(Some(named(name)?)))
}

fn sync_settings_in(default_dir: &Path, account: &Path) -> Result<(), String> {
    if !account.is_dir() || !shares(account) {
        return Ok(());
    }
    if same_folder(default_dir, account) {
        return Err(format!("{} is the folder Wings runs Claude Code under, so there is nothing to share with it", account.display()));
    }
    sync_settings(default_dir, account)
}

/// Only the settings of `sync_shared_setup`, for a terminal about to open in the account's folder `account` (as `pane_dir`
/// and `pick_dir` give it): what you changed in the default account's `settings.json` reaches the account the next time a
/// terminal opens for it. Cheap, since it reads two small files and writes only when they differ.
pub fn sync_shared_settings(account: &Path) -> Result<(), String> {
    sync_shared_settings_with(&dir(None), account)
}

/// `sync_shared_settings` for an app that is told the default account's folder, `default_dir`, instead of finding it.
pub fn sync_shared_settings_with(default_dir: &Path, account: &Path) -> Result<(), String> {
    sync_settings_in(default_dir, account)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn space(id: &str, account: Option<&str>) -> Space {
        Space {
            id: id.into(),
            name: id.into(),
            path: id.into(),
            parent: None,
            added_at: None,
            pinned: false,
            worktree_include: None,
            worktree_setup: None,
            claude_account: account.map(String::from),
        }
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
    fn a_session_shows_its_named_account_and_nothing_for_the_default_one() {
        let default = Path::new("/u/.claude");
        assert_eq!(label(Path::new("/u/.claude-work"), default), Some("work"));
        assert_eq!(label(default, default), None);
        // Wings may run under a named account's folder, which is then the default one.
        assert_eq!(label(Path::new("/u/.claude-work"), Path::new("/u/.claude-work/")), None);
        for other in ["/u/claude", "/u/.claude-", "/u/.claude-Work", "/"] {
            assert_eq!(label(Path::new(other), default), None, "{other:?}");
        }
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
        // An account no project uses, and one a project uses too.
        let others = BTreeSet::from(["personal".to_string(), "work".to_string()]);
        assert_eq!(connect_targets_with(&spaces, others), [None, Some("home".to_string()), Some("personal".to_string()), Some("work".to_string())]);
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
        for yes in [
            "settings.json",
            "settings.local.json",
            "CLAUDE.md",
            "RTK.md",
            "statusline.sh",
            "rules",
            "skills",
            "agents",
            "commands",
            "hooks",
            "output-styles",
            "plugins",
        ] {
            assert!(shared_name(yes), "{yes:?}");
        }
        for no in [
            ".claude.json",
            ".credentials.json",
            "credentials.json",
            "projects",
            "sessions",
            "history.jsonl",
            "todos",
            "shell-snapshots",
            "statsig",
            "ide",
            "cache",
            "backups",
            "notes.txt",
            ".hidden.md",
            ".md",
            "md",
            "x.shx",
            ".before-shared-setup",
            ".wings-own-setup",
        ] {
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

        /// The default account's `settings.json` has a model, and so does the account's own.
        const DEFAULT_SETTINGS: &str = r#"{"theme":"dark","model":"opus","effort":"low"}"#;
        const OWN_SETTINGS: &str = r#"{"model":"sonnet","theme":"mine","verbose":true}"#;

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
                ("settings.json", DEFAULT_SETTINGS),
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
                ("settings.json", OWN_SETTINGS),
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

        fn json(path: &Path) -> Value {
            serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
        }

        fn keys(path: &Path) -> Vec<String> {
            json(path).as_object().unwrap().keys().cloned().collect()
        }

        /// The account's `settings.json` is a file of its own that Wings made, not a link, and the marker says so.
        fn is_synced_copy(account: &Path) -> bool {
            fs::symlink_metadata(account.join("settings.json")).is_ok_and(|meta| meta.is_file()) && account.join(SYNCED_SETTINGS).exists()
        }

        #[test]
        fn turning_it_on_links_the_default_setup_and_moves_what_the_account_had_aside() {
            let f = folders("on");
            let default_before = tree(&f.default);
            share_into(&f.default, &f.config, &f.account).unwrap();
            for item in ["settings.local.json", "CLAUDE.md", "RTK.md", "statusline.sh", "rules", "skills", "commands", "hooks", "plugins"] {
                assert!(is_link_to(&f.account, &f.default, item), "{item}");
            }
            assert!(!f.account.join("settings.json").is_symlink(), "settings.json is a copy, since a link would share the model");
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
            assert_eq!(fs::read_to_string(backups.join("settings.json")).unwrap(), OWN_SETTINGS);
            assert_eq!(fs::read_to_string(backups.join("rules/own.md")).unwrap(), "own rule");
            assert_eq!(fs::read_dir(&backups).unwrap().count(), 2);
            // The account's settings are the default's with its own model, and what it writes there (as Claude Code does) stays its own.
            assert!(is_synced_copy(&f.account));
            assert_eq!(json(&f.account.join("settings.json")), serde_json::json!({ "theme": "dark", "model": "sonnet", "effort": "low" }));
            fs::write(f.account.join("settings.json"), r#"{"theme":"light"}"#).unwrap();
            // The default folder is only read, never written or reshaped.
            assert_eq!(tree(&f.default), default_before);
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
            // The account has used `/model` since: Claude Code wrote it into the copy.
            assert!(is_synced_copy(&f.account));
            fs::write(f.account.join("settings.json"), r#"{"model":"haiku"}"#).unwrap();
            set_shared_setup_in(&f.default, &f.config, &f.account, false).unwrap();
            assert_eq!(
                fs::read_to_string(f.account.join("settings.json")).unwrap(),
                OWN_SETTINGS,
                "the copy goes and the settings it had come back, byte for byte"
            );
            assert!(!f.account.join(SYNCED_SETTINGS).exists());
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
        fn the_first_sync_copies_the_defaults_settings_with_the_model_the_account_had() {
            use std::os::unix::fs::PermissionsExt;
            let f = folders("settings-first");
            let (default_settings, at) = (f.default.join("settings.json"), f.account.join("settings.json"));
            fs::set_permissions(&default_settings, fs::Permissions::from_mode(0o644)).unwrap();
            share_into(&f.default, &f.config, &f.account).unwrap();
            assert!(is_synced_copy(&f.account));
            // The default's keys in the default's order, the model in the default's place but the account's.
            assert_eq!(fs::read_to_string(&at).unwrap(), "{\n  \"theme\": \"dark\",\n  \"model\": \"sonnet\",\n  \"effort\": \"low\"\n}\n");
            assert_eq!(fs::metadata(&at).unwrap().permissions().mode() & 0o777, 0o644);
            assert!(!f.account.join("settings.json.wings-tmp").exists());
            assert_eq!(fs::read_to_string(&default_settings).unwrap(), DEFAULT_SETTINGS, "the default's file is only read");
            assert_eq!(fs::read_to_string(f.account.join(BACKUPS).join("settings.json")).unwrap(), OWN_SETTINGS);

            // The account had no model: it has none, though the default has one, and the default changing its own changes nothing.
            let g = folders("settings-no-model");
            put(&g.account.join("settings.json"), r#"{"theme":"mine"}"#);
            share_into(&g.default, &g.config, &g.account).unwrap();
            let at = g.account.join("settings.json");
            assert_eq!(json(&at), serde_json::json!({ "theme": "dark", "effort": "low" }));
            put(&g.default.join("settings.json"), r#"{"model":"opus-next","theme":"dark"}"#);
            share_into(&g.default, &g.config, &g.account).unwrap();
            assert_eq!(keys(&at), ["theme"]);

            // Nothing set aside, or what was set aside is not JSON: no model either, and the file is kept as it was.
            let h = folders("settings-not-json");
            put(&h.account.join("settings.json"), "own settings");
            share_into(&h.default, &h.config, &h.account).unwrap();
            assert_eq!(keys(&h.account.join("settings.json")), ["theme", "effort"]);
            assert_eq!(fs::read_to_string(h.account.join(BACKUPS).join("settings.json")).unwrap(), "own settings");
            let none = folders("settings-nothing");
            fs::remove_file(none.account.join("settings.json")).unwrap();
            share_into(&none.default, &none.config, &none.account).unwrap();
            assert_eq!(keys(&none.account.join("settings.json")), ["theme", "effort"]);

            // The default has no model and the account does: it is added last.
            let i = folders("settings-default-has-none");
            put(&i.default.join("settings.json"), r#"{"theme":"dark"}"#);
            share_into(&i.default, &i.config, &i.account).unwrap();
            assert_eq!(keys(&i.account.join("settings.json")), ["theme", "model"]);

            // A backup of the name is there already: the account's file goes to the next number, and its model is the one kept.
            let j = folders("settings-second-backup");
            put(&j.account.join(BACKUPS).join("settings.json"), r#"{"model":"haiku"}"#);
            share_into(&j.default, &j.config, &j.account).unwrap();
            assert_eq!(fs::read_to_string(j.account.join(BACKUPS).join("settings.json.2")).unwrap(), OWN_SETTINGS);
            assert_eq!(json(&j.account.join("settings.json"))["model"], "sonnet");
        }

        #[test]
        fn a_later_sync_keeps_the_accounts_model_and_picks_up_what_changed_in_the_default() {
            let f = folders("settings-later");
            share_into(&f.default, &f.config, &f.account).unwrap();
            let at = f.account.join("settings.json");
            // `/model` in the account: Claude Code writes the model into the settings file it reads.
            let mut own = json(&at);
            own["model"] = "haiku".into();
            fs::write(&at, serde_json::to_string_pretty(&own).unwrap()).unwrap();
            // The default changes a key, drops one, adds two and changes its own model.
            put(&f.default.join("settings.json"), r#"{"theme":"light","model":"opus-next","hooks":{"Stop":[]},"outputStyle":"concise"}"#);
            share_into(&f.default, &f.config, &f.account).unwrap();
            assert_eq!(json(&at), serde_json::json!({ "theme": "light", "model": "haiku", "hooks": { "Stop": [] }, "outputStyle": "concise" }));
            assert_eq!(keys(&at), ["theme", "model", "hooks", "outputStyle"]);
            // The copy is Wings' own, so it is not set aside again.
            assert_eq!(fs::read_dir(f.account.join(BACKUPS)).unwrap().count(), 2);
            assert_eq!(fs::read_to_string(f.account.join(BACKUPS).join("settings.json")).unwrap(), OWN_SETTINGS);
            // The account clears its model: it stays cleared.
            own = json(&at);
            own.as_object_mut().unwrap().shift_remove("model");
            fs::write(&at, serde_json::to_string_pretty(&own).unwrap()).unwrap();
            share_into(&f.default, &f.config, &f.account).unwrap();
            assert_eq!(keys(&at), ["theme", "hooks", "outputStyle"]);
            // A file that isn't JSON any more is not overwritten.
            fs::write(&at, "{ half").unwrap();
            assert!(share_into(&f.default, &f.config, &f.account).is_err());
            assert_eq!(fs::read_to_string(&at).unwrap(), "{ half");
        }

        #[test]
        fn a_settings_link_from_0_1_121_becomes_a_copy_and_the_default_is_left_alone() {
            let f = folders("settings-migrate");
            // As 0.1.121 left it: what the account had set aside, and a link to the default's file in its place.
            let backups = f.account.join(BACKUPS);
            fs::create_dir_all(&backups).unwrap();
            fs::rename(f.account.join("settings.json"), backups.join("settings.json")).unwrap();
            std::os::unix::fs::symlink(f.default.join("settings.json"), f.account.join("settings.json")).unwrap();
            let default_before = tree(&f.default);
            share_into(&f.default, &f.config, &f.account).unwrap();
            assert!(is_synced_copy(&f.account));
            assert_eq!(json(&f.account.join("settings.json")), serde_json::json!({ "theme": "dark", "model": "sonnet", "effort": "low" }));
            assert_eq!(tree(&f.default), default_before, "nothing was written through the link");
            assert_eq!(fs::read_to_string(backups.join("settings.json")).unwrap(), OWN_SETTINGS);
            assert!(!backups.join("settings.json.2").exists(), "the link is not a backup");
            // The same with nothing set aside: no model.
            let g = folders("settings-migrate-bare");
            fs::remove_file(g.account.join("settings.json")).unwrap();
            std::os::unix::fs::symlink(g.default.join("settings.json"), g.account.join("settings.json")).unwrap();
            share_into(&g.default, &g.config, &g.account).unwrap();
            assert!(is_synced_copy(&g.account));
            assert_eq!(keys(&g.account.join("settings.json")), ["theme", "effort"]);
        }

        #[test]
        fn a_settings_copy_that_is_up_to_date_is_not_written_again() {
            let f = folders("settings-same");
            share_into(&f.default, &f.config, &f.account).unwrap();
            let at = f.account.join("settings.json");
            let long_ago = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000);
            let age = |at: &Path| fs::File::options().write(true).open(at).unwrap().set_modified(long_ago).unwrap();
            let modified = |at: &Path| fs::metadata(at).unwrap().modified().unwrap();
            age(&at);
            share_into(&f.default, &f.config, &f.account).unwrap();
            assert_eq!(modified(&at), long_ago);
            // Claude Code saved it in another layout with the same content: also left as it is.
            fs::write(&at, r#"{"theme":"dark","model":"sonnet","effort":"low"}"#).unwrap();
            age(&at);
            sync_settings_in(&f.default, &f.account).unwrap();
            assert_eq!(modified(&at), long_ago);
            assert_eq!(fs::read_to_string(&at).unwrap(), r#"{"theme":"dark","model":"sonnet","effort":"low"}"#);
            // What the default changes is written.
            put(&f.default.join("settings.json"), r#"{"theme":"light","model":"opus","effort":"low"}"#);
            sync_settings_in(&f.default, &f.account).unwrap();
            assert_ne!(modified(&at), long_ago);
            assert_eq!(json(&at)["theme"], "light");
        }

        #[test]
        fn a_terminal_opening_for_an_account_syncs_its_settings_only_while_it_shares() {
            let f = folders("settings-pane");
            sync_settings_in(&f.default, &f.account).unwrap();
            assert!(is_synced_copy(&f.account));
            assert!(!taken(&f.account.join("CLAUDE.md")), "only the settings: the rest is linked when Wings connects");
            put(&f.default.join("settings.json"), r#"{"theme":"light"}"#);
            sync_settings_in(&f.default, &f.account).unwrap();
            assert_eq!(json(&f.account.join("settings.json")), serde_json::json!({ "theme": "light", "model": "sonnet" }));
            // Off: the account has its own settings back and a terminal leaves them be.
            set_shared_setup_in(&f.default, &f.config, &f.account, false).unwrap();
            sync_settings_in(&f.default, &f.account).unwrap();
            assert_eq!(fs::read_to_string(f.account.join("settings.json")).unwrap(), OWN_SETTINGS);
            // No folder, nothing made; the folder Wings runs under, an error.
            let nowhere = f.root.join(".claude-nowhere");
            sync_settings_in(&f.default, &nowhere).unwrap();
            assert!(!nowhere.exists());
            assert!(sync_settings_in(&f.default, &f.default).is_err());
        }

        #[test]
        fn a_second_backup_of_a_name_gets_a_number_and_a_place_in_use_is_not_overwritten() {
            let f = folders("numbered");
            put(&f.account.join("CLAUDE.md"), "own notes");
            share_into(&f.default, &f.config, &f.account).unwrap();
            // Something replaced the link with a file of its own, as a tool that writes by rename would.
            fs::remove_file(f.account.join("CLAUDE.md")).unwrap();
            put(&f.account.join("CLAUDE.md"), "replacement");
            share_into(&f.default, &f.config, &f.account).unwrap();
            let backups = f.account.join(BACKUPS);
            assert_eq!(fs::read_to_string(backups.join("CLAUDE.md")).unwrap(), "own notes");
            assert_eq!(fs::read_to_string(backups.join("CLAUDE.md.2")).unwrap(), "replacement");
            assert!(is_link_to(&f.account, &f.default, "CLAUDE.md"));
            // Off: the first comes back; the second has nowhere to go but stays, in the backup folder.
            unshare_from(&f.default, &f.account).unwrap();
            assert_eq!(fs::read_to_string(f.account.join("CLAUDE.md")).unwrap(), "own notes");
            assert_eq!(fs::read_to_string(backups.join("CLAUDE.md.2")).unwrap(), "replacement");
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
            put(
                &config,
                r#"{"oauthAccount":{"emailAddress":"work@example.com"},"projects":{"/p":{"allowedTools":[]}},"mcpServers":{"shared":{"command":"mine"}}}"#,
            );
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
            assert!(is_link_to(&f.account, &f.default, "settings.local.json") && is_synced_copy(&f.account));
            // Off, and Wings connecting again does not link it anew.
            set_shared_setup_in(&f.default, &f.config, &f.account, false).unwrap();
            assert!(!shares(&f.account));
            sync_in(&f.default, &f.config, &f.account).unwrap();
            assert_eq!(fs::read_to_string(f.account.join("settings.json")).unwrap(), OWN_SETTINGS);
            assert!(!f.account.join(SYNCED_SETTINGS).exists());
            // On again: the marker goes and the links come back, and the settings are copied again with the account's model.
            set_shared_setup_in(&f.default, &f.config, &f.account, true).unwrap();
            assert!(shares(&f.account) && !f.account.join(OWN_SETUP).exists());
            assert!(is_link_to(&f.account, &f.default, "settings.local.json") && is_synced_copy(&f.account));
            assert_eq!(json(&f.account.join("settings.json"))["model"], "sonnet");
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

    #[test]
    fn the_account_list_reads_what_claude_auth_status_prints() {
        let status = |logged_in, method: Option<&str>| AuthStatus { logged_in, method: method.map(String::from) };
        let signed_in = "{\n  \"loggedIn\": true,\n  \"authMethod\": \"claude.ai\",\n  \"apiProvider\": \"firstParty\",\n  \"email\": \"me@example.com\"\n}";
        let signed_out = "{\n  \"loggedIn\": false,\n  \"authMethod\": \"none\",\n  \"apiProvider\": \"firstParty\",\n  \"loginExpiresAt\": null\n}";
        assert_eq!(read_auth_status(signed_in), status(true, Some("claude.ai")));
        assert_eq!(read_auth_status(signed_out), status(false, Some("none")));
        // Output that isn't the status, or has no boolean `loggedIn`, is not a sign-in.
        for garbage in ["", "not json", "[]", "{}", r#"{"authMethod":"claude.ai"}"#, r#"{"loggedIn":"true","authMethod":"claude.ai"}"#] {
            assert_eq!(read_auth_status(garbage), status(false, None), "{garbage:?}");
        }
        assert_eq!(read_auth_status(r#"{"loggedIn":true}"#), status(true, None));
    }
}
