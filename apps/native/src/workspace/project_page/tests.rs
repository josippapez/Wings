//! The project page in a window: a plugin's section under the project in the sidebar, the page opening as a tab over
//! the desktop's stored facts, sections and blocks, editing it, and an agent's change reaching an open editor.

use std::time::{Duration, Instant};

use gpui::{TestApp, TestAppWindow, px, size};
use serde_json::json;
use wings_spaces::project_info::{Item, Op, Place, Who};

use super::{Added, InfoStore};
use crate::{
    installed::fixture::Data,
    workspace::{Active, Workspace, tests::open_sized},
};

const LISTER: &str = r#"
wings.onProjects(async (projects) => {
  for (const project of projects) {
    await wings.setProjectSection(project.id, 'issues', [
      { id: 'r1', label: '[ui] First issue', detail: '#12 · open', badge: { text: 'open', tone: 'success' }, icon: 'todo',
        open: { view: 'board', args: { id: 'r1' } } },
      { id: 'r2', label: 'Second', parent: 'r1', open: { view: 'board' } },
    ]);
  }
});
wings.onSectionSort(async ({ sectionId, sort }) => {
  await wings.setSectionSort(sectionId, sort);
  await wings.storage.set('sorted', sort);
});
"#;

const BOARD: &str = r#"
wings.render(JSON.stringify({ id: 0, type: '#root', props: {}, children: [
  { id: 1, type: '#text', text: 'Board ' + JSON.stringify(wings.viewArgs()) }] }));
"#;

const NOTES: &str = r#"
wings.render(JSON.stringify({ id: 0, type: '#root', props: {}, children: [
  { id: 1, type: '#text', text: 'Block page' }] }));
"#;

/// Runs the window, and the clock the page's timers read, until `done`.
fn until(
    app: &TestApp,
    window: &mut TestAppWindow<Workspace>,
    what: &str,
    done: impl Fn(&Workspace, &gpui::App) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !window.read(|w, cx| done(w, cx)) {
        assert!(Instant::now() < deadline, "{what}");
        app.advance_clock(Duration::from_millis(250));
        app.run_until_parked();
        window.draw();
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn a_plugins_section_lists_under_the_project_and_the_project_page_opens_over_the_stored_page() {
    let data = Data::temp();
    let project = data.root.join("project");
    std::fs::create_dir_all(&project).unwrap();
    let app_data = data.root.join("app-data");
    let info = InfoStore::open(&app_data);
    let id = project.to_string_lossy().into_owned();
    info.store
        .apply(
            &id,
            Op::SetFact {
                key: "Dev server".into(),
                value: "pnpm dev".into(),
                by: Who::Agent,
                old_key: None,
            },
        )
        .unwrap();
    info.store
        .apply(
            &id,
            Op::AppendSection {
                title: "Architecture".into(),
                body: "# Layers\n\n- **core**".into(),
                by: Who::You,
            },
        )
        .unwrap();
    data.install_script(
        "lister",
        json!({ "contributes": {
            "views": [{ "id": "board", "title": "Board", "page": "dist/board.html", "opens": ["tab"], "newTab": true }],
            "projectSections": [{ "id": "issues", "title": "Issues", "all": "board",
                "sorts": [{ "id": "status", "label": "Status" }, { "id": "date", "label": "Latest" }] }],
            "blocks": [{ "id": "notes", "title": "Notes block", "page": "dist/notes.html", "minHeight": 100 }],
        } }),
        LISTER,
        &[("board", BOARD), ("notes", NOTES)],
        "lister-scope",
    );
    let (app, mut window) = open_sized(Some(size(px(1000.0), px(640.0))), |config| {
        config.plugins = Some(data.paths());
        config.project = project.clone();
        config.project_info = Some(app_data.clone());
    });
    window.draw();

    // The plugin's section reaches the window, under the project it was set for.
    let key = "lister:issues";
    until(
        &app,
        &mut window,
        "the section never reached the sidebar",
        |w, _| {
            w.sections
                .rows(&project, key)
                .is_some_and(|rows| rows.len() == 2)
        },
    );
    window.draw();

    // The project page is a tab before Issues, and shows what the desktop stored.
    window.update(|w, window, cx| w.open_project_page(project.clone(), window, cx));
    window.read(|w, _| {
        assert_eq!(w.active(), Active::Project);
        assert_eq!(
            w.tabs().iter().rev().take(2).copied().collect::<Vec<_>>(),
            [Active::Issues, Active::Project]
        );
    });
    until(&app, &mut window, "the page never loaded", |w, cx| {
        w.project_page
            .as_ref()
            .is_some_and(|page| page.read(cx).data.is_some())
    });
    let page = window.read(|w, _| w.project_page.clone().unwrap());
    let stored = window.read(|_, cx| page.read(cx).data.clone().unwrap());
    assert_eq!(stored.facts[0].key, "Dev server");
    assert!(matches!(&stored.items[0], Item::Section { title, .. } if title == "Architecture"));

    // A fact typed in the add row is stored.
    window.update(|_, _, cx| {
        page.update(cx, |page, cx| {
            page.facts
                .name
                .update(cx, |field, _| field.text = "Test".into());
            page.facts
                .value
                .update(cx, |field, _| field.text = "cargo test".into());
            page.submit_fact(cx);
        });
    });
    until(&app, &mut window, "the fact was never stored", |_, cx| {
        page.read(cx)
            .data
            .as_ref()
            .is_some_and(|data| data.facts.len() == 2)
    });
    assert_eq!(info.store.get(&id).unwrap().facts[1].value, "cargo test");

    // A plugin's block is placed after the section and runs the plugin's page.
    window.update(|_, _, cx| {
        page.update(cx, |page, cx| {
            page.add_at(
                Place::End,
                Added::Block("lister".into(), "notes".into()),
                cx,
            )
        });
    });
    until(
        &app,
        &mut window,
        "the block never ran its page",
        |_, cx| {
            page.read(cx)
                .block_views
                .values()
                .any(|view| view.read(cx).text().contains("Block page"))
        },
    );

    // An open editor writes after typing stops, and takes what an agent appends meanwhile, keeping what was typed.
    let section = stored.items[0].id().to_owned();
    window.update(|_, window, cx| {
        page.update(cx, |page, cx| page.edit_section(&section, window, cx))
    });
    window.update(|_, _, cx| {
        page.update(cx, |page, cx| {
            page.editors[&section]
                .body
                .update(cx, |field, _| field.text = "# Layers\n\nmine".into());
            page.touched(&section, cx);
        });
    });
    until(
        &app,
        &mut window,
        "the edit was never stored",
        |_, _| matches!(&info.store.get(&id).unwrap().items[0], Item::Section { body, by: Who::You, .. } if body == "# Layers\n\nmine"),
    );
    info.store
        .apply(
            &id,
            Op::AppendSection {
                title: "Architecture".into(),
                body: "from an agent".into(),
                by: Who::Agent,
            },
        )
        .unwrap();
    until(
        &app,
        &mut window,
        "the open editor never took the agent's text",
        |_, cx| {
            page.read(cx).editors[&section]
                .body
                .read(cx)
                .text
                .ends_with("from an agent")
        },
    );
    window
        .update(|_, window, cx| page.update(cx, |page, cx| page.finish_edit(&section, window, cx)));
    assert!(window.read(|_, cx| page.read(cx).editors.is_empty()));

    // A sort picked in the section's menu reaches the plugin, which confirms it.
    window.update(|w, _, cx| w.pick_section_sort("lister", "issues", "date", cx));
    until(
        &app,
        &mut window,
        "the plugin never heard the sort",
        |_, _| data.saved("lister", "lister-scope", "sorted") == Some(json!("date")),
    );
    assert_eq!(
        window.read(|w, _| w.sections.sort(key).map(str::to_owned)),
        Some("date".into())
    );

    // A row opens its view as a tab of the project, with its args.
    window.update(|w, window, cx| {
        w.open_section_view(
            project.clone(),
            "lister",
            "board",
            Some(json!({ "id": "r1" })),
            false,
            window,
            cx,
        )
    });
    window.read(|w, _| {
        assert_eq!(w.plugin_tabs.len(), 1);
        assert_eq!(w.plugin_tabs[0].args, Some(json!({ "id": "r1" })));
        assert_eq!(w.active(), Active::Plugin(w.plugin_tabs[0].id));
    });

    // Closing the page's tab shows a neighbour and stops the page.
    window.update(|w, window, cx| {
        w.show(Active::Project, window, cx);
        w.close_project_page(window, cx)
    });
    window.read(|w, _| {
        assert!(w.project_page.is_none());
        assert!(!w.tabs().contains(&Active::Project));
    });
}

#[test]
fn a_plugins_block_keeps_the_state_it_saved_with_the_project_page() {
    let data = Data::temp();
    let project = data.root.join("block-state-project");
    std::fs::create_dir_all(&project).unwrap();
    let app_data = data.root.join("block-state-app-data");
    let info = InfoStore::open(&app_data);
    let id = project.to_string_lossy().into_owned();
    data.install_script(
        "keeper",
        json!({ "contributes": { "blocks": [{ "id": "kept", "title": "Kept", "page": "dist/kept.html" }] } }),
        "",
        &[("kept", r#"wings.blockState().then(async (s) => {
            if (s == null) await wings.setBlockState({ n: 1 });
            wings.render(JSON.stringify({ id: 0, type: '#root', props: {}, children: [{ id: 1, type: '#text', text: 'State ' + JSON.stringify(s) }] }));
        });"#)],
        "keeper-scope",
    );
    let (app, mut window) = open_sized(Some(size(px(1000.0), px(640.0))), |config| {
        config.plugins = Some(data.paths());
        config.project = project.clone();
        config.project_info = Some(app_data.clone());
    });
    window.update(|w, window, cx| w.open_project_page(project.clone(), window, cx));
    until(&app, &mut window, "the page never loaded", |w, cx| {
        w.project_page
            .as_ref()
            .is_some_and(|page| page.read(cx).data.is_some())
    });
    let page = window.read(|w, _| w.project_page.clone().unwrap());
    window.update(|_, _, cx| {
        page.update(cx, |page, cx| {
            page.add_at(Place::End, Added::Block("keeper".into(), "kept".into()), cx)
        })
    });
    until(
        &app,
        &mut window,
        "the block's state was never stored",
        |_, _| {
            info.store.get(&id).unwrap().items.iter().any(|item| matches!(item, Item::Block { state: Some(state), .. } if *state == json!({ "n": 1 })))
        },
    );

    window.update(|w, window, cx| {
        w.show(Active::Project, window, cx);
        w.close_project_page(window, cx);
        w.open_project_page(project.clone(), window, cx);
    });
    until(
        &app,
        &mut window,
        "the reopened block never read its state",
        |w, cx| {
            w.project_page.as_ref().is_some_and(|page| {
                page.read(cx)
                    .block_views
                    .values()
                    .any(|view| view.read(cx).text().contains(r#"State {"n":1}"#))
            })
        },
    );
}

/// Work item names `kanban` for its Sprint block, which the block's title and the Add menu showed on the desktop.
#[test]
fn a_plugins_block_shows_the_icon_its_manifest_names_over_it_and_in_the_add_menu() {
    let data = Data::temp();
    let project = data.root.join("block-icon-project");
    std::fs::create_dir_all(&project).unwrap();
    let app_data = data.root.join("block-icon-app-data");
    data.install_script(
        "sprints",
        json!({ "contributes": { "blocks": [{ "id": "sprint", "title": "Sprint", "page": "dist/sprint.html", "icon": "kanban" }] } }),
        "",
        &[("sprint", NOTES)],
        "sprints-scope",
    );
    let (app, mut window) = open_sized(Some(size(px(1000.0), px(640.0))), |config| {
        config.plugins = Some(data.paths());
        config.project = project.clone();
        config.project_info = Some(app_data.clone());
    });
    window.update(|w, window, cx| w.open_project_page(project.clone(), window, cx));
    until(&app, &mut window, "the page never loaded", |w, cx| {
        w.project_page
            .as_ref()
            .is_some_and(|page| page.read(cx).data.is_some())
    });
    let page = window.read(|w, _| w.project_page.clone().unwrap());
    let glyph = || crate::ui::spot_at("plugin-glyph:kanban");
    // Offered in the Add menu of the empty page.
    window.update(|_, _, cx| {
        page.update(cx, |page, cx| {
            page.menu = Some(Place::Top);
            cx.notify();
        })
    });
    window.draw();
    let offered = glyph().expect("the Add menu shows the block's icon");
    assert_eq!(offered.size, size(px(16.0), px(16.0)));
    window.update(|_, _, cx| {
        page.update(cx, |page, cx| {
            page.add_at(
                Place::End,
                Added::Block("sprints".into(), "sprint".into()),
                cx,
            )
        })
    });
    until(&app, &mut window, "the block was never placed", |_, cx| {
        page.read(cx).block_views.len() == 1
    });
    // Before the block's title, once the menu has slid away.
    app.advance_clock(Duration::from_secs(1));
    app.run_until_parked();
    window.draw();
    let titled = glyph().unwrap();
    assert_eq!(titled.size, size(px(14.0), px(14.0)));
}
