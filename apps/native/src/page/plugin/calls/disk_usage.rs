//! `wings.diskUsage`: the permission it needs, what it says of bad arguments (the desktop's words), and a real scan.

use std::fs;

use serde_json::{Value, json};

use crate::page::plugin::testing::Rig;

/// What `wings.diskUsage(path, options)` sends the host: the options it was given, and the token and `stream` it adds.
fn scan(path: impl serde::Serialize, options: Value) -> Value {
    let mut params = json!({ "path": path, "token": "1", "stream": false });
    params.as_object_mut().unwrap().extend(options.as_object().unwrap().clone());
    params
}

#[test]
fn a_scan_needs_the_disk_usage_permission_and_checks_its_arguments_first() {
    let denied = Rig::new(json!({}), vec![]);
    assert_eq!(
        denied.refuses("diskUsage", scan("/", json!({}))),
        "probe may not read disk usage"
    );
    let rig = Rig::new(json!({ "permissions": { "diskUsage": true } }), vec![]);
    for (params, said) in [
        (scan("", json!({})), "diskUsage needs a path"),
        (
            scan("/", json!({ "minBytes": -1 })),
            "diskUsage minBytes must be a number, 0 or more",
        ),
        (
            scan("/", json!({ "depth": 1.5 })),
            "diskUsage depth must be a whole number, 0 or more",
        ),
        (
            scan("/", json!({ "maxEntries": "9" })),
            "diskUsage maxEntries must be a whole number, 0 or more",
        ),
        (
            scan("/", json!({ "reuse": 1 })),
            "diskUsage reuse must be true or false",
        ),
    ] {
        assert_eq!(rig.refuses("diskUsage", params.clone()), said, "{params}");
    }
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

    let all = rig.allows("diskUsage", scan(&root, json!({ "minBytes": 0 })));
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
    let shallow = rig.allows("diskUsage", scan(&root, json!({ "minBytes": 200000, "depth": 1 })));
    assert_eq!(names(&shallow["root"]), ["big"]);
    assert!(shallow["root"]["smaller"]["items"].as_u64().unwrap() >= 1);
    assert!(names(&shallow["root"]["children"][0]).is_empty());
    assert!(shallow["floor"].as_u64().unwrap() >= 200_000);
    assert!(rig.host.shared.scans.lock().unwrap().is_empty());
}
