//! What takes the space on a disk, for plugins with `permissions.diskUsage`. A scan reads each folder, sizes the
//! files in it and walks its subfolders on a pool of threads, then folds them into a tree that keeps only what is big
//! enough to draw. Sizes are what the files take on disk, like `du`.

use std::{
    ffi::OsString,
    fs::{self, Metadata},
    io,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc, Arc, Mutex,
    },
    time::{Duration, Instant},
};

#[cfg(unix)]
use std::{collections::HashSet, os::unix::fs::MetadataExt};

use rayon::prelude::*;
use serde::Serialize;

/// The deepest a scan keeps entries, and the most it returns.
pub const MAX_DEPTH: u32 = 64;
pub const MAX_ENTRIES: usize = 100_000;
/// The most paths `find` returns.
pub const MAX_FOUND: usize = 10_000;
/// Measured on /Applications on an SSD: 4, 8 and 16 threads take about the same, and one takes three times as long.
const MAX_THREADS: usize = 8;
const REPORT_EVERY: Duration = Duration::from_millis(200);
const STOPPED: &str = "The scan was stopped";

/// What a scan keeps: entries of at least `min_bytes`, down `depth` levels, and no more than `max_entries` of them.
#[derive(Debug, Clone, Copy)]
pub struct Options {
    pub min_bytes: u64,
    pub depth: u32,
    pub max_entries: usize,
}

impl Default for Options {
    fn default() -> Self {
        Self { min_bytes: 1 << 20, depth: MAX_DEPTH, max_entries: 20_000 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Dir,
    File,
    /// A symbolic link, never followed.
    Link,
    /// A socket, a device and the like.
    Other,
}

/// The entries of a folder that were too small or too deep to keep, together.
#[derive(Debug, Default, Clone, Copy, Serialize)]
pub struct Smaller {
    pub size: u64,
    pub items: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    /// The scan's root has its full path instead.
    pub name: String,
    /// Bytes on disk. A folder's has everything in it.
    pub size: u64,
    pub kind: Kind,
    /// A folder's files and folders at any depth, kept or folded.
    pub items: u64,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<Entry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub smaller: Option<Smaller>,
    /// A folder that couldn't be opened: its size is its own, and what is in it is unknown.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub restricted: bool,
}

impl Entry {
    fn new(kind: Kind, size: u64) -> Self {
        Self { name: String::new(), size, kind, items: 0, children: Vec::new(), smaller: None, restricted: false }
    }

    /// Counts a child that isn't kept: `items` of it, and all of them take `size`.
    fn fold(&mut self, size: u64, items: u64) {
        let smaller = self.smaller.get_or_insert_with(Smaller::default);
        smaller.size += size;
        smaller.items += items;
    }
}

#[derive(Debug, Serialize)]
pub struct Scan {
    pub root: Entry,
    /// The least size an entry needed to be kept: `min_bytes`, or more when `max_entries` made it rise.
    pub floor: u64,
    /// Files and folders scanned, the root included.
    pub entries: u64,
    /// Folders that couldn't be opened.
    pub restricted: u64,
    pub ms: u64,
}

/// What `find` came back with.
#[derive(Debug, Serialize)]
pub struct Found {
    /// Full paths, sorted. With `MAX_FOUND` of them the search stopped there, and there may be more.
    pub paths: Vec<String>,
    /// Files and folders looked at, the root included.
    pub entries: u64,
    /// Folders that couldn't be opened.
    pub restricted: u64,
    pub ms: u64,
}

/// How far a scan has got.
#[derive(Debug, Serialize)]
pub struct Progress {
    pub entries: u64,
    pub bytes: u64,
    /// A folder it is reading.
    pub path: String,
}

/// What every thread of one scan shares.
struct Walk<'a> {
    floor: u64,
    depth: u32,
    cancel: &'a AtomicBool,
    /// The volume the scan stays on.
    volume: u64,
    /// A folder the scan leaves out, see `skipped`.
    skip: Option<PathBuf>,
    /// Files with several hard links that were already counted, by volume and inode.
    #[cfg(unix)]
    linked: Mutex<HashSet<(u64, u64)>>,
    pulse: Pulse,
}

/// What a walk tells `progress` while it runs.
struct Pulse {
    entries: AtomicU64,
    bytes: AtomicU64,
    restricted: AtomicU64,
    /// Set by the reporter when it wants a fresh `path`: the next folder a thread opens gives it, so the walk
    /// pays nothing per folder in between.
    want_path: AtomicBool,
    path: Mutex<String>,
}

impl Pulse {
    /// `entries` and `bytes` are what the root counts for.
    fn new(entries: u64, bytes: u64) -> Self {
        Self {
            entries: AtomicU64::new(entries),
            bytes: AtomicU64::new(bytes),
            restricted: AtomicU64::new(0),
            want_path: AtomicBool::new(true),
            path: Mutex::new(String::new()),
        }
    }

    fn note(&self, dir: &Path) {
        if self.want_path.load(Ordering::Relaxed) && self.want_path.swap(false, Ordering::Relaxed) {
            if let Ok(mut path) = self.path.try_lock() {
                *path = dir.to_string_lossy().into_owned();
            }
        }
    }

    fn snapshot(&self) -> Progress {
        Progress {
            entries: self.entries.load(Ordering::Relaxed),
            bytes: self.bytes.load(Ordering::Relaxed),
            path: self.path.lock().unwrap().clone(),
        }
    }
}

impl<'a> Walk<'a> {
    fn new(options: &Options, cancel: &'a AtomicBool, root: &Path, meta: &Metadata) -> Self {
        Self {
            floor: options.min_bytes,
            depth: options.depth,
            cancel,
            volume: volume(meta),
            skip: skipped(root),
            #[cfg(unix)]
            linked: Mutex::new(HashSet::new()),
            pulse: Pulse::new(1, allocated(meta)),
        }
    }

    /// Everything under `dir`, whose own blocks are `own` and which is `depth` levels under the root. A folder
    /// that can't be opened comes back restricted. The caller fills in the name.
    fn folder(&self, dir: &Path, depth: u32, own: u64) -> Entry {
        let mut out = Entry::new(Kind::Dir, own);
        if self.cancel.load(Ordering::Relaxed) {
            return out;
        }
        let Ok(read) = fs::read_dir(dir) else {
            self.pulse.restricted.fetch_add(1, Ordering::Relaxed);
            out.restricted = true;
            return out;
        };
        self.pulse.note(dir);
        // Its entries are one level deeper.
        let keep = depth < self.depth;
        let mut subdirs: Vec<(PathBuf, u64)> = Vec::new();
        let (mut seen, mut bytes) = (0, 0);
        for (n, entry) in read.flatten().enumerate() {
            // One folder can hold tens of thousands of entries, and a stop shouldn't wait for all of them.
            if n % 1024 == 1023 && self.cancel.load(Ordering::Relaxed) {
                return out;
            }
            let Ok(meta) = entry.metadata() else { continue };
            let kind = kind_of(&meta);
            if kind == Kind::Dir {
                if volume(&meta) != self.volume {
                    continue;
                }
                let path = entry.path();
                if self.skip.as_deref() == Some(path.as_path()) {
                    continue;
                }
                let own = allocated(&meta);
                (seen, bytes) = (seen + 1, bytes + own);
                subdirs.push((path, own));
                continue;
            }
            let size = self.counted(&meta);
            (seen, bytes) = (seen + 1, bytes + size);
            out.size += size;
            out.items += 1;
            if keep && size >= self.floor {
                let name = entry.file_name().to_string_lossy().into_owned();
                out.children.push(Entry { name, items: 0, ..Entry::new(kind, size) });
            } else {
                out.fold(size, 1);
            }
        }
        self.pulse.entries.fetch_add(seen, Ordering::Relaxed);
        self.pulse.bytes.fetch_add(bytes, Ordering::Relaxed);
        let kids: Vec<Entry> = subdirs.par_iter().map(|(path, own)| self.folder(path, depth + 1, *own)).collect();
        for ((path, _), mut kid) in subdirs.iter().zip(kids) {
            out.size += kid.size;
            out.items += kid.items + 1;
            if keep && (kid.size >= self.floor || kid.restricted) {
                kid.name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                out.children.push(kid);
            } else {
                out.fold(kid.size, kid.items + 1);
            }
        }
        out.children.sort_unstable_by(|a, b| b.size.cmp(&a.size).then_with(|| a.name.cmp(&b.name)));
        out
    }

    /// What a file adds to the total: a file with several hard links only the first time one of them is seen.
    #[cfg(unix)]
    fn counted(&self, meta: &Metadata) -> u64 {
        if meta.nlink() > 1 && !self.linked.lock().unwrap().insert((meta.dev(), meta.ino())) {
            return 0;
        }
        allocated(meta)
    }

    #[cfg(not(unix))]
    fn counted(&self, meta: &Metadata) -> u64 {
        allocated(meta)
    }

}

/// Bytes a file takes on disk, like `du`: its allocated blocks.
#[cfg(unix)]
fn allocated(meta: &Metadata) -> u64 {
    meta.blocks() * 512
}

#[cfg(not(unix))]
fn allocated(meta: &Metadata) -> u64 {
    meta.len()
}

#[cfg(unix)]
fn volume(meta: &Metadata) -> u64 {
    meta.dev()
}

/// Elsewhere every folder counts as on the volume the scan started on.
#[cfg(not(unix))]
fn volume(_: &Metadata) -> u64 {
    0
}

fn kind_of(meta: &Metadata) -> Kind {
    let kind = meta.file_type();
    if kind.is_dir() {
        Kind::Dir
    } else if kind.is_file() {
        Kind::File
    } else if kind.is_symlink() {
        Kind::Link
    } else {
        Kind::Other
    }
}

/// On macOS `/System/Volumes/Data` is on the same volume as `/`, which already reaches everything in it through
/// firmlinks, so a scan of `/` that walked it would count every file twice.
#[cfg(target_os = "macos")]
fn skipped(root: &Path) -> Option<PathBuf> {
    (root == Path::new("/")).then(|| PathBuf::from("/System/Volumes"))
}

#[cfg(not(target_os = "macos"))]
fn skipped(_: &Path) -> Option<PathBuf> {
    None
}

/// `~` and `~/...` are the home folder, and anything else has to be absolute.
fn expand(path: &str) -> Result<PathBuf, String> {
    let home = || dirs::home_dir().ok_or_else(|| "Couldn't find your home folder".to_string());
    let full = match path.strip_prefix('~') {
        Some("") => home()?,
        Some(rest) if rest.starts_with('/') => home()?.join(rest.trim_start_matches('/')),
        _ => PathBuf::from(path),
    };
    if !full.is_absolute() {
        return Err(format!("{path} isn't an absolute path"));
    }
    // Without a trailing slash or a doubled one, so the root's name matches the paths of what is under it.
    Ok(full.components().collect())
}

fn threads() -> usize {
    std::thread::available_parallelism().map_or(4, |n| n.get()).min(MAX_THREADS)
}

/// The file or folder `path` names, with its metadata. A link given as the root, like `/tmp`, is the folder it points
/// to. Links inside are never followed.
fn resolve(path: &str) -> Result<(PathBuf, Metadata), String> {
    let asked = expand(path)?;
    let unreadable = |root: &Path, e: io::Error| match e.kind() {
        io::ErrorKind::NotFound | io::ErrorKind::NotADirectory => format!("There's no file or folder at {}", root.display()),
        _ => format!("Couldn't read {}: {e}", root.display()),
    };
    // The real path, with `..` and every link on the way resolved, and a link given as the root, like `/tmp`, followed:
    // a plugin acts on the paths a scan names, so they have to name what the system would act on. Links inside are
    // never followed.
    let root = fs::canonicalize(&asked).map_err(|e| unreadable(&asked, e))?;
    let meta = fs::symlink_metadata(&root).map_err(|e| unreadable(&root, e))?;
    Ok((root, meta))
}

/// Runs `work` on a pool of scan threads, and calls `progress` with what `pulse` says about five times a second
/// while it runs.
fn on_pool<T: Send>(pulse: &Pulse, progress: &(impl Fn(&Progress) + Sync), work: impl FnOnce() -> T + Send) -> Result<T, String> {
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(threads())
        .thread_name(|i| format!("disk-scan-{i}"))
        .build()
        .map_err(|e| format!("Couldn't start the scan: {e}"))?;
    let (done, finished) = mpsc::channel::<()>();
    Ok(std::thread::scope(|scope| {
        scope.spawn(move || loop {
            pulse.want_path.store(true, Ordering::Relaxed);
            if !matches!(finished.recv_timeout(REPORT_EVERY), Err(mpsc::RecvTimeoutError::Timeout)) {
                break;
            }
            progress(&pulse.snapshot());
        });
        let out = pool.install(work);
        // The reporter's wait ends as the channel closes.
        drop(done);
        out
    }))
}

/// Scans a file, folder or disk. Calls `progress` about five times a second while it runs, and stops with an error
/// once `cancel` is set. The scan stays on the volume `path` is on and never follows a link.
pub fn scan(path: &str, options: Options, cancel: &AtomicBool, progress: impl Fn(&Progress) + Sync) -> Result<Scan, String> {
    let start = Instant::now();
    let (root, meta) = resolve(path)?;
    let name = root.to_string_lossy().into_owned();
    if cancel.load(Ordering::Relaxed) {
        return Err(STOPPED.into());
    }
    let walk = Walk::new(&options, cancel, &root, &meta);
    let mut tree = if meta.is_dir() {
        let own = allocated(&meta);
        on_pool(&walk.pulse, &progress, || walk.folder(&root, 0, own))?
    } else {
        Entry::new(kind_of(&meta), allocated(&meta))
    };
    if cancel.load(Ordering::Relaxed) {
        return Err(STOPPED.into());
    }
    tree.name = name;
    let floor = fit(&mut tree, options.min_bytes, options.max_entries);
    Ok(Scan {
        root: tree,
        floor,
        entries: walk.pulse.entries.load(Ordering::Relaxed),
        restricted: walk.pulse.restricted.load(Ordering::Relaxed),
        ms: u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX),
    })
}

/// What every thread of one search shares.
struct Seek<'a> {
    names: Vec<OsString>,
    /// Folders it doesn't open, by name, wherever they are.
    skip_names: Vec<OsString>,
    cancel: &'a AtomicBool,
    volume: u64,
    skip: Option<PathBuf>,
    pulse: Pulse,
    found: Mutex<Vec<String>>,
    /// How many paths it keeps.
    cap: usize,
    /// Set once `found` has `cap` paths, so the threads stop walking.
    full: AtomicBool,
}

impl Seek<'_> {
    /// Looks in `dir` for what is wanted, then in its subfolders. A match isn't opened, nor is a skipped name, a link
    /// or a folder on another volume. The listing says what an entry is, so only a folder costs a `stat`.
    fn folder(&self, dir: &Path) {
        if self.cancel.load(Ordering::Relaxed) || self.full.load(Ordering::Relaxed) {
            return;
        }
        let Ok(read) = fs::read_dir(dir) else {
            self.pulse.restricted.fetch_add(1, Ordering::Relaxed);
            return;
        };
        self.pulse.note(dir);
        let mut subdirs: Vec<PathBuf> = Vec::new();
        let mut seen = 0;
        for (n, entry) in read.flatten().enumerate() {
            // One folder can hold tens of thousands of entries, and a stop shouldn't wait for all of them.
            if n % 1024 == 1023 && self.cancel.load(Ordering::Relaxed) {
                return;
            }
            let name = entry.file_name();
            if self.names.contains(&name) {
                seen += 1;
                self.keep(entry.path());
                continue;
            }
            let Ok(kind) = entry.file_type() else { continue };
            if !kind.is_dir() || self.skip_names.contains(&name) {
                seen += 1;
                continue;
            }
            let Ok(meta) = entry.metadata() else { continue };
            if volume(&meta) != self.volume {
                continue;
            }
            let path = entry.path();
            if self.skip.as_deref() == Some(path.as_path()) {
                continue;
            }
            seen += 1;
            subdirs.push(path);
        }
        self.pulse.entries.fetch_add(seen, Ordering::Relaxed);
        subdirs.par_iter().for_each(|path| self.folder(path));
    }

    fn keep(&self, path: PathBuf) {
        let mut found = self.found.lock().unwrap();
        if found.len() < self.cap {
            found.push(path.to_string_lossy().into_owned());
        }
        if found.len() >= self.cap {
            self.full.store(true, Ordering::Relaxed);
        }
    }
}

/// Looks under a folder or a disk for files, folders and links named exactly one of `names`, and returns their full
/// paths, sorted. It never opens a match, a folder named in `skip`, a link or a folder on another volume, and reads only
/// the folders' listings, with no `stat` for a file. The root's own name isn't tested. At most `MAX_FOUND` paths come
/// back: when that many are found it stops, so there may be more. Roots, progress and stopping are as in `scan`;
/// `Progress.bytes` stays 0.
pub fn find(path: &str, names: &[String], skip: &[String], cancel: &AtomicBool, progress: impl Fn(&Progress) + Sync) -> Result<Found, String> {
    search(path, names, skip, MAX_FOUND, cancel, progress)
}

fn search(path: &str, names: &[String], skip: &[String], cap: usize, cancel: &AtomicBool, progress: impl Fn(&Progress) + Sync) -> Result<Found, String> {
    let start = Instant::now();
    let (root, meta) = resolve(path)?;
    if cancel.load(Ordering::Relaxed) {
        return Err(STOPPED.into());
    }
    let seek = Seek {
        names: names.iter().map(OsString::from).collect(),
        skip_names: skip.iter().map(OsString::from).collect(),
        cancel,
        volume: volume(&meta),
        skip: skipped(&root),
        pulse: Pulse::new(1, 0),
        found: Mutex::new(Vec::new()),
        cap,
        full: AtomicBool::new(false),
    };
    if meta.is_dir() {
        on_pool(&seek.pulse, &progress, || seek.folder(&root))?;
    }
    if cancel.load(Ordering::Relaxed) {
        return Err(STOPPED.into());
    }
    let mut paths = seek.found.into_inner().unwrap();
    paths.sort_unstable();
    Ok(Found {
        paths,
        entries: seek.pulse.entries.load(Ordering::Relaxed),
        restricted: seek.pulse.restricted.load(Ordering::Relaxed),
        ms: u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX),
    })
}

/// Keeps at most `max` entries under the root: when more passed `floor`, it rises to just above the size of the
/// first one that doesn't fit, and everything under that goes into its folder's `smaller`. A child is never bigger
/// than its folder, so no kept entry is left under one that went. Returns the floor.
fn fit(root: &mut Entry, floor: u64, max: usize) -> u64 {
    let mut sizes = Vec::new();
    sizes_under(root, &mut sizes);
    if sizes.len() <= max {
        return floor;
    }
    let (_, cutoff, _) = sizes.select_nth_unstable_by(max, |a, b| b.cmp(a));
    let raised = *cutoff + 1;
    prune(root, raised);
    floor.max(raised)
}

fn sizes_under(entry: &Entry, sizes: &mut Vec<u64>) {
    for child in &entry.children {
        sizes.push(child.size);
        sizes_under(child, sizes);
    }
}

fn prune(entry: &mut Entry, floor: u64) {
    let Entry { children, smaller, .. } = entry;
    children.retain_mut(|child| {
        if child.size >= floor {
            prune(child, floor);
            return true;
        }
        let smaller = smaller.get_or_insert_with(Smaller::default);
        smaller.size += child.size;
        smaller.items += child.items + 1;
        false
    });
}

/// The scans running, by `<plugin id>:<token>`, so a plugin can stop one it started.
static RUNNING: Mutex<Vec<(String, Arc<AtomicBool>)>> = Mutex::new(Vec::new());

/// A scan's stop flag, listed in `RUNNING` until this drops.
pub struct Running {
    key: String,
    flag: Arc<AtomicBool>,
}

impl Running {
    pub fn new(key: String) -> Self {
        let flag = Arc::new(AtomicBool::new(false));
        RUNNING.lock().unwrap().push((key.clone(), flag.clone()));
        Self { key, flag }
    }

    pub fn flag(&self) -> &AtomicBool {
        &self.flag
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        RUNNING.lock().unwrap().retain(|(key, flag)| !(*key == self.key && Arc::ptr_eq(flag, &self.flag)));
    }
}

/// Stops the scans listed under `key`. A key nothing is listed under is fine: the scan may have just finished.
pub fn stop(key: &str) {
    for (_, flag) in RUNNING.lock().unwrap().iter().filter(|(k, _)| k == key) {
        flag.store(true, Ordering::Relaxed);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Medium {
    Ssd,
    Hdd,
    Unknown,
}

/// A mounted disk.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Disk {
    pub name: String,
    pub mount: String,
    pub fs: String,
    pub kind: Medium,
    pub removable: bool,
    pub read_only: bool,
    pub total: u64,
    /// Bytes free. On macOS that counts what the system can purge as free, like Finder.
    pub free: u64,
}

/// The volumes macOS keeps under `/System/Volumes` aren't disks you'd see in Finder. Its startup disk shows twice:
/// as `/` and as `/System/Volumes/Data`, with the same sizes.
fn is_internal(mount: &Path) -> bool {
    mount.starts_with("/System/Volumes")
}

/// The disks that are mounted, the startup disk first.
pub fn disks() -> Vec<Disk> {
    let mut found: Vec<Disk> = sysinfo::Disks::new_with_refreshed_list()
        .list()
        .iter()
        .filter(|disk| !is_internal(disk.mount_point()))
        .map(|disk| Disk {
            name: disk.name().to_string_lossy().into_owned(),
            mount: disk.mount_point().to_string_lossy().into_owned(),
            fs: disk.file_system().to_string_lossy().into_owned(),
            kind: match disk.kind() {
                sysinfo::DiskKind::SSD => Medium::Ssd,
                sysinfo::DiskKind::HDD => Medium::Hdd,
                sysinfo::DiskKind::Unknown(_) => Medium::Unknown,
            },
            removable: disk.is_removable(),
            read_only: disk.is_read_only(),
            total: disk.total_space(),
            free: disk.available_space(),
        })
        .collect();
    // Stable, so the rest stay in the order sysinfo lists them.
    found.sort_by_key(|disk| disk.mount != "/");
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A folder of its own in the temp dir, removed when it drops.
    struct Tree(PathBuf);

    impl Tree {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("wings-disk-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            // A scan names its root by its real path, and macOS's temp folder is under `/var`, a link to `/private/var`.
            Self(fs::canonicalize(&dir).unwrap())
        }

        /// A file of `kib` KiB at `path` under the tree, with the folders above it.
        fn file(&self, path: &str, kib: usize) -> PathBuf {
            let full = self.0.join(path);
            fs::create_dir_all(full.parent().unwrap()).unwrap();
            fs::write(&full, vec![7u8; kib * 1024]).unwrap();
            full
        }

        fn scan(&self, options: Options) -> Scan {
            scan(self.0.to_str().unwrap(), options, &AtomicBool::new(false), |_| {}).unwrap()
        }
    }

    impl Drop for Tree {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn keep_all() -> Options {
        Options { min_bytes: 0, ..Options::default() }
    }

    /// What `path` itself takes on disk.
    fn own(path: &Path) -> u64 {
        allocated(&fs::symlink_metadata(path).unwrap())
    }

    /// What `path` and everything under it take on disk, and how many files and folders are under it, counted the
    /// plain way: one `lstat` per entry, no threads.
    #[cfg(unix)]
    fn on_disk(path: &Path) -> (u64, u64) {
        let (mut bytes, mut items) = (own(path), 0);
        if fs::symlink_metadata(path).unwrap().is_dir() {
            for entry in fs::read_dir(path).unwrap() {
                let (b, i) = on_disk(&entry.unwrap().path());
                bytes += b;
                items += i + 1;
            }
        }
        (bytes, items)
    }

    #[cfg(unix)]
    #[test]
    fn a_folders_size_is_the_blocks_of_everything_in_it() {
        let tree = Tree::new("size");
        tree.file("a.bin", 300);
        tree.file("sub/b.bin", 10);
        tree.file("sub/deep/c.bin", 5);
        fs::create_dir(tree.0.join("empty")).unwrap();
        std::os::unix::fs::symlink(tree.0.join("sub"), tree.0.join("link")).unwrap();
        let scan = tree.scan(keep_all());
        let (bytes, items) = on_disk(&tree.0);
        assert_eq!((scan.root.size, scan.root.items), (bytes, items));
        assert_eq!(items, 7, "3 files, sub, deep, empty and the link");
        assert_eq!(scan.entries, items + 1);
        assert_eq!(scan.root.name, tree.0.to_str().unwrap());
        let a = tree.0.join("a.bin");
        assert_eq!(own(&a), fs::symlink_metadata(&a).unwrap().blocks() * 512);
        let link = scan.root.children.iter().find(|c| c.name == "link").unwrap();
        assert_eq!((link.kind, link.items, link.children.len()), (Kind::Link, 0, 0), "a link isn't followed");
    }

    #[test]
    fn entries_under_the_floor_fold_into_smaller() {
        let tree = Tree::new("floor");
        tree.file("big.bin", 300);
        tree.file("small.bin", 8);
        tree.file("tiny/inner.bin", 4);
        let scan = tree.scan(Options { min_bytes: 100 * 1024, ..Options::default() });
        assert_eq!(scan.floor, 100 * 1024);
        let names: Vec<_> = scan.root.children.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["big.bin"]);
        // small.bin, tiny and the file in it.
        let smaller = scan.root.smaller.unwrap();
        assert_eq!(smaller.items, 3);
        assert!(smaller.size >= 12 * 1024);
        assert_eq!(scan.root.items, 4);
    }

    #[test]
    fn depth_one_keeps_the_roots_entries_and_folds_what_is_under_them() {
        let tree = Tree::new("depth");
        tree.file("top.bin", 200);
        tree.file("one/inner.bin", 200);
        tree.file("one/two/deeper.bin", 200);
        let scan = tree.scan(Options { depth: 1, ..keep_all() });
        assert_eq!(scan.root.children.len(), 2);
        let one = scan.root.children.iter().find(|c| c.name == "one").unwrap();
        assert!(one.children.is_empty());
        let smaller = one.smaller.unwrap();
        assert_eq!((smaller.items, one.items), (3, 3), "inner.bin, two and deeper.bin");
        assert_eq!(one.size, own(&tree.0.join("one")) + smaller.size);
        assert!(scan.root.smaller.is_none());
    }

    #[cfg(unix)]
    #[test]
    fn a_file_with_two_hard_links_counts_once() {
        let tree = Tree::new("links");
        let first = tree.file("one.bin", 200);
        fs::hard_link(&first, tree.0.join("two.bin")).unwrap();
        let scan = tree.scan(keep_all());
        let file = own(&first);
        assert!(file >= 200 * 1024);
        assert_eq!(scan.root.size, own(&tree.0) + file);
        let mut sizes: Vec<u64> = scan.root.children.iter().map(|c| c.size).collect();
        sizes.sort_unstable();
        assert_eq!(sizes, [0, file], "the second link takes nothing but is still an item");
        assert_eq!(scan.root.items, 2);
    }

    #[cfg(unix)]
    #[test]
    fn a_folder_that_cant_be_opened_is_restricted() {
        use std::os::unix::fs::PermissionsExt;
        let tree = Tree::new("restricted");
        tree.file("open/a.bin", 8);
        tree.file("locked/b.bin", 8);
        let locked = tree.0.join("locked");
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
        // Root, and file systems that ignore modes, can still read it.
        let readable = fs::read_dir(&locked).is_ok();
        // A floor nothing reaches: a restricted folder is kept anyway.
        let scan = tree.scan(Options { min_bytes: 1 << 40, ..Options::default() });
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
        if readable {
            return;
        }
        assert_eq!(scan.restricted, 1);
        let names: Vec<_> = scan.root.children.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["locked"]);
        let entry = &scan.root.children[0];
        assert!(entry.restricted);
        assert_eq!((entry.items, entry.size), (0, own(&locked)));
        assert_eq!(scan.root.items, 3, "open, a.bin and locked: what is in locked is unknown");
    }

    #[cfg(unix)]
    #[test]
    fn a_link_given_as_the_root_scans_the_folder_it_points_to() {
        let tree = Tree::new("link-root");
        tree.file("real/a.bin", 200);
        let link = tree.0.join("link");
        std::os::unix::fs::symlink(tree.0.join("real"), &link).unwrap();
        let scan = scan(link.to_str().unwrap(), keep_all(), &AtomicBool::new(false), |_| {}).unwrap();
        assert_eq!(scan.root.kind, Kind::Dir);
        assert_eq!(scan.root.name, fs::canonicalize(tree.0.join("real")).unwrap().to_str().unwrap());
        assert_eq!(scan.root.children.len(), 1);
    }

    /// `link/..` is the folder above where the link points, not the folder the link sits in, as it reads.
    #[cfg(unix)]
    #[test]
    fn the_root_is_named_by_its_real_path_after_links_and_dot_dot() {
        let tree = Tree::new("real-path");
        tree.file("real/inner/a.bin", 8);
        tree.file("real/b.bin", 8);
        std::os::unix::fs::symlink(tree.0.join("real/inner"), tree.0.join("link")).unwrap();
        let path = format!("{}/link/..", tree.0.display());
        let scan = scan(&path, keep_all(), &AtomicBool::new(false), |_| {}).unwrap();
        assert_eq!(scan.root.name, fs::canonicalize(tree.0.join("real")).unwrap().to_str().unwrap());
        let mut names: Vec<_> = scan.root.children.iter().map(|c| c.name.as_str()).collect();
        names.sort_unstable();
        assert_eq!(names, ["b.bin", "inner"]);
    }

    #[test]
    fn a_stopped_scan_is_an_error() {
        let tree = Tree::new("cancel");
        tree.file("a.bin", 4);
        let stopped = AtomicBool::new(true);
        let result = scan(tree.0.to_str().unwrap(), Options::default(), &stopped, |_| {});
        assert_eq!(result.unwrap_err(), "The scan was stopped");
    }

    fn find_in(tree: &Tree, names: &[&str], skip: &[&str], cap: usize) -> Found {
        let strings = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        search(tree.0.to_str().unwrap(), &strings(names), &strings(skip), cap, &AtomicBool::new(false), |_| {}).unwrap()
    }

    #[test]
    fn find_returns_what_is_named_and_doesnt_open_a_match_or_a_skipped_folder() {
        let tree = Tree::new("find");
        tree.file("a/.git/HEAD", 0);
        tree.file("a/.git/modules/inner/.git/HEAD", 0);
        tree.file("a/src/main.rs", 1);
        // A worktree's `.git` is a file.
        tree.file("b/linked/.git", 0);
        tree.file("b/notes/marker.txt", 0);
        tree.file("c/deep/er/.git/HEAD", 0);
        tree.file("c/node_modules/pkg/.git/HEAD", 0);
        tree.file("c/.Trash/old/.git/HEAD", 0);
        #[cfg(unix)]
        std::os::unix::fs::symlink(tree.0.join("c"), tree.0.join("b/to-c")).unwrap();
        let at = |rest: &str| tree.0.join(rest).to_str().unwrap().to_string();

        let found = find_in(&tree, &[".git"], &["node_modules", ".Trash"], MAX_FOUND);
        assert_eq!(found.paths, [at("a/.git"), at("b/linked/.git"), at("c/deep/er/.git")], "not the .git inside a .git, a skipped name or a link");
        // The root and what is listed in the folders it opened: a match or a skipped folder counts, what is in it doesn't.
        assert_eq!(found.restricted, 0);
        assert_eq!(found.entries, if cfg!(unix) { 17 } else { 16 });

        let unskipped = find_in(&tree, &[".git"], &[], MAX_FOUND);
        assert_eq!(unskipped.paths.len(), 5);
        assert!(unskipped.paths.contains(&at("c/node_modules/pkg/.git")) && unskipped.paths.contains(&at("c/.Trash/old/.git")));

        // Several names, a file among them.
        let both = find_in(&tree, &[".git", "marker.txt"], &["node_modules", ".Trash"], MAX_FOUND);
        assert_eq!(both.paths.len(), 4);
        assert!(both.paths.contains(&at("b/notes/marker.txt")));
    }

    #[test]
    fn find_stops_at_the_cap_and_when_stopped() {
        let tree = Tree::new("find-cap");
        for n in 0..5 {
            tree.file(&format!("r{n}/.git/HEAD"), 0);
        }
        let capped = find_in(&tree, &[".git"], &[], 3);
        assert_eq!(capped.paths.len(), 3);
        assert!(capped.paths.windows(2).all(|w| w[0] < w[1]), "sorted");
        assert_eq!(find_in(&tree, &[".git"], &[], 5).paths.len(), 5);

        let stopped = AtomicBool::new(true);
        let result = find(tree.0.to_str().unwrap(), &[".git".into()], &[], &stopped, |_| {});
        assert_eq!(result.unwrap_err(), "The scan was stopped");
    }

    #[test]
    fn over_the_budget_the_floor_rises_and_the_biggest_stay() {
        let tree = Tree::new("budget");
        for (name, kib) in [("a.bin", 400), ("b.bin", 300), ("c.bin", 200), ("d.bin", 100)] {
            tree.file(name, kib);
        }
        let scan = tree.scan(Options { max_entries: 2, ..keep_all() });
        let names: Vec<_> = scan.root.children.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["a.bin", "b.bin"]);
        let (c, d) = (own(&tree.0.join("c.bin")), own(&tree.0.join("d.bin")));
        assert_eq!(scan.floor, c + 1, "just above the biggest entry that didn't fit");
        let smaller = scan.root.smaller.unwrap();
        assert_eq!((smaller.items, smaller.size), (2, c + d));
        assert_eq!(scan.root.items, 4);
    }

    #[test]
    fn the_volumes_macos_keeps_inside_system_are_not_disks() {
        assert!(is_internal(Path::new("/System/Volumes/Data")));
        assert!(is_internal(Path::new("/System/Volumes/VM")));
        for mount in ["/", "/Volumes/Backup", "/System", "/System/Library", "/SystemVolumes/Data", "/Users/me/System/Volumes/Data"] {
            assert!(!is_internal(Path::new(mount)), "{mount}");
        }
    }
}
