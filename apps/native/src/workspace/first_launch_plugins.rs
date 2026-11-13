//! The first launch's question: install the official plugins? A new install has none, so the Issues tab is empty and
//! nothing says where to find them. The list comes from the official repo in the background, so the first frame never
//! waits for it. Asking is recorded when the question shows, so it is asked once whatever the answer; a list that
//! couldn't be fetched asks nothing and leaves it for the next launch.

use std::collections::VecDeque;

use gpui::{
    AnyElement, Context, FocusHandle, FontWeight, KeyDownEvent, MouseButton, SharedString, Task,
    Window, div, prelude::*, px, rgb,
};
use wings_plugins::store::Available;

use crate::{
    theme,
    ui::{self, Variant},
};

use super::Workspace;

#[derive(Default)]
pub(super) struct FirstLaunch {
    /// The question, while open.
    pub(super) asking: Option<FocusHandle>,
    /// What it lists; kept after it closes so it can finish leaving.
    pub(super) offered: Vec<Available>,
    /// Links of the plugins still to install after Install, one at a time.
    pending: VecDeque<String>,
    fetching: Option<Task<()>>,
}

impl Workspace {
    /// Fetches the official list and asks, unless this install was asked before or has plugins.
    pub(super) fn offer_official_plugins(&mut self, cx: &mut Context<Self>) {
        // Headless tests supply a source explicitly, never contact the real catalogue.
        #[cfg(test)]
        if self.manage.update_source.is_none() {
            return;
        }
        if self.prefs.official_plugins_asked || self.plugin_env.store.is_none() {
            return;
        }
        if !self.plugin_views().is_empty() {
            self.set_prefs(|prefs| prefs.official_plugins_asked = true, cx);
            return;
        }
        let source = self.updates_source();
        let list = cx
            .background_executor()
            .spawn(async move { source.available() });
        self.first_launch.fetching = Some(cx.spawn(async move |this, cx| {
            // A failed fetch asks nothing and records nothing, so the next launch tries again.
            let Ok(list) = list.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| this.official_plugins_listed(list, cx));
        }));
    }

    /// The list arrived: asks, once, if there is something to offer and nothing was installed meanwhile.
    pub fn official_plugins_listed(&mut self, list: Vec<Available>, cx: &mut Context<Self>) {
        self.first_launch.fetching = None;
        if list.is_empty()
            || self.prefs.official_plugins_asked
            || self.first_launch.asking.is_some()
            || !self.plugin_views().is_empty()
        {
            return;
        }
        let focus = cx.focus_handle();
        self.first_launch.asking = Some(focus.clone());
        // The question is the announcement of these plugins, so the next check doesn't announce them again.
        let ids: Vec<_> = list.iter().map(|plugin| plugin.id.clone()).collect();
        self.first_launch.offered = list;
        self.set_prefs(
            |prefs| {
                prefs.official_plugins_asked = true;
                for id in ids {
                    if !prefs.seen_plugins.contains(&id) {
                        prefs.seen_plugins.push(id);
                    }
                }
            },
            cx,
        );
        // The dialog takes the keys, so Escape closes it. The fetch ends with no window at hand.
        let workspace = cx.entity();
        cx.defer(move |cx| {
            cx.with_window(workspace.entity_id(), |window, cx| focus.focus(window, cx));
        });
    }

    /// Install: every listed plugin through the Install button's path, one after another. Each asks for its approval.
    pub(super) fn install_official_plugins(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.first_launch.pending = self
            .first_launch
            .offered
            .iter()
            .map(|plugin| plugin.link.clone())
            .collect();
        self.close_first_launch(window, cx);
        self.install_next_official(window, cx);
    }

    /// The next plugin of the list, once nothing else installs; each install's end calls this again.
    pub(super) fn install_next_official(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.manage.installing.is_some() {
            return;
        }
        if let Some(link) = self.first_launch.pending.pop_front() {
            self.install_available(link, window, cx);
        }
    }

    /// Closes the question, if open, and gives the keys back to the tab shown.
    pub(super) fn close_first_launch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(focus) = self.first_launch.asking.take() {
            self.leave("first-launch-plugins", focus, ui::motion::POPUP_IN, cx);
            self.show(self.active, window, cx);
        }
    }

    pub(super) fn first_launch_dialog(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let focus = self.first_launch.asking.as_ref()?;
        let (backdrop, panel) = ui::dialog(
            "first-launch-plugins",
            "Install the official plugins?",
            focus,
            480.0,
            None,
        );
        let muted = |text: SharedString| {
            div()
                .text_size(px(13.0))
                .text_color(rgb(theme::MUTED_FOREGROUND))
                .child(text)
        };
        Some(
            backdrop
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, window, cx| this.close_first_launch(window, cx)),
                )
                .child(ui::dialog_panel(
                    panel
                        .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                            if event.keystroke.key == "escape" {
                                this.close_first_launch(window, cx);
                                cx.stop_propagation();
                            }
                        }))
                        .child(
                            ui::dialog_body()
                                .child(ui::dialog_header(
                                    "Install the official plugins?",
                                    muted(
                                        "Plugins add pages and tools to Wings, like the Issues board. You'll review \
                                         what each one can do before it turns on. You can add or remove them later in \
                                         Settings, Plugins."
                                            .into(),
                                    ),
                                ))
                                .child(
                                    div()
                                        .id("first-launch-plugins-list")
                                        .max_h(px(300.0))
                                        .overflow_y_scroll()
                                        .flex()
                                        .flex_col()
                                        .gap(px(10.0))
                                        .children(self.first_launch.offered.iter().map(|plugin| {
                                            div()
                                                .flex()
                                                .flex_col()
                                                .gap(px(2.0))
                                                .child(
                                                    div()
                                                        .text_size(px(13.0))
                                                        .font_weight(FontWeight::MEDIUM)
                                                        .child(plugin.name.clone()),
                                                )
                                                .child(
                                                    div()
                                                        .text_size(px(12.0))
                                                        .text_color(rgb(theme::MUTED_FOREGROUND))
                                                        .child(plugin.description.clone()),
                                                )
                                        })),
                                )
                                .child(
                                    ui::dialog_footer()
                                        .child(
                                            ui::button(
                                                "first-launch-plugins-later",
                                                "Not now",
                                                Variant::Ghost,
                                                false,
                                                "Not now",
                                            )
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.close_first_launch(window, cx)
                                            })),
                                        )
                                        .child(
                                            ui::button(
                                                "first-launch-plugins-install",
                                                "Install",
                                                Variant::Default,
                                                false,
                                                "Install",
                                            )
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.install_official_plugins(window, cx)
                                            })),
                                        ),
                                ),
                        ),
                ))
                .into_any_element(),
        )
    }
}

#[cfg(all(test, feature = "test-support"))]
mod tests {
    use std::sync::Arc;

    use super::super::{
        manage_tests::{Updates, offered, package},
        plugin_tests::{open_plugins, until},
    };
    use crate::installed::fixture::Data;

    /// The window's state after launching with `source` as the official repo.
    fn launch(
        data: &Data,
        source: &Arc<Updates>,
    ) -> (gpui::TestApp, gpui::TestAppWindow<super::Workspace>) {
        let (app, mut window) = open_plugins(data, &Arc::default(), |c| {
            c.state = Some(data.path().join("window.json"))
        });
        window.update(|w, _, cx| {
            w.manage.update_source = Some(source.clone());
            w.offer_official_plugins(cx);
        });
        (app, window)
    }

    fn asking(window: &gpui::TestAppWindow<super::Workspace>) -> bool {
        window.read(|w, _| w.first_launch.asking.is_some())
    }

    fn source_of(ids: &[&str]) -> Arc<Updates> {
        let source = Arc::new(Updates::default());
        *source.catalog.lock().unwrap() = ids.iter().map(|id| offered(id)).collect();
        for id in ids {
            source
                .packages
                .lock()
                .unwrap()
                .insert(offered(id).link, package(id, "1.0.0", &[]));
        }
        source
    }

    #[test]
    fn the_first_launch_asks_once_the_list_arrives_and_a_later_launch_does_not() {
        let data = Data::temp();
        let source = source_of(&["issues", "hud"]);
        let (app, mut window) = launch(&data, &source);
        until(&app, &mut window, "never asked", |w| {
            w.first_launch.asking.is_some()
        });
        window.read(|w, _| {
            assert_eq!(w.first_launch.offered.len(), 2);
            assert!(w.prefs.official_plugins_asked);
            assert!(w.plugin_views().is_empty(), "nothing installs by itself");
        });
        // Quit without answering, then launch again on the same disk: it was asked.
        drop(window);
        let source = source_of(&["issues", "hud"]);
        let (app, mut window) = launch(&data, &source);
        for _ in 0..20 {
            app.run_until_parked();
            window.draw();
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(!asking(&window), "asked before");
        assert_eq!(
            source.calls.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "not even fetched"
        );
    }

    #[test]
    fn not_now_installs_nothing_and_it_is_not_asked_again() {
        let data = Data::temp();
        let source = source_of(&["issues"]);
        let (app, mut window) = launch(&data, &source);
        until(&app, &mut window, "never asked", |w| {
            w.first_launch.asking.is_some()
        });
        window.update(|w, window, cx| w.close_first_launch(window, cx));
        window.read(|w, _| {
            assert!(w.first_launch.asking.is_none());
            assert!(w.plugin_views().is_empty());
        });
        assert!(source.fetched.lock().unwrap().is_empty());
        window.update(|w, _, cx| w.offer_official_plugins(cx));
        window.update(|w, _, cx| {
            w.official_plugins_listed(vec![offered("issues")], cx);
        });
        assert!(!asking(&window), "never asked again");
    }

    #[test]
    fn install_takes_every_listed_plugin_through_the_install_path_and_asks_approval() {
        let data = Data::temp();
        let source = source_of(&["issues", "hud", "disk"]);
        let (app, mut window) = launch(&data, &source);
        until(&app, &mut window, "never asked", |w| {
            w.first_launch.asking.is_some()
        });
        window.update(|w, window, cx| w.install_official_plugins(window, cx));
        until(&app, &mut window, "never installed them all", |w| {
            w.plugin_views().len() == 3 && w.manage.installing.is_none()
        });
        assert_eq!(
            *source.fetched.lock().unwrap(),
            offered_links(&["issues", "hud", "disk"])
        );
        window.read(|w, _| {
            assert!(w.first_launch.asking.is_none());
            // Installed off and unapproved: each waits for its review, as after the Install button.
            assert_eq!(w.manage.reviews.len(), 3);
            assert!(w.plugin_views().iter().all(|view| !view.enabled));
        });
    }

    fn offered_links(ids: &[&str]) -> Vec<String> {
        ids.iter().map(|id| offered(id).link).collect()
    }

    #[test]
    fn a_failed_fetch_asks_nothing_and_leaves_it_for_the_next_launch() {
        let data = Data::temp();
        let source = source_of(&["issues"]);
        source
            .failed
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let (app, mut window) = launch(&data, &source);
        until(&app, &mut window, "never fetched", |_| {
            source.calls.load(std::sync::atomic::Ordering::SeqCst) == 1
        });
        for _ in 0..20 {
            app.run_until_parked();
            window.draw();
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        window.read(|w, _| {
            assert!(w.first_launch.asking.is_none());
            assert!(!w.prefs.official_plugins_asked);
        });
        drop(window);
        source
            .failed
            .store(false, std::sync::atomic::Ordering::SeqCst);
        let (app, mut window) = launch(&data, &source);
        until(&app, &mut window, "next launch never asked", |w| {
            w.first_launch.asking.is_some()
        });
    }
}
