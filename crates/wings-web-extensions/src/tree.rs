use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

use sha2::{Digest, Sha256};

use crate::{
    unpack::{MAX_FILES, MAX_TOTAL_BYTES},
    Error,
};

/// The SHA-256 of everything in the folder `dir`, as hex: each file's path and bytes, each folder's path, and each
/// symlink's path and target (never followed), in the order of their names, so the same files give the same hash on any
/// run and any change to one (a file edited, added, removed or renamed, a symlink put in) gives another. Stops at the
/// limits an extension is unpacked within ([`MAX_FILES`], [`MAX_TOTAL_BYTES`]).
pub fn tree_hash(dir: &Path) -> Result<String, Error> {
    let mut hash = Sha256::new();
    let (mut files, mut bytes) = (0usize, 0u64);
    // The folder itself must be one, not a symlink to one.
    if !fs::symlink_metadata(dir)?.is_dir() {
        return Err(Error::Unsafe(format!("{} is not a folder", dir.display())));
    }
    walk(dir, &PathBuf::new(), 0, &mut hash, &mut files, &mut bytes)?;
    Ok(hash.finalize().iter().map(|byte| format!("{byte:02x}")).collect())
}

/// Each entry is written as a kind, its path's length and the path, then for a file its length and bytes, so no two
/// trees write the same stream.
fn walk(dir: &Path, rel: &Path, depth: usize, hash: &mut Sha256, files: &mut usize, bytes: &mut u64) -> Result<(), Error> {
    if depth > 32 {
        return Err(Error::TooLarge("folders nested too deep".into()));
    }
    let mut entries: Vec<fs::DirEntry> = fs::read_dir(dir)?.collect::<Result<_, _>>()?;
    entries.sort_by(|a, b| a.file_name().cmp(&b.file_name()));
    for entry in entries {
        *files += 1;
        if *files > MAX_FILES {
            return Err(Error::TooLarge("too many files".into()));
        }
        let path = rel.join(entry.file_name());
        let kind = entry.file_type()?;
        let name = path.as_os_str().as_encoded_bytes();
        let mut write = |tag: u8| {
            hash.update([tag]);
            hash.update((name.len() as u64).to_le_bytes());
            hash.update(name);
        };
        if kind.is_symlink() {
            write(b'l');
            let target = fs::read_link(entry.path())?;
            let target = target.as_os_str().as_encoded_bytes();
            hash.update((target.len() as u64).to_le_bytes());
            hash.update(target);
        } else if kind.is_dir() {
            write(b'd');
            walk(&entry.path(), &path, depth + 1, hash, files, bytes)?;
        } else if kind.is_file() {
            write(b'f');
            let mut file = fs::File::open(entry.path())?;
            let size = file.metadata()?.len();
            *bytes += size;
            if *bytes > MAX_TOTAL_BYTES {
                return Err(Error::TooLarge("over the size limit".into()));
            }
            hash.update(size.to_le_bytes());
            // Never more than the size it said; one cut short while read hashes as another, by what was read.
            let (mut read, mut buffer) = (0u64, vec![0; 256 * 1024]);
            let mut part = file.by_ref().take(size);
            loop {
                let n = part.read(&mut buffer)?;
                if n == 0 {
                    break;
                }
                hash.update(&buffer[..n]);
                read += n as u64;
            }
            hash.update(read.to_le_bytes());
        } else {
            // A pipe or a socket: named, so one put in changes the hash.
            write(b'o');
        }
    }
    Ok(())
}

/// Reads the package (a ZIP, CRX or XPI) at `path`, which may be no bigger than an unpacked extension may be
/// ([`MAX_TOTAL_BYTES`]): refused before it is read whole into memory.
pub fn read_package(path: &Path) -> Result<Vec<u8>, Error> {
    let file = fs::File::open(path)?;
    if file.metadata()?.len() > MAX_TOTAL_BYTES {
        return Err(Error::TooLarge("over the size limit".into()));
    }
    let mut bytes = Vec::new();
    // It may grow after the check: never past the limit.
    file.take(MAX_TOTAL_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_TOTAL_BYTES {
        return Err(Error::TooLarge("over the size limit".into()));
    }
    Ok(bytes)
}
