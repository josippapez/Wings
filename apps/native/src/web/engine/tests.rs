use std::{io::Write, path::Path};

use super::*;

fn profile(plugin: &str) -> Profile {
    Profile { plugin: plugin.into(), scope: "main".into(), dir: PathBuf::from("/unused") }
}

/// A tar entry written by hand, as `tar::Builder` refuses `..` and absolute paths.
fn entry(tar: &mut Vec<u8>, name: &str, kind: u8, data: &[u8], link: &str) {
    let mut header = tar::Header::new_gnu();
    header.as_mut_bytes()[..name.len()].copy_from_slice(name.as_bytes());
    header.set_size(data.len() as u64);
    header.set_mode(0o644);
    header.set_entry_type(tar::EntryType::new(kind));
    if !link.is_empty() {
        header.set_link_name(link).unwrap();
    }
    header.set_cksum();
    tar.extend_from_slice(header.as_bytes());
    tar.extend_from_slice(data);
    tar.resize(tar.len().div_ceil(512) * 512, 0);
}

/// The archive's bytes, gzipped, for these (name, kind, data, link) entries.
fn archive(entries: &[(&str, u8, &[u8], &str)]) -> Vec<u8> {
    let mut tar = Vec::new();
    for (name, kind, data, link) in entries {
        entry(&mut tar, name, *kind, data, link);
    }
    tar.extend_from_slice(&[0; 1024]);
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    gz.write_all(&tar).unwrap();
    gz.finish().unwrap()
}

fn good() -> Vec<u8> {
    archive(&[
        ("Wings Chromium.app/", b'5', b"", ""),
        ("Wings Chromium.app/Contents/", b'5', b"", ""),
        ("Wings Chromium.app/Contents/Info.plist", b'0', b"plist", ""),
    ])
}

/// A pin for `bytes`, which the tests' files match.
fn pin_for(bytes: &[u8], unpacked: u64) -> Pinned {
    let sha: String = Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect();
    Pinned {
        version: "1.2.3",
        file: "engine.tar.gz",
        url: "https://example.invalid/engine.tar.gz",
        size: bytes.len() as u64,
        sha256: Box::leak(sha.into_boxed_str()),
        unpacked,
        tree: "",
    }
}

/// Downloads `bytes` into a fresh root and installs them with a signature check that passes.
fn install(bytes: &[u8], pin: &Pinned) -> (tempfile::TempDir, Result<(), String>) {
    let root = tempfile::tempdir().unwrap();
    download_from(root.path(), pin, bytes).unwrap();
    let result = install_in(root.path(), pin, |_| Ok(()));
    (root, result)
}

fn leftovers(root: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(root).unwrap().flatten().map(|entry| entry.file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    names
}

#[test]
fn an_install_unpacks_marks_itself_finished_and_deletes_the_download() {
    let bytes = good();
    let (root, result) = install(&bytes, &pin_for(&bytes, 5));
    result.unwrap();
    let pin = pin_for(&bytes, 5);
    assert_eq!(installed_in(root.path(), &pin), Some(root.path().join("1.2.3/Wings Chromium.app")));
    assert_eq!(fs::read(root.path().join("1.2.3/Wings Chromium.app/Contents/Info.plist")).unwrap(), b"plist");
    assert_eq!(leftovers(root.path()), ["1.2.3"]);
}

#[test]
fn a_folder_without_the_finished_marker_is_not_installed() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("1.2.3/Wings Chromium.app")).unwrap();
    assert_eq!(installed_in(root.path(), &pin_for(b"x", 1)), None);
}

/// An engine an older Wings installed, of another version than this build pins, is out of date: its Settings offer the
/// pinned one. Nothing installed, an install that didn't finish and the pinned one installed are not.
#[test]
fn only_a_finished_install_of_another_version_is_out_of_date() {
    let pin = pin_for(b"x", 1);
    let root = tempfile::tempdir().unwrap();
    assert!(!outdated_in(root.path(), &pin), "nothing installed");
    fs::create_dir_all(root.path().join("1.2.2/Wings Chromium.app")).unwrap();
    assert!(!outdated_in(root.path(), &pin), "an install that didn't finish");
    fs::write(root.path().join("1.2.2/finished"), "1.2.2").unwrap();
    fs::create_dir_all(root.path().join("profiles/Default")).unwrap();
    assert!(outdated_in(root.path(), &pin), "the older version, finished");
    fs::create_dir_all(root.path().join("1.2.3/Wings Chromium.app")).unwrap();
    fs::write(root.path().join("1.2.3/finished"), "1.2.3").unwrap();
    assert!(!outdated_in(root.path(), &pin), "the pinned version installed");
}

#[test]
fn a_download_of_another_size_is_refused_and_deleted() {
    let bytes = good();
    let root = tempfile::tempdir().unwrap();
    let mut pin = pin_for(&bytes, 5);
    download_from(root.path(), &pin, &bytes[..]).unwrap();
    pin.size += 1;
    assert_eq!(verify_in(root.path(), &pin), Err(WRONG_DOWNLOAD.into()));
    assert!(!downloads_in(root.path()).join(pin.file).exists());
}

#[test]
fn a_download_with_another_hash_is_refused_and_deleted() {
    let bytes = good();
    let root = tempfile::tempdir().unwrap();
    let pin = pin_for(&bytes, 5);
    let mut changed = bytes.clone();
    *changed.last_mut().unwrap() ^= 1;
    download_from(root.path(), &pin, &changed[..]).unwrap();
    assert_eq!(verify_in(root.path(), &pin), Err(WRONG_DOWNLOAD.into()));
    assert!(!downloads_in(root.path()).join(pin.file).exists());
    // The right bytes pass.
    download_from(root.path(), &pin, &bytes[..]).unwrap();
    assert_eq!(verify_in(root.path(), &pin), Ok(()));
}

#[test]
fn a_download_longer_than_the_pinned_size_stops_and_leaves_nothing() {
    let root = tempfile::tempdir().unwrap();
    let pin = pin_for(&[0; 100], 5);
    assert_eq!(download_from(root.path(), &pin, &[0u8; 200][..]), Err(WRONG_DOWNLOAD.into()));
    assert_eq!(leftovers(&downloads_in(root.path())), Vec::<String>::new());
}

#[test]
fn an_install_checks_the_download_again() {
    let bytes = good();
    let root = tempfile::tempdir().unwrap();
    let pin = pin_for(&bytes, 5);
    download_from(root.path(), &pin, &bytes[..]).unwrap();
    // It changed after the download was checked.
    fs::write(downloads_in(root.path()).join(pin.file), b"swapped").unwrap();
    assert_eq!(install_in(root.path(), &pin, |_| Ok(())), Err(WRONG_DOWNLOAD.into()));
    assert_eq!(installed_in(root.path(), &pin), None);
}

#[test]
fn archives_with_links_unsafe_paths_or_files_outside_the_bundle_are_refused_and_leave_nothing() {
    let bad: [(&str, Vec<(&str, u8, &[u8], &str)>); 8] = [
        ("symlink", vec![("Wings Chromium.app/", b'5', b"", ""), ("Wings Chromium.app/link", b'2', b"", "/etc/passwd")]),
        ("hard link", vec![("Wings Chromium.app/", b'5', b"", ""), ("Wings Chromium.app/hard", b'1', b"", "Wings Chromium.app/a")]),
        ("parent folder", vec![("Wings Chromium.app/", b'5', b"", ""), ("../x", b'0', b"x", "")]),
        ("parent inside", vec![("Wings Chromium.app/../x", b'0', b"x", "")]),
        ("absolute", vec![("/tmp/x", b'0', b"x", "")]),
        ("outside the bundle", vec![("Wings Chromium.app/", b'5', b"", ""), ("other/x", b'0', b"x", "")]),
        ("duplicate", vec![("Wings Chromium.app/a", b'0', b"1", ""), ("Wings Chromium.app/a", b'0', b"2", "")]),
        ("special file", vec![("Wings Chromium.app/", b'5', b"", ""), ("Wings Chromium.app/fifo", b'6', b"", "")]),
    ];
    for (what, entries) in bad {
        let bytes = archive(&entries);
        let (root, result) = install(&bytes, &pin_for(&bytes, 1000));
        assert!(result.is_err(), "{what} was installed");
        // No fresh folder, no version, and nothing written outside the root.
        assert_eq!(leftovers(root.path()), ["downloads"], "{what}");
        assert!(!root.path().parent().unwrap().join("x").exists(), "{what}");
    }
}

#[test]
fn an_archive_that_unpacks_to_more_than_pinned_is_refused() {
    // Small compressed, 9 MiB unpacked: over a pin of 0 and its margin.
    let bytes = archive(&[("Wings Chromium.app/big", b'0', &vec![7u8; 9 << 20], "")]);
    let (root, result) = install(&bytes, &pin_for(&bytes, 0));
    assert!(result.is_err());
    assert_eq!(leftovers(root.path()), ["downloads"]);
    // The same archive installs when the pin allows its size.
    let (_, result) = install(&bytes, &pin_for(&bytes, 9 << 20));
    result.unwrap();
}

#[test]
fn a_bundle_that_fails_its_check_is_not_installed() {
    let bytes = good();
    let root = tempfile::tempdir().unwrap();
    let pin = pin_for(&bytes, 5);
    download_from(root.path(), &pin, &bytes[..]).unwrap();
    assert_eq!(install_in(root.path(), &pin, |_| Err("bad".into())), Err("bad".into()));
    assert_eq!(leftovers(root.path()), ["downloads"]);
}

/// What is installed is what was hashed: the archive is hashed as it is unpacked, from the one reader, so another archive
/// read in its place (the file swapped or rewritten after `verify` checked it) is refused even when it unpacks cleanly.
#[test]
fn an_install_keeps_only_what_it_hashed_as_it_unpacked() {
    let pinned = good();
    let pin = pin_for(&pinned, 100);
    let swapped = archive(&[("Wings Chromium.app/", b'5', b"", ""), ("Wings Chromium.app/evil", b'0', b"evil", "")]);
    let root = tempfile::tempdir().unwrap();
    assert_eq!(install_from(root.path(), &pin, &swapped[..], |_| Ok(())), Err(WRONG_DOWNLOAD.into()));
    assert_eq!(leftovers(root.path()), Vec::<String>::new());
    // Changed part way through.
    let half = pinned.len() / 2;
    let changed = pinned[..half].iter().copied().chain(swapped[half..].iter().copied()).collect::<Vec<u8>>();
    assert_eq!(install_from(root.path(), &pin, &changed[..], |_| Ok(())), Err(WRONG_DOWNLOAD.into()));
    // Longer than pinned: the end of the file counts too.
    let longer = [&pinned[..], b"x"].concat();
    assert_eq!(install_from(root.path(), &pin, &longer[..], |_| Ok(())), Err(WRONG_DOWNLOAD.into()));
    assert_eq!(leftovers(root.path()), Vec::<String>::new());
    install_from(root.path(), &pin, &pinned[..], |_| Ok(())).unwrap();
    assert_eq!(fs::read(root.path().join("1.2.3/Wings Chromium.app/Contents/Info.plist")).unwrap(), b"plist");
}

/// An installed engine with `pin` holding its tree hash, unpacked from an archive with an executable in it.
fn installed() -> (tempfile::TempDir, PathBuf, Pinned) {
    let bytes = archive(&[
        ("Wings Chromium.app/", b'5', b"", ""),
        ("Wings Chromium.app/Contents/", b'5', b"", ""),
        ("Wings Chromium.app/Contents/Info.plist", b'0', b"plist", ""),
        ("Wings Chromium.app/Contents/Frameworks/", b'5', b"", ""),
        ("Wings Chromium.app/Contents/Frameworks/helper", b'0', b"\xcf\xfa\xed\xfe code", ""),
    ]);
    let mut pin = pin_for(&bytes, 100);
    let (root, result) = install(&bytes, &pin);
    result.unwrap();
    let bundle = installed_in(root.path(), &pin).unwrap();
    pin.tree = Box::leak(tree_hash(&bundle, pin.unpacked).unwrap().hash.into_boxed_str());
    (root, bundle, pin)
}

#[test]
fn an_installed_engine_matches_its_tree_hash_and_any_change_to_its_files_is_refused() {
    use std::os::unix::fs::PermissionsExt;
    let (_root, bundle, pin) = installed();
    assert_eq!(verify_against(&bundle, &pin), Ok(()));
    assert_eq!(failed_check(), None);
    let changes: [(&str, fn(&Path)); 8] = [
        ("a file's bytes", |bundle| fs::write(bundle.join("Contents/Frameworks/helper"), b"\xcf\xfa\xed\xfe evil").unwrap()),
        ("a byte added", |bundle| fs::write(bundle.join("Contents/Info.plist"), b"plist ").unwrap()),
        ("a file made executable", |bundle| fs::set_permissions(bundle.join("Contents/Info.plist"), fs::Permissions::from_mode(0o755)).unwrap()),
        ("a file added", |bundle| fs::write(bundle.join("Contents/Frameworks/libevil.dylib"), b"").unwrap()),
        ("a folder added", |bundle| fs::create_dir(bundle.join("Contents/Plugins")).unwrap()),
        ("a file removed", |bundle| fs::remove_file(bundle.join("Contents/Info.plist")).unwrap()),
        ("a file renamed", |bundle| fs::rename(bundle.join("Contents/Info.plist"), bundle.join("Contents/Info.plisu")).unwrap()),
        ("a file moved", |bundle| fs::rename(bundle.join("Contents/Frameworks/helper"), bundle.join("Contents/helper")).unwrap()),
    ];
    for (what, change) in changes {
        let (_root, bundle, pin) = installed();
        change(&bundle);
        assert_eq!(verify_against(&bundle, &pin), Err(CHANGED.into()), "{what}");
        // The Engine section says so.
        assert_eq!(failed_check().as_deref(), Some(CHANGED), "{what}");
    }
    // A second install starts afresh, on the same files.
    let (_root, again, _) = installed();
    assert_eq!(tree_hash(&again, 100).unwrap().hash, pin.tree);
}

/// The bundle holds only files and folders: a link (to a changed copy elsewhere, or to the same bytes) or a pipe in it is
/// refused, as is the bundle being a link itself.
#[test]
fn a_link_or_a_special_file_in_an_installed_engine_is_refused() {
    let (_root, bundle, pin) = installed();
    let elsewhere = tempfile::tempdir().unwrap();
    fs::copy(bundle.join("Contents/Info.plist"), elsewhere.path().join("Info.plist")).unwrap();
    let swaps: [(&str, Box<dyn Fn(&Path)>); 4] = [
        ("a link to the same bytes", Box::new(|bundle: &Path| {
            fs::remove_file(bundle.join("Contents/Info.plist")).unwrap();
            std::os::unix::fs::symlink(elsewhere.path().join("Info.plist"), bundle.join("Contents/Info.plist")).unwrap();
        })),
        ("a link to a folder", Box::new(|bundle: &Path| std::os::unix::fs::symlink(elsewhere.path(), bundle.join("Contents/Resources")).unwrap())),
        ("a named pipe", Box::new(|bundle: &Path| {
            let path = std::ffi::CString::new(bundle.join("Contents/fifo").as_os_str().as_encoded_bytes()).unwrap();
            assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o644) }, 0);
        })),
        ("the bundle itself a link", Box::new(|bundle: &Path| {
            let moved = bundle.with_file_name("moved.app");
            fs::rename(bundle, &moved).unwrap();
            std::os::unix::fs::symlink(&moved, bundle).unwrap();
        })),
    ];
    assert!(matches_pin(&bundle, &pin));
    for (what, swap) in swaps {
        let (_root, bundle, pin) = installed();
        swap(&bundle);
        assert!(tree_hash(&bundle, pin.unpacked).is_err(), "{what}");
        assert!(!matches_pin(&bundle, &pin), "{what}");
    }
}

#[test]
fn an_install_deletes_other_versions() {
    let bytes = good();
    let root = tempfile::tempdir().unwrap();
    let pin = pin_for(&bytes, 5);
    fs::create_dir_all(root.path().join("0.9.0/Wings Chromium.app")).unwrap();
    download_from(root.path(), &pin, &bytes[..]).unwrap();
    install_in(root.path(), &pin, |_| Ok(())).unwrap();
    assert_eq!(leftovers(root.path()), ["1.2.3"]);
}

#[test]
fn the_choice_is_saved_and_read_back_per_profile() {
    let bytes = good();
    let (root, result) = install(&bytes, &pin_for(&bytes, 5));
    result.unwrap();
    let pin = pin_for(&bytes, 5);
    let (one, two) = (profile("browser"), profile("other"));
    assert_eq!(choice_in(root.path(), &pin, true, &one), Kind::WebKit);
    set_choice_in(root.path(), &one, Kind::Chromium).unwrap();
    assert_eq!(choice_in(root.path(), &pin, true, &one), Kind::Chromium);
    assert_eq!(choice_in(root.path(), &pin, true, &two), Kind::WebKit);
    assert!(fs::read_to_string(root.path().join("choice.json")).unwrap().contains(r#""browser.main": "chromium""#));
    set_choice_in(root.path(), &one, Kind::WebKit).unwrap();
    assert_eq!(choice_in(root.path(), &pin, true, &one), Kind::WebKit);
}

#[test]
fn chromium_is_chosen_only_when_installed_and_runnable() {
    let bytes = good();
    let pin = pin_for(&bytes, 5);
    let root = tempfile::tempdir().unwrap();
    let one = profile("browser");
    set_choice_in(root.path(), &one, Kind::Chromium).unwrap();
    // Chosen but not installed.
    assert_eq!(choice_in(root.path(), &pin, true, &one), Kind::WebKit);
    download_from(root.path(), &pin, &bytes[..]).unwrap();
    install_in(root.path(), &pin, |_| Ok(())).unwrap();
    assert_eq!(choice_in(root.path(), &pin, true, &one), Kind::Chromium);
    // Installed and chosen, but this Mac can't run it.
    assert_eq!(choice_in(root.path(), &pin, false, &one), Kind::WebKit);
}

#[test]
fn removing_deletes_everything_and_puts_every_profile_on_webkit() {
    let bytes = good();
    let pin = pin_for(&bytes, 5);
    let root = tempfile::tempdir().unwrap();
    download_from(root.path(), &pin, &bytes[..]).unwrap();
    install_in(root.path(), &pin, |_| Ok(())).unwrap();
    fs::create_dir_all(root.path().join("profiles/browser")).unwrap();
    fs::create_dir_all(downloads_in(root.path())).unwrap();
    let (one, two) = (profile("browser"), profile("other"));
    set_choice_in(root.path(), &one, Kind::Chromium).unwrap();
    set_choice_in(root.path(), &two, Kind::Chromium).unwrap();
    remove_in(root.path()).unwrap();
    assert_eq!(leftovers(root.path()), ["choice.json"]);
    assert_eq!(read_choices(root.path()).values().collect::<Vec<_>>(), [&Kind::WebKit, &Kind::WebKit]);
    assert_eq!(installed_in(root.path(), &pin), None);
}

#[test]
fn the_generation_bumps() {
    let before = generation();
    bump();
    assert!(generation() > before);
}
