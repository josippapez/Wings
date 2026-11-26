use std::{
    fs,
    io::{Cursor, Read},
    path::{Path, PathBuf},
};

use zip::ZipArchive;

use crate::{
    limits::safe_relative,
    manifest::{read_manifest, Info},
    Error,
};

/// Most bytes one extension may take on disk.
pub const MAX_TOTAL_BYTES: u64 = 200 * 1024 * 1024;
/// Most files and folders one extension may hold.
pub const MAX_FILES: usize = 20_000;

/// Unpacks a store download into `into` and reads its manifest. Takes a ZIP, a CRX2 or CRX3 (`Cr24`) or an XPI.
///
/// CRX signatures are NOT verified: the package comes from the store over TLS and is only ever unpacked, never trusted
/// by a signature. Refuses absolute paths, `..`, backslashes and symlinks, and stops at [`MAX_TOTAL_BYTES`] written or
/// [`MAX_FILES`] entries. A ZIP whose files all sit in one top folder with `manifest.json` inside is unwrapped. On an
/// error a folder this call created is removed again.
pub fn unpack(bytes: &[u8], into: &Path) -> Result<Info, Error> {
    let zip_bytes = zip_part(bytes)?;
    let mut archive = ZipArchive::new(Cursor::new(zip_bytes))?;
    if archive.len() > MAX_FILES {
        return Err(Error::TooLarge(format!("{} entries", archive.len())));
    }

    struct Entry {
        index: usize,
        rel: PathBuf,
        dir: bool,
    }
    let mut entries = Vec::new();
    let mut declared = 0u64;
    for index in 0..archive.len() {
        let f = archive.by_index(index)?;
        let name = f.name().to_string();
        // macOS Finder adds these to ZIPs; they are not part of the extension.
        if name.starts_with("__MACOSX/") {
            continue;
        }
        if f.is_symlink() {
            return Err(Error::Unsafe(format!("symlink {name}")));
        }
        let rel = safe_relative(&name).ok_or_else(|| Error::Unsafe(name.clone()))?;
        declared = declared.saturating_add(f.size());
        if declared > MAX_TOTAL_BYTES {
            return Err(Error::TooLarge("over the size limit".into()));
        }
        entries.push(Entry { index, rel, dir: f.is_dir() });
    }

    let strip = top_folder(entries.iter().map(|e| e.rel.as_path()));
    let created = !into.exists();
    let result = (|| {
        fs::create_dir_all(into)?;
        let mut written = 0u64;
        for e in &entries {
            let rel = match &strip {
                Some(top) => match e.rel.strip_prefix(top) {
                    Ok(r) if !r.as_os_str().is_empty() => r,
                    _ => continue,
                },
                None => &e.rel,
            };
            let dest = into.join(rel);
            if e.dir {
                fs::create_dir_all(&dest)?;
                continue;
            }
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent)?;
            }
            let file = archive.by_index(e.index)?;
            // The declared size can lie: count what is really written.
            let mut limited = file.take(MAX_TOTAL_BYTES - written + 1);
            let n = std::io::copy(&mut limited, &mut fs::File::create(&dest)?)?;
            written += n;
            if written > MAX_TOTAL_BYTES {
                return Err(Error::TooLarge("over the size limit".into()));
            }
        }
        read_manifest(into)
    })();
    if result.is_err() && created {
        let _ = fs::remove_dir_all(into);
    }
    result
}

/// The ZIP inside a CRX, or the input itself when it already is a ZIP.
///
/// Layouts (Chromium `components/crx_file/crx3.proto`, and the CRX2 article in its `crx.html`): CRX3 is `Cr24`, version 3,
/// header length N (u32 little-endian), N header bytes, ZIP. CRX2 is `Cr24`, version 2, public key length, signature
/// length (both u32 little-endian), the key, the signature, ZIP.
fn zip_part(bytes: &[u8]) -> Result<&[u8], Error> {
    if !bytes.starts_with(b"Cr24") {
        return if bytes.starts_with(b"PK") { Ok(bytes) } else { Err(Error::Archive("not a ZIP or CRX".into())) };
    }
    let u32_at = |at: usize| -> Result<usize, Error> {
        let b: [u8; 4] = bytes.get(at..at + 4).and_then(|s| s.try_into().ok()).ok_or_else(|| Error::Archive("CRX header is cut short".into()))?;
        Ok(u32::from_le_bytes(b) as usize)
    };
    let start = match u32_at(4)? {
        3 => 12usize.checked_add(u32_at(8)?),
        2 => 16usize.checked_add(u32_at(8)?).and_then(|n| n.checked_add(u32_at(12).ok()?)),
        v => return Err(Error::Archive(format!("CRX version {v}"))),
    };
    start.and_then(|s| bytes.get(s..)).filter(|z| z.starts_with(b"PK")).ok_or_else(|| Error::Archive("no ZIP after the CRX header".into()))
}

/// The one folder every entry sits in, when `manifest.json` is inside it and not at the top.
fn top_folder<'a>(rels: impl Iterator<Item = &'a Path>) -> Option<PathBuf> {
    let mut top: Option<&std::ffi::OsStr> = None;
    let mut has_inner_manifest = false;
    for rel in rels {
        let mut parts = rel.components();
        let first = parts.next()?.as_os_str();
        if top.is_some_and(|t| t != first) {
            return None;
        }
        top = Some(first);
        if rel.components().count() == 1 && rel == Path::new("manifest.json") {
            return None;
        }
        has_inner_manifest |= rel.components().count() == 2 && rel.file_name().is_some_and(|n| n == "manifest.json");
    }
    top.filter(|_| has_inner_manifest).map(PathBuf::from)
}

/// Copies an installed extension's folder into `into` (so the browser it came from can update or remove its own copy),
/// then reads its manifest. Symlinks are skipped, never followed, and the same limits as [`unpack`] apply.
///
/// `_metadata/` (Chrome's `verified_contents.json`) is left in: WebKit's `WebExtension.cpp` and `WebExtensionCocoa.mm`
/// do not mention it, but whether anything else in WebKit reads it is unverified.
pub fn copy_folder(from: &Path, into: &Path) -> Result<Info, Error> {
    let created = !into.exists();
    let result = (|| {
        fs::create_dir_all(into)?;
        let (mut files, mut bytes) = (0usize, 0u64);
        copy_dir(from, into, 0, &mut files, &mut bytes)?;
        read_manifest(into)
    })();
    if result.is_err() && created {
        let _ = fs::remove_dir_all(into);
    }
    result
}

fn copy_dir(from: &Path, into: &Path, depth: usize, files: &mut usize, bytes: &mut u64) -> Result<(), Error> {
    if depth > 32 {
        return Err(Error::TooLarge("folders nested too deep".into()));
    }
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            continue;
        }
        *files += 1;
        if *files > MAX_FILES {
            return Err(Error::TooLarge("too many files".into()));
        }
        let dest = into.join(entry.file_name());
        if kind.is_dir() {
            fs::create_dir_all(&dest)?;
            copy_dir(&entry.path(), &dest, depth + 1, files, bytes)?;
        } else if kind.is_file() {
            *bytes += entry.metadata()?.len();
            if *bytes > MAX_TOTAL_BYTES {
                return Err(Error::TooLarge("over the size limit".into()));
            }
            fs::copy(entry.path(), &dest)?;
        }
    }
    Ok(())
}
