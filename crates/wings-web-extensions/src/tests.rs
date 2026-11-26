use std::{fs, io::Write, path::Path};

use tempfile::TempDir;
use zip::{write::SimpleFileOptions, ZipWriter};

use crate::{
    amo_file_url, copy_folder, detect::{detect_in, scan}, read_manifest, read_package, store_package, tree_hash, unpack, Error, Store,
    MAX_FILES, MAX_TOTAL_BYTES,
};

fn write(root: &Path, rel: &str, body: &str) {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, body).unwrap();
}

fn make_zip(files: &[(&str, &str)]) -> Vec<u8> {
    let mut w = ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, body) in files {
        w.start_file(*name, SimpleFileOptions::default()).unwrap();
        w.write_all(body.as_bytes()).unwrap();
    }
    w.finish().unwrap().into_inner()
}

const MANIFEST: &str = r#"{"manifest_version":3,"name":"Demo","version":"1.0","description":"Does things"}"#;

fn crx3(zip: &[u8]) -> Vec<u8> {
    let header = [1u8, 2, 3, 4, 5];
    let mut v = b"Cr24".to_vec();
    v.extend(3u32.to_le_bytes());
    v.extend((header.len() as u32).to_le_bytes());
    v.extend(header);
    v.extend(zip);
    v
}

fn crx2(zip: &[u8]) -> Vec<u8> {
    let mut v = b"Cr24".to_vec();
    v.extend(2u32.to_le_bytes());
    v.extend(3u32.to_le_bytes());
    v.extend(2u32.to_le_bytes());
    v.extend([9, 9, 9]);
    v.extend([8, 8]);
    v.extend(zip);
    v
}

#[test]
fn locale_placeholders_resolve_case_insensitively() {
    let d = TempDir::new().unwrap();
    write(
        d.path(),
        "manifest.json",
        r#"{"manifest_version":3,"name":"__MSG_extName__","description":"__MSG_Desc__ by me, __MSG_missing__","version":"2","default_locale":"de"}"#,
    );
    write(d.path(), "_locales/de/messages.json", r#"{"EXTNAME":{"message":"Ding"},"desc":{"message":"Tut"}}"#);
    write(d.path(), "_locales/en/messages.json", r#"{"extName":{"message":"Thing"}}"#);
    let info = read_manifest(d.path()).unwrap();
    assert_eq!(info.name, "Ding");
    assert_eq!(info.description, "Tut by me, __MSG_missing__");
}

#[test]
fn locale_falls_back_to_en_without_default_locale() {
    let d = TempDir::new().unwrap();
    write(d.path(), "manifest.json", r#"{"manifest_version":2,"name":"__MSG_n__","version":"1"}"#);
    write(d.path(), "_locales/en/messages.json", r#"{"n":{"message":"English"}}"#);
    assert_eq!(read_manifest(d.path()).unwrap().name, "English");
}

#[test]
fn icon_is_largest_up_to_128_then_action_icon() {
    let d = TempDir::new().unwrap();
    write(
        d.path(),
        "manifest.json",
        r#"{"manifest_version":3,"name":"I","version":"1","icons":{"16":"i16.png","48":"i48.png","128":"i128.png","256":"i256.png"}}"#,
    );
    for n in ["i16", "i48", "i128", "i256"] {
        write(d.path(), &format!("{n}.png"), "x");
    }
    assert_eq!(read_manifest(d.path()).unwrap().icon, Some(d.path().join("i128.png")));

    let d = TempDir::new().unwrap();
    write(d.path(), "manifest.json", r#"{"manifest_version":3,"name":"I","version":"1","icons":{"256":"big.png"},"action":{"default_icon":{"19":"a19.png","38":"a38.png"}}}"#);
    write(d.path(), "big.png", "x");
    write(d.path(), "a38.png", "x");
    assert_eq!(read_manifest(d.path()).unwrap().icon, Some(d.path().join("a38.png")));

    // Only a bigger icon exists: better than none. A named file that is missing is not returned.
    let d = TempDir::new().unwrap();
    write(d.path(), "manifest.json", r#"{"manifest_version":2,"name":"I","version":"1","icons":{"128":"gone.png","256":"/big.png"}}"#);
    write(d.path(), "big.png", "x");
    assert_eq!(read_manifest(d.path()).unwrap().icon, Some(d.path().join("big.png")));
}

#[test]
fn mv2_hosts_in_permissions_and_content_scripts_count_as_host_access() {
    let d = TempDir::new().unwrap();
    write(
        d.path(),
        "manifest.json",
        r#"{"manifest_version":2,"name":"P","version":"1","permissions":["storage","tabs","<all_urls>","https://a.example/*"],
            "content_scripts":[{"matches":["https://a.example/*","*://b.example/*"],"js":["c.js"]}]}"#,
    );
    let info = read_manifest(d.path()).unwrap();
    assert_eq!(info.permissions, ["storage", "tabs"]);
    assert_eq!(info.host_permissions, ["<all_urls>", "https://a.example/*", "*://b.example/*"]);
    assert_eq!(info.manifest_version, 2);

    let d = TempDir::new().unwrap();
    write(d.path(), "manifest.json", r#"{"manifest_version":3,"name":"P","version":"1","permissions":["storage"],"host_permissions":["https://c.example/*"]}"#);
    let info = read_manifest(d.path()).unwrap();
    assert_eq!((info.permissions, info.host_permissions), (vec!["storage".to_string()], vec!["https://c.example/*".to_string()]));
}

#[test]
fn themes_and_missing_manifests_are_errors() {
    let d = TempDir::new().unwrap();
    write(d.path(), "manifest.json", r#"{"manifest_version":3,"name":"T","version":"1","theme":{"colors":{}}}"#);
    assert!(matches!(read_manifest(d.path()), Err(Error::Unsupported(_))));
    let e = TempDir::new().unwrap();
    assert!(matches!(read_manifest(e.path()), Err(Error::Manifest(_))));
}

#[test]
fn detects_chromium_extensions_and_picks_the_highest_version() {
    let app = TempDir::new().unwrap();
    let chrome = app.path().join("Google/Chrome");
    let id = "a".repeat(32);
    write(&chrome, "Local State", r#"{"profile":{"info_cache":{"Profile 1":{"name":"Work"}}}}"#);
    write(&chrome, &format!("Profile 1/Extensions/{id}/1.9.0_0/manifest.json"), r#"{"manifest_version":3,"name":"Old","version":"1.9.0"}"#);
    write(&chrome, &format!("Profile 1/Extensions/{id}/1.10.0_0/manifest.json"), r#"{"manifest_version":3,"name":"New","version":"1.10.0"}"#);
    let theme = "b".repeat(32);
    write(&chrome, &format!("Default/Extensions/{theme}/1_0/manifest.json"), r#"{"manifest_version":3,"name":"Th","version":"1","theme":{}}"#);
    write(&chrome, "Default/Extensions/Temp/x/manifest.json", MANIFEST);
    let empty = "c".repeat(32);
    fs::create_dir_all(chrome.join(format!("Default/Extensions/{empty}/1_0"))).unwrap();
    // Arc keeps profiles under "User Data".
    let arc_id = "d".repeat(32);
    write(&app.path().join("Arc/User Data"), &format!("Default/Extensions/{arc_id}/2_0/manifest.json"), MANIFEST);

    let found = detect_in(app.path());
    assert_eq!(found.len(), 2, "{found:?}");
    let a = &found[1];
    assert_eq!((a.browser_name.as_str(), a.profile.as_str(), a.id.as_str(), a.name.as_str(), a.version.as_str()), ("Arc", "Default", arc_id.as_str(), "Demo", "1.0"));
    let c = &found[0];
    assert_eq!((c.browser_name.as_str(), c.profile.as_str(), c.name.as_str()), ("Chrome", "Work", "New"));
    assert!(c.source.ends_with("1.10.0_0"));
}

#[test]
fn detects_firefox_xpi_with_gecko_id() {
    let app = TempDir::new().unwrap();
    let ext = app.path().join("Firefox/Profiles/abc123.default-release/extensions");
    fs::create_dir_all(&ext).unwrap();
    let xpi = make_zip(&[("manifest.json", r#"{"manifest_version":2,"name":"Fox","version":"3","browser_specific_settings":{"gecko":{"id":"fox@example.org"}},"permissions":["tabs"]}"#)]);
    fs::write(ext.join("whatever.xpi"), xpi).unwrap();
    fs::write(ext.join("notes.txt"), "x").unwrap();
    let found = detect_in(app.path());
    assert_eq!(found.len(), 1);
    assert_eq!((found[0].browser_name.as_str(), found[0].profile.as_str(), found[0].id.as_str()), ("Firefox", "default-release", "fox@example.org"));
    assert_eq!(found[0].permissions, ["tabs"]);
}

#[test]
fn unpacks_zip_xpi_crx2_and_crx3() {
    let zip = make_zip(&[("manifest.json", MANIFEST), ("js/a.js", "1")]);
    for (label, bytes) in [("zip", zip.clone()), ("crx3", crx3(&zip)), ("crx2", crx2(&zip))] {
        let out = TempDir::new().unwrap();
        let into = out.path().join("e");
        let info = unpack(&bytes, &into).unwrap_or_else(|e| panic!("{label}: {e}"));
        assert_eq!(info.name, "Demo", "{label}");
        assert_eq!(fs::read_to_string(into.join("js/a.js")).unwrap(), "1", "{label}");
    }
}

#[test]
fn unwraps_one_top_folder_and_skips_macosx_noise() {
    let zip = make_zip(&[("ext/manifest.json", MANIFEST), ("ext/js/a.js", "1"), ("__MACOSX/ext/._manifest.json", "junk")]);
    let out = TempDir::new().unwrap();
    let into = out.path().join("e");
    assert_eq!(unpack(&zip, &into).unwrap().name, "Demo");
    assert!(into.join("js/a.js").is_file() && !into.join("ext").exists() && !into.join("__MACOSX").exists());
}

#[test]
fn refuses_traversal_absolute_symlink_and_garbage() {
    let out = TempDir::new().unwrap();
    for name in ["../evil.txt", "a/../../evil.txt", "/abs.txt", "a\\..\\evil.txt"] {
        let zip = make_zip(&[("manifest.json", MANIFEST), (name, "x")]);
        let into = out.path().join("e");
        assert!(matches!(unpack(&zip, &into), Err(Error::Unsafe(_))), "{name}");
        assert!(!into.exists(), "left a folder behind for {name}");
    }
    assert!(!out.path().join("evil.txt").exists());

    let mut w = ZipWriter::new(std::io::Cursor::new(Vec::new()));
    w.start_file("manifest.json", SimpleFileOptions::default()).unwrap();
    w.write_all(MANIFEST.as_bytes()).unwrap();
    w.add_symlink("link", "/etc/passwd", SimpleFileOptions::default()).unwrap();
    let zip = w.finish().unwrap().into_inner();
    assert!(matches!(unpack(&zip, &out.path().join("s")), Err(Error::Unsafe(_))));

    assert!(matches!(unpack(b"hello", &out.path().join("g")), Err(Error::Archive(_))));
    assert!(matches!(unpack(b"Cr24\x03\x00\x00\x00\xff\xff\xff\x7f", &out.path().join("h")), Err(Error::Archive(_))));
}

#[test]
fn caps_file_count() {
    let names: Vec<String> = (0..=MAX_FILES).map(|i| format!("f{i}")).collect();
    let mut w = ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for n in &names {
        w.start_file(n, SimpleFileOptions::default()).unwrap();
    }
    let zip = w.finish().unwrap().into_inner();
    let out = TempDir::new().unwrap();
    assert!(matches!(unpack(&zip, &out.path().join("e")), Err(Error::TooLarge(_))));
}

#[test]
fn copy_folder_copies_and_skips_symlinks() {
    let src = TempDir::new().unwrap();
    write(src.path(), "manifest.json", MANIFEST);
    write(src.path(), "js/a.js", "1");
    write(src.path(), "_metadata/verified_contents.json", "{}");
    std::os::unix::fs::symlink("/etc/passwd", src.path().join("link")).unwrap();
    let out = TempDir::new().unwrap();
    let into = out.path().join("e");
    assert_eq!(copy_folder(src.path(), &into).unwrap().name, "Demo");
    assert!(into.join("js/a.js").is_file() && into.join("_metadata").is_dir());
    assert!(!into.join("link").exists());
}

#[test]
fn a_trees_hash_changes_with_any_file_and_never_follows_a_symlink() {
    let d = TempDir::new().unwrap();
    write(d.path(), "manifest.json", MANIFEST);
    write(d.path(), "js/a.js", "1");
    let first = tree_hash(d.path()).unwrap();
    assert_eq!(tree_hash(d.path()).unwrap(), first, "the same files, the same hash");
    let copy = TempDir::new().unwrap();
    copy_folder(d.path(), copy.path()).unwrap();
    assert_eq!(tree_hash(copy.path()).unwrap(), first, "a copy hashes the same");

    // A file edited, added, renamed or a folder added: each another hash.
    let changes: [(&str, &dyn Fn(&Path)); 5] = [
        ("edited", &|root| write(root, "js/a.js", "2")),
        ("added", &|root| write(root, "js/b.js", "")),
        ("renamed", &|root| fs::rename(root.join("js/a.js"), root.join("js/c.js")).unwrap()),
        ("folder", &|root| fs::create_dir(root.join("empty")).unwrap()),
        ("symlink", &|root| std::os::unix::fs::symlink("a.js", root.join("js/link.js")).unwrap()),
    ];
    for (what, change) in changes {
        let at = TempDir::new().unwrap();
        copy_folder(d.path(), at.path()).unwrap();
        change(at.path());
        assert_ne!(tree_hash(at.path()).unwrap(), first, "{what}");
    }
    // Moving a file's bytes into its name doesn't hash the same.
    let a = TempDir::new().unwrap();
    write(a.path(), "ab", "");
    let b = TempDir::new().unwrap();
    write(b.path(), "a", "b");
    assert_ne!(tree_hash(a.path()).unwrap(), tree_hash(b.path()).unwrap());

    // A symlink to a folder is hashed by its target's name, not by what is there.
    let outside = TempDir::new().unwrap();
    write(outside.path(), "secret.txt", "one");
    std::os::unix::fs::symlink(outside.path(), d.path().join("out")).unwrap();
    let linked = tree_hash(d.path()).unwrap();
    write(outside.path(), "secret.txt", "two");
    assert_eq!(tree_hash(d.path()).unwrap(), linked);
    // And the folder itself is never one.
    let link = TempDir::new().unwrap();
    std::os::unix::fs::symlink(d.path(), link.path().join("ext")).unwrap();
    assert!(matches!(tree_hash(&link.path().join("ext")), Err(Error::Unsafe(_))));
}

#[test]
fn a_package_bigger_than_the_limit_is_refused_before_it_is_read() {
    let d = TempDir::new().unwrap();
    let big = d.path().join("big.zip");
    // Sparse: as big as it says, without the disk.
    fs::File::create(&big).unwrap().set_len(MAX_TOTAL_BYTES + 1).unwrap();
    assert!(matches!(read_package(&big), Err(Error::TooLarge(_))));
    let small = d.path().join("small.zip");
    fs::write(&small, make_zip(&[("manifest.json", MANIFEST)])).unwrap();
    assert_eq!(unpack(&read_package(&small).unwrap(), &d.path().join("e")).unwrap().name, "Demo");

    // Firefox's .xpi files are read the same way.
    let app = TempDir::new().unwrap();
    let ext = app.path().join("Firefox/Profiles/abc.default/extensions");
    fs::create_dir_all(&ext).unwrap();
    fs::File::create(ext.join("big.xpi")).unwrap().set_len(MAX_TOTAL_BYTES + 1).unwrap();
    assert!(detect_in(app.path()).is_empty());
}

#[test]
fn an_icon_reached_through_a_symlink_is_never_named() {
    let app = TempDir::new().unwrap();
    let chrome = app.path().join("Google/Chrome");
    let outside = TempDir::new().unwrap();
    write(outside.path(), "private.png", "not an icon");
    write(outside.path(), "128.png", "not an icon either");
    let manifest = r#"{"manifest_version":3,"name":"Linked","version":"1","icons":{"48":"icons/48.png","128":"pics/128.png"}}"#;
    // The icon a symlink to a file, and the folder it's in a symlink to another folder.
    for (id, link, target) in [("a", "icons/48.png", outside.path().join("private.png")), ("b", "pics", outside.path().to_path_buf())] {
        let version = chrome.join(format!("Default/Extensions/{}/1_0", id.repeat(32)));
        write(&version, "manifest.json", manifest);
        fs::create_dir_all(version.join("icons")).unwrap();
        std::os::unix::fs::symlink(&target, version.join(link)).unwrap();
        assert!(version.join(link).exists());
    }
    let found = detect_in(app.path());
    assert_eq!(found.len(), 2, "{found:?}");
    assert!(found.iter().all(|found| found.icon.is_none()), "{found:?}");
    // A real file is still found.
    let real = chrome.join(format!("Default/Extensions/{}/1_0", "c".repeat(32)));
    write(&real, "manifest.json", manifest);
    write(&real, "icons/48.png", "png");
    let found = detect_in(app.path());
    assert_eq!(found.iter().find(|found| found.name == "Linked" && found.icon.is_some()).and_then(|found| found.icon.clone()), Some(real.join("icons/48.png")));
}

#[test]
fn chrome_store_urls() {
    let id = "abcdefghijklmnopabcdefghijklmnop";
    for url in [
        format!("https://chromewebstore.google.com/detail/some-name/{id}"),
        format!("https://chromewebstore.google.com/detail/some-name/{id}/reviews?hl=en"),
        format!("https://chromewebstore.google.com/detail/{id}"),
        format!("https://chrome.google.com/webstore/detail/some-name/{id}?hl=en"),
        format!("https://chrome.google.com/webstore/detail/{id}/"),
    ] {
        let p = store_package(&url).unwrap_or_else(|| panic!("{url}"));
        assert_eq!((p.store, p.id.as_str()), (Store::Chrome, id), "{url}");
        assert_eq!(
            p.download,
            format!("https://clients2.google.com/service/update2/crx?response=redirect&prodversion=9999.0.9999.0&acceptformat=crx2,crx3&x=id%3D{id}%26uc")
        );
    }
}

#[test]
fn store_urls_reject_the_wrong_scheme_host_and_id() {
    let id = "abcdefghijklmnopabcdefghijklmnop";
    for url in [
        format!("http://chromewebstore.google.com/detail/n/{id}"),
        format!("https://evil.example/detail/n/{id}"),
        format!("https://chromewebstore.google.com.evil.example/detail/n/{id}"),
        format!("https://user@chromewebstore.google.com/detail/n/{id}"),
        format!("https://chromewebstore.google.com:8443/detail/n/{id}"),
        "https://chromewebstore.google.com/detail/n/short".to_string(),
        "https://chromewebstore.google.com/detail/n/abcdefghijklmnopabcdefghijklmnoz".to_string(),
        "https://chromewebstore.google.com/detail/n/ABCDEFGHIJKLMNOPABCDEFGHIJKLMNOP".to_string(),
        format!("https://chromewebstore.google.com/category/{id}"),
        format!("https://chrome.google.com/detail/n/{id}"),
        "http://addons.mozilla.org/en-US/firefox/addon/ublock-origin/".to_string(),
        "https://addons.mozilla.org/en-US/android/addon/ublock-origin/".to_string(),
        "https://addons.mozilla.org/en-US/firefox/addon/".to_string(),
        "https://addons.mozilla.org/en-US/firefox/addon/a%2Fb/".to_string(),
        "https://addons.mozilla.org.evil.example/en-US/firefox/addon/x/".to_string(),
        "not a url".to_string(),
    ] {
        assert_eq!(store_package(&url), None, "{url}");
    }
}

#[test]
fn amo_pages_and_api_json() {
    for url in ["https://addons.mozilla.org/en-US/firefox/addon/ublock-origin/", "https://addons.mozilla.org/de/firefox/addon/ublock-origin", "https://addons.mozilla.org/firefox/addon/ublock-origin/?src=search"] {
        let p = store_package(url).unwrap_or_else(|| panic!("{url}"));
        assert_eq!((p.store, p.id.as_str()), (Store::Firefox, "ublock-origin"));
        assert_eq!(p.download, "https://addons.mozilla.org/api/v5/addons/addon/ublock-origin/");
    }
    let json = r#"{"slug":"ublock-origin","current_version":{"version":"1","file":{"id":5,"url":"https://addons.mozilla.org/firefox/downloads/file/5308235/ublock_origin-1.77.2.xpi"}}}"#;
    assert_eq!(amo_file_url(json).as_deref(), Some("https://addons.mozilla.org/firefox/downloads/file/5308235/ublock_origin-1.77.2.xpi"));
    for bad in [
        r#"{"current_version":{"file":{"url":"http://addons.mozilla.org/x.xpi"}}}"#,
        r#"{"current_version":{"file":{"url":"https://evil.example/x.xpi"}}}"#,
        r#"{"current_version":{"file":{"url":"https://addons.mozilla.org.evil.example/x.xpi"}}}"#,
        r#"{"current_version":null}"#,
        r#"{"current_version":{"file":{}}}"#,
        "nope",
    ] {
        assert_eq!(amo_file_url(bad), None, "{bad}");
    }
}

/// A folder macOS refuses (EACCES from `chmod 000`, EPERM from a privacy rule: both `PermissionDenied`) is reported, not
/// taken for a browser with no extensions.
#[test]
fn a_refused_folder_is_reported_not_silently_empty() {
    use std::os::unix::fs::PermissionsExt;
    let app = TempDir::new().unwrap();
    let id = "a".repeat(32);
    let chrome = app.path().join("Google/Chrome");
    write(&chrome, &format!("Default/Extensions/{id}/1_0/manifest.json"), MANIFEST);
    let brave = app.path().join("BraveSoftware/Brave-Browser");
    write(&brave, &format!("Default/Extensions/{id}/1_0/manifest.json"), MANIFEST);
    let lock = |path: std::path::PathBuf, mode: u32| fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();

    // Chrome's whole folder refused, with Brave readable: Brave's is found, Chrome is named.
    lock(chrome.clone(), 0o000);
    let some = scan(app.path());
    assert_eq!((some.found.len(), some.refused.clone()), (1, vec!["Chrome".to_string()]));
    assert!(some.refusal_message().unwrap().starts_with("macOS doesn't let Wings read Chrome's extensions."));

    // Only a profile's Extensions folder refused.
    lock(chrome.clone(), 0o700);
    lock(chrome.join("Default/Extensions"), 0o000);
    let profile = scan(app.path());
    assert_eq!(profile.refused, ["Chrome"]);

    // Both refused, nothing found: the two are named.
    lock(brave.clone(), 0o000);
    let none = scan(app.path());
    lock(chrome.join("Default/Extensions"), 0o700);
    lock(brave, 0o700);
    assert!(none.found.is_empty());
    assert_eq!(none.refused, ["Chrome", "Brave"]);
    assert!(none.refusal_message().unwrap().starts_with("macOS doesn't let Wings read the extensions of Chrome and Brave."));

    // A browser that isn't there, or readable, is never reported.
    assert!(scan(app.path()).refused.is_empty());
    assert!(scan(TempDir::new().unwrap().path()).refused.is_empty());
}
