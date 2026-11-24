//! Each module's compiled code, kept on disk for the next launch: compiling is what a page's start costs.
//!
//! `Module::deserialize` runs what it reads as it is: "Arbitrary input could, for example, replace valid compiled code
//! with any other valid compiled code, meaning that this can trivially be used to execute arbitrary code" (wasmtime
//! 48's docs). The folder is this user's alone (`private_dir`), but a program you run, a plugin's among them, writes as
//! you. So a file is read back only with this install's signature over it and over the hash of the module it was
//! compiled from (HMAC-SHA256 with a secret made once): anything else, a file swapped in or one of another module, is
//! compiled again instead of run. In the installed app the secret is in the keychain, which another program can't read
//! without asking you. Debug builds and tests keep it in a file in the folder: each build is another binary, which the
//! keychain would ask about every time.

use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::OnceLock,
};

static DIR: OnceLock<PathBuf> = OnceLock::new();
static SECRET: OnceLock<Option<[u8; 32]>> = OnceLock::new();

/// The signature, before the code, in each file.
const TAG_BYTES: usize = 32;

pub(super) fn keep_in(dir: PathBuf) {
    let _ = DIR.set(dir);
}

/// Where one module's compiled code is kept, and what signs it.
pub(super) struct Kept {
    pub(super) file: PathBuf,
    /// The SHA-256 of the module.
    module: [u8; 32],
    secret: [u8; 32],
}

/// Where the compiled code of the module `bytes` is kept: a file named by the module's hash, so another build of the
/// page is another file. None without a folder, or without a secret to sign with.
pub(super) fn kept(bytes: &[u8]) -> Option<Kept> {
    let dir = DIR.get()?;
    crate::terminal_server::private_dir(dir).ok()?;
    let secret = (*SECRET.get_or_init(|| secret(dir)))?;
    let module: [u8; 32] = Sha256::digest(bytes).into();
    Some(Kept {
        file: dir.join(file_name(&module)),
        module,
        secret,
    })
}

impl Kept {
    /// The code in the file, when it is there and this install signed it for this module.
    pub(super) fn read(&self) -> Option<Vec<u8>> {
        let mut file = fs::read(&self.file).ok()?;
        if file.len() < TAG_BYTES {
            return None;
        }
        let (tag, code) = file.split_at(TAG_BYTES);
        self.signed(code).verify_slice(tag).ok()?;
        file.drain(..TAG_BYTES);
        Some(file)
    }

    /// Keeps `code` signed, whole or not at all: a launch that reads while this one writes sees the old file or none.
    pub(super) fn write(&self, code: &[u8]) {
        let tag = self.signed(code).finalize().into_bytes();
        let part = self.file.with_extension(format!("{}.part", std::process::id()));
        let written = fs::File::create(&part)
            .and_then(|mut out| out.write_all(&tag).and_then(|()| out.write_all(code)))
            .and_then(|()| fs::rename(&part, &self.file));
        if written.is_err() {
            let _ = fs::remove_file(&part);
        }
    }

    fn signed(&self, code: &[u8]) -> Hmac<Sha256> {
        let mut mac = Hmac::<Sha256>::new_from_slice(&self.secret).expect("an HMAC takes a key of any length");
        mac.update(&self.module);
        mac.update(code);
        mac
    }
}

fn file_name(module: &[u8; 32]) -> String {
    let hash: String = module.iter().map(|byte| format!("{byte:02x}")).collect();
    format!("{hash}.cwasm")
}

/// Removes from the folder the code of every module that isn't one of `modules` (files), and what a write of another
/// launch left half done. Each build of a module left a file a few times its size, and nothing removed them: 245 files
/// and 376 MB in a data folder after a few weeks. Returns how many it removed.
pub(super) fn prune(modules: &[PathBuf]) -> io::Result<usize> {
    match DIR.get() {
        Some(dir) => prune_folder(dir, modules),
        None => Ok(0),
    }
}

fn prune_folder(dir: &Path, modules: &[PathBuf]) -> io::Result<usize> {
    let keep: HashSet<String> = modules
        .iter()
        .filter_map(|module| fs::read(module).ok())
        .map(|bytes| file_name(&Sha256::digest(&bytes).into()))
        .collect();
    let writing = format!(".{}.part", std::process::id());
    let mut removed = 0;
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let gone = (name.ends_with(".cwasm") && !keep.contains(&name)) || (name.ends_with(".part") && !name.ends_with(&writing));
        if gone && fs::remove_file(entry.path()).is_ok() {
            removed += 1;
        }
    }
    Ok(removed)
}

/// This install's secret, made the first time: a file `secret` in the folder, which only this user can read.
#[cfg(any(test, debug_assertions, feature = "test-support"))]
fn secret(dir: &Path) -> Option<[u8; 32]> {
    use std::os::unix::fs::OpenOptionsExt;
    let file = dir.join("secret");
    if let Some(secret) = fs::read(&file).ok().and_then(|kept| kept.try_into().ok()) {
        return Some(secret);
    }
    let secret = random()?;
    match fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&file) {
        Ok(mut out) => out.write_all(&secret).ok().map(|()| secret),
        // Another launch made it first.
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => fs::read(&file).ok()?.try_into().ok(),
        Err(_) => None,
    }
}

/// This install's secret, made the first time, in the keychain: one for each data folder, as each has its own code.
#[cfg(not(any(test, debug_assertions, feature = "test-support")))]
fn secret(dir: &Path) -> Option<[u8; 32]> {
    let entry = keyring::Entry::new("dev.wings.app.wasm-cache", &dir.to_string_lossy()).ok()?;
    match entry.get_secret() {
        Ok(kept) => kept.try_into().ok(),
        Err(keyring::Error::NoEntry) => {
            let secret = random()?;
            entry.set_secret(&secret).ok().map(|()| secret)
        }
        Err(_) => None,
    }
}

fn random() -> Option<[u8; 32]> {
    let mut secret = [0; 32];
    fs::File::open("/dev/urandom").and_then(|mut source| source.read_exact(&mut secret)).ok()?;
    Some(secret)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Where `bytes`' code is kept in `dir`, signed with that folder's secret, for a test that has a folder of its own.
    pub(crate) fn kept_in(dir: &Path, bytes: &[u8]) -> Kept {
        crate::terminal_server::private_dir(dir).unwrap();
        let module: [u8; 32] = Sha256::digest(bytes).into();
        Kept { file: dir.join(file_name(&module)), module, secret: secret(dir).unwrap() }
    }

    /// The test for WIN-175's pruning: a cache file of a module that is gone is removed at the next start, and one of a
    /// module still installed, and the secret, stay.
    #[test]
    fn the_code_of_a_module_no_plugin_has_is_removed_and_the_rest_kept() {
        let dir = tempfile::tempdir().unwrap();
        let modules = tempfile::tempdir().unwrap();
        let (installed, gone) = (modules.path().join("installed.wasm"), modules.path().join("gone.wasm"));
        fs::write(&installed, b"\0asm\x01\0\0\0 installed").unwrap();
        fs::write(&gone, b"\0asm\x01\0\0\0 gone").unwrap();
        let (kept, left) = (kept_in(dir.path(), &fs::read(&installed).unwrap()), kept_in(dir.path(), &fs::read(&gone).unwrap()));
        kept.write(b"code");
        left.write(b"code");
        // What a launch that was killed while writing left.
        fs::write(dir.path().join("abc.cwasm.1.part"), b"half").unwrap();
        fs::remove_file(&gone).unwrap();

        assert_eq!(prune_folder(dir.path(), &[installed]).unwrap(), 2);
        let mut names: Vec<String> = fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(names, [kept.file.file_name().unwrap().to_string_lossy().into_owned(), "secret".to_owned()]);
        assert_eq!(kept.read().as_deref(), Some(&b"code"[..]));
    }

    /// A file is read back only with this folder's signature over it for this module: not a file of code alone, not a
    /// signed file of another module put in its place, and not one signed with another secret.
    #[test]
    fn only_code_this_install_signed_for_the_module_is_read_back() {
        let dir = tempfile::tempdir().unwrap();
        let first = kept_in(dir.path(), b"first");
        let second = kept_in(dir.path(), b"second");
        first.write(b"first's code");
        second.write(b"second's code");
        assert_eq!(first.read().as_deref(), Some(&b"first's code"[..]));

        fs::copy(&second.file, &first.file).unwrap();
        assert_eq!(first.read(), None, "another module's signed code");
        fs::write(&first.file, b"unsigned code that is long enough to hold a tag").unwrap();
        assert_eq!(first.read(), None, "code with no signature");
        let other = tempfile::tempdir().unwrap();
        let elsewhere = kept_in(other.path(), b"first");
        elsewhere.write(b"first's code");
        fs::copy(&elsewhere.file, &first.file).unwrap();
        assert_eq!(first.read(), None, "code signed by another install");
    }
}
