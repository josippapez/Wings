//! Each plugin's own key-value storage, for its settings and state. Plugin frames have an opaque origin,
//! so `localStorage` throws there. Values are JSON, kept in one file per plugin and install scope (the
//! scope its secrets use, see `plugin_store`), so a different package that later claims the same id
//! starts empty.

use std::{
    collections::{BTreeMap, HashMap},
    fs, io,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use serde_json::Value;

pub const MAX_KEY_LEN: usize = 128;
/// The stored JSON text: every key and value together.
pub const MAX_BYTES: usize = 1024 * 1024;

type Data = BTreeMap<String, Value>;

#[derive(Default)]
struct Bucket {
    /// Read from disk on first use, then written through on every change.
    data: Option<Data>,
    /// The scope was deleted. Scopes are never reused, so refusing it from now on keeps a write that was
    /// already on its way from bringing the file back.
    gone: bool,
}

pub struct Storage {
    root: PathBuf,
    /// One lock per plugin and scope: a plugin's main frame and sidebars can't lose each other's writes, and
    /// one plugin never waits on another's.
    buckets: Mutex<HashMap<String, Arc<Mutex<Bucket>>>>,
}

fn check_key(key: &str) -> Result<(), String> {
    let ok = !key.is_empty() && key.len() <= MAX_KEY_LEN && key.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.:/".contains(&b));
    ok.then_some(()).ok_or_else(|| format!("A storage key is 1 to {MAX_KEY_LEN} letters, digits and - _ . : /"))
}

fn load(path: &Path) -> Result<Data, String> {
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| format!("Couldn't read {}: {e}", path.display())),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Data::new()),
        Err(e) => Err(e.to_string()),
    }
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

impl Storage {
    pub fn new(root: PathBuf) -> Self {
        Self { root, buckets: Mutex::default() }
    }

    fn path(&self, plugin_id: &str, scope: &str) -> Result<PathBuf, String> {
        // Both come from Wings, not the plugin, but they name a file, so keep them to safe characters.
        let ok = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-');
        if !ok(plugin_id) || !ok(scope) {
            return Err(format!("no storage for {plugin_id}"));
        }
        Ok(self.root.join(format!("{plugin_id}.{scope}.json")))
    }

    fn bucket(&self, path: &Path) -> Arc<Mutex<Bucket>> {
        self.buckets.lock().unwrap().entry(path.to_string_lossy().into_owned()).or_default().clone()
    }

    /// Runs `f` on the plugin's data with its lock held.
    fn with<T>(&self, plugin_id: &str, scope: &str, f: impl FnOnce(&mut Data, &Path) -> Result<T, String>) -> Result<T, String> {
        let path = self.path(plugin_id, scope)?;
        let bucket = self.bucket(&path);
        let mut bucket = bucket.lock().unwrap();
        if bucket.gone {
            return Err(format!("{plugin_id}'s storage was deleted"));
        }
        if bucket.data.is_none() {
            bucket.data = Some(load(&path)?);
        }
        f(bucket.data.as_mut().expect("just loaded"), &path)
    }

    pub fn get(&self, plugin_id: &str, scope: &str, key: &str) -> Result<Option<Value>, String> {
        check_key(key)?;
        self.with(plugin_id, scope, |data, _| Ok(data.get(key).cloned()))
    }

    /// Every key, sorted.
    pub fn keys(&self, plugin_id: &str, scope: &str) -> Result<Vec<String>, String> {
        self.with(plugin_id, scope, |data, _| Ok(data.keys().cloned().collect()))
    }

    /// Refuses a write that would take the plugin's storage over `MAX_BYTES`, and changes nothing then.
    pub fn set(&self, plugin_id: &str, scope: &str, key: &str, value: Value) -> Result<(), String> {
        check_key(key)?;
        self.with(plugin_id, scope, |data, path| {
            let mut next = data.clone();
            next.insert(key.to_string(), value);
            let bytes = serde_json::to_vec(&next).map_err(|e| e.to_string())?;
            if bytes.len() > MAX_BYTES {
                return Err(format!(
                    "{plugin_id}'s storage is limited to 1 MB, and this would make it {} KB. Delete what it no longer needs first.",
                    bytes.len().div_ceil(1024)
                ));
            }
            save(path, &bytes)?;
            *data = next;
            Ok(())
        })
    }

    pub fn delete(&self, plugin_id: &str, scope: &str, key: &str) -> Result<(), String> {
        check_key(key)?;
        self.with(plugin_id, scope, |data, path| {
            if !data.contains_key(key) {
                return Ok(());
            }
            let mut next = data.clone();
            next.remove(key);
            save(path, &serde_json::to_vec(&next).map_err(|e| e.to_string())?)?;
            *data = next;
            Ok(())
        })
    }

    /// Deletes everything the plugin stored in this scope, when it's removed or replaced by a plugin from
    /// somewhere else. The scope is refused from then on.
    pub fn remove(&self, plugin_id: &str, scope: &str) -> Result<(), String> {
        let path = self.path(plugin_id, scope)?;
        let bucket = self.bucket(&path);
        let mut bucket = bucket.lock().unwrap();
        bucket.gone = true;
        bucket.data = None;
        let _ = fs::remove_file(tmp_path(&path));
        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn storage() -> (Storage, PathBuf) {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("wings-storage-{}-{nanos}", std::process::id()));
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

        s.delete("demo", "s1", "count").unwrap();
        s.delete("demo", "s1", "never-set").unwrap();
        assert_eq!(s.get("demo", "s1", "count").unwrap(), None);
        assert_eq!(s.keys("demo", "s1").unwrap(), ["history:2026-10", "settings"]);

        // It's on disk, so a restart reads it back.
        let again = Storage::new(dir.clone());
        assert_eq!(again.get("demo", "s1", "settings").unwrap(), Some(settings));
        assert_eq!(again.keys("demo", "s1").unwrap(), ["history:2026-10", "settings"]);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn refuses_bad_keys_and_writes_over_the_size_limit() {
        let (s, dir) = storage();
        for key in ["", "has space", "ünïcode", "new\nline", "quote\"", &"k".repeat(MAX_KEY_LEN + 1)] {
            assert!(s.set("demo", "s1", key, json!(1)).unwrap_err().contains("storage key"), "{key:?}");
            assert!(s.get("demo", "s1", key).is_err() && s.delete("demo", "s1", key).is_err(), "{key:?}");
        }
        s.set("demo", "s1", &"k".repeat(MAX_KEY_LEN), json!(1)).unwrap();
        s.set("demo", "s1", "a-Z_0.9:/x", json!(1)).unwrap();

        // Just under the limit fits; one write that goes over is refused and changes nothing.
        let used = serde_json::to_vec(&s.with("demo", "s1", |d, _| Ok(d.clone())).unwrap()).unwrap().len();
        let room = MAX_BYTES - used - r#","big":"""#.len();
        s.set("demo", "s1", "big", json!("x".repeat(room))).unwrap();
        let path = dir.join("demo.s1.json");
        assert_eq!(fs::metadata(&path).unwrap().len() as usize, MAX_BYTES);
        let error = s.set("demo", "s1", "more", json!("y")).unwrap_err();
        assert!(error.contains("limited to 1 MB") && error.contains("1025 KB"), "{error}");
        assert_eq!(s.get("demo", "s1", "more").unwrap(), None);
        assert_eq!(Storage::new(dir.clone()).keys("demo", "s1").unwrap().len(), 3);
        // Making room works.
        s.delete("demo", "s1", "big").unwrap();
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
        // Ids and scopes name the file, so they can't point anywhere else.
        assert!(s.get("../demo", "s1", "k").is_err() && s.get("demo", "s1/../s2", "k").is_err() && s.get("demo", "", "k").is_err());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn removing_a_scope_deletes_its_file_and_refuses_later_writes() {
        let (s, dir) = storage();
        s.set("demo", "s1", "k", json!(1)).unwrap();
        s.set("demo", "s2", "k", json!(2)).unwrap();
        s.remove("demo", "s1").unwrap();
        assert_eq!(files(&dir), ["demo.s2.json"]);
        // A write still on its way from the removed plugin doesn't bring the file back.
        assert!(s.set("demo", "s1", "k", json!(1)).unwrap_err().contains("deleted"));
        assert_eq!(files(&dir), ["demo.s2.json"]);
        // Removing something that was never stored is fine.
        s.remove("never", "s9").unwrap();
        assert_eq!(s.get("demo", "s2", "k").unwrap(), Some(json!(2)));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_failed_write_leaves_the_old_file_whole_and_no_partial_file() {
        let (s, dir) = storage();
        s.set("demo", "s1", "k", json!("old")).unwrap();
        s.set("demo", "s1", "k2", json!("also")).unwrap();
        assert_eq!(files(&dir), ["demo.s1.json"]);

        // A folder where the temp file goes makes the write fail before the rename.
        let tmp = dir.join("demo.s1.json.tmp");
        fs::create_dir(&tmp).unwrap();
        assert!(s.set("demo", "s1", "k", json!("new")).is_err());
        assert!(s.delete("demo", "s1", "k2").is_err());
        assert_eq!(s.get("demo", "s1", "k").unwrap(), Some(json!("old")));
        assert_eq!(s.get("demo", "s1", "k2").unwrap(), Some(json!("also")));
        let on_disk: Value = serde_json::from_slice(&fs::read(dir.join("demo.s1.json")).unwrap()).unwrap();
        assert_eq!(on_disk, json!({ "k": "old", "k2": "also" }));

        fs::remove_dir(&tmp).unwrap();
        s.set("demo", "s1", "k", json!("new")).unwrap();
        assert_eq!(files(&dir), ["demo.s1.json"]);
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
