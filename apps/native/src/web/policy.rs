//! Which files a plugin's page may name in a web view command (`upload`, `screenshotDiff`, `record`), by the rules of the
//! browse MCP (`browse-capture/src/files.rs`, `browse-page/src/files.rs`): paths stay inside the file roots, no symlinks on
//! the way, every folder and file below a root owned by this user and not group/world writable, no hard-linked files,
//! nothing written straight into a shared temporary folder, folders made 0700 and files written 0600 through a
//! temporary file renamed into place.
//!
//! The roots are the app's to decide, never a command's or a plugin's: the temporary folders and the ones
//! `WINGS_BROWSER_FILE_ROOTS` (or browse's `OPENCODE_BROWSER_FILE_ROOTS`) names (`Files::of`, the one place to add more). The plugin's own web folder is a root only for the `screenshots`, `recordings` and
//! `uploads` folders the app keeps in it: where a relative path goes, and where a picture Wings took, or a file passed
//! inline, is written.

use std::{
    io::{Read, Write},
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Component, Path, PathBuf},
};

use base64::{Engine, engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig}};

/// The variables that add roots, Wings' own first.
pub const ENV: [&str; 2] = ["WINGS_BROWSER_FILE_ROOTS", "OPENCODE_BROWSER_FILE_ROOTS"];

/// The most bytes of files an `upload` puts in a page by their paths, together.
pub const UPLOAD_MAX_BYTES: u64 = 50 * 1024 * 1024;

/// The most bytes of files an `upload` passes inline, decoded, together.
pub const INLINE_MAX_BYTES: u64 = 10 * 1024 * 1024;

/// What a refusal tells about the way out.
const ADD: &str = "add folders with WINGS_BROWSER_FILE_ROOTS";

/// The folders of the plugin's web folder the app keeps files in.
pub const SCREENSHOTS: &str = "screenshots";
pub const RECORDINGS: &str = "recordings";
const UPLOADS: &str = "uploads";

/// The file roots of one plugin profile, resolved.
pub struct Files {
    listed: Vec<PathBuf>,
    /// What paths are checked against: `listed` and the app's folders in the web folder.
    roots: Vec<PathBuf>,
    web: PathBuf,
}

fn uid() -> u32 {
    // SAFETY: getuid has no preconditions and cannot fail.
    unsafe { libc::getuid() }
}

/// Whether another user owns it or others can write it.
fn shared(meta: &std::fs::Metadata) -> bool {
    meta.uid() != uid() || meta.mode() & 0o022 != 0
}

fn lstat(path: &Path) -> Result<Option<std::fs::Metadata>, String> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) => Ok(Some(meta)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("{}: {error}", path.display())),
    }
}

/// `path` lexically normalized, refusing a `..` that climbs above the filesystem root.
fn normalize(path: &Path) -> Result<PathBuf, String> {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                if !normalized.pop() {
                    return Err(format!("{} climbs above the filesystem root", path.display()));
                }
            }
            Component::CurDir => {}
            other => normalized.push(other),
        }
    }
    Ok(normalized)
}

/// The real location `path` refers to: its nearest existing ancestor resolved, plus the parts that don't exist yet.
fn real_target(path: &Path) -> PathBuf {
    let mut current = path.to_path_buf();
    let mut rest = Vec::new();
    loop {
        if let Ok(real) = std::fs::canonicalize(&current) {
            return rest.iter().rev().fold(real, |real, part| real.join(part));
        }
        match (current.parent(), current.file_name()) {
            (Some(parent), Some(name)) => {
                rest.push(name.to_os_string());
                current = parent.to_path_buf();
            }
            _ => return path.to_path_buf(),
        }
    }
}

fn root_of<'a>(real: &Path, roots: &'a [PathBuf]) -> Option<&'a PathBuf> {
    roots.iter().filter(|root| real.starts_with(root)).max_by_key(|root| root.as_os_str().len())
}

/// A folder made 0700 if it isn't there, through its descriptor so a swapped link isn't followed.
fn private_folder(folder: &Path) -> Result<(), String> {
    if lstat(folder)?.is_none() {
        std::fs::DirBuilder::new().recursive(true).mode(0o700).create(folder).map_err(|error| format!("{}: {error}", folder.display()))?;
    }
    let file = std::fs::OpenOptions::new().read(true).custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW).open(folder).map_err(|_| refused_folder(folder))?;
    let (opened, current) = (file.metadata().map_err(|error| error.to_string())?, std::fs::symlink_metadata(folder).map_err(|error| error.to_string())?);
    if !opened.is_dir() || opened.uid() != uid() || current.file_type().is_symlink() || (current.dev(), current.ino()) != (opened.dev(), opened.ino()) {
        return Err(refused_folder(folder));
    }
    Ok(())
}

fn refused_folder(folder: &Path) -> String {
    format!("file outputs must use real, owned folders without group/world write access, not {}", folder.display())
}

/// The roots `WINGS_BROWSER_FILE_ROOTS` and `OPENCODE_BROWSER_FILE_ROOTS` name: absolute entries only.
fn from_env() -> Vec<PathBuf> {
    ENV.iter().filter_map(std::env::var_os).flat_map(|list| std::env::split_paths(&list).collect::<Vec<_>>()).filter(|path| path.is_absolute()).collect()
}

impl Files {
    /// The roots of the profile whose web folder is `web`: the temporary folders and the environment's.
    pub fn of(web: &Path) -> Self {
        let mut listed: Vec<PathBuf> = Vec::new();
        for path in [std::env::temp_dir(), PathBuf::from("/tmp")].into_iter().chain(from_env()) {
            if let Ok(real) = std::fs::canonicalize(&path)
                && !listed.contains(&real)
            {
                listed.push(real);
            }
        }
        let mut roots = listed.clone();
        // The app's folders in the web folder, made as they are first needed (`internal`).
        let web = real_target(web);
        roots.extend([SCREENSHOTS, RECORDINGS, UPLOADS].map(|name| web.join(name)));
        Self { listed, roots, web }
    }

    fn describe(&self) -> String {
        let list: Vec<String> = self.listed.iter().map(|root| root.display().to_string()).collect();
        format!("{} ({ADD})", list.join(", "))
    }

    /// The app's folder `name` in the web folder, made if it isn't there.
    fn internal(&self, name: &str) -> Result<PathBuf, String> {
        let folder = self.web.join(name);
        std::fs::create_dir_all(&self.web).map_err(|error| format!("{}: {error}", self.web.display()))?;
        private_folder(&folder)?;
        Ok(folder)
    }

    /// `raw` as an absolute, normalized path: a relative one goes into the web folder's `folder`.
    fn absolute(&self, raw: &str, folder: &str) -> Result<PathBuf, String> {
        if raw.is_empty() {
            return Err("file paths must be non-empty strings".into());
        }
        let raw = Path::new(raw);
        if raw.is_absolute() { normalize(raw) } else { normalize(&self.internal(folder)?.join(raw)) }
    }

    /// Port of assertExecuteFilePath: refuses `target` unless it is inside a root, reached without symlinks through owned
    /// private folders, and (when it exists) a regular file with one link or a folder. Its real location.
    fn check(&self, target: &Path) -> Result<PathBuf, String> {
        let resolved = real_target(target);
        let Some(root) = root_of(&resolved, &self.roots).cloned() else {
            return Err(format!("{} is outside the file roots {}", target.display(), self.describe()));
        };
        if let Some(meta) = lstat(&root)?
            && meta.mode() & 0o022 != 0
            && meta.mode() & 0o1000 == 0
        {
            return Err(format!("{} is writable by others and has no sticky bit, so it can't be a file root", root.display()));
        }
        // Symlinks in the path as written; only root-owned aliases of a root or a folder above one (macOS's /tmp and /var
        // links) are allowed.
        let mut spelling = target.to_path_buf();
        loop {
            if let Some(meta) = lstat(&spelling)?
                && meta.file_type().is_symlink()
            {
                let real = std::fs::canonicalize(&spelling).unwrap_or_default();
                if meta.uid() != 0 || !self.roots.iter().any(|root| root.starts_with(&real)) {
                    return Err("symlink paths are not allowed".into());
                }
            }
            if !spelling.pop() {
                break;
            }
        }
        let mut current = resolved.clone();
        while current.starts_with(&root) {
            if let Some(meta) = lstat(&current)? {
                if meta.file_type().is_symlink() {
                    return Err("symlink paths are not allowed".into());
                }
                if current != root && shared(&meta) {
                    return Err(format!("file paths must use folders and files you own without group/world write access ({} is not); {ADD}", current.display()));
                }
                if !meta.is_dir() && (!meta.is_file() || meta.nlink() != 1) {
                    return Err("expected a regular file with no hard links".into());
                }
            }
            if current == root || !current.pop() {
                break;
            }
        }
        if resolved.parent() == Some(root.as_path()) && std::fs::metadata(&root).is_ok_and(|meta| shared(&meta)) {
            return Err("use a subfolder of a shared temporary folder, not the folder itself".into());
        }
        Ok(resolved)
    }

    /// Checks an open file against its path: the same inode, a regular file with one link, owned by this user.
    fn check_open(&self, file: &std::fs::File, path: &Path) -> Result<(), String> {
        let (opened, current) = (file.metadata().map_err(|error| error.to_string())?, std::fs::symlink_metadata(path).map_err(|error| error.to_string())?);
        self.check(path)?;
        if !opened.is_file() || opened.nlink() != 1 || current.file_type().is_symlink() || (opened.ino(), opened.dev()) != (current.ino(), current.dev()) || opened.uid() != uid() {
            return Err("file changed while opening it".into());
        }
        Ok(())
    }

    /// A file to read, checked: an existing regular file. A relative path is one in the web folder's `folder`.
    pub fn input(&self, raw: &str, folder: &str) -> Result<PathBuf, String> {
        let path = self.absolute(raw, folder)?;
        let resolved = self.check(&path)?;
        if self.is_root(&resolved) {
            return Err(format!("{} is a file root; use a file inside it", path.display()));
        }
        match std::fs::symlink_metadata(&resolved) {
            Ok(meta) if meta.is_file() => Ok(resolved),
            Ok(_) => Err(format!("{} is not a regular file", path.display())),
            Err(_) => Err(format!("{} does not exist", path.display())),
        }
    }

    fn is_root(&self, path: &Path) -> bool {
        self.roots.iter().any(|root| root == path)
    }

    /// The bytes of the file `raw` names, at most `max`: opened without following a link and checked again once open.
    pub fn read(&self, raw: &str, folder: &str, max: u64) -> Result<Vec<u8>, String> {
        let path = self.input(raw, folder)?;
        let file = std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK).open(&path).map_err(|error| format!("{}: {error}", path.display()))?;
        self.check_open(&file, &path)?;
        let mut bytes = Vec::new();
        file.take(max + 1).read_to_end(&mut bytes).map_err(|error| error.to_string())?;
        if bytes.len() as u64 > max {
            return Err(format!("{} is larger than {} MiB", path.display(), max / 1024 / 1024));
        }
        Ok(bytes)
    }

    /// The files of an upload by their paths, each checked, at most `UPLOAD_MAX_BYTES` together.
    pub fn uploads(&self, raws: &[String]) -> Result<Vec<PathBuf>, String> {
        let mut total = 0u64;
        let mut found = Vec::with_capacity(raws.len());
        for raw in raws {
            let path = self.input(raw, UPLOADS)?;
            let file = std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK).open(&path).map_err(|error| format!("{}: {error}", path.display()))?;
            self.check_open(&file, &path)?;
            total += file.metadata().map_err(|error| error.to_string())?.len();
            if total > UPLOAD_MAX_BYTES {
                return Err(format!("upload takes at most {} MB of files by their paths per call", UPLOAD_MAX_BYTES / 1024 / 1024));
            }
            found.push(path);
        }
        Ok(found)
    }

    /// A file to write, checked, with nothing made yet: not a root or a folder. A relative path is one in the web
    /// folder's `folder`.
    pub fn output(&self, raw: &str, folder: &str) -> Result<PathBuf, String> {
        let path = self.absolute(raw, folder)?;
        let resolved = self.check(&path)?;
        if self.is_root(&resolved) {
            return Err(format!("{} is a file root; use a path inside it", path.display()));
        }
        if resolved.is_dir() {
            return Err(format!("{} is a folder; give a file path", path.display()));
        }
        Ok(resolved)
    }

    /// Makes the folders `path` needs, 0700, after refusing any existing one that is a link or shared below its root.
    /// The ones it made, outermost first.
    pub fn prepare(&self, path: &Path) -> Result<Vec<PathBuf>, String> {
        self.check(path)?;
        let folder = path.parent().ok_or_else(|| refused_folder(path))?;
        let root = root_of(folder, &self.roots).cloned().ok_or_else(|| refused_folder(folder))?;
        let mut missing = Vec::new();
        let mut current = folder.to_path_buf();
        loop {
            match lstat(&current)? {
                Some(meta) => {
                    if !meta.is_dir() || meta.file_type().is_symlink() || (current != root && shared(&meta)) {
                        return Err(refused_folder(&current));
                    }
                }
                None if current == root => return Err(refused_folder(&current)),
                None => missing.push(current.clone()),
            }
            if current == root || !current.pop() {
                break;
            }
        }
        let mut created = Vec::new();
        for step in missing.into_iter().rev() {
            match std::fs::DirBuilder::new().mode(0o700).create(&step) {
                Ok(()) => created.push(step.clone()),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => {
                    remove_created(&created);
                    return Err(error.to_string());
                }
            }
            if let Err(error) = private_folder(&step) {
                remove_created(&created);
                return Err(error);
            }
        }
        Ok(created)
    }

    /// Writes `bytes` to `path` (from `output`): folders 0700, the data in a new 0600 file beside it that is renamed over
    /// the target.
    pub fn write(&self, path: &Path, bytes: &[u8]) -> Result<(), String> {
        let created = self.prepare(path)?;
        let partial = partial_path(path, "tmp");
        let result = (|| {
            let mut file = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).custom_flags(libc::O_NOFOLLOW).open(&partial).map_err(|error| error.to_string())?;
            // Default ACLs can widen the mode create gave it.
            file.set_permissions(std::fs::Permissions::from_mode(0o600)).map_err(|error| error.to_string())?;
            self.check_open(&file, &partial)?;
            file.write_all(bytes).and_then(|()| file.sync_all()).map_err(|error| error.to_string())?;
            drop(file);
            self.check(path)?;
            std::fs::rename(&partial, path).map_err(|error| error.to_string())
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&partial);
            remove_created(&created);
        }
        result
    }

    /// Makes a file another part of the app wrote (a recording) private and checked: 0600 through its descriptor, then
    /// renamed over `path`.
    pub fn commit(&self, partial: &Path, path: &Path) -> Result<(), String> {
        let file = std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK).open(partial).map_err(|error| error.to_string())?;
        file.set_permissions(std::fs::Permissions::from_mode(0o600)).map_err(|error| error.to_string())?;
        self.check_open(&file, partial)?;
        self.check(path)?;
        std::fs::rename(partial, path).map_err(|error| error.to_string())
    }

    /// A path in the folder of `path` for a file being written, to be `commit`ted to it.
    pub fn partial(&self, path: &Path, extension: &str) -> PathBuf {
        partial_path(path, extension)
    }

    /// Files passed inline (`{ name, data }`, base64), written into a new private folder of the web folder, at most
    /// `INLINE_MAX_BYTES` decoded together: their paths.
    pub fn inline(&self, files: &[(String, String)]) -> Result<Vec<PathBuf>, String> {
        let base = self.internal(UPLOADS)?;
        prune(&base);
        let mut decoded = Vec::with_capacity(files.len());
        let mut total = 0u64;
        for (name, data) in files {
            if !plain_name(name) {
                return Err(format!("upload: {name:?} is not a plain file name; inline files take a name, not a path"));
            }
            let text: Vec<u8> = data.bytes().filter(|byte| !byte.is_ascii_whitespace()).collect();
            let bytes = INLINE_BASE64.decode(&text).map_err(|_| format!("upload: {name}: data is not valid base64"))?;
            total += bytes.len() as u64;
            if total > INLINE_MAX_BYTES {
                return Err(format!("upload passes at most {} MB of inline files per call", INLINE_MAX_BYTES / 1024 / 1024));
            }
            decoded.push((name, bytes));
        }
        let folder = base.join(format!("{:x}-{}", std::process::id(), suffix()));
        std::fs::DirBuilder::new().mode(0o700).create(&folder).map_err(|error| format!("{}: {error}", folder.display()))?;
        private_folder(&folder)?;
        let mut paths = Vec::with_capacity(decoded.len());
        for (name, bytes) in decoded {
            let path = self.output(&folder.join(name).to_string_lossy(), UPLOADS)?;
            if let Err(error) = self.write(&path, &bytes) {
                let _ = std::fs::remove_dir_all(&folder);
                return Err(error);
            }
            paths.push(path);
        }
        Ok(paths)
    }
}

/// A file's own name: no folder, no `.` or `..`, nothing the file system or a shell reads as more.
pub fn plain_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= 255 && name != "." && name != ".." && !name.contains(['/', '\\']) && !name.chars().any(char::is_control)
}

/// Inline data may be line-wrapped and may have lost its padding on the way.
const INLINE_BASE64: GeneralPurpose = GeneralPurpose::new(&base64::alphabet::STANDARD, GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent));

/// Removes the inline folders of an hour ago and before.
fn prune(base: &Path) {
    let Ok(entries) = std::fs::read_dir(base) else { return };
    for entry in entries.flatten() {
        let old = entry.metadata().ok().filter(|meta| meta.is_dir() && meta.uid() == uid()).and_then(|meta| meta.modified().ok()).and_then(|modified| modified.elapsed().ok()).is_some_and(|age| age > std::time::Duration::from_secs(3600));
        if old {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

/// Removes folders `prepare` made, innermost first, as long as they are empty.
fn remove_created(created: &[PathBuf]) {
    for folder in created.iter().rev() {
        let _ = std::fs::remove_dir(folder);
    }
}

/// Six random letters and digits.
fn suffix() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos() as u64;
    let mut seed = (nanos ^ u64::from(std::process::id()).rotate_left(32) ^ COUNTER.fetch_add(1, Ordering::Relaxed).wrapping_mul(0x9E37_79B9_7F4A_7C15)) | 1;
    (0..6)
        .map(|_| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            ALPHABET[(seed % ALPHABET.len() as u64) as usize] as char
        })
        .collect()
}

/// A sibling path for a file being written, renamed into place when it's done.
fn partial_path(path: &Path, extension: &str) -> PathBuf {
    let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    path.with_file_name(format!(".{name}.partial-{}-{}.{extension}", std::process::id(), suffix()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A private folder to be the only root, and a web folder beside it.
    struct Rig {
        _dir: tempfile::TempDir,
        root: PathBuf,
        web: PathBuf,
    }

    fn rig() -> Rig {
        let dir = tempfile::tempdir().unwrap();
        let base = std::fs::canonicalize(dir.path()).unwrap();
        let (root, web) = (base.join("root"), base.join("web"));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
        Rig { _dir: dir, root, web }
    }

    impl Rig {
        /// The roots with this folder alone, without the temporary folders.
        fn files(&self) -> Files {
            let mut files = Files::of(&self.web);
            files.listed = vec![self.root.clone()];
            files.roots.retain(|root| root.starts_with(&self.web));
            files.roots.push(self.root.clone());
            files
        }
    }

    fn mode(path: &Path) -> u32 {
        std::fs::metadata(path).unwrap().mode() & 0o777
    }

    #[test]
    fn paths_stay_inside_roots_and_a_root_itself_is_refused() {
        let rig = rig();
        let files = rig.files();
        assert_eq!(files.output(&rig.root.join("a/b.png").to_string_lossy(), SCREENSHOTS).unwrap(), rig.root.join("a/b.png"));
        let outside = files.output("/etc/passwd", SCREENSHOTS).unwrap_err();
        assert!(outside.contains("outside the file roots") && outside.contains(&rig.root.display().to_string()) && outside.contains("WINGS_BROWSER_FILE_ROOTS"), "{outside}");
        let dots = files.output(&format!("{}/../escape.png", rig.root.display()), SCREENSHOTS).unwrap_err();
        assert!(dots.contains("outside the file roots"), "..: {dots}");
        let itself = files.output(&rig.root.to_string_lossy(), SCREENSHOTS).unwrap_err();
        assert!(itself.contains("file root"), "{itself}");
        assert!(files.input(&rig.root.to_string_lossy(), SCREENSHOTS).unwrap_err().contains("file root"));
        let climbing = files.output("/../../x.png", SCREENSHOTS);
        assert!(climbing.is_err());
    }

    #[test]
    fn a_relative_path_goes_into_the_web_folder() {
        let rig = rig();
        let files = rig.files();
        let path = files.output("clip.mp4", RECORDINGS).unwrap();
        assert_eq!(path, rig.web.join("recordings/clip.mp4"));
        assert_eq!(mode(&rig.web.join("recordings")), 0o700);
        assert!(files.output("../x.mp4", RECORDINGS).unwrap_err().contains("outside the file roots"));
        files.write(&path, b"x").unwrap();
        assert_eq!(files.read("clip.mp4", RECORDINGS, 10).unwrap(), b"x");
        // What the app wrote there is read by its absolute path, the way a screenshot's answer names it.
        assert_eq!(files.input(&path.to_string_lossy(), SCREENSHOTS).unwrap(), path);
        // The web folder itself and its other folders are no root.
        std::fs::write(rig.web.join("other.txt"), b"x").unwrap();
        assert!(files.input(&rig.web.join("other.txt").to_string_lossy(), SCREENSHOTS).unwrap_err().contains("outside the file roots"));
    }

    #[test]
    fn a_symlink_on_the_way_is_refused() {
        let rig = rig();
        let files = rig.files();
        let secret = rig.root.parent().unwrap().join("secret");
        std::fs::create_dir_all(&secret).unwrap();
        std::fs::write(secret.join("s.txt"), b"x").unwrap();
        std::os::unix::fs::symlink(&secret, rig.root.join("link")).unwrap();
        std::os::unix::fs::symlink(secret.join("s.txt"), rig.root.join("file")).unwrap();
        assert!(files.output(&rig.root.join("link/x.png").to_string_lossy(), SCREENSHOTS).is_err(), "a linked folder");
        assert!(files.input(&rig.root.join("file").to_string_lossy(), SCREENSHOTS).is_err(), "a linked file");
        assert!(files.write(&rig.root.join("link/new/x.png"), b"x").is_err());
        assert!(!secret.join("new").exists(), "nothing was made outside");
    }

    #[test]
    fn writes_are_private_and_unsafe_targets_are_refused() {
        let rig = rig();
        let files = rig.files();
        let path = files.output(&rig.root.join("deep/er/x.png").to_string_lossy(), SCREENSHOTS).unwrap();
        files.write(&path, b"one").unwrap();
        assert_eq!((mode(&rig.root.join("deep")), mode(&rig.root.join("deep/er")), mode(&path)), (0o700, 0o700, 0o600));
        // An existing file is replaced by a 0600 one.
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        files.write(&path, b"two").unwrap();
        assert_eq!((std::fs::read(&path).unwrap(), mode(&path)), (b"two".to_vec(), 0o600));
        assert_eq!(files.read(&path.to_string_lossy(), SCREENSHOTS, 10).unwrap(), b"two");
        assert!(files.read(&path.to_string_lossy(), SCREENSHOTS, 2).unwrap_err().contains("larger"));
        // A hard link, a group-writable file and a group-writable folder are refused.
        std::fs::hard_link(&path, rig.root.join("deep/link.png")).unwrap();
        assert!(files.write(&path, b"x").unwrap_err().contains("no hard links"));
        assert!(files.read(&rig.root.join("deep/link.png").to_string_lossy(), SCREENSHOTS, 10).is_err());
        std::fs::remove_file(rig.root.join("deep/link.png")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o664)).unwrap();
        let error = files.read(&path.to_string_lossy(), SCREENSHOTS, 10).unwrap_err();
        assert!(error.contains("group/world write"), "{error}");
        std::fs::set_permissions(rig.root.join("deep"), std::fs::Permissions::from_mode(0o777)).unwrap();
        assert!(files.output(&rig.root.join("deep/new.png").to_string_lossy(), SCREENSHOTS).is_err());
        std::fs::set_permissions(rig.root.join("deep"), std::fs::Permissions::from_mode(0o700)).unwrap();
        assert!(files.output(&rig.root.join("deep").to_string_lossy(), SCREENSHOTS).unwrap_err().contains("folder"));
    }

    #[test]
    fn a_shared_temporary_folder_is_written_only_in_a_subfolder() {
        let rig = rig();
        std::fs::set_permissions(&rig.root, std::fs::Permissions::from_mode(0o1777)).unwrap();
        let files = rig.files();
        let direct = files.output(&rig.root.join("x.png").to_string_lossy(), SCREENSHOTS).unwrap_err();
        assert!(direct.contains("subfolder"), "{direct}");
        let nested = files.output(&rig.root.join("mine/x.png").to_string_lossy(), SCREENSHOTS).unwrap();
        files.write(&nested, b"x").unwrap();
        assert_eq!(mode(&rig.root.join("mine")), 0o700);
        // Without the sticky bit it isn't a root at all.
        std::fs::set_permissions(&rig.root, std::fs::Permissions::from_mode(0o777)).unwrap();
        assert!(files.output(&rig.root.join("mine/y.png").to_string_lossy(), SCREENSHOTS).unwrap_err().contains("sticky"));
    }

    #[test]
    fn uploads_by_path_are_checked_and_capped() {
        let rig = rig();
        let files = rig.files();
        let (a, b) = (rig.root.join("a.bin"), rig.root.join("b.bin"));
        for file in [&a, &b] {
            std::fs::write(file, vec![0u8; 30 * 1024 * 1024]).unwrap();
            std::fs::set_permissions(file, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        let name = |file: &Path| file.to_string_lossy().into_owned();
        assert_eq!(files.uploads(&[name(&a)]).unwrap(), vec![a.clone()]);
        assert!(files.uploads(&[name(&a), name(&b)]).unwrap_err().contains("50 MB"));
        assert!(files.uploads(&[name(&rig.root.join("missing"))]).unwrap_err().contains("does not exist"));
        assert!(files.uploads(&["/etc/hosts".into()]).unwrap_err().contains("outside"));
    }

    #[test]
    fn inline_files_go_to_a_private_folder_of_the_web_folder() {
        let rig = rig();
        let files = rig.files();
        let paths = files.inline(&[("notes.txt".into(), "aGVs\nbG8".into()), ("b.bin".into(), "AAEC".into())]).unwrap();
        assert_eq!(std::fs::read(&paths[0]).unwrap(), b"hello");
        assert_eq!(std::fs::read(&paths[1]).unwrap(), [0, 1, 2]);
        assert!(paths[0].starts_with(rig.web.join("uploads")) && paths[0].parent() == paths[1].parent());
        assert_eq!((mode(paths[0].parent().unwrap()), mode(&paths[0])), (0o700, 0o600));
        // The folders the app made are inside the roots: what it wrote can be named by its path.
        assert_eq!(files.uploads(&[paths[0].to_string_lossy().into_owned()]).unwrap().len(), 1);
        assert!(files.inline(&[("a.txt".into(), "not base64!".into())]).unwrap_err().contains("valid base64"));
        let big = "A".repeat((INLINE_MAX_BYTES as usize / 3 + 1) * 4);
        assert!(files.inline(&[("big.bin".into(), big)]).unwrap_err().contains("10 MB"));
        assert!(files.inline(&[("../x".into(), "AAEC".into())]).is_err(), "a name is a name, never a path");
    }

    #[test]
    fn the_roots_are_the_temp_folders_and_the_environments() {
        let rig = rig();
        let files = Files::of(&rig.web);
        assert!(files.listed.contains(&std::fs::canonicalize("/tmp").unwrap()));
        assert!(files.listed.iter().all(|root| root != &rig.web), "the web folder is no root of the list");
        let extra = rig.root.parent().unwrap().join("extra");
        std::fs::create_dir_all(&extra).unwrap();
        // SAFETY: only this test sets these variables.
        unsafe { std::env::set_var(ENV[1], format!("{}:relative", extra.display())) };
        let files = Files::of(&rig.web);
        unsafe { std::env::remove_var(ENV[1]) };
        assert!(files.listed.contains(&std::fs::canonicalize(&extra).unwrap()), "browse's variable counts too");
        assert!(!files.listed.iter().any(|root| root.ends_with("relative")), "only absolute entries");
    }
}
