//! Where a plugin's tab goes when its page asks: `openView`'s `after: "self"` puts a new tab right after the tab of the
//! page that opened it, as a chat's subagent opens beside the chat, and `showPage` brings a named page's tab to the front.

use crate::workspace::NotifyWorkspace;
use gpui::{Context, Window};

use crate::page::plugin::FrameState;

use super::{Active, Workspace};

impl Workspace {
    /// Moves a tab `plugin` just opened, the last of the plugin tabs and not one of `before`, to right after the tab whose
    /// page runs in `frame`. A tab that was open already stays where it is.
    pub(super) fn place_after(&mut self, plugin: &str, frame: u64, before: &[usize], cx: &mut Context<Self>) {
        let new = self.plugin_tabs.last().filter(|tab| !before.contains(&tab.id)).map(|tab| tab.id);
        let opener = self.plugin_tabs.iter().position(|tab| tab.plugin == plugin && tab.state.frame() == Some(frame));
        if let (Some(new), Some(opener)) = (new, opener) {
            self.move_tab_to(Active::Plugin(new), opener + 1, cx);
        }
    }

    /// Shows the tab that holds `state`, in its project, as a click on it would.
    pub(super) fn show_tab_of(&mut self, state: &FrameState, window: &mut Window, cx: &mut Context<Self>) {
        let here = self.plugin_tabs.iter().find(|tab| tab.state.same(state)).map(|tab| tab.id);
        let parked = || {
            self.parked.iter().find_map(|(path, parked)| parked.plugin_tabs.iter().find(|tab| tab.state.same(state)).map(|tab| (path.clone(), tab.id)))
        };
        let id = match here {
            Some(id) => id,
            None => {
                let Some((project, id)) = parked() else { return };
                self.switch_project(project, window, cx);
                id
            }
        };
        self.show(Active::Plugin(id), window, cx);
        cx.notify_workspace();
    }
}

#[cfg(all(test, feature = "test-support"))]
mod tests {
    use std::sync::Arc;

    use serde_json::{Value, json};

    use super::super::{
        Active, Workspace,
        plugin_tests::{open_plugins, until},
    };
    use crate::{
        installed::fixture::Data,
        page::runtime::fake::{self, Cx, Hear},
    };

    /// Pages of a chat view, by what their tab was opened with: the first names itself and opens a tab of the view right after
    /// its own, and that third tab brings the first one's to the front by its name.
    fn pages() -> String {
        fake::script("page", || {
            |cx: &mut Cx<'_>, hear: Hear<'_>| {
                if !matches!(hear, Hear::Start) {
                    return;
                }
                let args: Value = cx.link().view_args().and_then(|args| serde_json::from_str(&args).ok()).unwrap_or_default();
                if args["n"] == 1 {
                    cx.call("namePage", json!({ "name": "first" }));
                    cx.call("openView", json!({ "id": "chat", "as": "tab", "args": { "n": 3 }, "newTab": false, "after": "self" }));
                }
                if args["n"] == 3 {
                    cx.call("showPage", json!({ "name": "first" }));
                }
            }
        })
    }

    #[test]
    fn a_tab_a_page_opens_after_itself_goes_right_after_its_tab_and_a_named_pages_tab_comes_to_the_front() {
        let data = Data::temp();
        let views = json!([{ "id": "chat", "title": "Chat", "page": "dist/chat.html", "opens": ["tab"], "multiple": true }]);
        data.install_script("chatty", json!({ "contributes": { "views": views } }), &fake::idle("main"), &[("chat", &pages())], "chatty-scope");
        let (app, mut window) = open_plugins(&data, &Arc::default(), |_| {});
        window.update(|w, window, cx| {
            assert!(w.open_plugin_view("chatty", "chat", Some(json!({ "n": 1 })), window, cx));
            assert!(w.open_plugin_view("chatty", "chat", Some(json!({ "n": 2 })), window, cx));
        });
        until(&app, &mut window, "the first tab's page didn't open a tab", |w| w.plugin_tabs.len() == 3);
        let order = |w: &Workspace| -> Vec<Value> { w.plugin_tabs.iter().map(|tab| tab.args.as_ref().map_or(Value::Null, |args| args["n"].clone())).collect() };
        // Next to the tab that asked, not at the end of the strip.
        assert_eq!(window.read(|w, _| order(w)), [json!(1), json!(3), json!(2)]);
        until(&app, &mut window, "the third tab's page didn't bring the first tab to the front", |w| w.active == Active::Plugin(w.plugin_tabs[0].id));
    }

    /// A chat page that hands its session to a terminal as it starts, in its second tab only.
    fn handing_over() -> String {
        fake::script("page", || {
            |cx: &mut Cx<'_>, hear: Hear<'_>| {
                let args: Value = cx.link().view_args().and_then(|args| serde_json::from_str(&args).ok()).unwrap_or_default();
                if matches!(hear, Hear::Start) && args["n"] == 2 {
                    cx.call("openPane", json!({ "command": null, "args": [], "cwd": null, "placement": "replace" }));
                }
            }
        })
    }

    #[test]
    fn a_terminal_a_page_opens_in_place_of_its_tab_closes_that_tab_and_no_other() {
        let data = Data::temp();
        let project = data.root.join("project");
        std::fs::create_dir_all(&project).unwrap();
        let views = json!([{ "id": "chat", "title": "Chat", "page": "dist/chat.html", "opens": ["tab"], "multiple": true }]);
        data.install_script("chatty", json!({ "permissions": { "panes": [] }, "contributes": { "views": views } }), &fake::idle("main"), &[("chat", &handing_over())], "chatty-scope");
        let (app, mut window) = open_plugins(&data, &Arc::default(), |config| config.project = project.clone());
        let terminals = window.read(|w, _| w.terminals.len());
        window.update(|w, window, cx| {
            assert!(w.open_plugin_view("chatty", "chat", Some(json!({ "n": 1 })), window, cx));
            assert!(w.open_plugin_view("chatty", "chat", Some(json!({ "n": 2 })), window, cx));
        });
        until(&app, &mut window, "the page's tab didn't give way to a terminal", |w| w.plugin_tabs.len() == 1);
        window.read(|w, _| {
            assert_eq!(w.plugin_tabs[0].args, Some(json!({ "n": 1 })));
            assert_eq!(w.terminals.len(), terminals + 1);
            assert_eq!(w.active, Active::Terminal(w.terminals.last().unwrap().id));
        });
    }

    /// Two projects, the first on screen, with a plugin whose main asks `call` of Wings once it hears of the second: the
    /// folder, the first and the second.
    fn two_projects(data: &Data, call: &'static str, params: fn(&std::path::Path) -> Value) -> (std::path::PathBuf, std::path::PathBuf, std::path::PathBuf, (gpui::TestApp, gpui::TestAppWindow<Workspace>)) {
        let dir = crate::projects::test_dir("tab-project");
        let (shown, other) = (dir.join("shown"), dir.join("other"));
        std::fs::create_dir_all(&shown).unwrap();
        std::fs::create_dir_all(&other).unwrap();
        let (shown, other) = (shown.canonicalize().unwrap(), other.canonicalize().unwrap());
        let list = dir.join("spaces.json");
        std::fs::write(&list, json!([{ "id": shown, "name": "shown", "path": shown }, { "id": other, "name": "other", "path": other }]).to_string()).unwrap();
        let views = json!([{ "id": "chat", "title": "Chat", "page": "dist/chat.html", "opens": ["tab"], "multiple": true }]);
        let main = fake::script("main", {
            let other = other.clone();
            move || {
                let (other, mut asked) = (other.clone(), false);
                move |cx: &mut Cx<'_>, hear: Hear<'_>| {
                    // The list the plugin hears first may not have the project yet.
                    let known = matches!(&hear, Hear::Deliver("projects", _, text) if text.contains(other.to_str().unwrap()));
                    if known && !asked {
                        asked = true;
                        cx.call(call, params(&other));
                    }
                }
            }
        });
        data.install_script("chatty", json!({ "permissions": { "panes": [] }, "contributes": { "views": views } }), &main, &[("chat", &fake::idle("page"))], "chatty-scope");
        let opened = open_plugins(data, &Arc::default(), |config| {
            config.project = shown.clone();
            config.projects = Some(list.clone());
        });
        (dir, shown, other, opened)
    }

    #[test]
    fn a_tab_a_plugin_opens_for_a_project_joins_that_project_and_leaves_the_one_on_screen_alone() {
        let data = Data::temp();
        let (dir, shown, other, (app, mut window)) = two_projects(&data, "openView", |other| {
            json!({ "id": "chat", "as": "tab", "args": { "n": 1 }, "newTab": true, "projectId": other })
        });
        let (active, terminals) = window.read(|w, _| (w.active, w.terminals.len()));
        until(&app, &mut window, "the tab never opened", |w| w.parked.get(other.as_path()).is_some_and(|parked| parked.plugin_tabs.len() == 1));
        window.read(|w, _| {
            assert_eq!(w.project(), shown);
            assert_eq!((w.active, w.terminals.len(), w.plugin_tabs.len()), (active, terminals, 0));
        });
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_terminal_a_plugin_opens_in_the_background_joins_its_project_and_leaves_the_one_on_screen_alone() {
        let data = Data::temp();
        let (dir, shown, other, (app, mut window)) = two_projects(&data, "openPane", |other| {
            json!({ "command": null, "args": [], "cwd": other, "placement": "tab", "background": true })
        });
        let (active, terminals) = window.read(|w, _| (w.active, w.terminals.len()));
        until(&app, &mut window, "the terminal never opened", |w| w.parked.get(other.as_path()).is_some_and(|parked| !parked.terminals.is_empty()));
        window.read(|w, _| {
            assert_eq!(w.project(), shown);
            assert_eq!((w.active, w.terminals.len()), (active, terminals));
        });
        std::fs::remove_dir_all(dir).unwrap();
    }
}
