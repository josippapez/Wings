//! Disk, its map and its collector.
use super::*;

/// The handler of the button whose accessible name is `label`.
fn button_labelled(nodes: &[Node], label: &str) -> Option<String> {
    nodes.iter().find_map(|node| match node {
        Node::Button(button) if button.label.as_deref() == Some(label) => button.on_click.clone(),
        other => other
            .parts()
            .into_iter()
            .find_map(|part| button_labelled(part, label)),
    })
}

fn sunburst_in(nodes: &[Node]) -> Option<&crate::page::tree::Sunburst> {
    nodes.iter().find_map(|node| match node {
        Node::Sunburst(map) => Some(map),
        other => other.parts().into_iter().find_map(sunburst_in),
    })
}

#[test]
fn disk_scans_a_folder_in_its_main_and_its_native_page_draws_the_map_and_fills_the_collector() {
    let rig = Rig::with(load("disk"), true, vec![]);
    // Files of at least the 1 MiB a folder scan keeps, written out so that they take space on disk.
    let folder = rig.root.config("disk").root.join("scanned");
    for (path, mib) in [("Downloads/installer.dmg", 3), ("Code/app.bin", 2)] {
        let file = folder.join(path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, vec![7u8; mib << 20]).unwrap();
    }
    let id: String = folder.to_string_lossy().into();
    rig.host.publish_projects(vec![Project {
        id: id.clone(),
        path: id,
        name: "Scanned".into(),
        parent_id: None, hidden_sections: Vec::new(),
    }]);
    let (on_event, main_errors) = errors();
    rig.host
        .start_main(&module("disk", "main"), on_event)
        .unwrap();

    let (page, events) = open_page(&rig.host, &module("disk", "map"));
    let overview = tree(&events, "the disks and folders", |tree| {
        let text = tree.text();
        text.contains("Disks") && text.contains("Scanned") && !text.contains("Looking for disks")
    });
    let scan = button_labelled(&overview.children, "Scan Scanned").expect("a Scan button");
    assert!(page.dispatch(&scan, None));

    let mapped = tree(&events, "the map of the folder", |tree| {
        sunburst_in(&tree.children).is_some() && tree.text().contains("Downloads")
    });
    let map = sunburst_in(&mapped.children).unwrap();
    // Two folders of one file each: a wedge for each folder and one for its file.
    assert!(map.wedges.len() >= 4, "{:?}", map.wedges);
    assert!(mapped.text().contains(" MB"), "{}", mapped.text());

    // Pointing at a wedge says what it is in the middle, and a row's menu puts its folder in the Collector.
    let on_point = map.on_point.clone().expect("a pointer handler");
    assert!(page.dispatch(&on_point, Some(&json!({ "index": 0 }))));
    tree(&events, "the wedge's name in the middle", |tree| {
        sunburst_in(&tree.children).is_some_and(|map| map.title != "scanned")
    });
    // The app opens a row's menu itself: its items are in the page already.
    let mut add = Vec::new();
    buttons_named(&mapped.children, "Add to Collector", &mut add);
    assert!(page.dispatch(&add[0], None));
    tree(&events, "the Collector with the folder", |tree| {
        tree.text().contains("1 item,")
    });
    assert!(
        main_errors.try_recv().is_err(),
        "main reported {:?}",
        main_errors.try_recv()
    );
}

/// A folder of 5,000 files, each a block on disk, which the map asks Wings for in one answer: the page takes it in its
/// turn and draws the first rows of the list, with a button for the rest.
#[test]
fn disk_page_takes_a_folder_of_five_thousand_files_in_one_turn_and_draws_the_first_rows() {
    let rig = Rig::with(load("disk"), true, vec![]);
    let folder = rig.root.config("disk").root.join("many");
    fs::create_dir_all(&folder).unwrap();
    for n in 0..5000 {
        fs::write(folder.join(format!("file-{n:04}.txt")), "x").unwrap();
    }
    let id: String = folder.to_string_lossy().into();
    rig.host.publish_projects(vec![Project { id: id.clone(), path: id, name: "Many".into(), parent_id: None, hidden_sections: Vec::new() }]);
    let (on_event, main_errors) = errors();
    rig.host.start_main(&module("disk", "main"), on_event).unwrap();

    let (page, events) = open_page(&rig.host, &module("disk", "map"));
    let overview = tree(&events, "the disks and folders", |tree| tree.text().contains("Many") && !tree.text().contains("Looking for disks"));
    let scan = button_labelled(&overview.children, "Scan Many").expect("a Scan button");
    assert!(page.dispatch(&scan, None));
    let mapped = tree(&events, "the first rows of the folder", |tree| tree.text().contains("Show more (4880)"));
    assert!(mapped.text().contains("file-"), "{}", mapped.text());
    assert!(main_errors.try_recv().is_err(), "main reported {:?}", main_errors.try_recv());
}

/// 12,000 files of one block each are more than a folder's answer keeps, so they are all in its smaller items: opening
/// them asks Wings for every one, which the page takes in its turn and draws the first rows of.
#[test]
fn disk_page_opens_the_smaller_items_of_a_folder_of_twelve_thousand_files() {
    let rig = Rig::with(load("disk"), true, vec![]);
    let folder = rig.root.config("disk").root.join("most");
    fs::create_dir_all(&folder).unwrap();
    for n in 0..12_000 {
        fs::write(folder.join(format!("file-{n:05}.txt")), "x").unwrap();
    }
    let id: String = folder.to_string_lossy().into();
    rig.host.publish_projects(vec![Project { id: id.clone(), path: id, name: "Most".into(), parent_id: None, hidden_sections: Vec::new() }]);
    let (on_event, main_errors) = errors();
    rig.host.start_main(&module("disk", "main"), on_event).unwrap();

    let (page, events) = open_page(&rig.host, &module("disk", "map"));
    let overview = tree(&events, "the disks and folders", |tree| tree.text().contains("Most") && !tree.text().contains("Looking for disks"));
    assert!(page.dispatch(&button_labelled(&overview.children, "Scan Most").expect("a Scan button"), None));
    let mapped = tree(&events, "the folder with its smaller items", |tree| tree.text().contains("12,000 smaller items"));
    let open = list_button_showing(&mapped.children, "12,000 smaller items").expect("the smaller items row");
    assert!(page.dispatch(&open, None));
    tree(&events, "the first rows of the smaller items", |tree| tree.text().contains("Show more (11881)"));
    assert!(main_errors.try_recv().is_err(), "main reported {:?}", main_errors.try_recv());
}
