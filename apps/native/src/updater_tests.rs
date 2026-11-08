use super::*;
use std::{cell::RefCell, io::Cursor};

struct Fixture {
    manifest: Vec<u8>,
    signature: Vec<u8>,
    key: String,
    archive: Vec<u8>,
    calls: RefCell<Vec<String>>,
}
impl Transport for Fixture {
    fn get(&self, url: &str, max: u64, output: &mut dyn Write) -> Result<()> {
        self.calls.borrow_mut().push(url.to_owned());
        let bytes = if url.ends_with(".sig") {
            &self.signature
        } else if url.ends_with(".json") {
            &self.manifest
        } else {
            &self.archive
        };
        if bytes.len() as u64 > max {
            return Err("fixture exceeds requested limit".into());
        }
        output.write_all(bytes).map_err(err)
    }
}
fn signed(bytes: &[u8], seed: u8) -> (String, Vec<u8>) {
    let mut child = Command::new("node")
        .arg("--input-type=module")
        .arg("-e")
        .arg(include_str!("updater_test_sign.mjs").replace("process.argv[2]", "process.argv[1]"))
        .arg(seed.to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(bytes).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "fixture signer failed");
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    (
        result["key"].as_str().unwrap().into(),
        result["signature"].as_str().unwrap().as_bytes().to_vec(),
    )
}
fn archive(version: &str, omit: Option<&str>, extra: Option<(&str, tar::EntryType)>) -> Vec<u8> {
    let mut builder = tar::Builder::new(Vec::new());
    let plist = format!(
        r#"<?xml version="1.0"?><plist version="1.0"><dict><key>CFBundleIdentifier</key><string>dev.wings.native</string><key>CFBundleExecutable</key><string>wings-native</string><key>CFBundleShortVersionString</key><string>{version}</string><key>CFBundleVersion</key><string>{version}</string></dict></plist>"#
    );
    for (path, bytes, mode) in [
        ("Contents/Info.plist", plist.as_bytes(), 0o644),
        ("Contents/MacOS/wings-native", b"native", 0o755),
        ("Contents/MacOS/wings-plugin-worker", b"worker", 0o755),
        ("Contents/MacOS/wings", b"cli", 0o755),
        ("Contents/MacOS/wings-terminal-server", b"server", 0o755),
    ] {
        if omit == Some(path) {
            continue;
        }
        let mut h = tar::Header::new_ustar();
        h.set_size(bytes.len() as u64);
        h.set_mode(mode);
        h.set_cksum();
        builder
            .append_data(&mut h, format!("{ROOT}/{path}"), Cursor::new(bytes))
            .unwrap();
    }
    if let Some((path, kind)) = extra {
        let mut h = tar::Header::new_ustar();
        // Raw paths exercise inputs the safe builder itself refuses to create.
        h.as_mut_bytes()[..100].fill(0);
        h.as_mut_bytes()[..path.len()].copy_from_slice(path.as_bytes());
        h.set_size(0);
        h.set_mode(0o777);
        h.set_entry_type(kind);
        if kind.is_symlink() || kind.is_hard_link() {
            h.set_link_name("/outside").unwrap();
        }
        h.set_cksum();
        builder.append(&h, Cursor::new([])).unwrap();
    }
    let bytes = builder.into_inner().unwrap();
    let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    gzip.write_all(&bytes).unwrap();
    gzip.finish().unwrap()
}
fn fixture(archive: Vec<u8>, change: impl FnOnce(&mut Release)) -> Fixture {
    let mut release = Release {
        bundle_identifier: "dev.wings.native".into(),
        version: "0.2.0".into(),
        platform: platform().into(),
        artifact: format!("Wings-native-0.2.0-{}.app.tar.gz", platform()),
        size: archive.len() as u64,
        sha256: format!("{:x}", Sha256::digest(&archive)),
        notes: "Test notes".into(),
    };
    change(&mut release);
    let manifest = serde_json::to_vec(&release).unwrap();
    let (key, signature) = signed(&manifest, 1);
    Fixture {
        manifest,
        signature,
        key,
        archive,
        calls: RefCell::new(Vec::new()),
    }
}
fn checked(f: &Fixture) -> Result<Option<Available>> {
    check_with(f, &f.key, "0.1.0", platform())
}
fn temp() -> tempfile::TempDir {
    tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap()
}

#[test]
fn signed_check_download_stage_and_restart_share_the_production_path() {
    let f = fixture(archive("0.2.0", None, None), |_| {});
    let available = checked(&f).unwrap().unwrap();
    let dir = temp();
    let target = dir.path().join("Installed.app");
    fs::create_dir(&target).unwrap();
    fs::write(target.join("old"), "usable").unwrap();
    let prepared = prepare_with(&available, &target, &f, |_| Ok(())).unwrap();
    let staged = prepared.directory.path().join(ROOT);
    validate_bundle(&staged, "0.2.0").unwrap();
    #[cfg(target_os = "macos")]
    {
        replace_and_restart(&target, &staged, swap, |path| {
            validate_bundle(path, "0.2.0")
        })
        .unwrap();
        assert_eq!(fs::read_to_string(staged.join("old")).unwrap(), "usable");
        validate_bundle(&target, "0.2.0").unwrap();
    }
    assert_eq!(f.calls.borrow().len(), 3);
    assert!(
        f.calls.borrow()[2].starts_with(&format!("{REPO}/download/native-v0.2.0/Wings-native-"))
    );
}

#[test]
fn metadata_is_authenticated_before_it_is_parsed() {
    let mut f = fixture(archive("0.2.0", None, None), |_| {});
    f.manifest = b"not even JSON".to_vec();
    assert!(checked(&f).unwrap_err().contains("authentication"));
    let (wrong, _) = signed(b"unused", 2);
    f.key = wrong;
    assert!(checked(&f).is_err());
    assert_eq!(f.calls.borrow().len(), 4);
}

#[test]
fn signed_product_arch_version_size_and_paths_are_bound() {
    for change in [
        |r: &mut Release| r.bundle_identifier = "dev.wings.app".into(),
        |r: &mut Release| r.platform = "darwin-other".into(),
        |r: &mut Release| r.artifact = "../Wings.app.tar.gz".into(),
        |r: &mut Release| r.size = MAX_ARCHIVE + 1,
        |r: &mut Release| r.size = 0,
        |r: &mut Release| r.sha256 = "bad".into(),
        |r: &mut Release| r.version = "not-semver".into(),
    ] {
        assert!(checked(&fixture(archive("0.2.0", None, None), change)).is_err());
    }
    for version in ["0.1.0", "0.0.9"] {
        let f = fixture(archive(version, None, None), |r| {
            r.version = version.into();
            r.artifact = format!("Wings-native-{version}-{}.app.tar.gz", platform());
        });
        assert!(checked(&f).unwrap().is_none());
    }
}

#[test]
fn archive_tampering_and_bad_bundles_leave_the_old_app_untouched() {
    for bytes in [
        archive("0.2.0", Some("Contents/MacOS/wings"), None),
        archive("0.3.0", None, None),
    ] {
        let f = fixture(bytes, |_| {});
        let available = checked(&f).unwrap().unwrap();
        let dir = temp();
        let target = dir.path().join("Installed.app");
        fs::create_dir(&target).unwrap();
        assert!(prepare_with(&available, &target, &f, |_| Ok(())).is_err());
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }
    let mut f = fixture(archive("0.2.0", None, None), |_| {});
    let available = checked(&f).unwrap().unwrap();
    f.archive[20] ^= 1;
    let dir = temp();
    assert!(
        prepare_with(&available, &dir.path().join("app"), &f, |_| Ok(()))
            .unwrap_err()
            .contains("digest")
    );
    f.archive.push(0);
    assert!(prepare_with(&available, &dir.path().join("app"), &f, |_| Ok(())).is_err());
}

#[test]
fn hostile_tar_paths_links_and_unexpected_roots_are_rejected() {
    for (path, kind) in [
        ("Wings-native.app/../../outside", tar::EntryType::Regular),
        ("/outside", tar::EntryType::Regular),
        ("Other.app/file", tar::EntryType::Regular),
        ("Wings-native.app/link", tar::EntryType::Symlink),
        ("Wings-native.app/link", tar::EntryType::Link),
    ] {
        let f = fixture(archive("0.2.0", None, Some((path, kind))), |_| {});
        let available = checked(&f).unwrap().unwrap();
        let dir = temp();
        assert!(
            prepare_with(&available, &dir.path().join("app"), &f, |_| Ok(())).is_err(),
            "{path}"
        );
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    }
}

#[cfg(target_os = "macos")]
#[test]
fn atomic_swap_failure_and_restart_failure_preserve_the_old_bundle() {
    let dir = temp();
    let target = dir.path().join("old.app");
    let staged = dir.path().join("new.app");
    fs::create_dir(&target).unwrap();
    fs::create_dir(&staged).unwrap();
    fs::write(target.join("version"), "old").unwrap();
    fs::write(staged.join("version"), "new").unwrap();
    assert!(
        replace_and_restart(
            &target,
            &staged,
            |_, _| Err("swap failed".into()),
            |_| panic!("must not restart")
        )
        .is_err()
    );
    assert_eq!(fs::read_to_string(target.join("version")).unwrap(), "old");
    assert!(
        replace_and_restart(&target, &staged, swap, |_| Err("spawn failed".into()))
            .unwrap_err()
            .contains("original app was restored")
    );
    assert_eq!(fs::read_to_string(target.join("version")).unwrap(), "old");
    // Interruption immediately after the atomic swap still leaves both usable bundles.
    swap(&target, &staged).unwrap();
    assert_eq!(fs::read_to_string(target.join("version")).unwrap(), "new");
    assert_eq!(fs::read_to_string(staged.join("version")).unwrap(), "old");
    swap(&target, &staged).unwrap();
}

#[cfg(target_os = "macos")]
#[test]
fn failed_new_app_reopens_the_restored_original() {
    let dir = temp();
    let target = dir.path().join("old.app");
    let staged = dir.path().join("new.app");
    fs::create_dir(&target).unwrap();
    fs::create_dir(&staged).unwrap();
    fs::write(target.join("version"), "old").unwrap();
    fs::write(staged.join("version"), "new").unwrap();
    let opened = RefCell::new(Vec::new());
    let error = replace_and_restart(&target, &staged, swap, |path| {
        let version = fs::read_to_string(path.join("version")).unwrap();
        opened.borrow_mut().push(version.clone());
        if version == "new" {
            Err("new app failed".into())
        } else {
            Ok(())
        }
    })
    .unwrap_err();
    assert_eq!(*opened.borrow(), ["new", "old"]);
    assert!(error.contains("new app failed"));
    assert_eq!(fs::read_to_string(target.join("version")).unwrap(), "old");
}

#[test]
fn native_updates_must_include_the_terminal_server() {
    let fixture = fixture(
        archive("0.2.0", Some("Contents/MacOS/wings-terminal-server"), None),
        |_| {},
    );
    let available = checked(&fixture).unwrap().unwrap();
    let dir = temp();
    assert!(prepare_with(&available, &dir.path().join("app"), &fixture, |_| Ok(())).is_err());
}

#[cfg(target_os = "macos")]
#[test]
fn wrong_actual_binary_architecture_is_rejected() {
    let arch = if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        "x86_64"
    };
    verify_binary_architecture(&std::env::current_exe().unwrap(), arch).unwrap();
    let other = if arch == "arm64" { "x86_64" } else { "arm64e" };
    let dir = temp();
    let executable = dir.path().join("wrong-architecture");
    let result = Command::new("/usr/bin/lipo")
        .args(["/usr/bin/true", "-thin", other, "-output"])
        .arg(&executable)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(
        verify_binary_architecture(&executable, arch)
            .unwrap_err()
            .contains("does not support")
    );
}

#[cfg(target_os = "macos")]
#[test]
fn rollback_and_restored_restart_errors_include_the_original_failure() {
    let dir = temp();
    let target = dir.path().join("old.app");
    let staged = dir.path().join("new.app");
    fs::create_dir(&target).unwrap();
    fs::create_dir(&staged).unwrap();
    let swaps = RefCell::new(0);
    let error = replace_and_restart(
        &target,
        &staged,
        |a, b| {
            *swaps.borrow_mut() += 1;
            if *swaps.borrow() == 1 {
                swap(a, b)
            } else {
                Err("rollback refused".into())
            }
        },
        |_| Err("new app refused".into()),
    )
    .unwrap_err();
    assert!(error.contains("new app refused") && error.contains("rollback refused"));
    let restarts = RefCell::new(0);
    let error = replace_and_restart(&target, &staged, swap, |_| {
        *restarts.borrow_mut() += 1;
        Err(if *restarts.borrow() == 1 {
            "new app refused"
        } else {
            "old app refused"
        }
        .into())
    })
    .unwrap_err();
    assert!(error.contains("new app refused") && error.contains("old app refused"));
}

#[test]
fn source_runs_and_symlink_destinations_are_not_installable() {
    assert!(destination().is_err());
    #[cfg(unix)]
    {
        let dir = temp();
        fs::create_dir(dir.path().join("real.app")).unwrap();
        std::os::unix::fs::symlink(dir.path().join("real.app"), dir.path().join("link.app"))
            .unwrap();
        assert!(
            validate_bundle(&dir.path().join("link.app"), "0.1.0")
                .unwrap_err()
                .contains("symbolic link")
        );
    }
}

#[test]
fn only_https_github_asset_hosts_are_allowed() {
    for url in [
        "https://github.com/josippapez/Wings/releases/latest/download/native.json",
        "https://release-assets.githubusercontent.com/asset?signature=test",
        "https://objects.githubusercontent.com/asset",
    ] {
        trusted_url(url).unwrap();
    }
    for url in [
        "http://github.com/asset",
        "https://github.com.evil.test/asset",
        "https://evil.test/asset",
        "file:///tmp/update",
        "https://user@github.com/asset",
        "https://github.com:1234/asset",
    ] {
        assert!(trusted_url(url).is_err());
    }
}

#[cfg(target_os = "macos")]
#[test]
fn installer_waits_for_actual_process_exit_before_replacing() {
    let mut child = Command::new("/bin/cat")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let pid = child.id();
    let (ready_send, ready) = std::sync::mpsc::channel();
    let (exit_send, exited) = std::sync::mpsc::channel();
    let thread = std::thread::spawn(move || {
        let result = wait_for_exit(pid, || {
            ready_send.send(()).unwrap();
            Ok(())
        });
        exit_send.send(result).unwrap();
    });
    ready.recv_timeout(Duration::from_secs(10)).unwrap();
    assert!(exited.recv_timeout(Duration::from_millis(50)).is_err());
    drop(child.stdin.take());
    child.wait().unwrap();
    exited
        .recv_timeout(Duration::from_secs(10))
        .unwrap()
        .unwrap();
    thread.join().unwrap();
}

#[cfg(target_os = "macos")]
#[test]
fn relaunch_requires_the_new_app_to_confirm_startup() {
    let dir = temp();
    let target = dir.path().join("Test.app");
    fs::create_dir_all(target.join("Contents/MacOS")).unwrap();
    let executable = target.join("Contents/MacOS/wings-native");
    fs::write(&executable, "#!/bin/sh\nexec /bin/sh \"$@\"\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();
    let staged = tempfile::tempdir_in(dir.path()).unwrap();
    let args = ["-c".into(), "exec node -e 'const net=require(\"node:net\"); const s=net.connect(process.argv[1],()=>s.end(\"R\"))' \"$1\"".into()];
    restart_and_confirm(&target, &args, staged.path(), Duration::from_secs(10)).unwrap();
    let failed = tempfile::tempdir_in(dir.path()).unwrap();
    assert!(
        restart_and_confirm(
            &target,
            &["-c".into(), "exit 1".into()],
            failed.path(),
            Duration::from_millis(100)
        )
        .is_err()
    );
    restart_and_confirm(&target, &args, failed.path(), Duration::from_secs(10)).unwrap();
    assert!(!failed.path().join("restart.sock").exists());
}

#[test]
fn expanded_archive_limit_and_code_signature_failures_are_errors() {
    let mut header = tar::Header::new_ustar();
    header.set_path(format!("{ROOT}/huge")).unwrap();
    header.set_size(MAX_EXPANDED + 1);
    header.set_mode(0o644);
    header.set_cksum();
    let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    gzip.write_all(header.as_bytes()).unwrap();
    let f = fixture(gzip.finish().unwrap(), |_| {});
    let available = checked(&f).unwrap().unwrap();
    let dir = temp();
    assert!(
        prepare_with(&available, &dir.path().join("app"), &f, |_| Ok(()))
            .unwrap_err()
            .contains("expands")
    );
    let f = fixture(archive("0.2.0", None, None), |_| {});
    let available = checked(&f).unwrap().unwrap();
    assert!(
        prepare_with(&available, &dir.path().join("app"), &f, |_| Err(
            "invalid code signature".into()
        ))
        .unwrap_err()
        .contains("code signature")
    );
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
}

#[test]
fn native_verifier_accepts_the_release_cli_signature_format() {
    let dir = temp();
    let cli = Path::new(env!("CARGO_MANIFEST_DIR")).join("../desktop/node_modules/.bin/tauri");
    let key = dir.path().join("test.key");
    let generated = Command::new(&cli)
        .args([
            "signer",
            "generate",
            "--ci",
            "--password",
            "",
            "--write-keys",
        ])
        .arg(&key)
        .env_remove("TAURI_SIGNING_PRIVATE_KEY")
        .output()
        .unwrap();
    assert!(
        generated.status.success(),
        "{}",
        String::from_utf8_lossy(&generated.stderr)
    );
    let manifest = dir.path().join("native.json");
    let bytes = b"{\"test\":\"native metadata only\"}\n";
    fs::write(&manifest, bytes).unwrap();
    let signed = Command::new(&cli)
        .args(["signer", "sign", "--private-key-path"])
        .arg(&key)
        .args(["--password", ""])
        .arg(&manifest)
        .env_remove("TAURI_SIGNING_PRIVATE_KEY")
        .output()
        .unwrap();
    assert!(
        signed.status.success(),
        "{}",
        String::from_utf8_lossy(&signed.stderr)
    );
    let signature = fs::read(dir.path().join("native.json.sig")).unwrap();
    let key = fs::read_to_string(dir.path().join("test.key.pub")).unwrap();
    authenticate(bytes, &signature, &key).unwrap();
    assert!(authenticate(b"{\"test\":\"different bytes\"}\n", &signature, &key).is_err());
}

#[cfg(target_os = "macos")]
#[test]
#[ignore = "requires scripts/build-mac-app.sh to have built the local release bundle"]
fn packaged_release_downloads_verifies_stages_and_swaps_only_inside_a_fixture() {
    let dir = temp();
    let archive_path = dir.path().join("native.tar.gz");
    let release_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/release");
    let tar = Command::new("/usr/bin/tar")
        .arg("-czf")
        .arg(&archive_path)
        .arg("-C")
        .arg(release_dir)
        .arg(ROOT)
        .env("COPYFILE_DISABLE", "1")
        .output()
        .unwrap();
    assert!(
        tar.status.success(),
        "{}",
        String::from_utf8_lossy(&tar.stderr)
    );
    let fixture = fixture(fs::read(archive_path).unwrap(), |r| {
        r.version = VERSION.into();
        r.artifact = format!("Wings-native-{VERSION}-{}.app.tar.gz", platform());
    });
    let available = check_with(&fixture, &fixture.key, "0.0.0", platform())
        .unwrap()
        .unwrap();
    let target = dir.path().join("Installed.app");
    fs::create_dir(&target).unwrap();
    fs::write(target.join("original"), "still recoverable").unwrap();
    let prepared = prepare_with(&available, &target, &fixture, codesign).unwrap();
    let staged = prepared.directory.path().join(ROOT);
    replace_and_restart(&target, &staged, swap, |path| {
        validate_bundle(path, VERSION)
    })
    .unwrap();
    codesign(&target).unwrap();
    assert_eq!(
        fs::read_to_string(staged.join("original")).unwrap(),
        "still recoverable"
    );
}

/// The real app writes its confirmation and hangs up before the installer reads it, and macOS then refuses the read
/// timeout. That still counts as confirmed.
#[cfg(target_os = "macos")]
#[test]
fn a_confirmation_from_an_app_that_already_hung_up_counts() {
    use std::{
        io::Write,
        os::unix::net::{UnixListener, UnixStream},
    };
    let dir = temp();
    let socket = dir.path().join("restart.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let mut app = UnixStream::connect(&socket).unwrap();
    app.write_all(b"R").unwrap();
    drop(app);
    std::thread::sleep(Duration::from_millis(100));
    let (connection, _) = listener.accept().unwrap();
    read_confirmation(connection, Duration::from_secs(5)).unwrap();
}
