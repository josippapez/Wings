//! Each plugin's own key-value storage, for its settings and state. Plugin frames have an opaque origin,
//! so `localStorage` throws there. Values are JSON, one file per key in a folder per plugin and install scope
//! (the scope its secrets use, see `plugin_store`), so a different package that later claims the same id
//! starts empty, and a set writes only its own key.

use std::{
    collections::{BTreeMap, HashMap},
    fs, io,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use serde_json::Value;

pub const MAX_KEY_LEN: usize = 128;
/// One value's JSON text.
pub const MAX_VALUE_BYTES: usize = 1024 * 1024;
/// A plugin's storage in all: every value's JSON text and every key.
pub const MAX_BYTES: u64 = 50 * 1024 * 1024;

type Data = BTreeMap<String, Value>;

/// The keys on disk and what each takes (its value's JSON and the key), with their sum, so a set checks the cap
/// without listing the folder.
struct Index {
    sizes: BTreeMap<String, u64>,
    total: u64,
}

#[derive(Default)]
struct Bucket {
    /// Read from the folder on first use, then kept up to date by every change.
    index: Option<Index>,
    /// The scope was deleted. Scopes are never reused, so refusing it from now on keeps a write that was
    /// already on its way from bringing the folder back.
    gone: bool,
}

/// Where a plugin's storage in one scope lives, under `<root>`, for `<id>.<scope>`.
struct Paths {
    /// `<id>.<scope>/`, with a `<key in base32>.json` file per key.
    dir: PathBuf,
    /// `<id>.<scope>.json`: every key in one file, as Wings stored it before. Split into `dir` on first use.
    old: PathBuf,
    /// `<id>.<scope>.json.migrated`: the old file once its split is whole, kept as it was.
    migrated: PathBuf,
    /// `<id>.<scope>.split/`: the split being written. Renamed to `dir` once every key is in it.
    split: PathBuf,
}

pub struct Storage {
    root: PathBuf,
    /// One lock per plugin and scope: a plugin's main frame and sidebars can't lose each other's writes, and
    /// one plugin never waits on another's.
    buckets: Mutex<HashMap<String, Arc<Mutex<Bucket>>>>,
}

fn key_chars(key: &str) -> bool {
    key.len() <= MAX_KEY_LEN && key.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.:/".contains(&b))
}

fn check_key(key: &str) -> Result<(), String> {
    (!key.is_empty() && key_chars(key)).then_some(()).ok_or_else(|| format!("A storage key is 1 to {MAX_KEY_LEN} letters, digits and - _ . : /"))
}

const BASE32: &[u8; 32] = b"abcdefghijklmnopqrstuvwxyz234567";

/// A key as a file name: base32 in lower case. Keys differ by case and disks often don't (`A` and `a` are two keys),
/// and the longest key is 205 characters this way, under the 255 a file name may have.
fn file_name(key: &str) -> String {
    let mut name = String::with_capacity(key.len() * 8 / 5 + 6);
    let (mut buf, mut bits) = (0u32, 0);
    for &b in key.as_bytes() {
        buf = (buf << 8) | u32::from(b);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            name.push(BASE32[(buf >> bits & 31) as usize] as char);
        }
        buf &= (1 << bits) - 1;
    }
    if bits > 0 {
        name.push(BASE32[(buf << (5 - bits) & 31) as usize] as char);
    }
    name + ".json"
}

/// The key a file holds, or `None` for a file that isn't one (a `.tmp` left by a crash).
fn key_of(name: &str) -> Option<String> {
    let mut bytes = Vec::new();
    let (mut buf, mut bits) = (0u32, 0);
    for c in name.strip_suffix(".json")?.bytes() {
        buf = (buf << 5) | BASE32.iter().position(|&x| x == c)? as u32;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            bytes.push((buf >> bits) as u8);
        }
        buf &= (1 << bits) - 1;
    }
    let key = String::from_utf8(bytes).ok()?;
    (check_key(&key).is_ok() && file_name(&key) == name).then_some(key)
}

fn tmp_path(path: &Path) -> PathBuf {
    path.with_extension("json.tmp")
}

/// Write then rename, so a crash mid-write leaves the previous file whole.
fn save(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let tmp = tmp_path(path);
    fs::write(&tmp, bytes).and_then(|()| fs::rename(&tmp, path)).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        e.to_string()
    })
}

fn not_found_ok(r: io::Result<()>) -> Result<(), String> {
    match r {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e.to_string()),
        _ => Ok(()),
    }
}

/// Writes every key of the old single file into `split`, then renames that to `dir`, so `dir` only ever exists
/// whole. The old file stays where it was until then: a crash or an error before the rename leaves it in charge,
/// and the next use starts the split again. After the rename it becomes `.migrated`, kept as it was.
fn split(paths: &Paths) -> Result<(), String> {
    let data: Data = serde_json::from_slice(&fs::read(&paths.old).map_err(|e| e.to_string())?)
        .map_err(|e| format!("Couldn't read {}: {e}", paths.old.display()))?;
    not_found_ok(fs::remove_dir_all(&paths.split))?;
    let written = fs::create_dir_all(&paths.split).map_err(|e| e.to_string()).and_then(|()| {
        for (key, value) in &data {
            check_key(key)?;
            let bytes = serde_json::to_vec(value).map_err(|e| e.to_string())?;
            fs::write(paths.split.join(file_name(key)), bytes).map_err(|e| e.to_string())?;
        }
        fs::rename(&paths.split, &paths.dir).map_err(|e| e.to_string())
    });
    if let Err(e) = written {
        let _ = fs::remove_dir_all(&paths.split);
        return Err(format!("Couldn't move {} to one file per key: {e}", paths.old.display()));
    }
    Ok(())
}

/// The keys in `dir` and their sizes, after splitting an old single file when there is one.
fn open(paths: &Paths) -> Result<Index, String> {
    if paths.old.exists() {
        if !paths.dir.exists() {
            split(paths)?;
        }
        // `dir` is whole, so the old file is done with, also when a crash came between the two renames.
        fs::rename(&paths.old, &paths.migrated).map_err(|e| e.to_string())?;
    }
    let entries = match fs::read_dir(&paths.dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Index { sizes: BTreeMap::new(), total: 0 }),
        Err(e) => return Err(e.to_string()),
    };
    let mut sizes = BTreeMap::new();
    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        if let Some(key) = key_of(&entry.file_name().to_string_lossy()) {
            let len = entry.metadata().map_err(|e| e.to_string())?.len();
            sizes.insert(key.clone(), len + key.len() as u64);
        }
    }
    let total = sizes.values().sum();
    Ok(Index { sizes, total })
}

impl Storage {
    pub fn new(root: PathBuf) -> Self {
        Self { root, buckets: Mutex::default() }
    }

    fn paths(&self, plugin_id: &str, scope: &str) -> Result<Paths, String> {
        // Both come from Wings, not the plugin, but they name files, so keep them to safe characters.
        let ok = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-');
        if !ok(plugin_id) || !ok(scope) {
            return Err(format!("no storage for {plugin_id}"));
        }
        let at = |suffix: &str| self.root.join(format!("{plugin_id}.{scope}{suffix}"));
        Ok(Paths { dir: at(""), old: at(".json"), migrated: at(".json.migrated"), split: at(".split") })
    }

    fn bucket(&self, paths: &Paths) -> Arc<Mutex<Bucket>> {
        self.buckets.lock().unwrap().entry(paths.dir.to_string_lossy().into_owned()).or_default().clone()
    }

    /// Runs `f` on the plugin's index with its lock held.
    fn with<T>(&self, plugin_id: &str, scope: &str, f: impl FnOnce(&mut Index, &Path) -> Result<T, String>) -> Result<T, String> {
        let paths = self.paths(plugin_id, scope)?;
        let bucket = self.bucket(&paths);
        let mut bucket = bucket.lock().unwrap();
        if bucket.gone {
            return Err(format!("{plugin_id}'s storage was deleted"));
        }
        if bucket.index.is_none() {
            bucket.index = Some(open(&paths)?);
        }
        f(bucket.index.as_mut().expect("just opened"), &paths.dir)
    }

    pub fn get(&self, plugin_id: &str, scope: &str, key: &str) -> Result<Option<Value>, String> {
        check_key(key)?;
        self.with(plugin_id, scope, |index, dir| if index.sizes.contains_key(key) { read(dir, key).map(Some) } else { Ok(None) })
    }

    /// Every key, sorted.
    pub fn keys(&self, plugin_id: &str, scope: &str) -> Result<Vec<String>, String> {
        self.with(plugin_id, scope, |index, _| Ok(index.sizes.keys().cloned().collect()))
    }

    /// Every key that starts with `prefix` and its value, sorted by key. An empty prefix is every key.
    pub fn entries(&self, plugin_id: &str, scope: &str, prefix: &str) -> Result<Vec<(String, Value)>, String> {
        if !key_chars(prefix) {
            return Err(format!("A storage prefix is up to {MAX_KEY_LEN} letters, digits and - _ . : /"));
        }
        self.with(plugin_id, scope, |index, dir| {
            let keys = index.sizes.range(prefix.to_string()..).map(|(k, _)| k).take_while(|k| k.starts_with(prefix));
            keys.map(|k| Ok((k.clone(), read(dir, k)?))).collect()
        })
    }

    /// Refuses a value over `MAX_VALUE_BYTES`, or a write that would take the plugin's storage over `MAX_BYTES`,
    /// and changes nothing then.
    pub fn set(&self, plugin_id: &str, scope: &str, key: &str, value: Value) -> Result<(), String> {
        check_key(key)?;
        let bytes = serde_json::to_vec(&value).map_err(|e| e.to_string())?;
        if bytes.len() > MAX_VALUE_BYTES {
            return Err(format!("A stored value is limited to 1 MB, and this one is {} KB.", bytes.len().div_ceil(1024)));
        }
        self.with(plugin_id, scope, |index, dir| {
            let size = (bytes.len() + key.len()) as u64;
            let total = index.total - index.sizes.get(key).copied().unwrap_or(0) + size;
            if total > MAX_BYTES {
                return Err(format!(
                    "{plugin_id}'s storage is limited to 50 MB, and this would make it {} KB. Delete what it no longer needs first.",
                    total.div_ceil(1024)
                ));
            }
            save(&dir.join(file_name(key)), &bytes)?;
            index.sizes.insert(key.to_string(), size);
            index.total = total;
            Ok(())
        })
    }

    pub fn delete(&self, plugin_id: &str, scope: &str, key: &str) -> Result<(), String> {
        check_key(key)?;
        self.with(plugin_id, scope, |index, dir| {
            let Some(&size) = index.sizes.get(key) else { return Ok(()) };
            not_found_ok(fs::remove_file(dir.join(file_name(key))))?;
            index.sizes.remove(key);
            index.total -= size;
            Ok(())
        })
    }

    /// Deletes everything the plugin stored in this scope, when it's removed or replaced by a plugin from
    /// somewhere else. The scope is refused from then on.
    pub fn remove(&self, plugin_id: &str, scope: &str) -> Result<(), String> {
        let paths = self.paths(plugin_id, scope)?;
        let bucket = self.bucket(&paths);
        let mut bucket = bucket.lock().unwrap();
        bucket.gone = true;
        bucket.index = None;
        let _ = fs::remove_dir_all(&paths.split);
        let _ = fs::remove_file(tmp_path(&paths.old));
        not_found_ok(fs::remove_file(&paths.old))?;
        not_found_ok(fs::remove_file(&paths.migrated))?;
        not_found_ok(fs::remove_dir_all(&paths.dir))
    }
}

fn read(dir: &Path, key: &str) -> Result<Value, String> {
    let path = dir.join(file_name(key));
    serde_json::from_slice(&fs::read(&path).map_err(|e| e.to_string())?).map_err(|e| format!("Couldn't read {}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A dir no other test shares. Nanos alone can repeat for tests started in the same clock tick, and one test's
    /// cleanup then deletes another's files.
    fn storage() -> (Storage, PathBuf) {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("wings-storage-{}-{n}-{nanos}", std::process::id()));
        (Storage::new(dir.clone()), dir)
    }

    fn files(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir).map(|d| d.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect()).unwrap_or_default();
        names.sort();
        names
    }

    #[test]
    fn gets_sets_deletes_and_lists_json_values() {
        let (s, dir) = storage();
        assert_eq!(s.get("demo", "s1", "settings").unwrap(), None);
        assert!(s.keys("demo", "s1").unwrap().is_empty());
        // Reading creates nothing on disk.
        assert!(files(&dir).is_empty());

        let settings = json!({ "theme": "dark", "repos": ["a/b"], "limit": 5, "on": true, "none": null });
        s.set("demo", "s1", "settings", settings.clone()).unwrap();
        s.set("demo", "s1", "history:2026-10", json!([1, 2.5, "x"])).unwrap();
        s.set("demo", "s1", "count", json!(1)).unwrap();
        s.set("demo", "s1", "count", json!(2)).unwrap();
        assert_eq!(s.get("demo", "s1", "settings").unwrap(), Some(settings.clone()));
        assert_eq!(s.get("demo", "s1", "count").unwrap(), Some(json!(2)));
        assert_eq!(s.keys("demo", "s1").unwrap(), ["count", "history:2026-10", "settings"]);
        // One file per key, in the plugin's folder.
        assert_eq!(files(&dir), ["demo.s1"]);
        assert_eq!(files(&dir.join("demo.s1")).len(), 3);

        s.delete("demo", "s1", "count").unwrap();
        s.delete("demo", "s1", "never-set").unwrap();
        assert_eq!(s.get("demo", "s1", "count").unwrap(), None);
        assert_eq!(s.keys("demo", "s1").unwrap(), ["history:2026-10", "settings"]);
        assert_eq!(files(&dir.join("demo.s1")).len(), 2);

        // It's on disk, so a restart reads it back.
        let again = Storage::new(dir.clone());
        assert_eq!(again.get("demo", "s1", "settings").unwrap(), Some(settings));
        assert_eq!(again.keys("demo", "s1").unwrap(), ["history:2026-10", "settings"]);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn entries_reads_every_key_under_a_prefix_sorted() {
        let (s, dir) = storage();
        for (k, v) in [("p/b:2", 2), ("p/a:1", 1), ("p/b:10", 10), ("q/a", 0), ("p", -1)] {
            s.set("demo", "s1", k, json!(v)).unwrap();
        }
        let got = |prefix: &str| s.entries("demo", "s1", prefix).unwrap();
        assert_eq!(got("p/"), [("p/a:1".to_string(), json!(1)), ("p/b:10".to_string(), json!(10)), ("p/b:2".to_string(), json!(2))]);
        assert_eq!(got("p/b:1"), [("p/b:10".to_string(), json!(10))]);
        assert!(got("r").is_empty());
        assert_eq!(got("").len(), 5);
        assert!(s.entries("demo", "s1", "bad prefix").is_err());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn keys_become_file_names_that_read_back_and_differ_by_case() {
        let longest = "Az09-_.:/".repeat(15)[..MAX_KEY_LEN].to_string();
        for key in ["a", "A", "issues:/Users/me/My-App", "x-_.:/Y", longest.as_str()] {
            let name = file_name(key);
            assert!(name.len() <= 255 && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.'), "{name}");
            assert_eq!(key_of(&name).as_deref(), Some(key));
        }
        assert_eq!(file_name(&longest).len(), 210);
        // Files that aren't a key's are skipped: a crash's .tmp, a name that isn't base32 or doesn't read back the same.
        assert_eq!(key_of(&(file_name("a") + ".tmp")), None);
        assert_eq!(key_of("A0.json"), None);
        assert_eq!(key_of("mf.json"), None);

        // On a disk that ignores case, "A" and "a" are still two keys.
        let (s, dir) = storage();
        s.set("demo", "s1", "A", json!("upper")).unwrap();
        s.set("demo", "s1", "a", json!("lower")).unwrap();
        s.set("demo", "s1", &longest, json!("long")).unwrap();
        let again = Storage::new(dir.clone());
        assert_eq!(again.keys("demo", "s1").unwrap(), ["A".to_string(), longest.clone(), "a".into()]);
        assert_eq!(again.get("demo", "s1", "A").unwrap(), Some(json!("upper")));
        assert_eq!(again.get("demo", "s1", "a").unwrap(), Some(json!("lower")));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn refuses_bad_keys_and_writes_over_the_size_limits() {
        let (s, dir) = storage();
        for key in ["", "has space", "ünïcode", "new\nline", "quote\"", &"k".repeat(MAX_KEY_LEN + 1)] {
            assert!(s.set("demo", "s1", key, json!(1)).unwrap_err().contains("storage key"), "{key:?}");
            assert!(s.get("demo", "s1", key).is_err() && s.delete("demo", "s1", key).is_err(), "{key:?}");
        }
        s.set("demo", "s1", &"k".repeat(MAX_KEY_LEN), json!(1)).unwrap();
        s.set("demo", "s1", "a-Z_0.9:/x", json!(1)).unwrap();

        // One value: 1 MB of JSON fits, a byte more is refused.
        let string_of = |len: usize| json!("x".repeat(len - 2));
        s.set("demo", "s1", "one", string_of(MAX_VALUE_BYTES)).unwrap();
        let error = s.set("demo", "s1", "one", string_of(MAX_VALUE_BYTES + 1)).unwrap_err();
        assert!(error.contains("limited to 1 MB") && error.contains("1025 KB"), "{error}");
        assert_eq!(serde_json::to_vec(&s.get("demo", "s1", "one").unwrap().unwrap()).unwrap().len(), MAX_VALUE_BYTES);

        // In all: just under 50 MB fits; one write that goes over is refused and changes nothing.
        let used = |s: &Storage| s.with("demo", "s1", |index, _| Ok(index.total)).unwrap();
        for i in 0..48 {
            s.set("demo", "s1", &format!("big{i:02}"), string_of(MAX_VALUE_BYTES)).unwrap();
        }
        let room = (MAX_BYTES - used(&s)) as usize - "last".len();
        s.set("demo", "s1", "last", string_of(room)).unwrap();
        assert_eq!(used(&s), MAX_BYTES);
        let error = s.set("demo", "s1", "more", json!("y")).unwrap_err();
        assert!(error.contains("limited to 50 MB") && error.contains("51201 KB"), "{error}");
        assert_eq!(s.get("demo", "s1", "more").unwrap(), None);
        // Replacing a value counts its new size, not both.
        s.set("demo", "s1", "last", string_of(room)).unwrap();
        // The total a restart counts from the files is the one kept while running.
        let again = Storage::new(dir.clone());
        assert_eq!((used(&again), again.keys("demo", "s1").unwrap().len()), (MAX_BYTES, 52));
        // Making room works.
        s.delete("demo", "s1", "big00").unwrap();
        s.set("demo", "s1", "more", json!("y")).unwrap();
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_new_scope_or_another_plugin_sees_nothing() {
        let (s, dir) = storage();
        s.set("demo", "s1", "token-history", json!(["private"])).unwrap();
        assert_eq!(s.get("demo", "s2", "token-history").unwrap(), None);
        assert!(s.keys("demo", "s2").unwrap().is_empty());
        assert_eq!(s.get("other", "s1", "token-history").unwrap(), None);
        // Ids and scopes name the folder, so they can't point anywhere else.
        assert!(s.get("../demo", "s1", "k").is_err() && s.get("demo", "s1/../s2", "k").is_err() && s.get("demo", "", "k").is_err());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn removing_a_scope_deletes_its_folder_and_refuses_later_writes() {
        let (s, dir) = storage();
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("demo.s1.json"), br#"{"old":1}"#).unwrap();
        s.set("demo", "s1", "k", json!(1)).unwrap();
        s.set("demo", "s2", "k", json!(2)).unwrap();
        assert_eq!(files(&dir), ["demo.s1", "demo.s1.json.migrated", "demo.s2"]);
        s.remove("demo", "s1").unwrap();
        assert_eq!(files(&dir), ["demo.s2"]);
        // A write still on its way from the removed plugin doesn't bring the folder back.
        assert!(s.set("demo", "s1", "k", json!(1)).unwrap_err().contains("deleted"));
        assert_eq!(files(&dir), ["demo.s2"]);
        // Removing something that was never stored is fine.
        s.remove("never", "s9").unwrap();
        assert_eq!(s.get("demo", "s2", "k").unwrap(), Some(json!(2)));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_failed_write_leaves_the_old_value_whole_and_no_partial_file() {
        let (s, dir) = storage();
        s.set("demo", "s1", "k", json!("old")).unwrap();
        s.set("demo", "s1", "k2", json!("also")).unwrap();

        // A folder where the temp file goes makes the write fail before the rename.
        let tmp = tmp_path(&dir.join("demo.s1").join(file_name("k")));
        fs::create_dir(&tmp).unwrap();
        let total = s.with("demo", "s1", |index, _| Ok(index.total)).unwrap();
        assert!(s.set("demo", "s1", "k", json!("new value")).is_err());
        assert_eq!(s.get("demo", "s1", "k").unwrap(), Some(json!("old")));
        assert_eq!(s.with("demo", "s1", |index, _| Ok(index.total)).unwrap(), total);
        assert_eq!(Storage::new(dir.clone()).get("demo", "s1", "k").unwrap(), Some(json!("old")));

        fs::remove_dir(&tmp).unwrap();
        s.set("demo", "s1", "k", json!("new")).unwrap();
        assert_eq!(files(&dir.join("demo.s1")).len(), 2);
        let _ = fs::remove_dir_all(dir);
    }

    /// What Wings kept before: every key in one `<id>.<scope>.json`.
    fn old_file(dir: &Path, data: &Value) -> Vec<u8> {
        fs::create_dir_all(dir).unwrap();
        let bytes = serde_json::to_vec(data).unwrap();
        fs::write(dir.join("demo.s1.json"), &bytes).unwrap();
        bytes
    }

    #[test]
    fn splits_an_old_single_file_on_first_use_and_keeps_it_as_migrated() {
        let (s, dir) = storage();
        let data = json!({ "issues:/Users/me/app": { "v": 3, "issues": [1, 2] }, "view": "board", "Z": null });
        let before = old_file(&dir, &data);
        assert_eq!(s.keys("demo", "s1").unwrap(), ["Z", "issues:/Users/me/app", "view"]);
        assert_eq!(s.get("demo", "s1", "issues:/Users/me/app").unwrap(), Some(data["issues:/Users/me/app"].clone()));
        assert_eq!(s.get("demo", "s1", "Z").unwrap(), Some(Value::Null));
        assert_eq!(files(&dir), ["demo.s1", "demo.s1.json.migrated"]);
        assert_eq!(fs::read(dir.join("demo.s1.json.migrated")).unwrap(), before);
        // The total starts from the split files.
        s.set("demo", "s1", "view", json!("list")).unwrap();
        let again = Storage::new(dir.clone());
        assert_eq!(again.get("demo", "s1", "view").unwrap(), Some(json!("list")));
        assert_eq!(again.entries("demo", "s1", "").unwrap().len(), 3);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_split_that_fails_or_crashes_midway_leaves_the_old_file_in_charge() {
        let (s, dir) = storage();
        // A key Wings would refuse stops the split after it wrote some files.
        let bad = json!({ "a": 1, "b": 2, "has space": 3 });
        let before = old_file(&dir, &bad);
        let error = s.keys("demo", "s1").unwrap_err();
        assert!(error.contains("one file per key"), "{error}");
        assert!(s.set("demo", "s1", "c", json!(1)).is_err());
        assert_eq!(files(&dir), ["demo.s1.json"]);
        assert_eq!(fs::read(dir.join("demo.s1.json")).unwrap(), before);

        // A crash midway left a partial split behind: the next start splits again from the old file.
        old_file(&dir, &json!({ "a": 1, "b": 2 }));
        fs::create_dir_all(dir.join("demo.s1.split")).unwrap();
        fs::write(dir.join("demo.s1.split").join(file_name("a")), b"99").unwrap();
        let s = Storage::new(dir.clone());
        assert_eq!(s.entries("demo", "s1", "").unwrap(), [("a".to_string(), json!(1)), ("b".to_string(), json!(2))]);
        assert_eq!(files(&dir), ["demo.s1", "demo.s1.json.migrated"]);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_crash_after_the_split_keeps_the_split_and_retires_the_old_file() {
        let (s, dir) = storage();
        // The split folder was renamed into place and a write followed; the old file wasn't renamed yet.
        s.set("demo", "s1", "a", json!("newer")).unwrap();
        old_file(&dir, &json!({ "a": "older" }));
        let s = Storage::new(dir.clone());
        assert_eq!(s.get("demo", "s1", "a").unwrap(), Some(json!("newer")));
        assert_eq!(files(&dir), ["demo.s1", "demo.s1.json.migrated"]);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn concurrent_writes_from_several_frames_all_land() {
        let (s, dir) = storage();
        let s = Arc::new(s);
        let threads: Vec<_> = (0..8)
            .map(|t| {
                let s = s.clone();
                std::thread::spawn(move || {
                    for i in 0..25 {
                        s.set("demo", "s1", &format!("frame{t}-{i}"), json!(i)).unwrap();
                    }
                })
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }
        assert_eq!(s.keys("demo", "s1").unwrap().len(), 200);
        assert_eq!(Storage::new(dir.clone()).keys("demo", "s1").unwrap().len(), 200);
        let _ = fs::remove_dir_all(dir);
    }
}
