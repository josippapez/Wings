//! Keeps the `settings.json` of the default account and of the accounts that share its setup the same, as they change.
//!
//! A change in the default's file reaches every sharing account (`sync_settings`, which keeps each account's `model`); a
//! change in an account's copy goes into the default's file first (`share_back`, which keeps the default's `model`) and then
//! reaches the other accounts. When several files changed within one burst, the one changed last is the source (the whole
//! file is copied, so the others' other changes are replaced). A file that is in step, like the echo of one of Wings' own
//! writes, is no change: both directions write only when the content differs.
//!
//! Claude Code writes `settings.json` by rename, so the folders are watched, not the file; the events are filtered to the
//! name. Bursts settle for `SETTLE` before anything is read, and the work runs on the watcher's own thread.

use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::{
        mpsc::{self, Receiver, RecvTimeoutError, Sender},
        Mutex,
    },
    thread,
    time::{Duration, Instant, SystemTime},
};

use notify::{EventKind, RecursiveMode, Watcher as _};

use super::{back_change, dir, existing_names, same_folder, share_back, shares, sync_settings_in, SETTINGS};

/// Quiet time after the last write before syncing: Claude Code and Wings write a file and rename it within milliseconds, and
/// a burst should be read once it is finished.
const SETTLE: Duration = Duration::from_millis(300);
/// A file written without pause still syncs this often.
const MAX_WAIT: Duration = Duration::from_secs(2);

enum Msg {
    /// A `settings.json` somewhere was written: its path.
    Changed(PathBuf),
    /// The list of accounts changed.
    Resync,
    Stop,
}

/// The running watchers' channels, for `SettingsWatcher::resync_all`.
static RUNNING: Mutex<Vec<Sender<Msg>>> = Mutex::new(Vec::new());

/// A thread that follows the default account's folder and those of the accounts that share its setup and syncs their
/// `settings.json` both ways when one changes. Dropping it stops the thread.
pub struct SettingsWatcher {
    tx: Sender<Msg>,
}

impl SettingsWatcher {
    /// Starts following `default_dir` and the accounts that share its setup. `report` is called from the watcher's thread
    /// with what went wrong, one text each.
    pub fn start(default_dir: PathBuf, report: impl Fn(String) + Send + 'static) -> SettingsWatcher {
        let list = default_dir.clone();
        SettingsWatcher::start_with(default_dir, move || sharing_accounts(&list), report)
    }

    fn start_with(
        default_dir: PathBuf,
        accounts: impl Fn() -> Vec<PathBuf> + Send + 'static,
        report: impl Fn(String) + Send + 'static,
    ) -> SettingsWatcher {
        let (tx, rx) = mpsc::channel();
        let own = tx.clone();
        thread::spawn(move || run(own, rx, default_dir, accounts, report));
        let mut running = RUNNING.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        running.push(tx.clone());
        SettingsWatcher { tx }
    }

    /// Tells every running watcher the accounts changed (one was added, or sharing was turned on or off), so it follows the
    /// right folders.
    pub fn resync_all() {
        let mut running = RUNNING.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        // A watcher that ended has dropped its receiver.
        running.retain(|tx| tx.send(Msg::Resync).is_ok());
    }
}

impl Drop for SettingsWatcher {
    fn drop(&mut self) {
        // `run` keeps a sender for the file events, so the channel never closes on its own.
        let _ = self.tx.send(Msg::Stop);
    }
}

/// The folders of the named accounts that exist and share the default account's setup.
fn sharing_accounts(default_dir: &Path) -> Vec<PathBuf> {
    existing_names().iter().map(|name| dir(Some(name))).filter(|account| account.is_dir() && shares(account) && !same_folder(default_dir, account)).collect()
}

fn modified(folder: &Path) -> SystemTime {
    std::fs::metadata(folder.join(SETTINGS)).and_then(|meta| meta.modified()).unwrap_or(SystemTime::UNIX_EPOCH)
}

/// Syncs after the `settings.json` of the folders `changed` (the default's or an account's) were written. Only a folder that
/// is out of step counts (an account whose copy differs from what the default would give it, or the default while an account
/// does): Wings' own writes come back as changes of folders that are in step, and are no change. Of those, the one written
/// last is the source: an account's goes to the default (`share_back`) first; then the default goes to every account in
/// `accounts`. Returns what failed.
fn settle(default_dir: &Path, accounts: &[PathBuf], changed: &[PathBuf]) -> Vec<String> {
    let mut errors = Vec::new();
    let mut out_of_step = |account: &Path| match back_change(default_dir, account) {
        Ok(change) => change.is_some(),
        Err(e) => {
            errors.push(format!("Couldn't copy the settings of {} to the default account's: {e}", account.display()));
            false
        }
    };
    let mut source: Option<(&PathBuf, SystemTime)> = None;
    for folder in changed {
        let differs = if same_folder(default_dir, folder) { accounts.iter().any(|account| out_of_step(account)) } else { out_of_step(folder) };
        let at = modified(folder);
        if differs && source.is_none_or(|(_, newest)| at >= newest) {
            source = Some((folder, at));
        }
    }
    let Some((source, _)) = source else { return errors };
    if !same_folder(default_dir, source) {
        if let Err(e) = share_back(default_dir, source) {
            errors.push(format!("Couldn't copy the settings of {} to the default account's: {e}", source.display()));
        }
    }
    for account in accounts {
        if let Err(e) = sync_settings_in(default_dir, account) {
            errors.push(format!("Couldn't copy the default account's settings to {}: {e}", account.display()));
        }
    }
    errors
}

fn run(tx: Sender<Msg>, rx: Receiver<Msg>, default_dir: PathBuf, accounts: impl Fn() -> Vec<PathBuf>, report: impl Fn(String)) {
    let Ok(mut watcher) = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        if let Ok(event) = res {
            // Reading a file is not changing it.
            if !matches!(event.kind, EventKind::Access(_)) {
                for path in event.paths.into_iter().filter(|path| path.file_name().is_some_and(|name| name == SETTINGS)) {
                    let _ = tx.send(Msg::Changed(path));
                }
            }
        }
    }) else {
        report("Couldn't follow the Claude accounts' settings: the file watcher didn't start".into());
        return;
    };
    let mut folders: Vec<PathBuf> = Vec::new();
    let mut watched: HashSet<PathBuf> = HashSet::new();
    let mut pending: HashSet<PathBuf> = HashSet::new();
    let mut first = Instant::now();
    let mut last = first;
    let mut resync = true;
    loop {
        if resync {
            resync = false;
            folders = std::iter::once(default_dir.clone()).chain(accounts()).collect();
            let wanted: HashSet<PathBuf> = folders.iter().cloned().collect();
            for path in watched.difference(&wanted) {
                let _ = watcher.unwatch(path);
            }
            for path in wanted.difference(&watched) {
                if let Err(e) = watcher.watch(path, RecursiveMode::NonRecursive) {
                    report(format!("Couldn't follow the settings in {}: {e}", path.display()));
                }
            }
            watched = wanted;
            // An account that came in or turned sharing on is brought in step now, not at the next change: whichever
            // file is out of step and newest is the source.
            pending.extend(folders.iter().cloned());
            first = Instant::now();
            last = first;
        }
        let msg = if pending.is_empty() {
            match rx.recv() {
                Ok(msg) => Some(msg),
                Err(_) => return,
            }
        } else {
            let deadline = (last + SETTLE).min(first + MAX_WAIT);
            match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                Ok(msg) => Some(msg),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => return,
            }
        };
        match msg {
            Some(Msg::Resync) => resync = true,
            Some(Msg::Stop) => return,
            Some(Msg::Changed(path)) => {
                // The folder as the list has it (events may spell it another way, like `/private/var` for `/var`).
                let canonical = path.parent().and_then(|parent| parent.canonicalize().ok());
                let folder = folders.iter().find(|folder| {
                    path.parent() == Some(folder.as_path()) || canonical.as_ref().is_some_and(|canonical| folder.canonicalize().ok().as_ref() == Some(canonical))
                });
                if let Some(folder) = folder {
                    if pending.is_empty() {
                        first = Instant::now();
                    }
                    last = Instant::now();
                    pending.insert(folder.clone());
                }
            }
            None => {
                let changed: Vec<PathBuf> = pending.drain().collect();
                for error in settle(&default_dir, &accounts(), &changed) {
                    report(error);
                }
            }
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::claude_accounts::set_shared_setup_in;
    use serde_json::{json, Value};
    use std::{fs, sync::Arc};

    struct Setup {
        root: PathBuf,
        default: PathBuf,
        config: PathBuf,
    }

    impl Drop for Setup {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn read(folder: &Path) -> Value {
        serde_json::from_str(&fs::read_to_string(folder.join(SETTINGS)).unwrap()).unwrap()
    }

    fn put(folder: &Path, value: &Value) {
        fs::write(folder.join(SETTINGS), serde_json::to_string_pretty(value).unwrap()).unwrap();
    }

    /// A default account with a model, and one account per name, each with a model of its own, sharing as Wings sets it up.
    fn setup(test: &str, names: &[&str]) -> (Setup, Vec<PathBuf>) {
        let root = fs::canonicalize(std::env::temp_dir()).unwrap().join(format!("wings-watch-{test}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let default = root.join("default");
        fs::create_dir_all(&default).unwrap();
        put(&default, &json!({"theme": "dark", "model": "opus", "enabledPlugins": {}}));
        let setup = Setup { config: root.join("default.claude.json"), default, root };
        let accounts = names
            .iter()
            .map(|name| {
                let account = setup.root.join(format!(".claude-{name}"));
                fs::create_dir_all(&account).unwrap();
                put(&account, &json!({"model": format!("{name}-model")}));
                set_shared_setup_in(&setup.default, &setup.config, &account, true).unwrap();
                account
            })
            .collect();
        (setup, accounts)
    }

    fn modified_at(folder: &Path) -> SystemTime {
        fs::metadata(folder.join(SETTINGS)).unwrap().modified().unwrap()
    }

    #[test]
    fn a_change_in_the_default_reaches_every_account_and_each_keeps_its_model() {
        let (s, accounts) = setup("down", &["a", "b"]);
        put(&s.default, &json!({"theme": "light", "model": "opus", "enabledPlugins": {"x": true}}));
        let errors = settle(&s.default, &accounts, &[s.default.clone()]);
        assert!(errors.is_empty(), "{errors:?}");
        for (account, model) in accounts.iter().zip(["a-model", "b-model"]) {
            assert_eq!(read(account), json!({"theme": "light", "model": model, "enabledPlugins": {"x": true}}));
        }
    }

    #[test]
    fn a_change_in_an_account_reaches_the_default_without_its_model_and_then_the_other_accounts() {
        let (s, accounts) = setup("up", &["a", "b"]);
        // Claude Code under `a` enables a plugin, and the user ran /model there.
        put(&accounts[0], &json!({"theme": "dark", "model": "sonnet", "enabledPlugins": {"x": true}}));
        let errors = settle(&s.default, &accounts, &[accounts[0].clone()]);
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(read(&s.default), json!({"theme": "dark", "model": "opus", "enabledPlugins": {"x": true}}), "the default keeps its own model");
        assert_eq!(read(&accounts[0]), json!({"theme": "dark", "model": "sonnet", "enabledPlugins": {"x": true}}));
        assert_eq!(read(&accounts[1]), json!({"theme": "dark", "model": "b-model", "enabledPlugins": {"x": true}}));
    }

    #[test]
    fn a_default_without_a_model_stays_without_one() {
        let (s, accounts) = setup("nomodel", &["a"]);
        put(&s.default, &json!({"theme": "dark"}));
        settle(&s.default, &accounts, &[s.default.clone()]);
        put(&accounts[0], &json!({"theme": "light", "model": "sonnet"}));
        settle(&s.default, &accounts, &[accounts[0].clone()]);
        assert_eq!(read(&s.default), json!({"theme": "light"}));
    }

    #[test]
    fn the_copys_key_order_goes_to_the_default() {
        let (s, accounts) = setup("order", &["a"]);
        fs::write(accounts[0].join(SETTINGS), r#"{"zeta":1,"model":"m","alpha":2}"#).unwrap();
        settle(&s.default, &accounts, &[accounts[0].clone()]);
        let text = fs::read_to_string(s.default.join(SETTINGS)).unwrap();
        let at = |key: &str| text.find(key).unwrap();
        assert!(at("zeta") < at("model") && at("model") < at("alpha"), "{text}");
        assert!(text.contains("opus"));
    }

    #[test]
    fn an_account_that_does_not_share_is_left_alone() {
        let (s, accounts) = setup("own", &["a", "b"]);
        set_shared_setup_in(&s.default, &s.config, &accounts[1], false).unwrap();
        put(&accounts[1], &json!({"theme": "mine"}));
        // Its change doesn't reach the default, and the default's doesn't reach it.
        let sharing = [accounts[0].clone()];
        settle(&s.default, &sharing, &[accounts[1].clone()]);
        assert_eq!(read(&s.default)["theme"], "dark");
        put(&s.default, &json!({"theme": "light", "model": "opus"}));
        settle(&s.default, &sharing, &[s.default.clone()]);
        assert_eq!(read(&accounts[1]), json!({"theme": "mine"}));
        assert_eq!(read(&accounts[0])["theme"], "light");
    }

    #[test]
    fn an_account_with_settings_of_its_own_is_not_taken_for_a_copy() {
        let (s, accounts) = setup("nomarker", &["a"]);
        fs::remove_file(accounts[0].join(".wings-synced-settings")).unwrap();
        put(&accounts[0], &json!({"theme": "mine"}));
        share_back(&s.default, &accounts[0]).unwrap();
        assert_eq!(read(&s.default)["theme"], "dark");
    }

    #[test]
    fn repeating_the_sync_writes_nothing() {
        let (s, accounts) = setup("echo", &["a", "b"]);
        put(&accounts[0], &json!({"theme": "light", "model": "sonnet"}));
        let all: Vec<PathBuf> = std::iter::once(s.default.clone()).chain(accounts.iter().cloned()).collect();
        assert!(settle(&s.default, &accounts, &[accounts[0].clone()]).is_empty());
        let state = || all.iter().map(|folder| (fs::read_to_string(folder.join(SETTINGS)).unwrap(), modified_at(folder))).collect::<Vec<_>>();
        let before = state();
        std::thread::sleep(Duration::from_millis(30));
        // The events Wings' own writes make: every folder, again and again.
        for _ in 0..3 {
            assert!(settle(&s.default, &accounts, &all).is_empty());
        }
        assert_eq!(before, state(), "no file was written again");
    }

    #[test]
    fn a_copy_that_is_not_json_does_not_overwrite_the_default() {
        let (s, accounts) = setup("bad", &["a"]);
        let before = fs::read_to_string(s.default.join(SETTINGS)).unwrap();
        fs::write(accounts[0].join(SETTINGS), r#"{"theme": "li"#).unwrap();
        let errors = settle(&s.default, &accounts, &[accounts[0].clone()]);
        assert!(!errors.is_empty());
        assert_eq!(fs::read_to_string(s.default.join(SETTINGS)).unwrap(), before);
        assert_eq!(fs::read_to_string(accounts[0].join(SETTINGS)).unwrap(), r#"{"theme": "li"#, "the partial copy is left as it is");
    }

    #[test]
    fn the_file_changed_last_wins() {
        let (s, accounts) = setup("last", &["a"]);
        put(&accounts[0], &json!({"theme": "from-account", "model": "x"}));
        std::thread::sleep(Duration::from_millis(30));
        put(&s.default, &json!({"theme": "from-default", "model": "opus"}));
        settle(&s.default, &accounts, &[s.default.clone(), accounts[0].clone()]);
        assert_eq!(read(&s.default)["theme"], "from-default");
        assert_eq!(read(&accounts[0])["theme"], "from-default");
    }

    fn wait_for(within: Duration, mut done: impl FnMut() -> bool) -> bool {
        let end = Instant::now() + within;
        while Instant::now() < end {
            if done() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        done()
    }

    /// Writes the way Claude Code does: a temporary file in the folder, renamed over the file.
    fn write_by_rename(folder: &Path, value: &Value) {
        let tmp = folder.join("settings.json.tmp.1");
        fs::write(&tmp, serde_json::to_string_pretty(value).unwrap()).unwrap();
        fs::rename(&tmp, folder.join(SETTINGS)).unwrap();
    }

    #[test]
    fn the_watcher_follows_a_write_by_rename_both_ways_and_settles() {
        let (s, accounts) = setup("watch", &["a", "b"]);
        let errors = Arc::new(Mutex::new(Vec::new()));
        let seen = errors.clone();
        let list = accounts.clone();
        let _watcher = SettingsWatcher::start_with(s.default.clone(), move || list.clone(), move |e| seen.lock().unwrap().push(e));
        let within = Duration::from_secs(10);
        // The default changes: both accounts follow.
        write_by_rename(&s.default, &json!({"theme": "light", "model": "opus"}));
        assert!(wait_for(within, || accounts.iter().all(|a| read(a)["theme"] == "light")), "the default's change reached the accounts");
        assert_eq!(read(&accounts[1])["model"], "b-model");
        // An account changes: the default and the other account follow.
        write_by_rename(&accounts[0], &json!({"theme": "light", "model": "sonnet", "enabledPlugins": {"p": true}}));
        assert!(wait_for(within, || read(&accounts[1])["enabledPlugins"] == json!({"p": true})), "the account's change reached the other account");
        assert_eq!(read(&s.default), json!({"theme": "light", "model": "opus", "enabledPlugins": {"p": true}}));
        assert_eq!(read(&accounts[1])["model"], "b-model");
        // And it comes to rest: nothing is written after the burst has settled.
        std::thread::sleep(Duration::from_millis(1200));
        let stamps = |s: &Setup| std::iter::once(&s.default).chain(&accounts).map(|f| modified_at(f)).collect::<Vec<_>>();
        let first = stamps(&s);
        std::thread::sleep(Duration::from_millis(1200));
        assert_eq!(first, stamps(&s));
        assert!(errors.lock().unwrap().is_empty(), "{:?}", errors.lock().unwrap());
    }

    #[test]
    fn a_resync_follows_an_account_that_came_in_and_brings_it_up_to_date() {
        let (s, accounts) = setup("resync", &["a"]);
        let list = Arc::new(Mutex::new(accounts.clone()));
        let own = list.clone();
        let _watcher = SettingsWatcher::start_with(s.default.clone(), move || own.lock().unwrap().clone(), |_| {});
        let late = s.root.join(".claude-late");
        fs::create_dir_all(&late).unwrap();
        put(&late, &json!({"model": "late-model"}));
        set_shared_setup_in(&s.default, &s.config, &late, true).unwrap();
        put(&s.default, &json!({"theme": "light", "model": "opus"}));
        let mut all = accounts.clone();
        all.push(late.clone());
        *list.lock().unwrap() = all;
        SettingsWatcher::resync_all();
        assert!(wait_for(Duration::from_secs(10), || read(&late)["theme"] == "light"));
        write_by_rename(&late, &json!({"theme": "dark", "model": "late-model"}));
        assert!(wait_for(Duration::from_secs(10), || read(&s.default)["theme"] == "dark"), "the new account is followed");
    }

    #[test]
    fn dropping_the_watcher_ends_its_thread() {
        struct End(Sender<()>);
        impl Drop for End {
            fn drop(&mut self) {
                let _ = self.0.send(());
            }
        }
        let (ended, rx) = mpsc::channel();
        let end = End(ended);
        let (s, _) = setup("drop", &[]);
        let watcher = SettingsWatcher::start_with(
            s.default.clone(),
            move || {
                let _keep = &end;
                Vec::new()
            },
            |_| {},
        );
        drop(watcher);
        rx.recv_timeout(Duration::from_secs(5)).expect("the watcher thread outlived its handle");
    }
}
