//! `wings.diskUsage`: the permission it needs, what it says of bad arguments (the desktop's words), and a real scan.

use std::fs;

use serde_json::json;

use crate::page::plugin::testing::Rig;

#[test]
fn a_scan_needs_the_disk_usage_permission_and_checks_its_arguments_first() {
    let denied = Rig::new(json!({}), vec![]);
    assert_eq!(
        denied.refused("return await wings.diskUsage('/');"),
        "probe may not read disk usage"
    );
    let rig = Rig::new(json!({ "permissions": { "diskUsage": true } }), vec![]);
    for (body, said) in [
        (
            "return await wings.diskUsage('');",
            "diskUsage needs a path",
        ),
        (
            "return await wings.diskUsage('/', { minBytes: -1 });",
            "diskUsage minBytes must be a number, 0 or more",
        ),
        (
            "return await wings.diskUsage('/', { depth: 1.5 });",
            "diskUsage depth must be a whole number, 0 or more",
        ),
        (
            "return await wings.diskUsage('/', { maxEntries: '9' });",
            "diskUsage maxEntries must be a whole number, 0 or more",
        ),
        (
            "return await wings.diskUsage('/', { reuse: 1 });",
            "diskUsage reuse must be true or false",
        ),
    ] {
        assert_eq!(rig.refused(body), said, "{body}");
    }
    assert_eq!(
        rig.refused("const stop = new AbortController(); stop.abort(); return await wings.diskUsage('/', { signal: stop.signal });"),
        "The scan was stopped"
    );
    assert!(rig.host.shared.scans.lock().unwrap().is_empty());
}

#[test]
fn a_scan_sizes_a_folder_and_keeps_what_the_options_ask_for() {
    let rig = Rig::new(json!({ "permissions": { "diskUsage": true } }), vec![]);
    let root = rig.root.config("probe").root.join("usage");
    fs::create_dir_all(root.join("big/deep")).unwrap();
    fs::write(root.join("big/data.bin"), vec![1u8; 300_000]).unwrap();
    fs::write(root.join("big/deep/more.bin"), vec![1u8; 100_000]).unwrap();
    fs::write(root.join("note.txt"), "x").unwrap();

    let all = rig.allowed(&format!(
        "return await wings.diskUsage({}, {{ minBytes: 0 }});",
        json!(root)
    ));
    assert_eq!(all["root"]["name"], json!(root.canonicalize().unwrap()));
    assert_eq!(all["root"]["kind"], "dir");
    let names = |entry: &serde_json::Value| -> Vec<String> {
        entry["children"]
            .as_array()
            .map(|children| {
                children
                    .iter()
                    .map(|child| child["name"].as_str().unwrap().to_owned())
                    .collect()
            })
            .unwrap_or_default()
    };
    let big = all["root"]["children"]
        .as_array()
        .unwrap()
        .iter()
        .find(|child| child["name"] == "big")
        .unwrap();
    assert!(big["size"].as_u64().unwrap() >= 400_000, "{big}");
    assert!(names(big).contains(&"deep".to_owned()), "{big}");

    // The floor folds small entries into their folder's `smaller`, and `depth` stops short of the folders in `big`.
    let shallow = rig.allowed(&format!(
        "return await wings.diskUsage({}, {{ minBytes: 200000, depth: 1 }});",
        json!(root)
    ));
    assert_eq!(names(&shallow["root"]), ["big"]);
    assert!(shallow["root"]["smaller"]["items"].as_u64().unwrap() >= 1);
    assert!(names(&shallow["root"]["children"][0]).is_empty());
    assert!(shallow["floor"].as_u64().unwrap() >= 200_000);
    assert!(rig.host.shared.scans.lock().unwrap().is_empty());
}
