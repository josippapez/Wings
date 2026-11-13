//! `--external <folder>`: the separately shipped Cyclops plugin, from its repo in `<folder>` (`wings-cyclops`, with its
//! frames built over made-up answers: `sh rust/build.sh fake`), installed in a temporary plugins folder and shown in the
//! app at 1000 and 640 points wide: its sidebar in its states, and its popover under its title bar button.
//!
//! Nothing reaches the network or your account: the fake build makes up the answers. Sprint board and Claude accounts are
//! not here: they have no build over made-up answers to show.

use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use gpui::{AnyWindowHandle, AppContext, Entity, VisualTestAppContext, WindowHandle, px, size};
use wings_native::{host::Config, installed::fixture::Data, page::PageView, workspace::Workspace};

use super::{app, save, wait};

fn read(path: PathBuf) -> String {
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

/// A sidebar or popover state: its name in the PNG, the buttons to press on the page to reach it (by text or by an icon
/// button's name), and a text it shows once there.
struct State {
    name: &'static str,
    popover: bool,
    presses: &'static [&'static str],
    shows: &'static str,
    /// The text input, by its name, that takes the keyboard once there; the page's first when there's none.
    field: Option<&'static str>,
    /// Typed into that field.
    typed: Option<&'static str>,
    /// Where the pointer rests, in the 1000pt wide window only.
    hover: Option<(f32, f32)>,
}

const fn sidebar(
    name: &'static str,
    presses: &'static [&'static str],
    shows: &'static str,
) -> State {
    State {
        name,
        popover: false,
        presses,
        shows,
        field: None,
        typed: None,
        hover: None,
    }
}

const fn popover(name: &'static str, shows: &'static str) -> State {
    State {
        name,
        popover: true,
        presses: &[],
        shows,
        field: None,
        typed: None,
        hover: None,
    }
}

pub(super) fn external(out: PathBuf, root: PathBuf) {
    let mut cx = app();
    let cyclops = root.join("wings-cyclops");
    let project = out.join("external-projects").join("web");
    std::fs::create_dir_all(&project).unwrap();
    shoot(
        &mut cx,
        &out,
        &cyclops,
        &project,
        &[
            sidebar("running", &[], "Timer one"),
            // The recent tasks that match what is typed in the task field, under it.
            State {
                typed: Some("re"),
                ..sidebar("suggest", &[], "Timer one")
            },
            sidebar("edit", &["Edit Code review"], "Save"),
            // The pointer on an entry: its buttons take the place of its times.
            State {
                hover: Some((815.0, 500.0)),
                ..sidebar("entry-hover", &[], "Code review")
            },
            // The project picker's list, open over the page, and searched.
            State {
                field: Some("Project"),
                ..sidebar("projects", &[], "Timer one")
            },
            State {
                field: Some("Project"),
                typed: Some("north"),
                ..sidebar("projects-typed", &[], "Timer one")
            },
            // An entry's project picker, and its start time with 10 typed over 08:30.
            State {
                field: Some("Project"),
                ..sidebar("edit-projects", &["Edit Code review"], "Save")
            },
            State {
                field: Some("Start"),
                typed: Some("10"),
                ..sidebar("edit-time", &["Edit Code review"], "Save")
            },
            popover("popover", "Timer one"),
            // The recent tasks that match, over the popover.
            State {
                typed: Some("re"),
                ..popover("popover-suggest", "Timer one")
            },
        ],
    );
}

fn shoot(cx: &mut VisualTestAppContext, out: &Path, repo: &Path, project: &Path, states: &[State]) {
    let mut manifest: serde_json::Value =
        serde_json::from_str(&read(repo.join("wings-plugin.json"))).unwrap();
    // The plugin asks for a newer desktop than this branch's, whose version the native app takes.
    manifest.as_object_mut().unwrap().remove("minWingsVersion");
    let id = manifest["id"].as_str().unwrap().to_owned();
    let sidebar_id = "tracker";
    let name = "cyclops";
    for width in [1000.0, 640.0] {
        for state in states {
            // A fresh install for each, so a state starts from the plugin's first page.
            let data = Data::temp();
            let package = tempfile::tempdir().expect("a folder for the package");
            std::fs::create_dir_all(package.path().join("dist/native")).unwrap();
            std::fs::write(package.path().join("wings-plugin.json"), manifest.to_string()).unwrap();
            for frame in ["main", "sidebar", "popover"] {
                let module = repo.join(format!("rust/target/fake/native/{frame}.wasm"));
                std::fs::copy(&module, package.path().join(format!("dist/native/{frame}.wasm")))
                    .unwrap_or_else(|error| panic!("{}: {error}. Run sh rust/build.sh fake", module.display()));
            }
            data.install_folder(package.path(), "shot");
            let args = ["--project".into(), project.as_os_str().to_owned()];
            let mut config = Config::parse(args, project).unwrap().unwrap();
            config.state = None;
            config.projects = None;
            config.agents = None;
            config.plugins = Some(data.paths());
            let handle: WindowHandle<Workspace> = cx
                .open_offscreen_window(size(px(width), px(640.0)), move |window, cx| {
                    cx.new(|cx| Workspace::new(config, window, cx))
                })
                .unwrap();
            let window: AnyWindowHandle = handle.into();
            wait(cx, window, Duration::from_millis(500));
            let page: Entity<PageView> = if state.popover {
                handle
                    .update(cx, |w, _, cx| w.shot_popover(&id, sidebar_id, cx))
                    .unwrap();
                wait(cx, window, Duration::from_millis(200));
                handle
                    .read_with(cx, |w, _| w.popover_page(&id, sidebar_id))
                    .unwrap()
                    .expect("a popover page")
            } else {
                handle
                    .update(cx, |w, _, cx| w.shot_sidebar(&id, sidebar_id, cx))
                    .unwrap();
                wait(cx, window, Duration::from_millis(200));
                handle
                    .read_with(cx, |w, _| w.sidebar_page(&id, sidebar_id))
                    .unwrap()
                    .expect("a sidebar page")
            };
            let shows = |cx: &mut VisualTestAppContext, text: &str| {
                let deadline = Instant::now() + Duration::from_secs(10);
                loop {
                    let shown = cx.read(|cx| page.read(cx).text());
                    if shown.contains(text) {
                        return;
                    }
                    if Instant::now() > deadline {
                        eprintln!("{name} {} never showed {text:?}: {shown}", state.name);
                        return;
                    }
                    wait(cx, window, Duration::from_millis(50));
                }
            };
            for press in state.presses {
                let deadline = Instant::now() + Duration::from_secs(10);
                // A button by its text or name, else a chip, a list row or a row that opens.
                let pressed = |cx: &mut VisualTestAppContext| {
                    cx.read(|cx| {
                        let page = page.read(cx);
                        page.press(press, 0)
                            || page.press_chip(press)
                            || page.press_list_button(press)
                            || page.press_row(press, 0)
                    })
                };
                while !pressed(cx) {
                    if Instant::now() > deadline {
                        eprintln!(
                            "{name} {}: nothing to press for {press}: {}",
                            state.name,
                            cx.read(|cx| page.read(cx).text())
                        );
                        break;
                    }
                    wait(cx, window, Duration::from_millis(50));
                }
                wait(cx, window, Duration::from_millis(200));
            }
            shows(cx, state.shows);
            if state.field.is_some() || state.typed.is_some() {
                let focus = cx.read(|cx| match state.field {
                    Some(label) => page.read(cx).input_focus(label, cx),
                    None => page.read(cx).first_input_focus(cx),
                });
                match focus {
                    Some(focus) => {
                        let _ = cx.update_window(window, |_, window, cx| focus.focus(window, cx));
                    }
                    None => eprintln!("{name} {}: no field named {:?}", state.name, state.field),
                }
                wait(cx, window, Duration::from_millis(100));
                if let Some(text) = state.typed {
                    cx.simulate_input(window, text);
                }
            }
            if let Some((x, y)) = state.hover.filter(|_| width == 1000.0) {
                cx.simulate_mouse_move(
                    window,
                    gpui::point(px(x), px(y)),
                    None,
                    gpui::Modifiers::default(),
                );
            }
            wait(cx, window, Duration::from_millis(400));
            save(
                cx,
                window,
                out.join(format!("{name}-{}-{width}.png", state.name)),
            );
            let _ = cx.update_window(window, |_, window, _| window.remove_window());
        }
    }
}
